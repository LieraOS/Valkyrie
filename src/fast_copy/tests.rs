// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// Tests for the fast copy pipeline: reflink-or-stream outcome,
// inline hashing correctness and metadata preservation.

use super::*;

/// A fresh empty directory inside the system temp dir; `tag` makes
/// each test's path unique so parallel runs never interfere.
fn tempdir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("valkyrie-fastcopy-{}-{tag}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn copy_streams_content_and_hashes_source() {
    let dir = tempdir("stream");
    let src = dir.join("src.bin");
    let dst = dir.join("dst.bin");
    let payload: Vec<u8> = (0..1_000_000).map(|i| (i % 251) as u8).collect();
    fs::write(&src, &payload).unwrap();

    let cancel = gio::Cancellable::new();
    let outcome = copy(&src, &dst, true, false, &cancel, &mut |_| {}).unwrap();
    match outcome {
        CopyOutcome::Reflinked => {
            // CoW filesystem: contents are shared — still identical.
        }
        CopyOutcome::Copied(Some(hash)) => {
            assert_eq!(hash, blake3::hash(&payload));
        }
        CopyOutcome::Copied(None) => panic!("hashing was requested"),
    }
    assert_eq!(fs::read(&dst).unwrap(), payload);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn copy_without_hash_returns_none() {
    let dir = tempdir("nohash");
    let src = dir.join("a");
    let dst = dir.join("b");
    fs::write(&src, b"hello").unwrap();
    let cancel = gio::Cancellable::new();
    let outcome = copy(&src, &dst, false, false, &cancel, &mut |_| {}).unwrap();
    match outcome {
        CopyOutcome::Reflinked => {}
        CopyOutcome::Copied(hash) => assert!(hash.is_none()),
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn hash_file_matches_payload() {
    let dir = tempdir("hash");
    let f = dir.join("f");
    fs::write(&f, b"check me").unwrap();
    let cancel = gio::Cancellable::new();
    assert_eq!(hash_file(&f, &cancel).unwrap(), blake3::hash(b"check me"));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn copy_preserves_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempdir("perms");
    let src = dir.join("p");
    let dst = dir.join("q");
    let mut f = fs::File::create(&src).unwrap();
    f.write_all(b"x").unwrap();
    f.sync_all().unwrap();
    drop(f);
    fs::set_permissions(&src, fs::Permissions::from_mode(0o740)).unwrap();
    let cancel = gio::Cancellable::new();
    copy(&src, &dst, false, false, &cancel, &mut |_| {}).unwrap();
    assert_eq!(
        fs::metadata(&dst).unwrap().permissions().mode() & 0o777,
        0o740
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn paced_copy_writes_exact_payload() {
    // Pacing is requested, but unresolvable devices (tmpfs) simply
    // no-op — either way the destination must hold the exact bytes,
    // including the unaligned tail.
    let dir = tempdir("paced");
    let src = dir.join("s");
    let dst = dir.join("d");
    let payload: Vec<u8> = (0..3_000_005).map(|i| (i % 253) as u8).collect();
    fs::write(&src, &payload).unwrap();
    let cancel = gio::Cancellable::new();
    let outcome = copy(&src, &dst, false, true, &cancel, &mut |_| {}).unwrap();
    if let CopyOutcome::Copied(_) = outcome {
        assert_eq!(fs::read(&dst).unwrap(), payload);
        assert_eq!(fs::metadata(&dst).unwrap().len(), payload.len() as u64);
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn cancel_aborts_copy() {
    let dir = tempdir("cancel");
    let src = dir.join("big");
    let dst = dir.join("big.out");
    fs::write(&src, vec![7u8; 8 * 1024 * 1024]).unwrap();
    let cancel = gio::Cancellable::new();
    cancel.cancel();
    let r = copy(&src, &dst, false, false, &cancel, &mut |_| {});
    // A cancelled operation may only ever "succeed" via an instant
    // reflink — the streamed path must report the interruption.
    assert!(r.is_err() || matches!(r, Ok(CopyOutcome::Reflinked)));
    let _ = fs::remove_dir_all(&dir);
}
