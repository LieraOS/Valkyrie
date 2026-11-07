// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// Public type surface of the tree-ops module: collision actions and
// their resolver, the shared byte counter and the per-batch copy
// options. Kept separate from the walker implementations so each
// stays under the project's per-file line budget.

use std::sync::atomic::AtomicI64;
use std::sync::Arc;

use gtk4::gio;

use super::VerifyCtl;
use crate::copy_pool::CopyPool;

/// How a name collision gets resolved. Produced by the conflict
/// dialog (conflict.rs) or a remembered apply-to-all choice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// dst exists: overwrite the file / merge into the directory.
    Overwrite,
    /// Write the source under this new leaf name in dst's parent.
    Rename(String),
    /// Leave dst untouched, continue with the next item.
    Skip,
    /// Abort the whole batch.
    Cancel,
}

/// Signature of the collision resolver handed to copy/move trees.
/// Receives (source, existing destination) and decides what happens.
pub type Resolver<'a> = dyn FnMut(&gio::File, &gio::File) -> Action + 'a;

/// Shared byte counter for a copy/move batch — atomic so pool
/// workers and the sequential walker advance the same total.
pub type ByteCounter = Arc<AtomicI64>;

/// Per-batch options for `copy_tree`.
#[derive(Default)]
pub struct CopyOptions<'a> {
    /// Integrity checking: `enabled` is sampled before each file —
    /// local files hash the stream inline and re-hash the
    /// destination; results go to `report`.
    pub verify: Option<VerifyCtl<'a>>,
    /// Worker pool for local file payloads. When set, local files are
    /// enqueued instead of copied inline; the walk itself (directory
    /// creation, enumeration, conflicts) stays sequential.
    pub pool: Option<&'a CopyPool>,
    /// Pace writes so the page-cache backlog never outruns the
    /// destination device — progress then tracks the real write-out
    /// and the final fsync stays short. Set once per batch by
    /// jobs.rs after probing the destination for removability.
    pub paced: bool,
}
