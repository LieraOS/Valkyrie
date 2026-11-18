// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
//! Notebook-level tab-close interactions.
//!
//! GtkNotebook processes tab-strip presses through its own capture-phase
//! gesture (tab switching and drag reordering), so widgets inside tab
//! headers do not reliably receive button events. The controller here
//! resolves presses at notebook level instead:
//!
//! - left click inside the header's "tab-close" button closes that tab;
//! - middle click anywhere on a tab header closes that tab;
//! - every other press passes through untouched, so tab switching,
//!   reordering and drag-out keep working.
//!
//! "Inside" is resolved geometrically rather than through pick(),
//! because pick() does not reliably reach children of tab-label
//! widgets.

use gtk4::prelude::*;

/// Attach the tab-close click controller to `notebook`.
pub(crate) fn attach(notebook: &gtk4::Notebook) {
    let ctrl = gtk4::GestureClick::new();
    ctrl.set_button(0); // any button: filter by current_button()
    ctrl.set_propagation_phase(gtk4::PropagationPhase::Capture);
    let nb = notebook.clone();
    ctrl.connect_pressed(move |gesture, _n, x, y| {
        let Some(picked) = nb.pick(x, y, gtk4::PickFlags::DEFAULT) else {
            return;
        };
        let button = gesture.current_button();
        for page in nb.pages().iter::<gtk4::NotebookPage>().flatten() {
            if !press_inside_tab(&picked, &page) {
                continue;
            }
            if button == 2 || (button == 1 && on_close_button(&nb, &page, x, y)) {
                detach(page.child().upcast_ref());
                gesture.set_state(gtk4::EventSequenceState::Claimed);
            }
            // Press is on this tab's header either way: resolved.
            return;
        }
    });
    notebook.add_controller(ctrl);
}

/// Whether the picked widget belongs to the tab header of `page`:
/// page.tab() is the tab-label widget, and presses on its parent (the
/// notebook's tab gizmo padding) count as inside the tab as well.
fn press_inside_tab(picked: &gtk4::Widget, page: &gtk4::NotebookPage) -> bool {
    let Some(tab) = page.tab() else { return false };
    let Some(gizmo) = tab.parent() else {
        return false;
    };
    let mut node = Some(picked.clone());
    while let Some(w) = node {
        if w == tab || w == gizmo {
            return true;
        }
        node = w.parent();
    }
    false
}

/// Whether a left-click press at notebook coordinates (x, y) lands
/// inside the "tab-close" button of the page's tab header — computed
/// by translating the press point into the button's coordinate space.
fn on_close_button(nb: &gtk4::Notebook, page: &gtk4::NotebookPage, x: f64, y: f64) -> bool {
    let Some(tab) = page.tab() else { return false };
    let press = gtk4::graphene::Point::new(x as f32, y as f32);
    let mut child = tab.first_child();
    while let Some(widget) = child {
        if widget.widget_name() == "tab-close" {
            let a = widget.allocation();
            return nb
                .compute_point(&widget, &press)
                .is_some_and(|p| point_in_rect(p.x(), p.y(), a.width() as f32, a.height() as f32));
        }
        child = widget.next_sibling();
    }
    false
}

/// Whether point (px, py) lies inside a rectangle of `w`×`h` anchored
/// at the origin — the widget-local hit test for the close button.
fn point_in_rect(px: f32, py: f32, w: f32, h: f32) -> bool {
    px >= 0.0 && py >= 0.0 && px < w && py < h
}

/// Detach the notebook page hosting `page_widget`, whichever notebook
/// it currently lives in — the tab may have been dragged to another
/// window. Pages sit inside the notebook's internal stack widget, so
/// the containing notebook is found by walking the ancestors. Closing
/// the very last tab closes the window instead, matching Ctrl+W —
/// an empty notebook would leave a broken window.
fn detach(page_widget: &gtk4::Widget) {
    let mut node = page_widget.parent();
    while let Some(current) = node {
        if let Some(nb) = current.downcast_ref::<gtk4::Notebook>() {
            if nb.n_pages() <= 1 {
                if let Some(window) = page_widget
                    .root()
                    .and_then(|r| r.downcast::<gtk4::Window>().ok())
                {
                    window.close();
                }
            } else {
                nb.detach_tab(page_widget);
            }
            return;
        }
        node = current.parent();
    }
}

#[cfg(test)]
mod tests {
    use super::point_in_rect;

    #[test]
    fn close_button_hit_test() {
        // A 22x30 close button at origin.
        assert!(point_in_rect(0.0, 0.0, 22.0, 30.0));
        assert!(point_in_rect(10.0, 15.0, 22.0, 30.0));
        assert!(point_in_rect(21.9, 29.9, 22.0, 30.0));
        // Outside on every side.
        assert!(!point_in_rect(-0.1, 10.0, 22.0, 30.0));
        assert!(!point_in_rect(10.0, -0.1, 22.0, 30.0));
        assert!(!point_in_rect(22.0, 10.0, 22.0, 30.0));
        assert!(!point_in_rect(10.0, 30.0, 22.0, 30.0));
        assert!(!point_in_rect(40.0, 40.0, 22.0, 30.0));
    }
}
