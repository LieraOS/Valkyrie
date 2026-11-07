// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// Schedule drivers for synced folder pairs. Each pair runs on its own
// clock as configured in the preferences: an interval timer re-syncs
// periodically, a GFileMonitor on the local tree triggers a
// (debounced) sync on every change. rebuild() is called at startup
// and whenever the sync-folder settings change; dropping the old
// watches first keeps the map a faithful mirror of the settings.
//
// All scheduled syncs run quiet — background work must never pop a
// progress window in the user's face.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use gtk4::prelude::*;
use gtk4::{gio, glib};

use super::pairs::Schedule;
use super::sync;

/// Debounce for live watch: bursts of filesystem events (a big save
/// produces many) collapse into one sync run.
const LIVE_DEBOUNCE: Duration = Duration::from_secs(5);

/// One running schedule — keep the SourceIds/Monitors alive or they
/// cancel on drop.
enum Watch {
    Interval(glib::SourceId),
    Live {
        monitor: gio::FileMonitor,
        pending: Rc<RefCell<Option<glib::SourceId>>>,
    },
}

thread_local! {
    static WATCHES: RefCell<HashMap<String, Watch>> = RefCell::new(HashMap::new());
    /// Kept alive so the settings-changed handler survives.
    static SETTINGS: RefCell<Option<gio::Settings>> = const { RefCell::new(None) };
}

/// Tear one watch down: SourceIds are not RAII, so timers and the
/// pending debounce must be removed explicitly — dropping them
/// silently would keep firing against stale settings.
fn teardown(watch: Watch) {
    match watch {
        Watch::Interval(id) => id.remove(),
        Watch::Live { monitor, pending } => {
            if let Some(id) = pending.borrow_mut().take() {
                id.remove();
            }
            monitor.cancel();
        }
    }
}

/// (Re)build every watch from the current settings. Called at startup
/// and from the settings-changed hook installed in start().
pub fn rebuild() {
    WATCHES.with(|w| {
        for (_, watch) in w.borrow_mut().drain() {
            teardown(watch);
        }
    });
    for pair in super::pairs::all() {
        match pair.schedule {
            Schedule::Manual => {}
            Schedule::Interval(minutes) => {
                let key = pair.key();
                let pair = pair.clone();
                let id = glib::timeout_add_local(
                    Duration::from_secs(u64::from(minutes) * 60),
                    move || {
                        // Resolve the dialog anchor at fire time — the
                        // first rebuild runs before the window exists,
                        // so capturing it here would silently install
                        // no watch at all.
                        if let Some(anchor) = anchor_widget() {
                            sync::sync_pair(&anchor, &pair, true);
                        }
                        glib::ControlFlow::Continue
                    },
                );
                WATCHES.with(|w| w.borrow_mut().insert(key, Watch::Interval(id)));
            }
            Schedule::Live => {
                let key = pair.key();
                let file = gio::File::for_path(&pair.local);
                let Ok(monitor) = file
                    .monitor_directory(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE)
                else {
                    continue;
                };
                let pending: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));
                let pair = pair.clone();
                let pending2 = pending.clone();
                monitor.connect_changed(move |_, _, _, _| {
                    // Collapse the event burst: push the pending run
                    // back by the debounce window on every event.
                    if let Some(id) = pending2.borrow_mut().take() {
                        id.remove();
                    }
                    let pair = pair.clone();
                    let pending3 = pending2.clone();
                    *pending2.borrow_mut() =
                        Some(glib::timeout_add_local_once(LIVE_DEBOUNCE, move || {
                            *pending3.borrow_mut() = None;
                            // Anchor at fire time — see Interval arm.
                            if let Some(anchor) = anchor_widget() {
                                sync::sync_pair(&anchor, &pair, true);
                            }
                        }));
                });
                WATCHES.with(|w| w.borrow_mut().insert(key, Watch::Live { monitor, pending }));
            }
        }
    }
}

/// Install the settings-change hook once and build the first set of
/// watches. Called from app startup.
pub fn start() {
    SETTINGS.with(|slot| {
        if slot.borrow().is_some() {
            return;
        }
        *slot.borrow_mut() = crate::settings::connect_nextcloud_changed(rebuild);
    });
    rebuild();
}

/// Any widget of a live window as the progress/error dialog parent —
/// the first toplevel found works since the window is transient anyway.
fn anchor_widget() -> Option<gtk4::Widget> {
    gtk4::Window::list_toplevels()
        .first()
        .and_then(|o| o.downcast_ref::<gtk4::Window>())
        .cloned()
        .map(|w| w.upcast::<gtk4::Widget>())
}
