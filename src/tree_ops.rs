// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// Recursive filesystem tree operations on a worker thread: measuring
// byte and item totals, copying directories (gio::File::copy() is
// intentionally shallow — it refuses to recurse), moving trees with
// merge semantics, and post-order deletion of non-empty directories.
// Progress is reported through plain callbacks and name collisions
// are delegated to a resolver callback, so this module stays
// UI-agnostic; progress.rs + conflict.rs turn both into channel
// messages and a dialog. Every gio call takes the shared Cancellable
// so cancel stays instant mid-tree.

use std::sync::atomic::Ordering;

use gtk4::prelude::*;
use gtk4::{gio, glib};

mod options;
mod verify;

use crate::fast_copy::{self, CopyOutcome};

pub use options::{Action, ByteCounter, CopyOptions, Resolver};
pub use verify::{VerifyCtl, VerifyResult};

/// Attributes read for every tree walk: type decides recursion,
/// size feeds the progress denominator, name locates children.
const TREE_ATTRS: &str = "standard::name,standard::type,standard::size,standard::symlink-target";

/// Recursively copy `src` to `dst`: directories are created, then their
/// children are copied one by one. `done` counts batch-wide bytes so
/// several sources never make the bar jump backwards. `report` receives
/// (current leaf name, cumulative bytes done). Existing `dst` entries
/// go through `resolve`; directories can merge into existing ones.
/// `opts` bundles the optional features: integrity verification
/// (samples `verify.enabled` per file) and the worker pool local
/// file payloads are submitted to when parallel copying is enabled.
/// The fast local pipeline is used either way.
pub fn copy_tree(
    src: &gio::File,
    dst: &gio::File,
    cancel: &gio::Cancellable,
    done: &ByteCounter,
    report: &mut dyn FnMut(&str, i64),
    resolve: &mut Resolver,
    opts: &mut CopyOptions,
) -> Result<(), glib::Error> {
    let info = src.query_info(
        TREE_ATTRS,
        gio::FileQueryInfoFlags::NOFOLLOW_SYMLINKS,
        Some(cancel),
    )?;
    let is_dir = info.file_type() == gio::FileType::Directory;

    if dst.query_file_type(
        gio::FileQueryInfoFlags::NOFOLLOW_SYMLINKS,
        gio::Cancellable::NONE,
    ) != gio::FileType::Unknown
    {
        let dst_is_dir = is_directory(dst, cancel);
        match resolve(src, dst) {
            // Kinds must match — the dialog never offers Overwrite for
            // file-vs-directory, but a remembered apply-to-all answer can
            // still produce it; failing loudly beats corrupting data.
            Action::Overwrite if is_dir != dst_is_dir => return Err(kind_mismatch(src, dst)),
            Action::Overwrite if is_dir => {
                // Merging: keep the existing directory, recurse in.
            }
            Action::Overwrite => {} // file: overwritten below via flag
            Action::Rename(new_name) => {
                // Recurse so a still-colliding new name resolves again
                // instead of silently overwriting under OVERWRITE.
                return copy_tree(
                    src,
                    &renamed(dst, &new_name),
                    cancel,
                    done,
                    report,
                    resolve,
                    opts,
                );
            }
            Action::Skip => {
                return Ok(());
            }
            Action::Cancel => return Err(cancelled()),
        }
    }

    if is_dir {
        if !dst.query_exists(gio::Cancellable::NONE) {
            dst.make_directory(Some(cancel))?;
        }
        let en = src.enumerate_children(
            TREE_ATTRS,
            gio::FileQueryInfoFlags::NOFOLLOW_SYMLINKS,
            Some(cancel),
        )?;
        while let Some(child_info) = en.next_file(Some(cancel))? {
            copy_tree(
                &en.child(&child_info),
                &dst.child(child_info.name()),
                cancel,
                done,
                report,
                resolve,
                opts,
            )?;
        }
        return Ok(());
    }

    // Symlinks are recreated as links — never dereferenced. A dangling
    // link must not abort the batch, and a link pointing outside the
    // source tree must not pull foreign content into the copy.
    if info.file_type() == gio::FileType::SymbolicLink {
        let target = info.symlink_target().ok_or_else(|| {
            glib::Error::new(
                gio::IOErrorEnum::NotSupported,
                &format!("cannot read link target: {}", leaf_name(src)),
            )
        })?;
        if dst.query_file_type(
            gio::FileQueryInfoFlags::NOFOLLOW_SYMLINKS,
            gio::Cancellable::NONE,
        ) != gio::FileType::Unknown
        {
            // Resolve already answered Overwrite — remove whatever sits
            // there so the new link can take its place.
            dst.delete(Some(cancel))?;
        }
        dst.make_symbolic_link(&target, Some(cancel))?;
        done.fetch_add(attr_size(&info), Ordering::Relaxed);
        return Ok(());
    }

    let size = attr_size(&info);
    let base = done.load(Ordering::Relaxed);
    let name = leaf_name(src);
    match (src.path(), dst.path()) {
        (Some(sp), Some(dp)) => {
            if let Some(pool) = opts.pool {
                // Pool workers own the byte accounting for their jobs
                // (fetch_add per chunk) — `done` must not be touched
                // here or bytes would count twice.
                pool.submit(sp, dp, name);
                return Ok(());
            }
            // Local fast path: reflink, else streamed copy. When the
            // verify toggle is on for this file, the stream is hashed
            // inline and the destination re-hashed while still hot in
            // the page cache — one extra read, zero source re-reads.
            let want = opts.verify.as_ref().map(|v| (v.enabled)()).unwrap_or(false);
            let outcome = fast_copy::copy(&sp, &dp, want, opts.paced, cancel, &mut |cur| {
                report(&name, base + cur as i64)
            })
            .map_err(|e| glib::Error::new(gio::IOErrorEnum::Failed, &e.to_string()))?;
            if want {
                if let Some(v) = opts.verify.as_mut() {
                    match outcome {
                        CopyOutcome::Reflinked => (v.report)(&name, VerifyResult::Reflinked),
                        CopyOutcome::Copied(src_hash) => {
                            // Lane present: the re-hash runs in the
                            // background while the next file copies.
                            if let Some(lane) = v.lane {
                                lane.submit(name, dp.clone(), src_hash);
                            } else {
                                let result = match (src_hash, fast_copy::hash_file(&dp, cancel)) {
                                    (Some(a), Ok(b)) if a == b => VerifyResult::Verified,
                                    (Some(_), Ok(_)) => VerifyResult::Mismatch,
                                    _ => VerifyResult::Unverifiable,
                                };
                                (v.report)(&name, result);
                            }
                        }
                    }
                }
            }
        }
        _ => {
            // GVFS/virtual locations stay on the gio path — no fast
            // syscall path exists for them, and no local hash either.
            src.copy(
                dst,
                gio::FileCopyFlags::OVERWRITE,
                Some(cancel),
                Some(&mut |cur: i64, _file_total: i64| report(&name, base + cur)),
            )?;
            if let Some(v) = opts.verify.as_mut() {
                if (v.enabled)() {
                    (v.report)(&name, VerifyResult::Unverifiable);
                }
            }
        }
    }
    done.fetch_add(size, Ordering::Relaxed);
    Ok(())
}

