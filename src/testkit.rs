// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// Shared test scaffolding for display-dependent tests. GTK objects
// (DirectoryList, SortListModel, CustomFilter, …) may only be used
// on the thread that ran gtk4::init() — while the cargo test harness
// distributes tests across an unpredictable pool. The single worker
// thread below owns GTK for the whole process: tests post closures
// through the channel, the worker runs them sequentially on the GTK
// thread. Tests that only touch pure GIO types (FileMonitor,
// ListStore, FileInfo) need no display and run anywhere.

use std::sync::mpsc;
use std::sync::OnceLock;

/// Result of the one-time GTK init on the worker thread.
static READY: OnceLock<bool> = OnceLock::new();
/// Job queue into the GTK worker thread.
static JOBS: OnceLock<mpsc::SyncSender<Job>> = OnceLock::new();

type Job = Box<dyn FnOnce() + Send>;

/// Spawn the GTK worker once, reporting whether init succeeded
/// (false on display-less runners — callers then skip).
fn ensure() -> bool {
    *READY.get_or_init(|| {
        let (tx, rx) = mpsc::sync_channel::<Job>(8);
        let (itx, irx) = mpsc::sync_channel::<bool>(1);
        std::thread::Builder::new()
            .name("gtk-test".into())
            .spawn(move || {
                let ok = gtk4::init().is_ok();
                let _ = itx.send(ok);
                while ok {
                    match rx.recv() {
                        Ok(job) => job(),
                        Err(_) => break,
                    }
                }
            })
            .expect("spawn gtk test thread");
        JOBS.set(tx).ok();
        irx.recv().unwrap_or(false)
    })
}

/// Run `f` on the single GTK-capable thread and return its result;
/// `None` when no display is available (tests skip then). Callers
/// may rely on serialization — every job lands on this one thread.
pub fn on_gtk<R: Send + 'static>(f: impl FnOnce() -> R + Send + 'static) -> Option<R> {
    if !ensure() {
        return None;
    }
    let (tx, rx) = mpsc::sync_channel::<R>(1);
    JOBS.get()?
        .send(Box::new(move || {
            let _ = tx.send(f());
        }))
        .ok()?;
    rx.recv().ok()
}
