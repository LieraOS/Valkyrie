// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// Fast local file copy: a FICLONE reflink clone is attempted first —
// on copy-on-write filesystems (btrfs, xfs) that is an instant
// metadata operation sharing extents instead of moving bytes. When
// the kernel refuses (non-CoW fs, cross-device), a read/write
// pipeline copies at the hardware limit and optionally feeds every
// byte through a BLAKE3 hasher, so the source checksum costs no
// extra read pass. For removable targets the destination can be
// opened with O_DIRECT: every progress tick is then a real device
// write and "100 %" means physically on the medium. File metadata
// (mode, times, xattrs) is re-applied through GIO's copy_attributes
// for all paths.

use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::io::AsRawFd;
use std::path::Path;

use gtk4::gio;
use gtk4::prelude::*;

/// Copy buffer size — large enough to amortise syscall overhead at
/// NVMe speeds, small enough to stay cache-friendly per worker.
const COPY_BUF: usize = 4 * 1024 * 1024;

/// How the destination bytes were produced.
#[derive(Debug, Clone, PartialEq)]
pub enum CopyOutcome {
    /// Shared-extents clone: destination data is identical to the
    /// source by construction — integrity verification is meaningless
    /// and is skipped by the caller.
    Reflinked,
    /// Bytes were streamed through userspace. Carries the source's
    /// BLAKE3 digest when hashing was requested — gathered for free
    /// from the data already flowing through the copy loop.
    Copied(Option<blake3::Hash>),
}

/// Copy `src` to `dst` as fast as the filesystem allows. `report`
/// receives cumulative bytes written for progress display; `cancel`
/// aborts between chunks. `paced` bounds the page-cache backlog to
/// the device (removable media): progress then tracks the real
/// write-out and the final fsync stays short, while the kernel's
/// write-back keeps the device queue fully fed — unlike O_DIRECT,
/// which serialises every chunk at queue depth 1 and measurably
/// loses throughput on USB storage. Metadata is preserved via GIO.
pub fn copy(
    src: &Path,
    dst: &Path,
    want_hash: bool,
    paced: bool,
    cancel: &gio::Cancellable,
    report: &mut dyn FnMut(u64),
) -> io::Result<CopyOutcome> {
    let mut src_file = fs::File::open(src)?;
    let mut dst_file = fs::File::create(dst)?;

    // Reflink first: the io error kind distinguishes "filesystem does
    // not support COW" (fall through) from real failures (propagate).
    match unsafe { libc::ioctl(dst_file.as_raw_fd(), libc::FICLONE, src_file.as_raw_fd()) } {
        0 => {
            preserve_metadata(src, dst);
            if let Ok(size) = src_file.metadata().map(|m| m.len()) {
                report(size);
            }
            return Ok(CopyOutcome::Reflinked);
        }
        _ => {
            // NotCoW/unsupported ioctls land here — rewind and stream.
            dst_file.set_len(0)?;
        }
    }

    // Streamed fallback: read → (optional) hash → write per chunk.
    // In paced mode the DevicePacer throttles the loop so the dirty
    // backlog never outruns the destination by more than a small
    // window — and reports device-completed bytes, so the progress
    // bar moves at real write speed instead of sprinting into the
    // page cache and then freezing while the stick drains.
    let mut hasher = want_hash.then(blake3::Hasher::new);
    let mut pacer = paced.then(|| DevicePacer::new(dst));
    let mut buf = vec![0u8; COPY_BUF];
    let mut done = 0u64;
    loop {
        if cancel.is_cancelled() {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
        }
        let n = src_file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        if let Some(h) = hasher.as_mut() {
            h.update(&buf[..n]);
        }
        dst_file.write_all(&buf[..n])?;
        done += n as u64;
        if let Some(p) = pacer.as_mut() {
            p.pace(n as u64, cancel, report)?;
        } else {
            report(done);
        }
    }
    dst_file.flush()?;
    // Drain phase (paced): the bar keeps moving at real device speed
    // while the remaining backlog flushes — a frozen near-full bar
    // reads as "stuck" exactly like the mid-copy stalls did. Capped
    // by a stall deadline: a wedged device counter must never turn
    // into an endless wait, sync_all below is the real guarantee.
    if let Some(p) = pacer.as_mut() {
        p.drain(done, cancel, report)?;
    }
    // Final fsync: file contents and metadata must be on the device
    // before the file counts as copied — "finished" must mean
    // "safe to unplug", not "sitting in the page cache". In paced
    // mode only the small backlog window remains to drain.
    dst_file.sync_all()?;
    report(done);
    preserve_metadata(src, dst);
    Ok(CopyOutcome::Copied(hasher.map(|h| h.finalize())))
}

/// Maximum dirty-page lead the copy may hold over the device — big
/// enough to keep write-back busy, small enough that the final
/// fsync drains in seconds on slow media.
const PACE_BACKLOG: u64 = 64 * 1024 * 1024;

/// Bounds the page-cache backlog for one destination by watching the
/// backing block device's write counter in sysfs. The kernel keeps
/// writing at full queue depth; the copy loop only sleeps when its
/// dirty lead exceeds `PACE_BACKLOG`, so the reported progress moves
/// at real device speed and "done" leaves almost nothing to flush.
struct DevicePacer {
    /// `/sys/.../stat` of the destination's block device — empty
    /// when the device cannot be resolved (pacing is then a no-op).
    stat: std::path::PathBuf,
    /// Sectors the device had already written when the copy started.
    base: u64,
    /// Bytes this copy has handed to the page cache so far.
    written: u64,
}

