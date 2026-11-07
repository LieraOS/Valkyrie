// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// Synced folder pairs: a local directory bound to a remote path below
// the user's Nextcloud root, plus the per-pair direction and schedule
// the user picked in the preferences. Persisted as the
// `nextcloud-sync-folders` GSettings key (array of tuples); this
// module is the single place that knows the tuple layout.

/// How a paired folder is kept in sync.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Local changes are mirrored to the cloud; remote-only files and
    /// remote changes stay untouched (local is the source of truth,
    /// deletions never propagate).
    UploadOnly,
    /// Full mirroring: changes flow both ways, deletions propagate,
    /// and a file edited on both sides keeps the newer version while
    /// the loser is preserved under a "(conflict)" name.
    TwoWay,
}

/// When the pair is synchronized.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Schedule {
    /// Only when the user asks (context menu / "Sync Now").
    Manual,
    /// A GFileMonitor watches the local tree; changes trigger a sync
    /// after a short debounce.
    Live,
    /// Re-sync every N minutes while Valkyrie runs.
    Interval(u32),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncPair {
    /// Local directory, as a plain filesystem path.
    pub local: String,
    /// Remote directory, relative to the dav root ("" = root itself;
    /// leading slashes are tolerated on load and normalized away).
    pub remote: String,
    pub direction: Direction,
    pub schedule: Schedule,
}

impl SyncPair {
    /// Stable identity used for the state file name and the monitor
    /// map — two pairs with the same endpoints share a sync state.
    pub fn key(&self) -> String {
        format!("{}|{}", self.local, self.remote)
    }
}

/// All configured pairs; unknown direction/schedule encodings degrade
/// to the safe defaults (upload only, manual).
pub fn all() -> Vec<SyncPair> {
    crate::settings::nextcloud_sync_folders()
        .into_iter()
        .map(|(local, remote, direction, schedule)| SyncPair {
            local,
            remote: remote.trim_start_matches('/').to_string(),
            direction: if direction == "twoway" {
                Direction::TwoWay
            } else {
                Direction::UploadOnly
            },
            schedule: match schedule {
                -1 => Schedule::Live,
                0 => Schedule::Manual,
                n if n > 0 => Schedule::Interval(n as u32),
                _ => Schedule::Manual,
            },
        })
        .collect()
}

/// Write the whole pair list back to GSettings.
pub fn store(pairs: &[SyncPair]) {
    let raw: Vec<(String, String, String, i32)> = pairs
        .iter()
        .map(|p| {
            (
                p.local.clone(),
                p.remote.clone(),
                match p.direction {
                    Direction::UploadOnly => "upload".to_string(),
                    Direction::TwoWay => "twoway".to_string(),
                },
                match p.schedule {
                    Schedule::Live => -1,
                    Schedule::Manual => 0,
                    Schedule::Interval(minutes) => minutes as i32,
                },
            )
        })
        .collect();
    crate::settings::set_nextcloud_sync_folders(&raw);
}

/// The pair that has `local_path` as its root, if any.
pub fn find_by_local(local_path: &str) -> Option<SyncPair> {
    all().into_iter().find(|p| p.local == local_path)
}

/// Add `pair` or replace an existing pair with the same local root.
pub fn upsert(pair: SyncPair) {
    let mut pairs = all();
    pairs.retain(|p| p.local != pair.local);
    pairs.push(pair);
    store(&pairs);
}

/// Remove the pair rooted at `local_path` (no-op if absent).
pub fn remove(local_path: &str) {
    let mut pairs = all();
    pairs.retain(|p| p.local != local_path);
    store(&pairs);
}
