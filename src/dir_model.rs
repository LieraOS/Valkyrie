// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// Live directory model: owns the ListStore behind the view pipeline
// and the file monitor that keeps it honest. Monitor events map onto
// the smallest possible store mutation:
//   * single-path events (created/changed/deleted/moved-out) refresh
//     just that row through a debounced query_info — the view
//     updates live without ever rebuilding the whole listing,
//   * moves and renames upsert both involved paths the same way,
//   * anything set-level (the dir itself, unmounts, unknowns)
//     coalesces into a trailing re-enumeration applied as a diff,
//   * a burst of distinct dirty paths escalates to that same diff
//     pass, so event storms degrade into a few whole-dir
//     enumerations instead of thousands of row queries.

mod diff;
mod enumerate;
mod row;
#[cfg(test)]
mod tests;

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gtk4::prelude::*;
use gtk4::{gio, glib};

/// Set-level events coalesce for this long before a re-enumeration.
const STRUCTURAL_DEBOUNCE: Duration = Duration::from_millis(300);
/// …but never longer: during a non-stop event storm the view still
/// refreshes about once per second.
const STRUCTURAL_MAX_STALE: Duration = Duration::from_secs(1);
/// Same shape for per-row updates — a file being written re-queries
/// once per quiet gap (paced copies stall regularly, so the size
/// column still advances visibly during a transfer).
const ROW_DEBOUNCE: Duration = Duration::from_millis(300);
const ROW_MAX_STALE: Duration = Duration::from_secs(1);
/// Above this many distinct dirty paths a structural refresh is
/// cheaper than hundreds of single-row queries.
const ROW_ESCALATION: usize = 64;

/// Shared driver state behind [`DirModel`]. Held in an Rc so the
/// monitor callback and pending timers reach it independently of
/// how many model clones exist.
pub(crate) struct State {
    /// Displayed directory — parent of every row's FileObject.
    pub location: gio::File,
    /// Rows of the displayed directory (flat — the pipeline's
    /// SortListModel orders them for display). Living inside State
    /// lets the trailing timers apply updates without needing the
    /// DirModel shell.
    pub store: gio::ListStore,
    /// An enumeration is in flight; a refresh asked for meanwhile
    /// marks refresh_dirty and queues one trailing pass.
    pub enum_busy: bool,
    pub refresh_dirty: bool,
    /// Stale enumeration results (from before a newer refresh) are
    /// dropped through this serial.
    pub generation: u64,
    /// Trailing structural timer plus the instant the set first got
    /// dirty (the staleness cap). Timers are spawn_local futures on
    /// the thread-default context so tests can run the model on a
    /// private context — glib's timeout_add_*_once family hard-wires
    /// the global default context.
    pub refresh_pending: Option<glib::JoinHandle<()>>,
    pub first_dirty: Option<Instant>,
    /// Per-path trailing row updates, keyed by file URI: timer task
    /// plus the instant the path first turned dirty.
    pub row_pending: HashMap<String, (glib::JoinHandle<()>, Instant)>,
    /// Enumeration-error listeners: fired with `Some(err)` when a
    /// listing fails for a directory that still exists (e.g. no read
    /// permission) and with `None` once a listing succeeds again —
    /// the banner in the tab tracks exactly this.
    pub error_notify: Vec<ErrorCallback>,
}

/// Listener fired on enumeration failure (`Some`) or recovery
/// (`None`) — see [`State::error_notify`].
pub(crate) type ErrorCallback = Box<dyn Fn(Option<&glib::Error>) + 'static>;

/// Live view of one directory. `store` feeds the ViewModel pipeline
/// directly; clones share the same backing store and driver state.
#[derive(Clone)]
pub struct DirModel {
    /// Rows of the displayed directory — the same ListStore `state`
    /// mutates, exposed for the model pipeline.
    pub store: gio::ListStore,
    pub(crate) state: Rc<RefCell<State>>,
    /// Held outside `state`: the monitor callback keeps an
    /// Rc<State>, so state→monitor→callback→state would leak the
    /// watch forever. Here the watch dies with the last clone — no
    /// new events can arrive afterwards and pending timers simply
    /// drain.
    _watch: Option<Rc<gio::FileMonitor>>,
}

