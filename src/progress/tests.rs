// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// Unit tests for the copy/move source filter: drops into the own
// directory and into a source's own subtree must never start a
// transfer. Runs on real temp directories — no GTK display needed.

use super::*;

/// A fresh empty directory inside the system temp dir; each test gets
/// a unique path so parallel `cargo test` runs never interfere.
fn temp_dir(tag: &str) -> gio::File {
    let path = std::env::temp_dir().join(format!("valkyrie-prog-{}-{tag}", std::process::id()));
    let dir = gio::File::for_path(&path);
    let _ = dir.make_directory(gio::Cancellable::NONE);
    dir
}

fn cleanup(dir: &gio::File) {
    // Reuse the real implementation — it is what the tests verify.
    let cancel = gio::Cancellable::new();
    let _ = crate::tree_ops::delete_tree(dir, &cancel, &mut |_| {}, 0);
}

#[test]
fn filter_same_dir_move_is_noop() {
    let dir = temp_dir("same-dir");
    let file = dir.child("a.txt");
    // A file moved into its own directory must be filtered out…
    assert!(filter_sources(vec![file.clone()], &dir, true).is_empty());
    // …while copying it there stays allowed (creates "a (2).txt").
    assert_eq!(filter_sources(vec![file], &dir, false).len(), 1);
    cleanup(&dir);
}

#[test]
fn filter_dir_into_itself() {
    let dir = temp_dir("into-self");
    let sub = dir.child("sub");
    let _ = sub.make_directory(gio::Cancellable::NONE);
    let deeper = sub.child("deep");
    // Moving or copying a directory into itself/descendant is out.
    assert!(filter_sources(vec![dir.clone()], &sub, true).is_empty());
    assert!(filter_sources(vec![dir.clone()], &sub, false).is_empty());
    assert!(filter_sources(vec![dir.clone()], &deeper, false).is_empty());
    cleanup(&dir);
}

#[test]
fn filter_keeps_normal_case() {
    let dir = temp_dir("normal-src");
    let dest = temp_dir("normal-dst");
    let file = dir.child("a.txt");
    assert_eq!(filter_sources(vec![file], &dest, true).len(), 1);
    cleanup(&dir);
    cleanup(&dest);
}
