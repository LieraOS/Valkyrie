// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// Parallel file-payload pool for copy batches. The tree walk itself
// (directory creation, enumeration, collision resolution) stays
// sequential in tree_ops — only the expensive byte movement of
// regular files is handed to N workers pulling from a shared FIFO.
// That ordering guarantees a destination directory exists before a
// job referencing it runs. Workers share one atomic byte counter
// for progress, sample the verify flag per job (mid-operation
// toggles work), and each runs the same reflink-or-stream fast path
// the sequential code uses — nothing about copying is duplicated.

use std::collections::VecDeque;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use gtk4::gio;
use gtk4::prelude::*;

use crate::fast_copy::{self, CopyOutcome};
use crate::tree_ops::VerifyResult;

/// Shared flag the dialog checkbox toggles; sampled per job.
pub type VerifyFlag = Arc<AtomicBool>;

/// Byte-progress sink: (current entry name, cumulative batch bytes,
/// workers currently copying) — the third value lets the dialog show
/// real parallelism instead of a single-name flicker.
pub type ReportFn = Arc<dyn Fn(&str, i64, u32) + Send + Sync>;

/// Per-file verify outcome sink.
pub type VerifyReportFn = Arc<dyn Fn(&str, VerifyResult) + Send + Sync>;

/// Everything a worker needs — bundled so `run` stays readable.
pub struct PoolCfg {
    pub cancel: gio::Cancellable,
    /// Batch-wide byte counter shared with the sequential walker.
    pub done: Arc<AtomicI64>,
    /// Integrity toggle (None = verification off for the whole job).
    pub verify_flag: Option<VerifyFlag>,
    /// (current entry name, cumulative batch bytes) → progress.
    pub report: ReportFn,
    /// Per-file verify outcome channel (None when no verify flag).
    pub verify_report: Option<VerifyReportFn>,
    /// Background verify lane — workers queue freshly copied files
    /// instead of blocking on the destination re-hash. When absent,
    /// verification runs inline (legacy path used by tests).
    pub verify_lane: Option<Arc<crate::verify_lane::VerifyLane>>,
    /// Removable-target mode: workers pace their writes so progress
    /// reflects real device speed on slow media.
    pub paced: bool,
}

/// One payload copy: local paths only — GVFS locations keep the
/// sequential gio path because no fast syscall exists for them.
struct Job {
    src: PathBuf,
    dst: PathBuf,
    name: String,
}

/// Queue plus bookkeeping shared between producer and workers.
struct Shared {
    deque: Mutex<VecDeque<Job>>,
    cv: Condvar,
    closed: AtomicBool,
    err: Mutex<Option<String>>,
    /// Workers inside `process` right now — reported to the UI so
    /// parallel copying is visible, not just internal.
    active: AtomicI64,
}

/// Producer handle: the sequential walk submits file jobs here.
pub struct CopyPool {
    shared: Arc<Shared>,
}

impl CopyPool {
    /// Enqueue a file copy for the next free worker.
    pub fn submit(&self, src: PathBuf, dst: PathBuf, name: String) {
        if let Ok(mut q) = self.shared.deque.lock() {
            q.push_back(Job { src, dst, name });
        }
        self.shared.cv.notify_one();
    }
}

/// Run `body` with a live pool of `workers` threads, then shut it
/// down: the queue is closed, remaining jobs finish, workers join.
/// Returns the body's result plus the first worker error, if any.
pub fn run<R>(
    workers: usize,
    cfg: PoolCfg,
    body: impl FnOnce(&CopyPool) -> R,
) -> (R, Option<String>) {
    let shared = Arc::new(Shared {
        deque: Mutex::new(VecDeque::new()),
        cv: Condvar::new(),
        closed: AtomicBool::new(false),
        err: Mutex::new(None),
        active: AtomicI64::new(0),
    });
    let cfg = Arc::new(cfg);
    let pool = CopyPool {
        shared: shared.clone(),
    };
    let err_after;
    let result = std::thread::scope(|s| {
        for _ in 0..workers.max(1) {
            let shared = shared.clone();
            let cfg = cfg.clone();
            s.spawn(move || worker_loop(shared, cfg));
        }
        let r = body(&pool);
        pool.shared.closed.store(true, Ordering::Release);
        pool.shared.cv.notify_all();
        // Scoped threads join here — every queued job is done.
        r
    });
    if let Ok(err) = shared.err.lock() {
        err_after = err.clone();
    } else {
        err_after = None;
    }
    (result, err_after)
}

