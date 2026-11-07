// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// Per-pair sync state: a journal of what BOTH trees looked like after
// the last sync run. Without it a two-way sync cannot tell "file
// deleted locally" apart from "file newly appeared remotely" — the
// classic ambiguity every sync engine needs a journal for.
//
// Each journaled path carries one fingerprint per side. Per-side (not
// shared) fingerprints are required because WebDAV does not reliably
// preserve mtimes on upload: the remote file's post-sync mtime can
// differ from the local one even though both hold identical content.
// Recording what was actually observed — by re-listing both trees
// after the ops ran — keeps the next run from mistaking an upload for
// a remote-side edit (which would re-conflict forever).
//
// Format: one entry per line,
// `relpath<TAB>l_mtime<TAB>l_size<TAB>r_mtime<TAB>r_size`, where an
// absent side is written as `-` twice. The file lives under
// $XDG_DATA_HOME/valkyrie/sync/<key-hash>.state; writes are atomic
// (temp file + rename) so a crash mid-sync cannot leave a
// half-written journal.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

/// One tree entry as the sync engine sees it — both sides produce
/// the same shape so the plan logic can compare them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Path relative to the pair root, '/'-separated.
    pub path: String,
    pub is_dir: bool,
    pub size: i64,
    /// Seconds since epoch (time::modified), 0 when unknown.
    pub mtime: i64,
}

/// Content fingerprint used for change detection: (mtime, size).
pub fn fingerprint(e: &Entry) -> (i64, i64) {
    (e.mtime, e.size)
}

/// What the journal remembers about one path: the fingerprint each
/// side showed after the last sync, `None` when the path did not
/// exist on that side. "Was synced" (both Some) vs "seen on one
/// side only" is what delete propagation is decided on — a remote-
/// only file must never be treated as "the local side deleted it".
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct JournalEntry {
    /// Path relative to the pair root, '/'-separated.
    pub path: String,
    /// (mtime, size) the local side had post-sync.
    pub local: Option<(i64, i64)>,
    /// (mtime, size) the remote side had post-sync.
    pub remote: Option<(i64, i64)>,
}

fn state_dir() -> PathBuf {
    // glib has no user_state_dir in 0.20 — the journal lives in the
    // user data dir like the rest of the app's persistent data.
    gtk4::glib::user_data_dir().join("valkyrie").join("sync")
}

fn state_file(pair_key: &str) -> PathBuf {
    let mut h = DefaultHasher::new();
    pair_key.hash(&mut h);
    state_dir().join(format!("{:016x}.state", h.finish()))
}

/// Parse one optional "(mtime size)" pair — `-` marks an absent side.
fn parse_side(mtime: &str, size: &str) -> Option<(i64, i64)> {
    if mtime == "-" {
        None
    } else {
        Some((mtime.parse().ok()?, size.parse().ok()?))
    }
}

/// Load the previous journal; an absent or unreadable file yields an
/// empty journal, which makes the next sync treat every present file
/// as new (the conservative, data-preserving default).
pub fn load(pair_key: &str) -> Vec<JournalEntry> {
    let Ok(text) = std::fs::read_to_string(state_file(pair_key)) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|line| {
            let mut parts = line.split('\t');
            let path = parts.next()?.to_string();
            // No `?` on parse_side: None is a legitimate value
            // (the side did not have the path), not a parse error.
            let local = parse_side(parts.next()?, parts.next()?);
            let remote = parse_side(parts.next()?, parts.next()?);
            Some(JournalEntry {
                path,
                local,
                remote,
            })
        })
        .collect()
}

/// Persist the journal; a directory that cannot be created simply
/// degrades the next sync to "everything is new" — never a failure.
pub fn save(pair_key: &str, entries: &[JournalEntry]) {
    let dir = state_dir();
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let side = |fp: &Option<(i64, i64)>| match fp {
        Some((m, s)) => format!("{m}\t{s}"),
        None => "-\t-".to_string(),
    };
    let target = state_file(pair_key);
    let tmp = target.with_extension("tmp");
    let mut text = String::new();
    for e in entries {
        text.push_str(&format!(
            "{}\t{}\t{}\n",
            e.path,
            side(&e.local),
            side(&e.remote)
        ));
    }
    if std::fs::write(&tmp, text).is_ok() {
        let _ = std::fs::rename(&tmp, &target);
    }
}

/// Fold two freshly-listed trees into the journal entries save()
/// persists — the journal records observed reality, not the plan.
pub fn journal_from_trees(local: &[Entry], remote: &[Entry]) -> Vec<JournalEntry> {
    let mut map: std::collections::HashMap<String, JournalEntry> = std::collections::HashMap::new();
    for e in local {
        map.entry(e.path.clone())
            .or_insert_with(|| JournalEntry {
                path: e.path.clone(),
                ..JournalEntry::default()
            })
            .local = Some(fingerprint(e));
    }
    for e in remote {
        map.entry(e.path.clone())
            .or_insert_with(|| JournalEntry {
                path: e.path.clone(),
                ..JournalEntry::default()
            })
            .remote = Some(fingerprint(e));
    }
    map.into_values().collect()
}

/// Drop the journal for a pair that is being unpaired — otherwise a
/// later re-pair would resurrect phantom deletions.
pub fn discard(pair_key: &str) {
    let _ = std::fs::remove_file(state_file(pair_key));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn journal_from_trees_marks_sides() {
        let l = Entry {
            path: "a".into(),
            is_dir: false,
            size: 1,
            mtime: 10,
        };
        let r = Entry {
            path: "a".into(),
            is_dir: false,
            size: 1,
            mtime: 99, // server-side mtime, differs from local
        };
        let journal = journal_from_trees(&[l], &[r]);
        assert_eq!(journal.len(), 1);
        assert_eq!(journal[0].local, Some((10, 1)));
        assert_eq!(journal[0].remote, Some((99, 1)));
    }
}
