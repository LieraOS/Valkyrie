// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// Thumbnail generation + shared-cache maintenance for thumbnails.rs:
// image decode/scale, PDF via pdftoppm, video via ffmpegthumbnailer,
// freedesktop tEXt metadata (Thumb::URI + Thumb::MTime), the atomic
// cache store — and the staleness check that decides whether a
// cached PNG may still be trusted for its file. Runs on pool worker
// threads only.

use gtk4::gdk_pixbuf::Pixbuf;
use gtk4::prelude::*;
use gtk4::{gio, glib};

use super::THUMB_SIZE;
pub(super) fn read_cached_or(
    path: &std::path::Path,
    file: &gio::File,
    ct: &str,
) -> Option<Vec<u8>> {
    let bytes = std::fs::read(path).ok();
    if let Some(b) = &bytes {
        if thumb_mtime_matches(b, file) {
            return bytes;
        }
    }
    exec_fresh(file, ct)
}

/// Content-type dispatch for fresh generation — native files only,
/// which `plan` guarantees on the primary path and which is checked
/// again here for the stale-cache fallback.
pub(super) fn exec_fresh(file: &gio::File, ct: &str) -> Option<Vec<u8>> {
    if !file.is_native() {
        return None;
    }
    if ct.starts_with("image/") {
        return generate(file);
    }
    if ct == "application/pdf" {
        return generate_pdf(file);
    }
    if ct.starts_with("video/") {
        return generate_video(file);
    }
    None
}

/// True when the PNG's tEXt `Thumb::MTime` equals the file's current
/// mtime — the freedesktop invalidation key. A malformed/missing
/// key counts as stale (safer to regenerate than to show wrong data).
pub(super) fn thumb_mtime_matches(png: &[u8], file: &gio::File) -> bool {
    let Some(thumb) = png_text(png, b"Thumb::MTime") else {
        return false;
    };
    let Ok(thumb_secs) = thumb.trim().parse::<u64>() else {
        return false;
    };
    let Some(path) = file.path() else {
        return false;
    };
    let file_secs = std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs());
    file_secs == Some(thumb_secs)
}

/// Extract a `tEXt` chunk value from a PNG byte stream — walks the
/// chunk table, no image decode needed.
pub(super) fn png_text(png: &[u8], key: &[u8]) -> Option<String> {
    const SIG: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
    if !png.starts_with(SIG) {
        return None;
    }
    let mut off = SIG.len();
    while off + 12 <= png.len() {
        let len = u32::from_be_bytes(png[off..off + 4].try_into().ok()?) as usize;
        let ctype = &png[off + 4..off + 8];
        let data_end = off + 8 + len;
        if data_end + 4 > png.len() {
            return None;
        }
        if ctype == b"tEXt" {
            let data = &png[off + 8..data_end];
            if data.starts_with(key) && data.get(key.len()) == Some(&0) {
                return String::from_utf8(data[key.len() + 1..].to_vec()).ok();
            }
        }
        off = data_end + 4;
    }
    None
}

/// Decode `file`, scale to the thumbnail size, encode as PNG with the
/// freedesktop metadata and store it in the shared cache.
pub(super) fn generate(file: &gio::File) -> Option<Vec<u8>> {
    let path = file.path()?;
    let pixbuf = Pixbuf::from_file_at_scale(&path, THUMB_SIZE, THUMB_SIZE, true).ok()?;
    let uri = file.uri().to_string();
    let mtime = std::fs::metadata(&path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs().to_string())
        .unwrap_or_default();
    let bytes = pixbuf
        .save_to_bufferv(
            "png",
            &[
                ("tEXt::Thumb::URI", uri.as_str()),
                ("tEXt::Thumb::MTime", mtime.as_str()),
                ("tEXt::Software", "Valkyrie"),
            ],
        )
        .ok()?;
    cache_store(&uri, &bytes);
    Some(bytes)
}

/// Render the first page of a PDF to a thumbnail via pdftoppm
/// (poppler-utils). The PNG is scaled to THUMB_SIZE and cached.
pub(super) fn generate_pdf(file: &gio::File) -> Option<Vec<u8>> {
    let path = file.path()?;
    let tmp = std::env::temp_dir().join(format!(
        "valkyrie-pdf-{}-{}.png",
        std::process::id(),
        path.file_name()?.to_string_lossy()
    ));
    // -png -singlefile -scale-to 256 → one PNG at THUMB_SIZE max edge.
    let out = std::process::Command::new("pdftoppm")
        .arg("-png")
        .arg("-singlefile")
        .arg("-scale-to")
        .arg(THUMB_SIZE.to_string())
        .arg(&path)
        .arg(&tmp)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let png_path = if tmp.exists() {
        tmp.clone()
    } else {
        // pdftoppm appends .png despite -singlefile on some versions.
        tmp.with_extension("png")
    };
    // Re-encode with freedesktop metadata via Pixbuf so the cache
    // entry matches the spec (Thumb::URI + MTime). Scale first, then
    // remove the temp file — removing before reading always failed.
    let pixbuf = Pixbuf::from_file_at_scale(&png_path, THUMB_SIZE, THUMB_SIZE, true).ok()?;
    let _ = std::fs::remove_file(&png_path);
    let uri = file.uri().to_string();
    let mtime = std::fs::metadata(&path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs().to_string())
        .unwrap_or_default();
    let bytes = pixbuf
        .save_to_bufferv(
            "png",
            &[
                ("tEXt::Thumb::URI", uri.as_str()),
                ("tEXt::Thumb::MTime", mtime.as_str()),
                ("tEXt::Software", "Valkyrie"),
            ],
        )
        .ok()?;
    cache_store(&uri, &bytes);
    Some(bytes)
}

