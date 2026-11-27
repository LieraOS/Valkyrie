// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// Worker side of the progress module: measures the batch, filters
// impossible sources, then walks the trees on a plain thread while
// the window (progress.rs) drains the message channel. Copy jobs
// create the shared verify flag the dialog checkbox toggles.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::mpsc;
use std::sync::Arc;

use gtk4::gio;
use gtk4::prelude::*;

use super::{spawn_impl, Msg};
use crate::copy_pool::{self, PoolCfg};
use crate::i18n::tr;
use crate::tree_ops::{self, Action, ByteCounter, CopyOptions, VerifyCtl, VerifyResult};

/// Copy or move `sources` into `dest` with a progress window.
/// `cut` picks move vs. copy; only copies get the integrity option.
pub fn copy_move(parent: &gtk4::Widget, sources: Vec<gio::File>, dest: gio::File, cut: bool) {
    let sources = filter_sources(sources, &dest, cut);
    if sources.is_empty() {
        return;
    }
    let title = if cut {
        tr("Moving…")
    } else {
        tr("Copying…")
    };
    // The checkbox lives in the dialog; the worker samples the flag
    // before every file so toggling mid-operation takes effect.
    let verify_flag = (!cut).then(super::new_verify_flag);
    let (tx, cancel) = spawn_impl(parent, &title, verify_flag.clone());
    std::thread::spawn(move || {
        crate::job_priority::background();
        // Measure once so the bar can show real byte progress.
        // Best effort: unreadable subtrees shrink the denominator.
        let mut totals = Vec::with_capacity(sources.len());
        for src in &sources {
            let _ = tx.send(Msg::Progress {
                name: format!("{}: {}", tr("Preparing"), tree_ops::leaf_name(src)),
                done: 0,
                total: 0,
            });
            totals.push(tree_ops::measure_bytes(src, &cancel));
        }
        // Removable targets (USB sticks etc.) get paced writes:
        // the dirty backlog is bounded to the device, so progress
        // moves at true write-out speed and "100 %" means the data
        // is physically on the medium — no page-cache flush tail
        // that would look like a hang on slow media.
        let paced = !cut
            && dest
                .path()
                .is_some_and(|p| crate::fast_copy::is_removable(&p));
        let total_bytes: i64 = totals.iter().sum();
        let done_bytes: ByteCounter = Arc::new(AtomicI64::new(0));

        // Collision resolver bridged to the UI thread. A remembered
        // "apply to all" answers later conflicts without asking again;
        // Rename is never remembered (each file needs its own name).
        let remembered: Rc<RefCell<Option<Action>>> = Rc::new(RefCell::new(None));
        let txq = tx.clone();
        let canc = cancel.clone();
        let mut resolve = move |src: &gio::File, dst: &gio::File| -> Action {
            if canc.is_cancelled() {
                return Action::Cancel;
            }
            if let Some(action) = remembered.borrow().clone() {
                return action;
            }
            let (rtx, rrx) = mpsc::channel::<(Action, bool)>();
            if txq
                .send(Msg::Conflict {
                    src: src.clone(),
                    dst: dst.clone(),
                    reply: rtx,
                })
                .is_err()
            {
                return Action::Cancel;
            }
            match rrx.recv() {
                Ok((action, all)) => {
                    if all && !matches!(action, Action::Rename(_)) {
                        *remembered.borrow_mut() = Some(action.clone());
                    }
                    action
                }
                Err(_) => Action::Cancel,
            }
        };

        // Per-file byte progress → the pump derives rate and ETA. An
        // Arc'd variant feeds the pool; the FnMut adapter feeds
        // copy_tree's sequential signature.
        let txp = tx.clone();
        let report_arc: copy_pool::ReportFn = Arc::new(move |name: &str, d: i64, active: u32| {
            // With a pool running, the label shows the live parallel
            // count — "big.iso (+1)" means one more file is copying
            // at the same time. Sequential copies show just the name.
            let label = if active > 1 {
                format!("{name} (+{})", active - 1)
            } else {
                name.to_string()
            };
            let _ = txp.send(Msg::Progress {
                name: label,
                done: d,
                total: total_bytes,
            });
        });
        let rep = report_arc.clone();
        let mut report = move |name: &str, d: i64| rep(name, d, 1);

        // Verify results stream to the dialog's live counter.
        let txv = tx.clone();
        let vreport_arc: copy_pool::VerifyReportFn =
            Arc::new(move |name: &str, r: VerifyResult| {
                let _ = txv.send(Msg::Verify {
                    name: name.to_string(),
                    result: r,
                });
            });
        let vrep = vreport_arc.clone();
        let mut verify_report = move |name: &str, r: VerifyResult| vrep(name, r);
        let vflag = verify_flag.clone();
        let verify_enabled = move || {
            vflag
                .as_ref()
                .map(|f| f.load(Ordering::Acquire))
                .unwrap_or(false)
        };

        // Background verify lane: copied files are queued for the
        // destination re-hash instead of stalling the copy loop.
        // Exists whenever the dialog has a verify checkbox — the
        // toggle can be flipped mid-copy, so the lane must be ready.
        let verify_lane = verify_flag.as_ref().map(|_| {
            Arc::new(crate::verify_lane::VerifyLane::new(
                vreport_arc.clone(),
                cancel.clone(),
            ))
        });

        // Parallel workers only ever make sense for copies — moves on
        // the same filesystem are renames, and cross-device moves are
        // already copy+delete bound by the target medium.
        let workers = if !cut && crate::settings::copy_parallel() {
            crate::settings::copy_workers().max(1) as usize
        } else {
            1
        };

        let mut first_err: Option<String> = None;
        let mut run_source = |src: &gio::File,
                              pool: Option<&copy_pool::CopyPool>,
                              report: &mut dyn FnMut(&str, i64),
                              resolve: &mut tree_ops::Resolver,
                              verify_report: &mut dyn FnMut(&str, VerifyResult)|
         -> bool {
            let name = src
                .basename()
                .map(|b| b.to_string_lossy().into_owned())
                .unwrap_or_else(|| tree_ops::leaf_name(src));
            let target = dest.child(&name);
            let result = if cut {
                tree_ops::move_tree(src, &target, &cancel, &done_bytes, report, resolve)
            } else {
                let ctl = VerifyCtl {
                    enabled: &verify_enabled,
                    report: verify_report,
                    lane: verify_lane.as_deref(),
                };
                let mut opts = CopyOptions {
                    verify: verify_flag.is_some().then_some(ctl),
                    pool,
                    paced,
                };
                tree_ops::copy_tree(
                    src,
                    &target,
                    &cancel,
                    &done_bytes,
                    report,
                    resolve,
                    &mut opts,
                )
            };
            match result {
                Ok(()) => true,
                Err(e) => {
                    if cancel.is_cancelled() || e.matches(gio::IOErrorEnum::Cancelled) {
                        return false;
                    }
                    first_err = Some(e.message().to_string());
                    false
                }
            }
        };

        if workers > 1 {
            // Payload pool: the walk stays on this thread so dirs,
            // enumeration and conflicts keep their strict order.
            let cfg = PoolCfg {
                cancel: cancel.clone(),
                done: done_bytes.clone(),
                verify_flag: verify_flag.clone(),
                report: report_arc,
                verify_report: verify_flag.as_ref().map(|_| vreport_arc),
                verify_lane: verify_lane.clone(),
                paced,
            };
            let (_, pool_err) = copy_pool::run(workers, cfg, |pool| {
                for src in &sources {
                    if cancel.is_cancelled()
                        || !run_source(
                            src,
                            Some(pool),
                            &mut report,
                            &mut resolve,
                            &mut verify_report,
                        )
                    {
                        break;
                    }
                }
            });
            if first_err.is_none() {
                first_err = pool_err;
            }
        } else {
            for src in &sources {
                if cancel.is_cancelled()
                    || !run_source(src, None, &mut report, &mut resolve, &mut verify_report)
                {
                    break;
                }
            }
        }

        if let Some(e) = first_err {
            let _ = tx.send(Msg::Failed(e));
            return;
        }
        // One filesystem-level flush for the whole batch: directory
        // entries, symlinks and any straggler write-back — "done" must
        // mean the destination is safe to unplug. Path-less (GVFS)
        // destinations cannot be syncfs'd and are skipped.
        if !cancel.is_cancelled() {
            if let Some(dp) = dest.path() {
                copy_pool::flush_fs(&dp);
            }
            // Pending verifications finish while their results stream
            // to the dialog — closing at 100 % means everything,
            // including the last re-hash, is really done.
            if let Some(lane) = &verify_lane {
                lane.wait();
            }
        }
        let _ = tx.send(Msg::Done);
    });
}

