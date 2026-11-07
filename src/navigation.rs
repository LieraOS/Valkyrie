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
    let legacy = gtk4::EventControllerLegacy::new();
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
        if event.event_type() == gtk4::gdk::EventType::ButtonRelease {
            // Work around a GTK bug (present in 4.18 and still in main):
            // handle_pointing_event() creates an implicit pointer-focus
            // grab on EVERY button press, but only clears it when the
            // released button is 1-5 — GdkModifierType has no bits for
            // buttons 6+. A button-8/9 release therefore leaks the grab
            // and the next regular click is routed to the stale grab
            // target instead of the widget under the pointer — i.e.
            // swallowed. The only public lever to drop that grab is a
            // widget grab on a widget that is NOT an ancestor of the
            // stale target: _gtk_window_group_add_grab() revokes exactly
            // those implicit grabs. A mapped Popover adds a grab via
            // gtk_grab_add internally — ours is invisible (1px, opacity
            // 0) and pops itself down the instant it maps.
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
    tab.widget.add_controller(legacy);

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

/// Icon/list toolbar toggle — writes back to the view and GSettings.
pub fn wire_view_toggle(tab: &BrowserTab, toggle: &gtk4::ToggleButton) {
    let view = tab.file_view.clone();
    toggle.set_active(crate::settings::view_mode() == "icons");
    toggle.connect_toggled(move |b| {
        view.set_icon_mode(b.is_active());
        crate::settings::set_view_mode(if b.is_active() { "icons" } else { "list" });
    });
}

/// Drop GTK's leaked implicit pointer-focus grab after a side-button
/// (8/9) sequence. `handle_pointing_event()` grabs the pointer focus on
/// every button press but only clears the grab when the released button
/// is 1-5, so a back/forward click leaks a grab that swallows the next
/// regular click. The only public API that forces gtkwindow to revoke
/// it is adding a widget grab on a widget that is not an ancestor of
/// the stale target — a mapped GtkPopover does exactly that internally
/// (gtk_popover_map → gtk_grab_add → _gtk_window_group_add_grab →
/// gtk_window_maybe_revoke_implicit_grab). The popover is invisible
/// (1x1 px, fully transparent) and unmaps itself on ::map, so the whole
/// add+remove cycle is invisible and finishes inside one frame.
fn revoke_stale_pointer_grab(tab: &BrowserTab) {
    thread_local! {
        static POPOVER: std::cell::RefCell<Option<gtk4::Popover>> =
            const { std::cell::RefCell::new(None) };
    }
    POPOVER.with(|slot| {
        let popover = slot
            .borrow_mut()
            .get_or_insert_with(|| {
                let pop = gtk4::Popover::new();
                pop.set_parent(&tab.widget);
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