/// Extract a video frame as a thumbnail via ffmpegthumbnailer.
/// Returns None silently when the tool is not installed — the icon
/// view keeps the generic video icon in that case.
pub(super) fn generate_video(file: &gio::File) -> Option<Vec<u8>> {
    let path = file.path()?;
    let tmp = std::env::temp_dir().join(format!(
        "valkyrie-vid-{}-{}.png",
        std::process::id(),
        path.file_name()?.to_string_lossy()
    ));
    let out = std::process::Command::new("ffmpegthumbnailer")
        .arg("-i")
        .arg(&path)
        .arg("-o")
        .arg(&tmp)
        .arg("-s")
        .arg(THUMB_SIZE.to_string())
        .arg("-c")
        .arg("png")
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let bytes = std::fs::read(&tmp).ok()?;
    let _ = std::fs::remove_file(&tmp);
    // Re-encode with freedesktop metadata so the cache entry matches
    // the spec. Pixbuf::from_file_at_scale can't reuse the deleted
    // tmp, so decode the raw bytes we already hold.
    let loader = gtk4::gdk_pixbuf::PixbufLoader::new();
    loader.write(&bytes).ok()?;
    loader.close().ok()?;
    let pixbuf = loader.pixbuf()?;
    let uri = file.uri().to_string();
    let mtime = std::fs::metadata(&path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs().to_string())
        .unwrap_or_default();
    let bytes = pixbuf
        .save_to_bufferv(
            "png",
            &[
                ("tEXt::Thumb::URI", uri.as_str()),
                ("tEXt::Thumb::MTime", mtime.as_str()),
                ("tEXt::Software", "Valkyrie"),
            ],
        )
        .ok()?;
    cache_store(&uri, &bytes);
    Some(bytes)
}
/// Written via a temporary file + rename so a kill mid-write can never
/// leave a half-written PNG behind for other applications.
pub(super) fn cache_store(uri: &str, bytes: &[u8]) {
    let Some(sum) = glib::compute_checksum_for_string(glib::ChecksumType::Md5, uri) else {
        return;
    };
    let dir = glib::user_cache_dir().join("thumbnails/large");
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let target = dir.join(format!("{sum}.png"));
    let tmp = dir.join(format!(".{sum}.tmp"));
    if std::fs::write(&tmp, bytes).is_ok() {
        let _ = std::fs::rename(&tmp, &target);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal PNG with one tEXt chunk — enough for the
    /// chunk-table walker (signature + IHDR stub + tEXt + IEND).
    fn png_with_text(key: &str, value: &str) -> Vec<u8> {
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        // IHDR: 13 zero bytes — content is irrelevant for tEXt scans.
        png.extend_from_slice(&13u32.to_be_bytes());
        png.extend_from_slice(b"IHDR");
        png.extend_from_slice(&[0; 13]);
        png.extend_from_slice(&0u32.to_be_bytes());
        let data = format!("{key}\0{value}");
        png.extend_from_slice(&(data.len() as u32).to_be_bytes());
        png.extend_from_slice(b"tEXt");
        png.extend_from_slice(data.as_bytes());
        png.extend_from_slice(&0u32.to_be_bytes());
        png.extend_from_slice(&0u32.to_be_bytes());
        png.extend_from_slice(b"IEND");
        png.extend_from_slice(&0u32.to_be_bytes());
        png
    }

    #[test]
    fn png_text_finds_embedded_key() {
        let png = png_with_text("Thumb::MTime", "1700000000");
        assert_eq!(
            png_text(&png, b"Thumb::MTime").as_deref(),
            Some("1700000000")
        );
        assert_eq!(png_text(&png, b"Thumb::URI"), None);
    }

    #[test]
    fn mtime_match_and_mismatch() {
        let dir = std::env::temp_dir().join(format!("valk-thumbtest-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("pic.png");
        std::fs::write(&path, b"x").unwrap();
        let secs = std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .unwrap()
            .as_secs();
        let file = gio::File::for_path(&path);
        // Thumb recorded at file's own mtime → fresh.
        let fresh = png_with_text("Thumb::MTime", &secs.to_string());
        assert!(thumb_mtime_matches(&fresh, &file));
        // Thumb one second older → stale, must regenerate.
        let stale = png_with_text("Thumb::MTime", &(secs - 1).to_string());
        assert!(!thumb_mtime_matches(&stale, &file));
        // Missing key → stale.
        assert!(!thumb_mtime_matches(
            &png_with_text("Thumb::URI", "file:///x"),
            &file
        ));
        std::fs::remove_dir_all(&dir).ok();
    }
}
