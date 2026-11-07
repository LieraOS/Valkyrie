// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// Handoff for tabs dragged between Valkyrie windows. A GtkNotebook
// drag moves only the page widget: the BrowserTab object (view,
// model, title logic) lives in the sending window's tab vec and
// would die with it. The sender therefore stashes the BrowserTab
// here on page-removed, and the receiver claims it on page-added —
// so the tab keeps working in whichever window it lands in.

use std::cell::RefCell;

use gtk4::prelude::*;

use crate::browser_tab::BrowserTab;

thread_local! {
    /// (page widget, its BrowserTab) for tabs in flight between
    /// windows. Entries exist only between the two notebook signals
    /// of one drag; stale entries are pruned on every access.
    static IN_FLIGHT: RefCell<Vec<(gtk4::Widget, BrowserTab)>> =
        const { RefCell::new(Vec::new()) };
}

/// Prune entries whose page no longer lives in any window root —
/// these are tabs closed (not dragged) while stashed, never claimed.
/// `keep` is protected (it may be unrooted mid-adoption).
fn prune_keep(keep: &gtk4::Widget) {
    IN_FLIGHT.with(|r| {
        r.borrow_mut()
            .retain(|(w, _)| w == keep || w.root().is_some());
    });
}

/// Stash a departing page's tab so the window receiving the drag can
/// adopt it (see [`take_in_flight`]).
pub fn stash_in_flight(page: &gtk4::Widget, tab: BrowserTab) {
    IN_FLIGHT.with(|r| {
        prune_keep(page);
        r.borrow_mut().push((page.clone(), tab));
    });
}

/// Claim the BrowserTab for a page that just arrived in a notebook.
/// Returns None for pages this registry never held (freshly built
/// tabs are pushed by their window's own open path, not a drag).
pub fn take_in_flight(page: &gtk4::Widget) -> Option<BrowserTab> {
    IN_FLIGHT.with(|r| {
        prune_keep(page);
        let mut v = r.borrow_mut();
        let pos = v.iter().position(|(w, _)| w == page)?;
        Some(v.remove(pos).1)
    })
}
