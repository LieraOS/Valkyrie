// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// Executor side of the sync engine: turn the abstract Op list
// (plan.rs) into real file operations via tree_ops, and walk local
// and remote trees into the Entry vectors plan() works on.

use gtk4::prelude::*;
use gtk4::{gio, glib};

use super::pairs::SyncPair;
use super::plan::Op;
use super::state::Entry;
use crate::tree_ops::{self, Action, CopyOptions};

const LIST_ATTRS: &str = "standard::name,standard::type,standard::size,time::modified";

/// Recursively list `root` into relative-path entries. Runs on the
/// worker thread; remote listings are plain GVFS enumerates — the
/// dav backend answers them like a local filesystem.
pub fn list(root: &gio::File, prefix: &str, out: &mut Vec<Entry>, cancel: &gio::Cancellable) {
    let Ok(en) = root.enumerate_children(LIST_ATTRS, gio::FileQueryInfoFlags::NONE, Some(cancel))
    else {
        return;
    };
    while let Ok(Some(info)) = en.next_file(Some(cancel)) {
        let name = info.name().to_string_lossy().into_owned();
        let path = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        let is_dir = info.file_type() == gio::FileType::Directory;
        out.push(Entry {
            path: path.clone(),
            is_dir,
            size: if info.has_attribute("standard::size") {
                info.size()
            } else {
                0
            },
            mtime: info
                .modification_date_time()
                .map(|dt| dt.to_unix())
                .unwrap_or(0),
        });
        if is_dir {
            list(&en.child(&info), &path, out, cancel);
        }
    }
}

/// `root` + a plan-relative path → the concrete gio::File.
fn at(root: &gio::File, rel: &str) -> gio::File {
    rel.split('/').fold(root.clone(), |f, part| f.child(part))
}

/// Sibling file named `stem (conflict).ext`; an already-existing
/// conflict file gets numbered " (2)" suffixes so conflicts never
/// overwrite each other.
fn conflict_file(file: &gio::File) -> Option<gio::File> {
    let parent = file.parent()?;
    let name = tree_ops::leaf_name(file);
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name.as_str(), ""),
    };
    for n in 1..=99 {
        let suffix = if n == 1 {
            String::new()
        } else {
            format!(" {n}")
        };
        let candidate = parent.child(format!("{stem} (conflict{suffix}){ext}"));
        if !candidate.query_exists(gio::Cancellable::NONE) {
            return Some(candidate);
        }
    }
    None
}

/// Run one op. Copies always allow overwrite — the plan already
/// decided that overwriting is the right thing (sync semantics are
/// not user-prompted; conflicts are pre-resolved as renames).
pub fn execute(
    op: &Op,
    pair: &SyncPair,
    remote_root: &gio::File,
    cancel: &gio::Cancellable,
    report: &mut dyn FnMut(&str),
) -> Result<(), glib::Error> {
    let local_root = gio::File::for_path(&pair.local);
    let mut resolve = |_: &gio::File, _: &gio::File| Action::Overwrite;
    let done = std::sync::Arc::new(std::sync::atomic::AtomicI64::new(0));
    let mut bytes = |_n: &str, _b: i64| {};
    let mut copy = |src: &gio::File, dst: &gio::File| {
        tree_ops::copy_tree(
            src,
            dst,
            cancel,
            &done,
            &mut bytes,
            &mut resolve,
            &mut CopyOptions::default(),
        )
    };
    match op {
        Op::Upload(p) => {
            report(p);
            copy(&at(&local_root, p), &at(remote_root, p))
        }
        Op::Download(p) => {
            report(p);
            copy(&at(remote_root, p), &at(&local_root, p))
        }
        Op::DeleteRemote(p) => {
            report(p);
            tree_ops::delete_tree(&at(remote_root, p), cancel, &mut |_| {}, 0)
        }
        Op::DeleteLocal(p) => {
            report(p);
            tree_ops::delete_tree(&at(&local_root, p), cancel, &mut |_| {}, 0)
        }
        Op::ConflictUpload(p) => {
            report(p);
            let remote = at(remote_root, p);
            if let Some(conflict) = conflict_file(&remote) {
                remote.move_(&conflict, gio::FileCopyFlags::NONE, Some(cancel), None)?;
            }
            copy(&at(&local_root, p), &remote)
        }
        Op::ConflictDownload(p) => {
            report(p);
            let local = at(&local_root, p);
            if let Some(conflict) = conflict_file(&local) {
                local.move_(&conflict, gio::FileCopyFlags::NONE, Some(cancel), None)?;
            }
            copy(&at(remote_root, p), &local)
        }
    }
}