/// Recursively move `src` to `dst`. A fresh destination is a plain
/// `move_()` — an atomic rename on the same filesystem, a recursive
/// copy+delete handled inside gio across mounts. A colliding dst goes
/// through `resolve`: Overwrite merges directories (each child is
/// moved in, nested collisions re-resolve) and overwrites files.
pub fn move_tree(
    src: &gio::File,
    dst: &gio::File,
    cancel: &gio::Cancellable,
    done: &ByteCounter,
    report: &mut dyn FnMut(&str, i64),
    resolve: &mut Resolver,
) -> Result<(), glib::Error> {
    let info = src.query_info(
        TREE_ATTRS,
        gio::FileQueryInfoFlags::NOFOLLOW_SYMLINKS,
        Some(cancel),
    )?;
    let is_dir = info.file_type() == gio::FileType::Directory;

    if dst.query_file_type(
        gio::FileQueryInfoFlags::NOFOLLOW_SYMLINKS,
        gio::Cancellable::NONE,
    ) != gio::FileType::Unknown
    {
        let dst_is_dir = is_directory(dst, cancel);
        match resolve(src, dst) {
            Action::Skip => return Ok(()),
            Action::Cancel => return Err(cancelled()),
            Action::Rename(new_name) => {
                return move_tree(src, &renamed(dst, &new_name), cancel, done, report, resolve);
            }
            Action::Overwrite if is_dir != dst_is_dir => return Err(kind_mismatch(src, dst)),
            Action::Overwrite if is_dir => {
                // Merge: move each child in; nested collisions resolve
                // again, then the emptied source dir is removed.
                let en = src.enumerate_children(
                    TREE_ATTRS,
                    gio::FileQueryInfoFlags::NOFOLLOW_SYMLINKS,
                    Some(cancel),
                )?;
                let mut children = Vec::new();
                while let Some(child_info) = en.next_file(Some(cancel))? {
                    children.push((en.child(&child_info), child_info.name()));
                }
                drop(en);
                for (child_src, child_name) in children {
                    move_tree(
                        &child_src,
                        &dst.child(child_name),
                        cancel,
                        done,
                        report,
                        resolve,
                    )?;
                }
                src.delete(Some(cancel))?;
                return Ok(());
            }
            Action::Overwrite => {} // file: falls through to move_ below
        }
    }

    let size = attr_size(&info);
    let base = done.load(Ordering::Relaxed);
    let name = leaf_name(src);
    src.move_(
        dst,
        gio::FileCopyFlags::OVERWRITE,
        Some(cancel),
        Some(&mut |cur: i64, _file_total: i64| report(&name, base + cur)),
    )?;
    done.fetch_add(size, Ordering::Relaxed);
    Ok(())
}