/// Permanently delete `sources` with an item-count progress window.
/// Directories are measured recursively so the bar is meaningful.
/// `wipe_passes` > 0 enables secure delete: regular files are
/// overwritten with random data that many times before unlinking.
pub fn delete(parent: &gtk4::Widget, sources: Vec<gio::File>, wipe_passes: u32) {
    if sources.is_empty() {
        return;
    }
    let title = tr("Deleting…");
    let (tx, cancel) = spawn_impl(parent, &title, None);
    std::thread::spawn(move || {
        crate::job_priority::background();
        let mut total = 0i64;
        for src in &sources {
            total += tree_ops::measure_items(src, &cancel);
        }
        let done_items = Cell::new(0i64);
        let txp = tx.clone();
        let mut report = move |name: String| {
            let d = done_items.get() + 1;
            done_items.set(d);
            let _ = txp.send(Msg::Progress {
                name,
                done: d,
                total,
            });
        };
        for src in &sources {
            if cancel.is_cancelled() {
                break;
            }
            if let Err(e) = tree_ops::delete_tree(src, &cancel, &mut report, wipe_passes) {
                if cancel.is_cancelled() || e.matches(gio::IOErrorEnum::Cancelled) {
                    break;
                }
                let _ = tx.send(Msg::Failed(e.message().to_string()));
                return;
            }
        }
        let _ = tx.send(Msg::Done);
    });
}

/// Drop sources that could never produce a sane operation:
/// - cut+drops into the same directory are no-ops,
/// - nothing (copy or move) may land inside its own subtree — the
///   walk would recurse into the copy it is creating.
pub(crate) fn filter_sources(
    sources: Vec<gio::File>,
    dest: &gio::File,
    cut: bool,
) -> Vec<gio::File> {
    sources
        .into_iter()
        .filter(|src| {
            if cut && src.parent().is_some_and(|p| p.equal(dest)) {
                return false;
            }
            // dest inside src (or equal) → the copy would chase itself.
            !dest.has_prefix(src) && !dest.equal(src)
        })
        .collect()
}
