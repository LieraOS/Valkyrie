// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// Split view plumbing for BrowserTab: the lazy secondary PaneView, the
// active-pane tracking and the sibling-location registry that lets a
// context menu offer "Copy/Move to Other Pane".

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use gtk4::prelude::*;
use gtk4::{gio, glib};

use super::BrowserTab;
use crate::file_view::FileView;
use crate::pane_view::PaneView;

/// Identifies which pane currently owns tab-level operations
/// (navigate, clipboard ops, zoom, search, status).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActivePane {
    Primary,
    Secondary,
}

// Sibling registry: FileView identity -> closure resolving the other
// pane's current location. Registered when a tab splits; the Weak keeps
// stale entries from outliving a destroyed tab.
type SiblingSource = dyn Fn() -> Option<gio::File>;

thread_local! {
    static SIBLINGS: RefCell<Vec<(usize, Weak<SiblingSource>)>> =
        const { RefCell::new(Vec::new()) };
}

fn view_key(view: &FileView) -> usize {
    // The location cell is stable for the FileView's lifetime — the Rc
    // pointer is a cheap unique identity without touching the view.
    Rc::as_ptr(&view.location) as usize
}

fn register_sibling(view: &FileView, source: Rc<SiblingSource>) {
    let key = view_key(view);
    SIBLINGS.with(|m| {
        let mut m = m.borrow_mut();
        m.retain(|(k, w)| *k != key && w.upgrade().is_some());
        m.push((key, Rc::downgrade(&source)));
    });
}

/// The other pane's location for `view`, or None when the owning tab is
/// not split. Called by the row context menu.
pub(crate) fn other_location_for(view: &FileView) -> Option<gio::File> {
    let key = view_key(view);
    SIBLINGS.with(|m| {
        m.borrow_mut().retain(|(_, w)| w.upgrade().is_some());
        m.borrow()
            .iter()
            .find(|(k, _)| *k == key)
            .and_then(|(_, w)| w.upgrade())
            .and_then(|f| f())
    })
}

impl BrowserTab {
    /// The FileView of the currently focused pane — single-pane tabs
    /// always resolve to the primary view.
    pub(crate) fn active_view(&self) -> FileView {
        self.secondary_if_active()
            .map(|p| p.file_view.clone())
            .unwrap_or_else(|| self.file_view.clone())
    }

    /// The secondary pane, but only while it exists and is focused.
    pub(crate) fn secondary_if_active(&self) -> Option<Rc<PaneView>> {
        if self.active_pane.get() != ActivePane::Secondary {
            return None;
        }
        self.secondary.borrow().clone()
    }

    /// Run `f` on the primary view and — while split — the secondary's.
    /// Shared view options (mode, sort, hidden, icon size) go through
    /// this so both panes stay consistent.
    pub(crate) fn for_each_view(&self, f: impl Fn(&FileView)) {
        f(&self.file_view);
        if let Some(pane) = self.secondary.borrow().as_ref() {
            f(&pane.file_view);
        }
    }

    /// Current split state — the View menu's checkbox reads this.
    pub fn is_split(&self) -> bool {
        self.secondary.borrow().is_some()
    }

    /// Location of the pane opposite the focused one — destination for
    /// "Copy/Move to Other Pane".
    pub(crate) fn other_pane_location(&self) -> Option<gio::File> {
        let pane = self.secondary.borrow().clone()?;
        match self.active_pane.get() {
            ActivePane::Primary => pane.location(),
            ActivePane::Secondary => self.file_view.location(),
        }
    }

    /// Split this tab or collapse it back to one pane. Splitting lazily
    /// creates the secondary pane at the primary's current directory.
    pub fn set_split(&self, on: bool) {
        if on == self.is_split() {
            return;
        }
        if !on {
            self.secondary.replace(None);
            self.split_paned.set_end_child(None::<&gtk4::Widget>);
            self.active_pane.set(ActivePane::Primary);
            self.refresh_after_pane_change();
            return;
        }

        let start = self
            .location()
            .unwrap_or_else(|| gio::File::for_path(glib::home_dir()));
        let pane = PaneView::new(&start);

        // Primary view interactions mark the primary pane active.
        let tab = self.clone();
        self.file_view.connect_selection_notify(move || {
            tab.active_pane.set(ActivePane::Primary);
            tab.update_status();
            tab.update_nav_sensitivity();
        });

        // Secondary marks itself active on any selection change and
        // keeps status/nav in step; navigation in it refreshes the
        // shared widgets.
        let tab = self.clone();
        pane.file_view.connect_selection_notify(move || {
            tab.active_pane.set(ActivePane::Secondary);
            tab.update_status();
            tab.update_nav_sensitivity();
        });
        let tab = self.clone();
        pane.file_view.connect_count_notify(move |_| {
            tab.update_status();
        });
        let tab = self.clone();
        pane.connect_nav_notify(move || {
            tab.refresh_after_pane_change();
        });

        // "Open in New Tab/Window" inside the secondary pane forwards
        // through the primary view's handler list — the window rewires
        // that list on adoption, so the secondary needs no rewiring.
        let primary = self.file_view.clone();
        pane.file_view.connect_open_in_tab(move |f| {
            primary.emit_open_in_tab(f);
        });
        let primary = self.file_view.clone();
        pane.file_view.connect_open_in_window(move |f| {
            primary.emit_open_in_window(f);
        });

        // Mirror the shared view options into the new pane.
        pane.file_view.set_view_mode(self.file_view.view_mode());
        pane.file_view.set_icon_size(self.file_view.icon_size());
        pane.file_view.set_sort(
            self.file_view.sort_field(),
            self.file_view.sort_descending(),
        );
        pane.file_view.set_show_hidden(self.file_view.show_hidden());

        // Sibling sources for the cross-pane context-menu entries.
        let other = pane.clone();
        register_sibling(&self.file_view, Rc::new(move || other.location()));
        let primary_view = self.file_view.clone();
        register_sibling(&pane.file_view, Rc::new(move || primary_view.location()));

        self.split_paned.set_end_child(Some(&pane.widget));
        // First split: share the width evenly.
        let w = self.split_paned.allocated_width();
        if w > 0 {
            self.split_paned.set_position(w / 2);
        }
        self.secondary.replace(Some(pane));
        self.refresh_after_pane_change();
    }

    /// Recompute title/status/nav buttons — called after pane switches
    /// and after navigation inside the secondary pane.
    pub(crate) fn refresh_after_pane_change(&self) {
        self.update_title();
        self.update_nav_sensitivity();
        self.update_status();
    }
}
