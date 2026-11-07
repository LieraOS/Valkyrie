// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// Unit tests for the recursive search walk: matching is a
// case-insensitive substring test over name and display-name, hits
// arrive through the channel, a cancelled Cancellable stops the walk.
// Runs on real temp directories — no GTK display needed.

use super::*;

/// Build a small tree: top/alpha.txt, top/beta.txt, top/nested/alpha2.txt.
/// Returns the root; the caller removes it afterwards.
fn tree(tag: &str) -> gio::File {
    let path = std::env::temp_dir().join(format!("valkyrie-search-{}-{tag}", std::process::id()));
    let root = gio::File::for_path(&path);
    let _ = root.make_directory(gio::Cancellable::NONE);
    let nested = root.child("nested");
    let _ = nested.make_directory(gio::Cancellable::NONE);
    for file in [
        root.child("alpha.txt"),
        root.child("beta.txt"),
        nested.child("alpha2.txt"),
    ] {
        file.create(gio::FileCreateFlags::NONE, gio::Cancellable::NONE)
            .unwrap();
    }
    root
}

fn remove_tree(dir: &gio::File) {
    if let Ok(en) = dir.enumerate_children(
        "standard::name,standard::type",
        gio::FileQueryInfoFlags::NONE,
        gio::Cancellable::NONE,
    ) {
        while let Ok(Some(info)) = en.next_file(gio::Cancellable::NONE) {
            let child = en.child(&info);
            if info.file_type() == gio::FileType::Directory {
                remove_tree(&child);
            } else {
                let _ = child.delete(gio::Cancellable::NONE);
            }
        }
    }
    let _ = dir.delete(gio::Cancellable::NONE);
}

/// Collect every Hit the walk produces until Done.
fn hits(root: &gio::File, needle: &str) -> Vec<gio::File> {
    let (tx, rx) = std::sync::mpsc::channel::<Msg>();
    let cancel = gio::Cancellable::new();
    let mut count = 0usize;
    walk(root, needle, &cancel, &tx, &mut count, None);
    drop(tx);
    rx.iter()
        .filter_map(|m| match m {
            Msg::Hit(f) => Some(f),
            Msg::Done => None,
        })
        .collect()
}

#[test]
fn walk_finds_nested_matches() {
    let root = tree("nested");
    let found = hits(&root, "alpha");
    let mut names: Vec<String> = found
        .iter()
        .filter_map(|f| f.basename().map(|b| b.to_string_lossy().into_owned()))
        .collect();
    names.sort();
    assert_eq!(names, ["alpha.txt", "alpha2.txt"]);
    remove_tree(&root);
}

#[test]
fn walk_is_case_insensitive() {
    let root = tree("case");
    assert_eq!(hits(&root, "ALPHA").len(), 2);
    remove_tree(&root);
}

#[test]
fn walk_no_match_returns_empty() {
    let root = tree("nomatch");
    assert!(hits(&root, "zzzz").is_empty());
    remove_tree(&root);
}

#[test]
fn walk_respects_cancellation() {
    let root = tree("cancel");
    let cancel = gio::Cancellable::new();
    cancel.cancel();
    let (tx, rx) = std::sync::mpsc::channel::<Msg>();
    let mut count = 0usize;
    walk(&root, "alpha", &cancel, &tx, &mut count, None);
    drop(tx);
    assert!(rx.iter().next().is_none());
    assert_eq!(count, 0);
    remove_tree(&root);
}

/// A symlinked directory must not be descended into — merged-usr
/// systems alias /bin -> /usr/bin etc., and following the link would
/// report every match twice under two different paths. The link
/// itself still counts as a hit when its name matches.
#[test]
#[cfg(unix)]
fn walk_does_not_descend_into_symlinked_dirs() {
    let root = tree("symlink");
    let link = root.child("linkdir");
    std::os::unix::fs::symlink(root.child("nested").path().unwrap(), link.path().unwrap()).unwrap();
    let found = hits(&root, "alpha");
    let mut names: Vec<String> = found
        .iter()
        .map(|f| {
            f.path()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default()
        })
        .collect();
    names.sort();
    // alpha2.txt appears once via nested/, never via linkdir/.
    let alpha2: Vec<&String> = names.iter().filter(|n| n.contains("alpha2")).collect();
    assert_eq!(alpha2.len(), 1);
    assert!(!alpha2[0].contains("linkdir"));
    remove_tree(&root);
}
