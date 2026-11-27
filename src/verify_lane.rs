// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// Background verification lane for copy jobs. The destination
// re-hash used to run inline after every file — on slow media it
// re-read the data back from the device while the copy loop sat
// idle and the progress bar froze. Now each copied file is queued
// here and hashed by a dedicated thread while the next file is
// already copying; results stream to the dialog as they land.
// `wait` lets the batch end block until every queued file is
// verified, so "finished" still means fully verified.

use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex};

use gtk4::gio;
use gtk4::prelude::*;

use crate::copy_pool::VerifyReportFn;
use crate::tree_ops::VerifyResult;

/// One queued verification: re-hash the destination and compare it
/// against the digest collected inline during the copy stream.
struct VerifyJob {
    name: String,
    dst: PathBuf,
    src_hash: Option<blake3::Hash>,
}

/// Shared handle: producers call `submit`, the owning thread calls
/// `wait` once at batch end. The worker thread exits when all
/// senders drop — nothing needs joining.
pub struct VerifyLane {
    tx: Mutex<Option<mpsc::Sender<VerifyJob>>>,
    pending: Arc<(Mutex<usize>, Condvar)>,
}

impl VerifyLane {
    /// Spawn the lane thread. `report` receives per-file outcomes on
    /// whatever thread the lane runs — callers already marshal them
    /// into the UI message channel. `cancel` stops hashing fast on
    /// cancel: queued jobs are drained without hashing.
    pub fn new(report: VerifyReportFn, cancel: gio::Cancellable) -> Self {
        let (tx, rx) = mpsc::channel::<VerifyJob>();
        let pending = Arc::new((Mutex::new(0usize), Condvar::new()));
        let pending_w = pending.clone();
        std::thread::spawn(move || {
            crate::job_priority::background();
            while let Ok(job) = rx.recv() {
                // A cancel during the copy aborts the whole batch;
                // verifying leftovers would only stall shutdown.
                if !cancel.is_cancelled() {
                    let result =
                        match (job.src_hash, crate::fast_copy::hash_file(&job.dst, &cancel)) {
                            (Some(a), Ok(b)) if a == b => VerifyResult::Verified,
                            (Some(_), Ok(_)) => VerifyResult::Mismatch,
                            _ => VerifyResult::Unverifiable,
                        };
                    if !cancel.is_cancelled() {
                        report(&job.name, result);
                    }
                }
                let (lock, cv) = &*pending_w;
                if let Ok(mut p) = lock.lock() {
                    *p = p.saturating_sub(1);
                    cv.notify_all();
                }
            }
        });
        Self {
            tx: Mutex::new(Some(tx)),
            pending,
        }
    }

    /// Queue a copied file for background re-hashing. `src_hash` is
    /// the digest gathered inline during the copy stream — the lane
    /// only re-reads the destination.
    pub fn submit(&self, name: String, dst: PathBuf, src_hash: Option<blake3::Hash>) {
        let Ok(guard) = self.tx.lock() else {
            return;
        };
        if let Some(tx) = guard.as_ref() {
            let (lock, _) = &*self.pending;
            if let Ok(mut p) = lock.lock() {
                *p += 1;
            }
            let _ = tx.send(VerifyJob {
                name,
                dst,
                src_hash,
            });
        }
    }

    /// Block until every queued file has been verified. Called once
    /// at batch end, after the final filesystem flush — the verify
    /// results keep streaming to the dialog while it waits, so the
    /// tail is visible instead of a silent stall.
    pub fn wait(&self) {
        let (lock, cv) = &*self.pending;
        if let Ok(mut p) = lock.lock() {
            while *p > 0 {
                match cv.wait(p) {
                    Ok(guard) => p = guard,
                    Err(_) => break,
                }
            }
        }
    }
}

impl Drop for VerifyLane {
    /// Detach the worker: closing the channel ends its recv loop —
    /// nothing joins it, shutdown never waits on hashing.
    fn drop(&mut self) {
        if let Ok(mut guard) = self.tx.lock() {
            guard.take();
        }
    }
}

#[cfg(test)]
mod tests;
