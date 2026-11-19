// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager

use std::io::Write;
use std::sync::mpsc;
use std::sync::Arc;

use super::VerifyLane;
use crate::tree_ops::VerifyResult;

/// Submitted files are verified off-thread and `wait` returns only
/// after every queued job reported — the batch-end contract.
#[test]
fn lane_verifies_and_drains() {
    let dir = std::env::temp_dir().join(format!("vky-lane-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let dst = dir.join("file.bin");
    let payload: Vec<u8> = (0..65536u32).map(|i| (i % 251) as u8).collect();
    std::fs::File::create(&dst)
        .unwrap()
        .write_all(&payload)
        .unwrap();
    let good = blake3::hash(&payload);
    let wrong = blake3::hash(b"different bytes");

    let (rtx, rrx) = mpsc::channel::<(String, VerifyResult)>();
    let lane = VerifyLane::new(
        Arc::new(move |name: &str, r: VerifyResult| {
            let _ = rtx.send((name.to_string(), r));
        }),
        gtk4::gio::Cancellable::new(),
    );
    lane.submit("ok".into(), dst.clone(), Some(good));
    lane.submit("bad".into(), dst.clone(), Some(wrong));
    lane.submit("missing".into(), dir.join("nope.bin"), Some(good));
    lane.wait();

    let mut got: Vec<_> = rrx.try_iter().collect();
    got.sort_by_key(|(n, _)| n.clone());
    assert_eq!(got.len(), 3);
    assert!(got.contains(&("ok".to_string(), VerifyResult::Verified)));
    assert!(got.contains(&("bad".to_string(), VerifyResult::Mismatch)));
    assert!(got.contains(&("missing".to_string(), VerifyResult::Unverifiable)));

    // wait() on an empty lane returns immediately.
    lane.wait();
    std::fs::remove_dir_all(&dir).ok();
}
