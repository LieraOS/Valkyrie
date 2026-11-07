// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// The pure heart of the sync engine: `plan()` maps three snapshots —
// the current local tree, the current remote tree and the journal
// from the last completed sync — onto the ordered operation list.
// No GVFS, no filesystem: everything runs on Entry vectors plus the
// per-side JournalEntry map, so the whole decision matrix is
// unit-testable.
//
// Conflict rule everywhere: never silently lose data. When both
// sides changed since the journal entry, the newer version keeps
// the name and the loser is preserved under a "(conflict)" name.

use std::collections::{HashMap, HashSet};

use super::pairs::Direction;
use super::state::{self, Entry, JournalEntry};

/// One step of a sync run, in execution order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    /// Copy the local file/dir over the remote side.
    Upload(String),
    /// Copy the remote file/dir over the local side.
    Download(String),
    /// Remove the remote entry (two-way delete propagation).
    DeleteRemote(String),
    /// Remove the local entry (two-way delete propagation).
    DeleteLocal(String),
    /// Both sides changed; local wins the name, remote version is
    /// renamed to "(conflict)" first.
    ConflictUpload(String),
    /// Both sides changed; remote wins the name, local version is
    /// renamed to "(conflict)" first.
    ConflictDownload(String),
}

/// Compute the operation list for one pair.
///
/// `prev` is the post-sync journal of the previous run, carrying one
/// fingerprint per side. An empty journal (first sync) treats both
/// sides as never-seen — the only case where a file existing on both
/// sides with different content is undecidable; it is resolved by
/// mtime like a normal conflict.
pub fn plan(
    local: &[Entry],
    remote: &[Entry],
    prev: &[JournalEntry],
    direction: Direction,
) -> Vec<Op> {
    let local: HashMap<&str, &Entry> = local.iter().map(|e| (e.path.as_str(), e)).collect();
    let remote: HashMap<&str, &Entry> = remote.iter().map(|e| (e.path.as_str(), e)).collect();
    let prev: HashMap<&str, &JournalEntry> = prev.iter().map(|e| (e.path.as_str(), e)).collect();

    // Paths already covered by a directory-level op — a copied dir
    // brings its whole subtree, a deleted dir takes it away. RefCell
    // so the emit closure and the ancestor check can share it.
    let covered = std::cell::RefCell::new(HashSet::new());
    let covered_by = |path: &str| {
        let covered = covered.borrow();
        let mut p = path;
        loop {
            match p.rfind('/') {
                Some(i) => {
                    p = &p[..i];
                    if covered.contains(p) {
                        return true;
                    }
                }
                None => return false,
            }
        }
    };

    // None = no journal record for that side (never synced there).
    // Some(true/false) = the side's fingerprint differs/matches the
    // journal. Per-side comparison is essential: a WebDAV server may
    // stamp its own mtime on upload, so the remote fingerprint is
    // only ever compared against the remote journal entry.
    let local_changed = |e: &Entry| -> Option<bool> {
        prev.get(e.path.as_str())
            .and_then(|j| j.local)
            .map(|fp| fp != state::fingerprint(e))
    };
    let remote_changed = |e: &Entry| -> Option<bool> {
        prev.get(e.path.as_str())
            .and_then(|j| j.remote)
            .map(|fp| fp != state::fingerprint(e))
    };

    let mut ops = Vec::new();
    let mut emit = |op: Op| {
        let (Op::Upload(p)
        | Op::Download(p)
        | Op::DeleteRemote(p)
        | Op::DeleteLocal(p)
        | Op::ConflictUpload(p)
        | Op::ConflictDownload(p)) = &op;
        covered.borrow_mut().insert(p.clone());
        ops.push(op);
    };

    // --- paths present locally, parents before children ---
    // Sorted order guarantees "d" is seen before "d/x", which the
    // covered check relies on (HashMap iteration order is random).
    let mut local_paths: Vec<&str> = local.keys().copied().collect();
    local_paths.sort_unstable();
    for path in local_paths {
        if covered_by(path) {
            continue;
        }
        let e = local[path];
        match remote.get(path) {
            None => {
                // Remote side does not have the path.
                let j = prev.get(path);
                let was_synced = j.is_some_and(|j| j.local.is_some() && j.remote.is_some());
                if was_synced && direction == Direction::TwoWay {
                    // Both sides had it, the remote copy vanished →
                    // the remote side deleted it; propagate locally.
                    emit(Op::DeleteLocal(path.to_string()));
                } else {
                    // Fresh local content, a previously failed upload,
                    // or (upload-only) a remote copy that disappeared:
                    // (re)upload heals all three cases the same way.
                    emit(Op::Upload(path.to_string()));
                }
            }
            Some(r) => {
                if e.is_dir {
                    continue; // children carry the real work
                }
                match (direction, local_changed(e), remote_changed(r)) {
                    // Neither side moved since the journal.
                    (_, Some(false), Some(false)) => {}
                    // Only one side moved — mirror that side.
                    (_, Some(true), Some(false)) => {
                        emit(Op::Upload(path.to_string()));
                    }
                    (Direction::TwoWay, Some(false), Some(true)) => {
                        emit(Op::Download(path.to_string()));
                    }
                    // Upload-only with a remote-side edit: local wins
                    // the name but the remote version is preserved.
                    (Direction::UploadOnly, Some(false), Some(true)) => {
                        emit(Op::ConflictUpload(path.to_string()));
                    }
                    // Both moved, or first-sync divergence with no
                    // journal: resolve by mtime, loser preserved.
                    (_, lc, _) => {
                        if lc.is_none() && state::fingerprint(e) == state::fingerprint(r) {
                            continue; // identical content, just unseen
                        }
                        let op = if e.mtime >= r.mtime {
                            Op::ConflictUpload(path.to_string())
                        } else {
                            Op::ConflictDownload(path.to_string())
                        };
                        emit(op);
                    }
                }
            }
        }
    }

    // --- paths present only remotely, parents before children ---
    let mut remote_paths: Vec<&str> = remote.keys().copied().collect();
    remote_paths.sort_unstable();
    for path in remote_paths {
        if covered_by(path) || local.contains_key(path) {
            continue;
        }
        if direction != Direction::TwoWay {
            continue; // upload-only leaves remote-only content alone
        }
        let was_synced = prev
            .get(path)
            .is_some_and(|j| j.local.is_some() && j.remote.is_some());
        if was_synced {
            // Was on both sides, now local is missing → the local
            // side deleted it; propagate.
            emit(Op::DeleteRemote(path.to_string()));
        } else {
            // Brand-new remote content (or remote-only content not
            // yet pulled under two-way) → download.
            emit(Op::Download(path.to_string()));
        }
    }

    ops
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Local or remote tree entry shorthand.
    fn f(path: &str, mtime: i64) -> Entry {
        Entry {
            path: path.to_string(),
            is_dir: false,
            size: 1,
            mtime,
        }
    }

    /// Journal shorthand: file synced with fingerprint (mtime, size)
    /// on the sides given (None = the side did not have it).
    fn j(path: &str, l: Option<i64>, r: Option<i64>) -> JournalEntry {
        JournalEntry {
            path: path.to_string(),
            local: l.map(|m| (m, 1)),
            remote: r.map(|m| (m, 1)),
        }
    }

    #[test]
    fn first_sync_uploads_new_local() {
        let ops = plan(&[f("a.txt", 10)], &[], &[], Direction::UploadOnly);
        assert_eq!(ops, vec![Op::Upload("a.txt".to_string())]);
    }

    #[test]
    fn first_sync_remote_only_ignored_upload_only() {
        let ops = plan(&[], &[f("a.txt", 10)], &[], Direction::UploadOnly);
        assert!(ops.is_empty());
    }

    #[test]
    fn first_sync_downloads_remote_only_twoway() {
        let ops = plan(&[], &[f("a.txt", 10)], &[], Direction::TwoWay);
        assert_eq!(ops, vec![Op::Download("a.txt".to_string())]);
    }

    #[test]
    fn unchanged_journal_no_ops() {
        let e = f("a.txt", 10);
        let ops = plan(
            std::slice::from_ref(&e),
            std::slice::from_ref(&e),
            &[j("a.txt", Some(10), Some(10))],
            Direction::TwoWay,
        );
        assert!(ops.is_empty());
    }

    /// The whole point of per-side fingerprints: after an upload the
    /// server's own mtime differs from the local one. The journal
    /// stored BOTH observed values, so neither side looks "changed".
    #[test]
    fn divergent_side_mtimes_stay_unchanged() {
        let ops = plan(
            &[f("a.txt", 10)],
            &[f("a.txt", 99)],
            &[j("a.txt", Some(10), Some(99))],
            Direction::UploadOnly,
        );
        assert!(ops.is_empty());
    }

    #[test]
    fn local_edit_uploads() {
        let ops = plan(
            &[f("a.txt", 20)],
            &[f("a.txt", 10)],
            &[j("a.txt", Some(10), Some(10))],
            Direction::TwoWay,
        );
        assert_eq!(ops, vec![Op::Upload("a.txt".to_string())]);
    }

    #[test]
    fn remote_edit_downloads_twoway() {
        let ops = plan(
            &[f("a.txt", 10)],
            &[f("a.txt", 20)],
            &[j("a.txt", Some(10), Some(10))],
            Direction::TwoWay,
        );
        assert_eq!(ops, vec![Op::Download("a.txt".to_string())]);
    }

    #[test]
    fn remote_edit_conflicts_upload_only() {
        let ops = plan(
            &[f("a.txt", 10)],
            &[f("a.txt", 20)],
            &[j("a.txt", Some(10), Some(10))],
            Direction::UploadOnly,
        );
        assert_eq!(ops, vec![Op::ConflictUpload("a.txt".to_string())]);
    }

    #[test]
    fn local_delete_propagates_twoway() {
        let ops = plan(
            &[],
            &[f("a.txt", 10)],
            &[j("a.txt", Some(10), Some(10))],
            Direction::TwoWay,
        );
        assert_eq!(ops, vec![Op::DeleteRemote("a.txt".to_string())]);
    }

    /// A file that was always remote-only must NOT be read as
    /// "locally deleted" — otherwise switching a pair to two-way
    /// would delete remote files that were never synced.
    #[test]
    fn remote_only_journal_downloads_instead_of_deleting() {
        let ops = plan(
            &[],
            &[f("a.txt", 10)],
            &[j("a.txt", None, Some(10))],
            Direction::TwoWay,
        );
        assert_eq!(ops, vec![Op::Download("a.txt".to_string())]);
    }

    #[test]
    fn local_delete_kept_upload_only() {
        let ops = plan(
            &[],
            &[f("a.txt", 10)],
            &[j("a.txt", Some(10), Some(10))],
            Direction::UploadOnly,
        );
        assert!(ops.is_empty());
    }

    #[test]
    fn remote_delete_propagates_twoway() {
        let ops = plan(
            &[f("a.txt", 10)],
            &[],
            &[j("a.txt", Some(10), Some(10))],
            Direction::TwoWay,
        );
        assert_eq!(ops, vec![Op::DeleteLocal("a.txt".to_string())]);
    }

    /// Upload-only and the remote copy vanished — mirror semantics
    /// say heal it, not delete the local file.
    #[test]
    fn remote_delete_reuploads_upload_only() {
        let ops = plan(
            &[f("a.txt", 10)],
            &[],
            &[j("a.txt", Some(10), Some(10))],
            Direction::UploadOnly,
        );
        assert_eq!(ops, vec![Op::Upload("a.txt".to_string())]);
    }

    #[test]
    fn both_changed_conflict_by_mtime() {
        // Remote newer → remote wins the canonical name.
        let ops = plan(
            &[f("a.txt", 20)],
            &[f("a.txt", 30)],
            &[j("a.txt", Some(10), Some(10))],
            Direction::TwoWay,
        );
        assert_eq!(ops, vec![Op::ConflictDownload("a.txt".to_string())]);
    }

    #[test]
    fn first_sync_identical_fp_skips() {
        let e = f("a.txt", 10);
        let ops = plan(
            std::slice::from_ref(&e),
            std::slice::from_ref(&e),
            &[],
            Direction::TwoWay,
        );
        assert!(ops.is_empty());
    }

    #[test]
    fn dir_upload_covers_children() {
        let dir = Entry {
            path: "d".to_string(),
            is_dir: true,
            size: 0,
            mtime: 0,
        };
        let ops = plan(
            &[dir, f("d/a.txt", 1), f("d/b.txt", 1)],
            &[],
            &[],
            Direction::UploadOnly,
        );
        assert_eq!(ops, vec![Op::Upload("d".to_string())]);
    }
}
