// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// Custom rubber-band selection for the file views. GTK's built-in
// enable_rubberband races the row DragSources — it can claim the
// pointer sequence on a press inside a cell, killing the drag — and
// it does not mark covered items while the rectangle is still being
// drawn. This controller is deterministic instead:
//
//   press on a cell     -> the row's DragSource (capture phase) owns
//                          the sequence, this gesture denies itself
//   press on background -> the band starts, and every motion event
//                          updates the MultiSelection live, so the
//                          user sees covered items highlighted while
//                          still dragging (classic file managers' behaviour)
//
// Ctrl held at press time keeps the existing selection and adds to
// it; a plain drag replaces it.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{gdk, glib};

/// Distance to the overlay edge that triggers auto-scrolling.
const EDGE_ZONE: f64 = 28.0;
/// Pixels scrolled per motion event inside the edge zone.
const EDGE_STEP: f64 = 18.0;

/// Per-controller shared state for one band gesture.
struct Band {
    /// Visible rectangle, laid out via margins + size request.
    rect_widget: gtk4::Widget,
    /// GridView/ColumnView whose cells are hit-tested.
    view: gtk4::Widget,
    scrolled: gtk4::ScrolledWindow,
    selection: gtk4::MultiSelection,
    /// Press point in overlay coordinates.
    anchor: (f64, f64),
    /// Positions currently marked by this band.
    banded: HashSet<u32>,
}

