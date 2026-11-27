// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// Wiring between the tab's widgets and its navigation state: the
// back/forward/up buttons, the breadcrumb pathbar, the Ctrl+L location
// entry, the view toggle and file activation.

use gtk4::prelude::*;
use gtk4::{gio, glib};

use crate::browser_tab::{self, BrowserTab};

/// Connect every navigation-related signal of a tab.
pub fn wire(
    tab: &BrowserTab,
    back: &gtk4::Button,
    forward: &gtk4::Button,
    up: &gtk4::Button,
    path_stack: &gtk4::Stack,
) {
    let t = tab.clone();
    back.connect_clicked(move |_| t.go_back());
    let t = tab.clone();
    forward.connect_clicked(move |_| t.go_forward());
    let t = tab.clone();
    up.connect_clicked(move |_| {
        if let Some(parent) = t.location().and_then(|f| f.parent()) {
            t.navigate_to(&parent);
        }
    });

    // Mouse side buttons: 8 = back, 9 = forward. A plain GestureClick
    // cannot be used here: GTK routes mouse button 8/9 sequences to
    // widgets listening on "any" buttons (list rows even activate on
    // them), and the shared NULL event-sequence bookkeeping then loses
    // the accounting — the next normal click is swallowed ("Broken
    // accounting of active state"). Instead the events are consumed
    // one level earlier by an EventControllerLegacy in the capture
    // phase: no widget ever sees the press/release pair, the stale
    // state cannot form, and navigation fires off the release.
    //
    // The controller is attached to the TOPLEVEL, not the tab widget:
    // once GTK's leaked implicit grab retargets events onto a widget
    // outside the tab subtree (a side-button press over the sidebar,
    // the header area, dead space), a tab-scoped controller never sees
    // the release again — no navigation, and the revoke trick below
    // never runs. Every tab adds its own controller; the mapped guard
    // makes only the visible tab respond.
    let legacy = std::rc::Rc::new(gtk4::EventControllerLegacy::new());
    legacy.set_propagation_phase(gtk4::PropagationPhase::Capture);
    let t = tab.clone();
    legacy.connect_event(move |_, event| {
        let Some(button) = event
            .downcast_ref::<gtk4::gdk::ButtonEvent>()
            .map(gtk4::gdk::ButtonEvent::button)
        else {
            return glib::Propagation::Proceed;
        };
        if !matches!(button, 8 | 9) {
            return glib::Propagation::Proceed;
        }
        if !t.widget.is_mapped() {
            // Inactive tab sharing the same toplevel — leave the
            // event for the visible tab's controller.
            return glib::Propagation::Proceed;
        }
        if event.event_type() == gtk4::gdk::EventType::ButtonRelease {
            // Work around a GTK bug (present in 4.18 and still in
            // 4.24): handle_pointing_event() creates an implicit
            // pointer-focus grab on EVERY button press, but only
            // clears it when the released button is 1-5 —
            // GdkModifierType has no bits for buttons 6+. A button-
            // 8/9 release therefore leaks the grab and the next
            // regular click is routed to the stale grab target
            // instead of the widget under the pointer — i.e.
            // swallowed. The only public lever to drop that grab is
            // a widget grab on a widget that is NOT a descendant of
            // the stale target: _gtk_window_group_add_grab() revokes
            // exactly those implicit grabs. A mapped Popover adds a
            // grab via gtk_grab_add internally — ours is invisible
            // (1px, opacity 0) and pops itself down the instant it
            // maps.
            revoke_stale_pointer_grab(&t);

            let back = button == 8;
            let t2 = t.clone();
            // Idle-deferred so the model swap cannot happen while the
            // event is still mid-dispatch.
            glib::idle_add_local_once(move || {
                if back {
                    t2.go_back();
                } else {
                    t2.go_forward();
                }
            });
        }
        glib::Propagation::Stop
    });
    // The tab is not yet inside a toplevel at wiring time — attach on
    // first map (one-shot, every later map is a no-op).
    let attached = std::cell::Cell::new(false);
    tab.widget.connect_map(move |w| {
        if attached.replace(true) {
            return;
        }
        if let Some(root) = w.root() {
            root.add_controller((*legacy).clone());
        }
    });

    // Pathbar breadcrumb activates a directory.
    let t = tab.clone();
    tab.pathbar
        .connect_location_selected(move |f| t.navigate_to(f));

    // Entry commits a typed path/URI; Esc cancels back to the pathbar.
    let t = tab.clone();
    let stack = path_stack.clone();
    tab.location_entry.connect_activate(move |entry| {
        stack.set_visible_child_name("path");
        let text = entry.text().to_string();
        let file = if text.contains("://") {
            gio::File::for_uri(&text)
        } else {
            gio::File::for_path(text)
        };
        t.navigate_to(&file);
    });
    let stack = path_stack.clone();
    let entry_controller = gtk4::EventControllerKey::new();
    let entry = tab.location_entry.clone();
    entry_controller.connect_key_pressed(move |_, key, _, _| {
        if key == gtk4::gdk::Key::Escape {
            entry.set_visible(false);
            stack.set_visible_child_name("path");
            glib::Propagation::Stop
        } else {
            glib::Propagation::Proceed
        }
    });
    tab.location_entry.add_controller(entry_controller);
    // Clicking the pathbar (empty area) offers the text entry.
    let entry = tab.location_entry.clone();
    let stack = path_stack.clone();
    let click = gtk4::GestureClick::new();
    click.connect_released(move |_, _, _, _| {
        stack.set_visible_child_name("entry");
        entry.set_visible(true);
        entry.grab_focus();
    });
    tab.pathbar.widget.add_controller(click);

    // Double-click / activation inside the file view: dirs navigate,
    // plain files open with their default application.
    let t = tab.clone();
    tab.file_view.connect_activate(move |file, is_dir| {
        if is_dir {
            t.navigate_to(file);
        } else {
            browser_tab::open_with_default(file);
        }
    });
}

