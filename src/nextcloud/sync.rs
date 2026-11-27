// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// Sync orchestration: sync_pair() is the single entry point used by
// the context menu ("Sync now"), the interval timers and the live
// file monitor. It mounts the cloud, then runs a worker that lists
// both trees (exec.rs), asks plan.rs for the operation list, applies
// it and journals the result through state.rs — the journal is what
// lets a later run tell "deleted" apart from "newly appeared".
//
// The journal is rebuilt AFTER the ops by re-listing both trees:
// the engine records the fingerprints actually observed on each side
// rather than the values it hoped to write. This matters because
// WebDAV does not reliably preserve mtimes — journaled expectations
// would look like permanent remote-side edits and re-conflict on
// every run.
//
// `quiet` runs (timers, live watch) work without a progress window —
// background sync must not pop UI; a failed quiet run is retried on
// the next trigger instead of nagging.

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

use gtk4::gio;
use gtk4::prelude::*;

use super::exec;
use super::pairs::SyncPair;
use super::plan;
use super::state;
use crate::i18n::tr;
use crate::progress;

/// Pairs currently syncing — guards against a live-watch event or an
/// interval tick firing while a manual run is still on the wire.
static IN_FLIGHT: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

fn in_flight() -> &'static Mutex<HashSet<String>> {
    IN_FLIGHT.get_or_init(|| Mutex::new(HashSet::new()))
}

fn finish_in_flight(key: &str) {
    in_flight()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(key);
}

/// Re-list both trees and persist the result as the new journal.
/// Runs even after a failed op or a cancel — recording the real
/// post-run state keeps the retry from re-flagging applied ops.
fn journal_now(key: &str, local_root: &gio::File, remote_root: &gio::File) {
    let fresh = gio::Cancellable::new();
    let mut post_local = Vec::new();
    let mut post_remote = Vec::new();
    exec::list(local_root, "", &mut post_local, &fresh);
    exec::list(remote_root, "", &mut post_remote, &fresh);
    state::save(key, &state::journal_from_trees(&post_local, &post_remote));
}

/// Run one pair. Manual runs show the shared progress window; quiet
/// runs do the identical work silently and only remember the
/// outcome in the journal.
pub fn sync_pair(parent: &gtk4::Widget, pair: &SyncPair, quiet: bool) {
    let key = pair.key();
    {
        let mut guard = in_flight().lock().unwrap_or_else(|e| e.into_inner());
        if !guard.insert(key.clone()) {
            return; // already syncing this pair
        }
    }
    let pair = pair.clone();
    let parent = parent.clone();
    let key_for_fail = key.clone();
    super::mount_and_use_result(
        &parent.clone(),
        move |remote_root| {
            // Clone for the worker: the closure is Fn and `key` must
            // stay usable for the fail path below.
            let key = key.clone();
            let remote_root = remote_root.clone();
            let pair = pair.clone();
            let (tx, cancel) = if quiet {
                (None, gio::Cancellable::new())
            } else {
                let (tx, c) = progress::spawn_window(&parent, &tr("Synchronizing…"));
                (Some(tx), c)
            };
            std::thread::spawn(move || {
                crate::job_priority::background();
                let send = |msg: progress::Msg| {
                    if let Some(tx) = &tx {
                        let _ = tx.send(msg);
                    }
                };

                let local_root = gio::File::for_path(&pair.local);
                let mut local = Vec::new();
                let mut remote = Vec::new();
                exec::list(&local_root, "", &mut local, &cancel);
                exec::list(&remote_root, "", &mut remote, &cancel);
                let prev = state::load(&key);
                let ops = plan::plan(&local, &remote, &prev, pair.direction);

                let total = ops.len().max(1) as i64;
                let mut count = 0i64;
                for op in &ops {
                    if cancel.is_cancelled() {
                        break;
                    }
                    let result = exec::execute(op, &pair, &remote_root, &cancel, &mut |p| {
                        count += 1;
                        send(progress::Msg::Progress {
                            name: p.to_string(),
                            done: count,
                            total,
                        });
                    });
                    if let Err(e) = result {
                        journal_now(&key, &local_root, &remote_root);
                        send(progress::Msg::Failed(format!("{e}")));
                        finish_in_flight(&key);
                        return;
                    }
                }
                journal_now(&key, &local_root, &remote_root);
                send(progress::Msg::Done);
                finish_in_flight(&key);
            });
        },
        move || finish_in_flight(&key_for_fail),
    );
}

/// Manual trigger from the context menu / settings "Sync Now".
pub fn sync_now(parent: &gtk4::Widget, pair: &SyncPair) {
    sync_pair(parent, pair, false);
}
