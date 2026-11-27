// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// Row-level updates: one debounced timer per touched path, firing an
// async query_info whose result upserts or removes exactly one row.
// This is the hot path while a file is being written (copy targets,
// downloads, logs): the rest of the directory — and the main loop —
// stays untouched, so nothing flickers and nothing starves.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

use gtk4::prelude::*;
use gtk4::{gio, glib};

use super::{State, ROW_DEBOUNCE, ROW_MAX_STALE};
use crate::file_object::FileObject;
use crate::view_model::ATTRIBUTES;

/// Queue a trailing refresh for `file`'s row. Repeated events for
/// the same path re-arm the timer; once the path sat dirty for
/// ROW_MAX_STALE the refresh fires immediately, so a continuously
/// written file still ticks about once a second. A burst over many
/// distinct paths escalates to a structural refresh instead.
pub fn schedule(state: &Rc<RefCell<State>>, file: &gio::File) {
    let key = file.uri().to_string();
    let now = Instant::now();
    let mut fire_now = false;
    {
        let mut s = state.borrow_mut();
        match s.row_pending.remove(&key) {
            // Freshly dirty path: arm a new trailing timer — unless
            // the burst has already grown past the escalation point.
            None => {
                if s.row_pending.len() >= super::ROW_ESCALATION {
                    drop(s);
                    super::structural_dirty(state);
                    return;
                }
                s.row_pending.insert(key, (arm(state, file), now));
            }
            // Already dirty: re-arm for another quiet gap, or fire at
            // once when the staleness cap is reached.
            Some((task, first)) => {
                task.abort();
                if first.elapsed() >= ROW_MAX_STALE {
                    fire_now = true;
                } else {
                    s.row_pending.insert(key, (arm(state, file), first));
                }
            }
        }
    }
    if fire_now {
        fire(state, file);
    }
}

/// One trailing timer task: sleeps ROW_DEBOUNCE, then drops its
/// row_pending slot and runs the targeted refresh.
fn arm(state: &Rc<RefCell<State>>, file: &gio::File) -> glib::JoinHandle<()> {
    let st = state.clone();
    let file = file.clone();
    let key = file.uri().to_string();
    glib::MainContext::ref_thread_default().spawn_local(async move {
        glib::timeout_future(ROW_DEBOUNCE).await;
        st.borrow_mut().row_pending.remove(&key);
        fire(&st, &file);
    })
}

/// The timer body: one async metadata query per path. Runs on the
/// thread-default main context so tests can drive it with their own.
fn fire(state: &Rc<RefCell<State>>, file: &gio::File) {
    let st = state.clone();
    let file = file.clone();
    glib::MainContext::ref_thread_default().spawn_local(async move {
        match file
            .query_info_future(
                ATTRIBUTES,
                gio::FileQueryInfoFlags::NONE,
                glib::Priority::DEFAULT,
            )
            .await
        {
            Ok(info) => {
                let (store, location) = {
                    let s = st.borrow();
                    (s.store.clone(), s.location.clone())
                };
                upsert(&store, &location, &file, &info);
            }
            // Gone → drop the row (covers deletes and move-outs).
            Err(_) => {
                let store = st.borrow().store.clone();
                remove(&store, &file);
            }
        }
    });
}

/// Insert or update the row for `file`. Rows are matched by raw
/// filename — the store is flat, so a basename compare is exact.
fn upsert(store: &gio::ListStore, parent: &gio::File, file: &gio::File, info: &gio::FileInfo) {
    let name = file
        .basename()
        .map(|b| b.to_string_lossy().into_owned())
        .unwrap_or_default();
    match find_row(store, &name) {
        Some((pos, fo)) => {
            if !fo.same_state(info) {
                fo.set_info(info);
                store.items_changed(pos, 1, 1);
            }
        }
        None => store.append(&FileObject::new(&parent.child(&name), Some(info))),
    }
}

/// Remove the row for `file` if it is still in the store.
fn remove(store: &gio::ListStore, file: &gio::File) {
    let name = file
        .basename()
        .map(|b| b.to_string_lossy().into_owned())
        .unwrap_or_default();
    if let Some((pos, _)) = find_row(store, &name) {
        store.remove(pos);
    }
}

/// Linear scan by raw name — fine for the per-path timer (each path
/// pays O(n) once per quiet gap, not per monitor event).
fn find_row(store: &gio::ListStore, name: &str) -> Option<(u32, FileObject)> {
    for pos in 0..store.n_items() {
        let Some(fo) = store
            .item(pos)
            .and_then(|o| o.downcast::<FileObject>().ok())
        else {
            continue;
        };
        if fo.raw_name() == name {
            return Some((pos, fo));
        }
    }
    None
}