/// Three-way view switcher (icons/list/compact) — writes back to
/// the view and GSettings. `buttons` matches ViewMode's order.
pub fn wire_view_switcher(tab: &BrowserTab, buttons: [&gtk4::ToggleButton; 3]) {
    use crate::file_view::ViewMode::*;
    const MODES: [crate::file_view::ViewMode; 3] = [Icons, List, Compact];
    // Seed the persisted mode — exactly one button is pressed.
    let mode = crate::file_view::ViewMode::from_str(&crate::settings::view_mode());
    let idx = MODES.iter().position(|m| *m == mode).unwrap_or(0);
    buttons[idx].set_active(true);
    for (i, b) in buttons.iter().enumerate() {
        let view = tab.file_view.clone();
        b.connect_toggled(move |b| {
            // Group buttons fire toggled on release too — act only
            // on the press.
            if b.is_active() {
                view.set_view_mode(MODES[i]);
                crate::settings::set_view_mode(MODES[i].as_str());
            }
        });
    }
}

/// Drop GTK's leaked implicit pointer-focus grab after a side-button
/// (8/9) sequence. `handle_pointing_event()` grabs the pointer focus on
/// every button press but only clears the grab when the released button
/// is 1-5, so a back/forward click leaks a grab that swallows the next
/// regular click. The only public API that forces gtkwindow to revoke
/// it is adding a widget grab on a widget that is not a descendant of
/// the stale target — a mapped GtkPopover does exactly that internally
/// (gtk_popover_map → gtk_grab_add → _gtk_window_group_add_grab →
/// gtk_window_maybe_revoke_implicit_grab).
///
/// The popover is parented to the TOPLEVEL, not the tab: the ancestor
/// check inside maybe_revoke rejects a revoke whenever the stale
/// target sits ABOVE the popover's parent chain — a press on empty
/// space can pick the tab widget itself (or higher), which made a
/// tab-parented popover silently useless and the window permanently
/// dead. With the window as parent the only stale target the trick
/// cannot cover is the window widget itself, which picking virtually
/// never returns.
///
/// The popover is invisible (1x1 px, fully transparent) and unmaps
/// itself on ::map, so the whole add+remove cycle is invisible and
/// finishes inside one frame.
fn revoke_stale_pointer_grab(tab: &BrowserTab) {
    thread_local! {
        // One popover per toplevel — a popover only revokes grabs in
        // its own window (maybe_revoke skips foci of other toplevels),
        // and a widget can have only one parent.
        static POPOVERS: std::cell::RefCell<
            std::collections::HashMap<usize, gtk4::Popover>,
        > = std::cell::RefCell::new(std::collections::HashMap::new());
    }
    POPOVERS.with(|slot| {
        let parent: gtk4::Widget = match tab.widget.root() {
            Some(root) => root.upcast(),
            // Not yet inside a toplevel — there cannot be a pointer
            // grab to revoke anyway.
            None => return,
        };
        let key = parent.as_ptr() as usize;
        let popover = slot
            .borrow_mut()
            .entry(key)
            .or_insert_with(|| {
                let pop = gtk4::Popover::new();
                pop.set_parent(&parent);
                pop.set_has_arrow(false);
                // NOTE: autohide must stay TRUE — gtk_popover_map only
                // calls gtk_grab_add() (the grab that revokes GTK's
                // leaked implicit grab) when autohide is enabled.
                pop.set_opacity(0.0);
                pop.set_pointing_to(Some(&gtk4::gdk::Rectangle::new(0, 0, 1, 1)));
                pop.set_child(Some(&gtk4::Box::new(gtk4::Orientation::Vertical, 0)));
                // gtk_grab_add runs inside gtk_popover_map — pop down
                // again immediately so no surface stays visible.
                pop.connect_map(|p| p.popdown());
                pop
            })
            .clone();
        popover.popup();
    });
}