impl DirModel {
    /// Build the store for `location`, install the directory monitor
    /// and start the initial enumeration.
    pub fn new(location: &gio::File) -> Self {
        let store = gio::ListStore::new::<crate::file_object::FileObject>();
        let state = Rc::new(RefCell::new(State {
            location: location.clone(),
            store: store.clone(),
            enum_busy: false,
            refresh_dirty: false,
            generation: 0,
            refresh_pending: None,
            first_dirty: None,
            row_pending: HashMap::new(),
            error_notify: Vec::new(),
        }));
        // WATCH_MOVES gives rename events real MovedIn/MovedOut/
        // Renamed semantics instead of plain create+delete pairs.
        let watch = location
            .monitor_directory(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE)
            .ok()
            .map(|monitor| {
                let st = state.clone();
                monitor.connect_changed(move |_, file, other, event| {
                    on_event(&st, file, other, event);
                });
                Rc::new(monitor)
            });
        let model = Self {
            store,
            state,
            _watch: watch,
        };
        model.refresh();
        model
    }

    /// Re-enumerate and apply a minimal diff — the path structural
    /// events and manual reloads take.
    pub fn refresh(&self) {
        enumerate::request(&self.state);
    }

    /// Register `f` for enumeration failures and recoveries — the
    /// tab shows/hides its error banner through this.
    pub fn connect_error_notify<F: Fn(Option<&glib::Error>) + 'static>(&self, f: F) {
        self.state.borrow_mut().error_notify.push(Box::new(f));
    }
}

/// Classify a monitor event. Anything touching a single path gets a
/// row-level refresh; set-level changes go structural.
fn on_event(
    state: &Rc<RefCell<State>>,
    file: &gio::File,
    other: Option<&gio::File>,
    event: gio::FileMonitorEvent,
) {
    use gio::FileMonitorEvent as E;
    match event {
        // A moved-in or renamed file: upsert the new name and let the
        // old one's query fail into a removal.
        E::MovedIn | E::Renamed => {
            row::schedule(state, file);
            if let Some(o) = other {
                row::schedule(state, o);
            }
        }
        // Everything that only mutates one path: upsert-or-remove by
        // re-querying just that file.
        E::Created
        | E::Deleted
        | E::Changed
        | E::AttributeChanged
        | E::ChangesDoneHint
        | E::MovedOut => row::schedule(state, file),
        // Set-level and unknown events (unmount, the dir itself being
        // moved, backend-specific extras): whole-dir diff.
        _ => structural_dirty(state),
    }
}

/// Mark the entry set dirty: coalesce the burst into one trailing
/// re-enumeration, capped at STRUCTURAL_MAX_STALE so a sustained
/// storm still produces a refresh about once a second. Row timers
/// are drained first — the enumeration covers them all.
fn structural_dirty(state: &Rc<RefCell<State>>) {
    let fire_now = {
        let mut s = state.borrow_mut();
        for (_, (task, _)) in s.row_pending.drain() {
            task.abort();
        }
        let now = Instant::now();
        let first = s.first_dirty.get_or_insert(now);
        let overdue = first.elapsed() >= STRUCTURAL_MAX_STALE;
        if let Some(task) = s.refresh_pending.take() {
            task.abort();
        }
        if overdue {
            s.first_dirty = None;
        } else {
            let st = state.clone();
            s.refresh_pending = Some(glib::MainContext::ref_thread_default().spawn_local(
                async move {
                    glib::timeout_future(STRUCTURAL_DEBOUNCE).await;
                    {
                        let mut s = st.borrow_mut();
                        s.refresh_pending = None;
                        s.first_dirty = None;
                    }
                    enumerate::request(&st);
                },
            ));
        }
        overdue
    };
    if fire_now {
        enumerate::request(state);
    }
}
