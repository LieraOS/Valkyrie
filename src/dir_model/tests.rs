// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// DirModel behaviour under a real file monitor: enumeration fills
// the store, single-path events mutate single rows (no full rebuild,
// no flicker), deletes drop rows, and refresh() diffs in place.

use std::time::{Duration, Instant};

use gtk4::prelude::*;
use gtk4::{gio, glib};

use super::DirModel;
use crate::file_object::FileObject;

/// Drive a private main context until `until` holds or the deadline
/// passes — every spawn/timer lands on this context because the
/// whole test body runs inside with_thread_default.
fn spin(ctx: &glib::MainContext, secs: u64, mut until: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < deadline {
        while ctx.iteration(false) {}
        if until() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(15));
    }
    until()
}

fn tempdir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("vky-dm-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Row lookup by raw name — mirrors the store scan the row updates
/// perform, kept test-local for assertions.
fn row_for(store: &gio::ListStore, name: &str) -> Option<FileObject> {
    (0..store.n_items()).find_map(|i| {
        let fo = store.item(i)?.downcast::<FileObject>().ok()?;
        (fo.raw_name() == name).then_some(fo)
    })
}

/// The initial enumeration lands in the store — the same path the
/// view pipeline takes at navigation time.
#[test]
fn enumeration_fills_store() {
    let dir = tempdir("fill");
    std::fs::write(dir.join("a.txt"), b"a").unwrap();
    std::fs::write(dir.join("b.txt"), b"b").unwrap();
    let location = gio::File::for_path(&dir);

    let ctx = glib::MainContext::new();
    let filled = ctx
        .with_thread_default(|| {
            let model = DirModel::new(&location);
            spin(&ctx, 5, || model.store.n_items() == 2)
        })
        .unwrap_or(false);
    std::fs::remove_dir_all(&dir).ok();
    assert!(filled, "initial enumeration must fill the store");
}

/// A file appearing in a monitored directory shows up through the
/// row-level path — no structural rebuild needed for a single create.
#[test]
fn created_file_appears() {
    let dir = tempdir("create");
    let location = gio::File::for_path(&dir);

    let ctx = glib::MainContext::new();
    let appeared = ctx
        .with_thread_default(|| {
            let model = DirModel::new(&location);
            spin(&ctx, 5, || model.store.n_items() == 0);
            let target = dir.join("fresh.txt");
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(150));
                std::fs::write(&target, b"hi").unwrap();
            });
            spin(&ctx, 6, || row_for(&model.store, "fresh.txt").is_some())
        })
        .unwrap_or(false);
    std::fs::remove_dir_all(&dir).ok();
    assert!(appeared, "monitor must surface the created file");
}

/// In-place overwrite: the row count stays put and the stored
/// attributes flip to the new state — the diff/row-update path must
/// not duplicate or leave stale values.
#[test]
fn overwrite_updates_row_in_place() {
    let dir = tempdir("overwrite");
    let path = dir.join("logo.png");
    std::fs::write(&path, b"x").unwrap();
    let location = gio::File::for_path(&dir);

    let ctx = glib::MainContext::new();
    let updated = ctx
        .with_thread_default(|| {
            let model = DirModel::new(&location);
            spin(&ctx, 5, || model.store.n_items() == 1);
            let p = path.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(150));
                std::fs::write(&p, [7u8; 512]).unwrap();
            });
            spin(&ctx, 6, || {
                model.store.n_items() == 1
                    && row_for(&model.store, "logo.png")
                        .map(|fo| fo.size_raw() == 512)
                        .unwrap_or(false)
            })
        })
        .unwrap_or(false);
    std::fs::remove_dir_all(&dir).ok();
    assert!(updated, "overwrite must update the row in place");
}

/// Deleted files drop their row through the same targeted path —
/// the enumeration diff is not needed for a lone removal.
#[test]
fn deleted_file_disappears() {
    let dir = tempdir("delete");
    std::fs::write(dir.join("doomed.txt"), b"d").unwrap();
    let location = gio::File::for_path(&dir);

    let ctx = glib::MainContext::new();
    let gone = ctx
        .with_thread_default(|| {
            let model = DirModel::new(&location);
            spin(&ctx, 5, || model.store.n_items() == 1);
            let p = dir.join("doomed.txt");
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(150));
                std::fs::remove_file(&p).unwrap();
            });
            spin(&ctx, 6, || model.store.n_items() == 0)
        })
        .unwrap_or(false);
    std::fs::remove_dir_all(&dir).ok();
    assert!(gone, "delete must drop the row");
}

/// refresh() is a diff, not a rebuild: calling it on an unchanged
/// directory must keep every existing row (same objects, same count).
#[test]
fn refresh_diffs_without_rebuilding() {
    let dir = tempdir("diff");
    std::fs::write(dir.join("keep.txt"), b"k").unwrap();
    let location = gio::File::for_path(&dir);

    let ctx = glib::MainContext::new();
    let stable = ctx
        .with_thread_default(|| {
            let model = DirModel::new(&location);
            spin(&ctx, 5, || model.store.n_items() == 1);
            let before = row_for(&model.store, "keep.txt");
            model.refresh();
            let stable = spin(&ctx, 5, || model.store.n_items() == 1);
            stable
                && before
                    .map(|old| {
                        row_for(&model.store, "keep.txt")
                            .map(|now| now == old)
                            .unwrap_or(false)
                    })
                    .unwrap_or(false)
        })
        .unwrap_or(false);
    std::fs::remove_dir_all(&dir).ok();
    assert!(stable, "refresh must keep unchanged rows identical");
}