/// Attach the rubber-band machinery to `overlay`, which wraps a
/// ScrolledWindow containing `view`. Called once per view page when
/// the stack page is built (file_view::wrap_view_page).
pub fn attach(
    overlay: &gtk4::Overlay,
    view: &gtk4::Widget,
    scrolled: &gtk4::ScrolledWindow,
    selection: &gtk4::MultiSelection,
) {
    let rect_widget = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    rect_widget.add_css_class("rubberband-rect");
    rect_widget.set_visible(false);
    rect_widget.set_halign(gtk4::Align::Start);
    rect_widget.set_valign(gtk4::Align::Start);
    // The rectangle is purely visual — it must never become the
    // event target or presses inside it would be swallowed (the
    // "first click does nothing" bug).
    rect_widget.set_can_target(false);
    rect_widget.set_can_focus(false);
    overlay.add_overlay(&rect_widget);

    let state = Rc::new(RefCell::new(Band {
        rect_widget: rect_widget.upcast::<gtk4::Widget>(),
        view: view.clone(),
        scrolled: scrolled.clone(),
        selection: selection.clone(),
        anchor: (0.0, 0.0),
        banded: HashSet::new(),
    }));

    let drag = gtk4::GestureDrag::new();
    drag.set_button(gdk::BUTTON_PRIMARY);

    drag.connect_drag_begin({
        let state = state.clone();
        let overlay = overlay.clone();
        move |gesture, sx, sy| {
            // The row DragSource claims sequences that start on a
            // cell before we ever get here — but verify anyway: a
            // press on a cell, the scrollbar or a popover must never
            // become a band.
            if let Some(picked) = overlay.pick(sx, sy, gtk4::PickFlags::DEFAULT) {
                let bg = is_background(&picked, &state.borrow().view);
                if !bg {
                    gesture.set_state(gtk4::EventSequenceState::Denied);
                    return;
                }
            }
            let mut st = state.borrow_mut();
            st.anchor = (sx, sy);
            st.banded.clear();
            let additive = gesture
                .current_event_state()
                .contains(gdk::ModifierType::CONTROL_MASK);
            if !additive {
                st.selection.unselect_all();
            }
            st.rect_widget.set_visible(true);
        }
    });

    drag.connect_drag_update({
        let state = state.clone();
        let overlay = overlay.clone();
        move |_gesture, dx, dy| {
            update_band(&state, &overlay, dx, dy);
            edge_autoscroll(&state, &overlay, dx, dy);
        }
    });

    let hide_band = |state: &Rc<RefCell<Band>>| {
        let mut st = state.borrow_mut();
        st.rect_widget.set_visible(false);
        st.banded.clear();
    };
    drag.connect_drag_end({
        let state = state.clone();
        move |_, _, _| hide_band(&state)
    });
    drag.connect_cancel({
        let state = state.clone();
        move |_, _| hide_band(&state)
    });

    // A plain click on background clears the selection. This lives
    // on the overlay (same level as the band gesture) so it cannot
    // be lost to propagation order — the grid-level clear in
    // view_widgets.rs stays as well; both are idempotent.
    let click = gtk4::GestureClick::new();
    click.set_button(gdk::BUTTON_PRIMARY);
    // CAPTURE phase: the overlay is an ancestor of the ScrolledWindow,
    // so this runs before the scroller's own capture-phase drag
    // gesture can claim the press (which swallowed the first
    // deselect-click). Denied below keeps it a pure observer.
    click.set_propagation_phase(gtk4::PropagationPhase::Capture);
    click.connect_pressed({
        let state = state.clone();
        let overlay = overlay.clone();
        move |gesture, _, x, y| {
            gesture.set_state(gtk4::EventSequenceState::Denied);
            let Some(picked) = overlay.pick(x, y, gtk4::PickFlags::DEFAULT) else {
                return;
            };
            let bg = is_background(&picked, &state.borrow().view);
            if bg {
                state.borrow().selection.unselect_all();
            }
        }
    });
    overlay.add_controller(click);
    overlay.add_controller(drag);

    // Wayland quirk: the first button press after a cancelled DnD
    // operation is delivered to the drag's leftover grab target and
    // never reaches any of our gestures — the "first click does not
    // deselect" bug. A legacy controller on the *window* sees the
    // raw event stream regardless of grabs, so deselecting on
    // background presses works even then.
    let legacy = gtk4::EventControllerLegacy::new();
    legacy.connect_event({
        let state = state.clone();
        let overlay = overlay.clone();
        move |_, event| {
            if event.event_type() != gdk::EventType::ButtonPress {
                return glib::Propagation::Proceed;
            }
            let Some(button) = event.downcast_ref::<gdk::ButtonEvent>().map(|e| e.button()) else {
                return glib::Propagation::Proceed;
            };
            if button != gdk::BUTTON_PRIMARY {
                return glib::Propagation::Proceed;
            }
            let Some((sx, sy)) = event.position() else {
                return glib::Propagation::Proceed;
            };
            let Some(root) = overlay.root() else {
                return glib::Propagation::Proceed;
            };
            let Some(p) =
                root.compute_point(&overlay, &gtk4::graphene::Point::new(sx as f32, sy as f32))
            else {
                return glib::Propagation::Proceed;
            };
            if let Some(picked) = overlay.pick(p.x() as f64, p.y() as f64, gtk4::PickFlags::DEFAULT)
            {
                let bg = is_background(&picked, &state.borrow().view);
                if bg {
                    state.borrow().selection.unselect_all();
                }
            }
            glib::Propagation::Proceed
        }
    });
    // The window only exists once the overlay is realized.
    overlay.connect_map(move |overlay| {
        if let Some(root) = overlay.root() {
            root.add_controller(legacy.clone());
        }
    });
}

/// Whether `picked` is background space rather than list content:
/// true when the widget is the view itself or any ancestor that is
/// not a cell (viewport, scrolled window, overlay). False for cells
/// (direct children of the view), scrollbars and popovers — those
/// must not start a band.
fn is_background(picked: &gtk4::Widget, view: &gtk4::Widget) -> bool {
    let mut w = picked.clone();
    loop {
        if w.is::<gtk4::Scrollbar>() || w.is::<gtk4::Popover>() || w.is::<gtk4::Window>() {
            return false;
        }
        if &w == view {
            return true;
        }
        let Some(parent) = w.parent() else {
            return false;
        };
        if &parent == view {
            // Direct child of the view: a ListItemWidget cell.
            return false;
        }
        w = parent;
    }
}

