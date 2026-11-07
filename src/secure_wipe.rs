// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// Secure-delete helpers: overwrite a regular file with random data
// before it is unlinked, plus a media probe used to warn when the
// storage device cannot honour the promise (SSD wear-leveling,
// copy-on-write filesystems).

use std::io::{Seek, SeekFrom, Write};
use std::path::Path;

/// Overwrite `path` `passes` times with random bytes, fsyncing after
/// each pass. The caller unlinks afterwards. Returns Err on I/O
/// failure — the caller should abort the delete then, so the file
/// is never silently removed half-wiped.
pub fn wipe_file(path: &Path, passes: u32) -> std::io::Result<()> {
    let mut f = std::fs::OpenOptions::new().write(true).open(path)?;
    let len = f.metadata()?.len();
    if len == 0 {
        return Ok(());
    }
    const CHUNK: usize = 1024 * 1024;
    let mut buf = vec![0u8; CHUNK.min(len as usize)];
    for _ in 0..passes {
        f.seek(SeekFrom::Start(0))?;
        let mut left = len;
        while left > 0 {
            let n = buf.len().min(left as usize);
            // /dev/urandom via std — no extra dependency. Read failure
            // must not abort wiping: fall back to zeroes, still better
            // than leaving the content untouched.
            if std::fs::File::open("/dev/urandom")
                .and_then(|mut r| std::io::Read::read_exact(&mut r, &mut buf[..n]))
                .is_err()
            {
                buf[..n].fill(0);
            }
            f.write_all(&buf[..n])?;
            left -= n as u64;
        }
        f.sync_data()?;
    }
    Ok(())
}

/// True when the medium hosting `path` can realistically honour an
/// overwrite promise: rotational HDD on a conventional filesystem.
/// False for SSD/flash (rotational=0) and COW filesystems (btrfs,
/// f2fs, zfs, nilfs2) where the old bytes stay behind in stale
/// blocks or snapshots.
pub fn medium_is_reliable(path: &Path) -> bool {
    let Ok(canon) = path.canonicalize() else {
        return true; // cannot tell → do not warn
    };
    // Find the longest matching mount in /proc/self/mountinfo.
    let Ok(mi) = std::fs::read_to_string("/proc/self/mountinfo") else {
        return true;
    };
    let mut best: Option<(usize, String, String)> = None; // (len, fstype, source)
    for line in mi.lines() {
        let mut it = line.split_whitespace();
        let mount = it.nth(4).unwrap_or("").replace("\\040", " ");
        if !canon.starts_with(&mount) {
            continue;
        }
        let sep = line.find(" - ").map(|i| &line[i + 3..]);
        let Some(tail) = sep else { continue };
        let mut t = tail.split_whitespace();
        let (fstype, source) = (t.next().unwrap_or(""), t.nth(1).unwrap_or(""));
        if best.as_ref().is_none_or(|(l, _, _)| mount.len() > *l) {
            best = Some((mount.len(), fstype.to_string(), source.to_string()));
        }
    }
    let Some((_, fstype, source)) = best else {
        return true;
    };
    const COW: &[&str] = &["btrfs", "zfs", "f2fs", "nilfs2"];
    if COW.contains(&fstype.as_str()) {
        return false;
    }
    // Rotational flag of the backing block device: /dev/sda3 → sda.
    let dev = source.rsplit('/').next().unwrap_or("");
    let base = dev.trim_end_matches(|c: char| c.is_ascii_digit());
    let base = base.strip_suffix('p').unwrap_or(base);
    match std::fs::read_to_string(format!("/sys/class/block/{base}/queue/rotational")) {
        Ok(v) => v.trim() != "0",
        Err(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wipe_overwrites_content() {
        let dir = std::env::temp_dir().join(format!("vwtest-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("secret.bin");
        std::fs::write(&f, b"TOPSECRETDATA-".repeat(1000)).unwrap();
        wipe_file(&f, 2).unwrap();
        let after = std::fs::read(&f).unwrap();
        assert_eq!(after.len(), 14000);
        assert!(
            !after.windows(14).any(|w| w == b"TOPSECRETDATA-"),
            "file must contain no original bytes after wipe"
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