/// Recursively delete `file` post-order. Children are collected before
/// removal — deleting while iterating an open enumerator is not safe
/// on all backends. `report` fires once per removed entry.
///
/// `wipe_passes` > 0 enables secure delete: every regular file with a
/// local filesystem path is overwritten with random data that many
/// times before it is unlinked. Non-local URIs and trash:// items are
/// unlinked directly — wiping is meaningless there.
pub fn delete_tree(
    file: &gio::File,
    cancel: &gio::Cancellable,
    report: &mut dyn FnMut(String),
    wipe_passes: u32,
) -> Result<(), glib::Error> {
    // Trash URIs: the trash backend does not support enumerating
    // children or recursive deletion via trash:// URIs — it returns
    // G_IO_ERROR_NOT_SUPPORTED ("Objects in Trash may not be
    // modified"). A single delete() on a trash item permanently
    // removes it and its entire contents; the backend handles the
    // recursion internally.
    if file.uri().starts_with("trash:") {
        file.delete(Some(cancel))?;
        report(leaf_name(file));
        return Ok(());
    }
    let info = file.query_info(
        "standard::type",
        gio::FileQueryInfoFlags::NOFOLLOW_SYMLINKS,
        Some(cancel),
    )?;
    if info.file_type() == gio::FileType::Directory {
        let en = file.enumerate_children(
            "standard::name",
            gio::FileQueryInfoFlags::NOFOLLOW_SYMLINKS,
            Some(cancel),
        )?;
        let mut children = Vec::new();
        while let Some(child_info) = en.next_file(Some(cancel))? {
            children.push(en.child(&child_info));
        }
        drop(en);
        for child in children {
            delete_tree(&child, cancel, report, wipe_passes)?;
        }
    } else if wipe_passes > 0 && info.file_type() == gio::FileType::Regular {
        // Regular file under a wipe policy: overwrite first, then
        // unlink. A wipe failure aborts the delete so content is
        // never removed half-overwritten.
        if let Some(path) = file.path() {
            eprintln!(
                "valkyrie: secure-wiping {} ({} passes)",
                path.display(),
                wipe_passes
            );
            crate::secure_wipe::wipe_file(&path, wipe_passes)
                .map_err(|e| glib::Error::new(gio::IOErrorEnum::Failed, &e.to_string()))?;
        }
    }
    file.delete(Some(cancel))?;
    report(leaf_name(file));
    Ok(())
}