/// Redraw the rectangle and update the selection from the cells it
/// covers. `dx`/`dy` is the drag offset from the anchor point in
/// overlay coordinates.
fn update_band(state: &Rc<RefCell<Band>>, overlay: &gtk4::Overlay, dx: f64, dy: f64) {
    let (ax, ay, rect_widget, view) = {
        let st = state.borrow();
        (
            st.anchor.0,
            st.anchor.1,
            st.rect_widget.clone(),
            st.view.clone(),
        )
    };
    let (cx, cy) = (ax + dx, ay + dy);
    let (x0, x1) = (ax.min(cx), ax.max(cx));
    let (y0, y1) = (ay.min(cy), ay.max(cy));

    rect_widget.set_margin_start(x0.max(0.0) as i32);
    rect_widget.set_margin_top(y0.max(0.0) as i32);
    rect_widget.set_size_request((x1 - x0).max(1.0) as i32, (y1 - y0).max(1.0) as i32);

    let rect = gtk4::graphene::Rect::new(x0 as f32, y0 as f32, (x1 - x0) as f32, (y1 - y0) as f32);

    // Hit-test every realized cell against the rectangle. Only
    // realized cells have widgets; cells outside the viewport are
    // evaluated when scrolling brings them on screen.
    // A band smaller than this still counts as a click: drag_begin
    // already cleared the selection, and re-selecting the cell under
    // a few pixels of jitter is exactly the "first click does not
    // deselect" behaviour seen in the dense icon grid.
    const MIN_BAND: f64 = 6.0;
    let mut hits = HashSet::new();
    let tiny = (x1 - x0) < MIN_BAND && (y1 - y0) < MIN_BAND;
    let mut cell = if tiny { None } else { view.first_child() };
    while let Some(c) = cell {
        if let Some(bounds) = c.compute_bounds(overlay) {
            if bounds.intersection(&rect).is_some() {
                if let Some(pos) = cell_position(&c) {
                    hits.insert(pos);
                }
            }
        }
        cell = c.next_sibling();
    }

    // Apply the diff: items leaving the rect are unmarked, items
    // entering it are marked — live, while the drag is still open.
    let mut st = state.borrow_mut();
    let leaving: Vec<u32> = st.banded.difference(&hits).copied().collect();
    for pos in leaving {
        st.selection.unselect_item(pos);
    }
    let entering: Vec<u32> = hits.difference(&st.banded).copied().collect();
    for pos in entering {
        st.selection.select_item(pos, false);
    }
    st.banded = hits;
}

/// Position of a list cell, stored on the row widget by the
/// factory bind (view_grid.rs / view_widgets.rs). GtkListItemWidget
/// exposes no public position getter in the Rust bindings, so the
/// bind writes it as qdata instead.
fn cell_position(cell: &gtk4::Widget) -> Option<u32> {
    let child = cell.first_child()?;
    unsafe { child.data::<u32>("valkyrie-pos").map(|p| *p.as_ptr()) }
}

/// Nudge the scroll position when the pointer hovers near the top or
/// bottom edge of the overlay, then re-evaluate the band so newly
/// realized cells enter the selection immediately.
fn edge_autoscroll(state: &Rc<RefCell<Band>>, overlay: &gtk4::Overlay, dx: f64, dy: f64) {
    let (ay, scrolled) = {
        let st = state.borrow();
        (st.anchor.1, st.scrolled.clone())
    };
    let cy = ay + dy;
    let h = overlay.height() as f64;
    let adj = scrolled.vadjustment();
    let new = if cy < EDGE_ZONE {
        adj.value() - EDGE_STEP
    } else if cy > h - EDGE_ZONE {
        adj.value() + EDGE_STEP
    } else {
        return;
    };
    let upper_free = (adj.upper() - adj.page_size()).max(adj.lower());
    adj.set_value(new.clamp(adj.lower(), upper_free));
    // The scroll changed cell positions; re-run the hit test.
    update_band(state, overlay, dx, dy);
}