/// Pop jobs until the queue is closed and empty; copy each through
/// the shared fast path. Cancelling the operation or a sibling
/// worker's error ends the loop.
fn worker_loop(shared: Arc<Shared>, cfg: Arc<PoolCfg>) {
    loop {
        let job = {
            let Ok(mut q) = shared.deque.lock() else {
                return;
            };
            loop {
                if let Some(j) = q.pop_front() {
                    break j;
                }
                if shared.closed.load(Ordering::Acquire) {
                    return;
                }
                match shared.cv.wait(q) {
                    Ok(guard) => q = guard,
                    Err(_) => return,
                }
            }
        };
        if cfg.cancel.is_cancelled() || shared_has_error(&shared) {
            return;
        }
        process(&shared, &cfg, job);
    }
}

fn shared_has_error(shared: &Shared) -> bool {
    shared.err.lock().map(|e| e.is_some()).unwrap_or(false)
}

/// Copy one file: chunk-wise byte accounting into the shared counter
/// (so the bar stays smooth on big files), then the optional verify
/// pass — destination re-hash only, the source digest already came
/// free inside the copy stream.
fn process(shared: &Arc<Shared>, cfg: &Arc<PoolCfg>, job: Job) {
    let want = cfg
        .verify_flag
        .as_ref()
        .is_some_and(|f| f.load(Ordering::Acquire));
    shared.active.fetch_add(1, Ordering::Relaxed);
    let shared_r = shared.clone();
    let mut file_done = 0u64;
    let name = job.name;
    let outcome = fast_copy::copy(
        &job.src,
        &job.dst,
        want,
        cfg.paced,
        &cfg.cancel,
        &mut |cur| {
            let delta = cur - file_done;
            file_done = cur;
            let global = cfg.done.fetch_add(delta as i64, Ordering::Relaxed) + delta as i64;
            (cfg.report)(
                &name,
                global,
                shared_r.active.load(Ordering::Relaxed).max(1) as u32,
            );
        },
    );
    shared.active.fetch_sub(1, Ordering::Relaxed);
    match outcome {
        Ok(outcome) => {
            if !want {
                return;
            }
            match outcome {
                CopyOutcome::Reflinked => {
                    if let Some(report) = &cfg.verify_report {
                        report(&name, VerifyResult::Reflinked);
                    }
                }
                CopyOutcome::Copied(src_hash) => {
                    // Lane present: the destination re-hash runs on
                    // the background thread — this worker takes the
                    // next copy job immediately instead of stalling
                    // the bar on a device re-read.
                    if let Some(lane) = &cfg.verify_lane {
                        lane.submit(name, job.dst, src_hash);
                    } else {
                        let result = match (src_hash, fast_copy::hash_file(&job.dst, &cfg.cancel)) {
                            (Some(a), Ok(b)) if a == b => VerifyResult::Verified,
                            (Some(_), Ok(_)) => VerifyResult::Mismatch,
                            _ => VerifyResult::Unverifiable,
                        };
                        if let Some(report) = &cfg.verify_report {
                            report(&name, result);
                        }
                    }
                }
            }
        }
        Err(e) => {
            // A user cancel surfaces as Interrupted — that is a normal
            // shutdown path, not an error worth reporting.
            if !cfg.cancel.is_cancelled() && e.kind() != io::ErrorKind::Interrupted {
                if let Ok(mut slot) = shared.err.lock() {
                    if slot.is_none() {
                        *slot = Some(e.to_string());
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;

/// `syncfs` on the filesystem containing `path` — flushes the whole
/// destination filesystem (directory entries, reflink extents) so a
/// completed copy is truly durable, not just in the write-back
/// cache. Runs once per batch from jobs.rs: called per job it would
/// serialise pool workers on each other's pending write-back.
/// Path-less (GVFS) destinations are skipped by the caller.
pub(crate) fn flush_fs(path: &std::path::Path) {
    let Some(parent) = path.parent() else { return };
    let Ok(dir) = std::fs::File::open(parent) else {
        return;
    };
    unsafe {
        libc::syncfs(std::os::unix::io::AsRawFd::as_raw_fd(&dir));
    }
}