/// Total byte size of `file` and everything below it. Directories
/// contribute zero; unreadable subtrees are skipped so the progress
/// denominator stays a best-effort estimate, never a hard failure.
pub fn measure_bytes(file: &gio::File, cancel: &gio::Cancellable) -> i64 {
    if cancel.is_cancelled() {
        return 0;
    }
    let Ok(info) = file.query_info(
        TREE_ATTRS,
        gio::FileQueryInfoFlags::NOFOLLOW_SYMLINKS,
        Some(cancel),
    ) else {
        return 0;
    };
    if info.file_type() != gio::FileType::Directory {
        return attr_size(&info);
    }
    let mut total = 0i64;
    if let Ok(en) = file.enumerate_children(
        TREE_ATTRS,
        gio::FileQueryInfoFlags::NOFOLLOW_SYMLINKS,
        Some(cancel),
    ) {
        while let Ok(Some(child)) = en.next_file(Some(cancel)) {
            total += measure_bytes(&en.child(&child), cancel);
        }
    }
    total
}

/// Total entry count of `file` and everything below it — the delete
/// path counts items because removal has no byte progress.
pub fn measure_items(file: &gio::File, cancel: &gio::Cancellable) -> i64 {
    if cancel.is_cancelled() {
        return 0;
    }
    let Ok(info) = file.query_info(
        "standard::type",
        gio::FileQueryInfoFlags::NOFOLLOW_SYMLINKS,
        Some(cancel),
    ) else {
        return 1; // Unknown type still gets one delete call.
    };
    let mut total = 1i64;
    if info.file_type() == gio::FileType::Directory {
        if let Ok(en) = file.enumerate_children(
            "standard::name",
            gio::FileQueryInfoFlags::NOFOLLOW_SYMLINKS,
            Some(cancel),
        ) {
            while let Ok(Some(child)) = en.next_file(Some(cancel)) {
                total += measure_items(&en.child(&child), cancel);
            }
        }
    }
    total
}

/// Whether `file` is a directory — false when the info query fails,
/// letting the caller fall back to the plain file path.
fn is_directory(file: &gio::File, cancel: &gio::Cancellable) -> bool {
    file.query_info(
        "standard::type",
        gio::FileQueryInfoFlags::NOFOLLOW_SYMLINKS,
        Some(cancel),
    )
    .map(|i| i.file_type() == gio::FileType::Directory)
    .unwrap_or(false)
}

/// Error for an overwrite between different kinds — gio cannot replace
/// a file with a directory or vice versa in a single operation.
fn kind_mismatch(src: &gio::File, dst: &gio::File) -> glib::Error {
    glib::Error::new(
        gio::IOErrorEnum::IsDirectory,
        &format!(
            "cannot overwrite different kinds: {} -> {}",
            leaf_name(src),
            leaf_name(dst)
        ),
    )
}

/// `dst`'s parent + `new_name` — the target after a rename resolution.
/// Only the leaf component is kept: a name containing '/' would
/// otherwise escape the destination folder via path resolution.
fn renamed(dst: &gio::File, new_name: &str) -> gio::File {
    let leaf = new_name.rsplit('/').next().unwrap_or(new_name);
    match dst.parent() {
        Some(parent) => parent.child(leaf),
        None => dst.clone(),
    }
}

/// File size with a sparse-backend guard — the attribute may be absent
/// even though it was requested (GVFS virtual mounts).
fn attr_size(info: &gio::FileInfo) -> i64 {
    if info.has_attribute("standard::size") {
        info.size()
    } else {
        0
    }
}

/// Display label for an item: the basename, or the URI as a fallback
/// for locations that have no filename component.
/// pub(crate): the progress worker labels the batch item with it.
pub(crate) fn leaf_name(file: &gio::File) -> String {
    file.basename()
        .map(|b| b.to_string_lossy().into_owned())
        .unwrap_or_else(|| file.uri().to_string())
}

/// Cancelled-error used when the resolver aborts the batch; the
/// worker recognises it via is_cancelled and closes quietly.
fn cancelled() -> glib::Error {
    glib::Error::new(gio::IOErrorEnum::Cancelled, "cancelled by user")
}

#[cfg(test)]
mod tests;