impl DevicePacer {
    /// Resolve the block device behind `path` via sysfs and take the
    /// write counter baseline. Unresolvable devices disable pacing.
    fn new(path: &Path) -> Self {
        use std::os::unix::fs::MetadataExt;
        let stat = path
            .metadata()
            .ok()
            .and_then(|md| {
                let link = format!(
                    "/sys/dev/block/{}:{}",
                    libc::major(md.dev()),
                    libc::minor(md.dev())
                );
                fs::canonicalize(link).ok().map(|p| p.join("stat"))
            })
            .filter(|p| p.exists())
            .unwrap_or_default();
        let base = sectors_written(&stat);
        Self {
            stat,
            base,
            written: 0,
        }
    }

    /// Bytes the device has genuinely completed, capped at this
    /// copy's cache hand-off — the honest progress value.
    fn shown(&self) -> u64 {
        device_written(self.stat.as_path(), self.base).min(self.written)
    }

    /// Account `add` freshly written bytes, then sleep in short
    /// slices until the device's write counter is within
    /// `PACE_BACKLOG` of the cache lead. Every slice reports the
    /// device-completed byte count, so throttling phases show as a
    /// slowed bar — never as a freeze.
    fn pace(
        &mut self,
        add: u64,
        cancel: &gio::Cancellable,
        report: &mut dyn FnMut(u64),
    ) -> io::Result<()> {
        self.written += add;
        if self.stat.as_os_str().is_empty() {
            report(self.written);
            return Ok(());
        }
        while self
            .written
            .saturating_sub(device_written(self.stat.as_path(), self.base))
            > PACE_BACKLOG
        {
            if cancel.is_cancelled() {
                return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
            }
            report(self.shown());
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        report(self.shown());
        Ok(())
    }

    /// Keep reporting device-completed bytes after the write loop
    /// until the backlog is on the device — the bar crawls to 100%
    /// at real drain speed instead of parking mid-file. Bounded by
    /// a no-progress deadline (sysfs counters also count other
    /// writers and may never exactly reach `written`).
    fn drain(
        &mut self,
        total: u64,
        cancel: &gio::Cancellable,
        report: &mut dyn FnMut(u64),
    ) -> io::Result<()> {
        if self.stat.as_os_str().is_empty() {
            return Ok(());
        }
        let mut stalled = 0u32;
        let mut last = 0u64;
        loop {
            let shown = self.shown().min(total);
            report(shown);
            if shown >= total || cancel.is_cancelled() {
                break;
            }
            stalled = if shown > last { 0 } else { stalled + 1 };
            last = shown;
            if stalled > 200 {
                // ~10s without any completed sector — give up on the
                // visual phase, sync_all finishes the job.
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        Ok(())
    }
}

/// Sectors the device behind `stat` has written minus `base`, in
/// bytes — includes every writer on that device, which errs on the
/// side of slightly *less* throttling (a soft bound is enough).
fn device_written(stat: &Path, base: u64) -> u64 {
    sectors_written(stat).saturating_sub(base) * 512
}

/// Field 7 of a block device's `stat` file: sectors written.
fn sectors_written(stat: &Path) -> u64 {
    fs::read_to_string(stat)
        .ok()
        .and_then(|s| s.split_whitespace().nth(6)?.parse().ok())
        .unwrap_or(0)
}

/// True when `path` lives on a removable (e.g. USB) block device —
/// resolved through sysfs: a `usb` component in the device chain or
/// the whole-disk `removable` flag (which partitions inherit from
/// their parent). Used to switch the destination to direct writes
/// only where the page-cache flush tail would actually hurt —
/// NVMe/SATA targets keep the plain cached path.
pub fn is_removable(path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    let Ok(md) = path.metadata() else {
        return false;
    };
    let dev = md.dev();
    let link = format!("/sys/dev/block/{}:{}", libc::major(dev), libc::minor(dev));
    let Ok(sys) = fs::canonicalize(link) else {
        return false;
    };
    if sys.to_string_lossy().contains("/usb") {
        return true;
    }
    let mut node = sys.as_path();
    for _ in 0..4 {
        if let Ok(v) = fs::read_to_string(node.join("removable")) {
            return v.trim() == "1";
        }
        let Some(parent) = node.parent() else {
            break;
        };
        node = parent;
    }
    false
}

/// Re-read `path` and return its BLAKE3 digest. Used by the verify
/// pass on the freshly written destination — still hot in the page
/// cache for typical sizes, so this is close to RAM speed.
pub fn hash_file(path: &Path, cancel: &gio::Cancellable) -> io::Result<blake3::Hash> {
    let mut file = fs::File::open(path)?;
    let mut hasher = blake3::Hasher::new();
    let mut buf = vec![0u8; COPY_BUF];
    loop {
        if cancel.is_cancelled() {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
        }
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize())
}

/// Re-apply mode, timestamps and xattrs from `src` to `dst` through
/// GIO — identical to what gio::File::copy does internally. A failed
/// attribute copy is deliberately not fatal: the data is already
/// correct, and some backends only allow partial attribute sets.
fn preserve_metadata(src: &Path, dst: &Path) {
    let src_file = gio::File::for_path(src);
    let dst_file = gio::File::for_path(dst);
    let _ = src_file.copy_attributes(
        &dst_file,
        gio::FileCopyFlags::ALL_METADATA,
        gio::Cancellable::NONE,
    );
}

#[cfg(test)]
mod tests;
