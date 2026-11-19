// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// Tests for the parallel copy pool: jobs are distributed over workers,
// contents land correctly, byte accounting matches and verify results
// stream back per file.

use super::*;
use std::fs;
use std::sync::mpsc;

fn tempdir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("valkyrie-pool-{}-{tag}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn cfg(
    cancel: &gio::Cancellable,
    done: &Arc<AtomicI64>,
    verify_flag: Option<VerifyFlag>,
) -> (PoolCfg, mpsc::Receiver<(String, VerifyResult)>) {
    let (tx, rx) = mpsc::channel();
    (
        PoolCfg {
            cancel: cancel.clone(),
            done: done.clone(),
            verify_flag,
            report: Arc::new(|_, _, _| {}),
            verify_report: Some(Arc::new(move |n, r| {
                let _ = tx.send((n.to_string(), r));
            })),
            verify_lane: None,
            paced: false,
        },
        rx,
    )
}

#[test]
fn pool_copies_all_files_with_workers() {
    let dir = tempdir("all");
    let src_dir = dir.join("src");
    let dst_dir = dir.join("dst");
    fs::create_dir_all(&src_dir).unwrap();
    fs::create_dir_all(&dst_dir).unwrap();
    let mut payloads = Vec::new();
    for i in 0..40 {
        let payload: Vec<u8> = (0..10_000 + i).map(|b| (b % 253) as u8).collect();
        fs::write(src_dir.join(format!("f{i}.bin")), &payload).unwrap();
        payloads.push(payload);
    }

    let cancel = gio::Cancellable::new();
    let done = Arc::new(AtomicI64::new(0));
    let flag = Arc::new(AtomicBool::new(true));
    let (cfg, rx) = cfg(&cancel, &done, Some(flag));
    let dst = dst_dir.clone();
    let src = src_dir.clone();
    let (_, err) = run(4, cfg, |pool| {
        for i in 0..40 {
            pool.submit(
                src.join(format!("f{i}.bin")),
                dst.join(format!("f{i}.bin")),
                format!("f{i}.bin"),
            );
        }
    });
    assert!(err.is_none(), "worker error: {err:?}");
    for (i, payload) in payloads.iter().enumerate() {
        assert_eq!(&fs::read(dst.join(format!("f{i}.bin"))).unwrap(), payload);
    }
    // Every file reported a verify outcome; content must match.
    let mut results = Vec::new();
    while let Ok(r) = rx.try_recv() {
        results.push(r);
    }
    assert_eq!(results.len(), 40);
    for (_, r) in &results {
        assert!(matches!(
            r,
            VerifyResult::Verified | VerifyResult::Reflinked
        ));
    }
    let total: i64 = payloads.iter().map(|p| p.len() as i64).sum();
    assert_eq!(done.load(Ordering::Relaxed), total);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn pool_records_first_error() {
    let dir = tempdir("err");
    let cancel = gio::Cancellable::new();
    let done = Arc::new(AtomicI64::new(0));
    let (cfg, _rx) = cfg(&cancel, &done, None);
    let missing = dir.join("does-not-exist");
    let out = dir.join("out");
    let (_, err) = run(2, cfg, |pool| {
        pool.submit(missing, out, "x".to_string());
    });
    assert!(err.is_some());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn cancelled_pool_stops_workers() {
    let dir = tempdir("cancel");
    let src = dir.join("src");
    fs::write(&src, vec![1u8; 4 * 1024 * 1024]).unwrap();
    let cancel = gio::Cancellable::new();
    cancel.cancel();
    let done = Arc::new(AtomicI64::new(0));
    let (cfg, _rx) = cfg(&cancel, &done, None);
    let dst = dir.join("dst");
    let (_, err) = run(2, cfg, |pool| {
        for _ in 0..10 {
            pool.submit(src.clone(), dst.clone(), "f".to_string());
        }
    });
    // Cancellation is a quiet shutdown — never an error surface.
    assert!(err.is_none());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn pool_jobs_run_concurrently() {
    // Three large files on three workers must overlap in time —
    // sequential execution would report every chunk of file A before
    // file B's first chunk. We prove overlap by checking that all
    // three names appear among the first progress reports.
    let dir = tempdir("concurrent");
    let src_dir = dir.join("src");
    let dst_dir = dir.join("dst");
    fs::create_dir_all(&src_dir).unwrap();
    fs::create_dir_all(&dst_dir).unwrap();
    // > 4 MiB so each job emits several chunk-level reports.
    for i in 0..3 {
        fs::write(
            src_dir.join(format!("big{i}")),
            vec![i as u8; 24 * 1024 * 1024],
        )
        .unwrap();
    }
    let (tx, rx) = mpsc::channel::<String>();
    let cancel = gio::Cancellable::new();
    let done = Arc::new(AtomicI64::new(0));
    let cfg = PoolCfg {
        cancel: cancel.clone(),
        done: done.clone(),
        verify_flag: None,
        report: Arc::new(move |name, _, _| {
            let _ = tx.send(name.to_string());
        }),
        verify_report: None,
        verify_lane: None,
        paced: false,
    };
    let (src, dst) = (src_dir.clone(), dst_dir.clone());
    let (_, err) = run(3, cfg, |pool| {
        for i in 0..3 {
            pool.submit(
                src.join(format!("big{i}")),
                dst.join(format!("big{i}")),
                format!("big{i}"),
            );
        }
    });
    assert!(err.is_none(), "{err:?}");
    let mut seen = Vec::new();
    while let Ok(n) = rx.try_recv() {
        seen.push(n);
    }
    // The first three distinct names must all appear before any file
    // has finished — i.e. all three are already inside the first
    // handful of reports (a single worker would emit ~6 chunk reports
    // of one file before touching the next).
    let first_three: std::collections::HashSet<_> = seen.iter().take(9).collect();
    assert_eq!(
        first_three.len(),
        3,
        "reports were not interleaved: {:?}",
        &seen[..seen.len().min(20)]
    );
    let _ = fs::remove_dir_all(&dir);
}
