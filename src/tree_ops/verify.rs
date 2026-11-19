// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// Integrity-verify types shared between the copy walker and the
// progress dialog: the per-file outcome enum and the control block
// that lets the UI toggle verification mid-operation.

/// Outcome reported per file to the verify callback after a copy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifyResult {
    /// Shared-extents clone — contents are identical by construction;
    /// hashing would compare a file with itself.
    Reflinked,
    /// Destination digest equals the source digest.
    Verified,
    /// Destination digest differs — the copy is corrupted.
    Mismatch,
    /// Backend is not a local path or hashing failed — nothing could
    /// be verified.
    Unverifiable,
}

/// Integrity-check control handed to `copy_tree`. `enabled` is
/// sampled before *each* file so the dialog toggle reacts mid-tree;
/// `report` receives the per-file outcome. When `lane` is set, the
/// destination re-hash runs on the background verify lane instead of
/// stalling the copy loop (see src/verify_lane.rs).
pub struct VerifyCtl<'a> {
    pub enabled: &'a dyn Fn() -> bool,
    pub report: &'a mut dyn FnMut(&str, VerifyResult),
    /// Background verify lane shared with the pool path — the batch
    /// end waits on its drain, so "done" still means fully verified.
    pub lane: Option<&'a crate::verify_lane::VerifyLane>,
}
