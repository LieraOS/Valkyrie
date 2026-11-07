// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// Archive helpers: detect archive content types, extract and compress
// via the system tools (7z, tar, unzip). Browsing inside archives
// falls back to file-roller — gvfsd-archive is not shipped in Debian's
// gvfs-backends package despite the description mentioning it.

use std::path::Path;
use std::process::Command;

/// Content types Valkyrie treats as extractable archives. MIME
/// detection is done by GIO via query_info("standard::content-type").
const ARCHIVE_TYPES: &[&str] = &[
    "application/zip",
    "application/x-zip-compressed",
    "application/x-tar",
    "application/x-compressed-tar",
    "application/x-bzip-compressed-tar",
    "application/x-gzip",
    "application/x-xz-compressed-tar",
    "application/x-7z-compressed",
    "application/x-rar",
    "application/x-rar-compressed",
    "application/x-lzma-compressed-tar",
    "application/x-bzip",
    "application/x-xz",
    "application/gzip",
];

/// Whether `content_type` is a known archive format Valkyrie can
/// extract with the available system tools.
pub fn is_archive(content_type: &str) -> bool {
    ARCHIVE_TYPES.contains(&content_type)
}

/// Extract `archive` into `dest_dir`. The right tool is picked from
/// the file extension — 7z handles zip/7z/rar, tar handles tar
/// variants. Returns the tool's stdout/stderr for error display.
pub fn extract(archive: &Path, dest_dir: &Path) -> Result<(), String> {
    let name = archive
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_lowercase();

    // tar variants: .tar, .tar.gz, .tgz, .tar.bz2, .tbz2, .tar.xz, .txz
    if name.ends_with(".tar")
        || name.ends_with(".tar.gz")
        || name.ends_with(".tgz")
        || name.ends_with(".tar.bz2")
        || name.ends_with(".tbz2")
        || name.ends_with(".tar.xz")
        || name.ends_with(".txz")
        || name.ends_with(".tar.lz")
        || name.ends_with(".tar.lzma")
    {
        return run_tar_extract(archive, dest_dir);
    }

    // Everything else (zip, 7z, rar, gz, xz, bz2) → 7z
    run_7z_extract(archive, dest_dir)
}

fn run_tar_extract(archive: &Path, dest_dir: &Path) -> Result<(), String> {
    let out = Command::new("tar")
        .arg("-xf")
        .arg(archive)
        .arg("-C")
        .arg(dest_dir)
        .output()
        .map_err(|e| format!("tar: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "tar failed: {}",
            String::from_utf8_lossy(&out.stderr)
        ))
    }
}

fn run_7z_extract(archive: &Path, dest_dir: &Path) -> Result<(), String> {
    let out = Command::new("7z")
        .arg("x")
        .arg(format!("-o{}", dest_dir.display()))
        .arg("-y")
        .arg(archive)
        .output()
        .map_err(|e| format!("7z: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "7z failed: {}",
            String::from_utf8_lossy(&out.stderr)
        ))
    }
}

/// Compress `sources` into `dest` using `format`. Supported formats:
/// "zip", "tar.gz", "7z". Returns the archive path on success.
pub fn compress(sources: &[&Path], dest: &Path, format: &str) -> Result<(), String> {
    if sources.is_empty() {
        return Err("no files selected".to_string());
    }
    match format {
        "zip" => run_7z_compress(sources, dest, "zip"),
        "7z" => run_7z_compress(sources, dest, "7z"),
        "tar.gz" => run_tar_compress(sources, dest),
        _ => Err(format!("unsupported format: {format}")),
    }
}

fn run_7z_compress(
    sources: &[&std::path::Path],
    dest: &std::path::Path,
    fmt: &str,
) -> Result<(), String> {
    // 7z stores paths relative to its working directory. Run it in
    // the parent of the first source and pass only basenames, so
    // the archive contains "folder/" not "/home/user/folder/".
    let parent = sources
        .first()
        .and_then(|s| s.parent())
        .ok_or_else(|| "no source paths".to_string())?;
    let mut cmd = Command::new("7z");
    cmd.arg("a").arg(format!("-t{fmt}")).arg(dest);
    for s in sources {
        match s.file_name() {
            Some(name) => cmd.arg(name),
            None => return Err("invalid source path".to_string()),
        };
    }
    cmd.current_dir(parent);
    let out = cmd.output().map_err(|e| format!("7z: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "7z failed: {}",
            String::from_utf8_lossy(&out.stderr)
        ))
    }
}

fn run_tar_compress(sources: &[&Path], dest: &std::path::Path) -> Result<(), String> {
    // tar stores paths relative to its working directory. If we pass
    // absolute paths, the archive contains "home/user/..." and
    // extraction recreates that tree. Fix: run tar in the parent
    // directory of the first source and pass only basenames. All
    // sources must share the same parent (the normal case — the user
    // selects files from one folder).
    let parent = sources
        .first()
        .and_then(|s| s.parent())
        .ok_or_else(|| "no source paths".to_string())?;
    let mut cmd = Command::new("tar");
    cmd.arg("-czf").arg(dest).arg("-C").arg(parent);
    for s in sources {
        // Only the basename — tar stores it without the parent path.
        match s.file_name() {
            Some(name) => cmd.arg(name),
            None => return Err("invalid source path".to_string()),
        };
    }
    let out = cmd.output().map_err(|e| format!("tar: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "tar failed: {}",
            String::from_utf8_lossy(&out.stderr)
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_common_archive_types() {
        assert!(is_archive("application/zip"));
        assert!(is_archive("application/x-tar"));
        assert!(is_archive("application/x-7z-compressed"));
        assert!(!is_archive("text/plain"));
        assert!(!is_archive("image/png"));
    }

    #[test]
    fn roundtrip_zip_extract() {
        // Create a temp file, zip it, extract it, verify content.
        let tmp = std::env::temp_dir().join(format!("vkat-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let src = tmp.join("hello.txt");
        std::fs::write(&src, b"archive test payload").unwrap();
        let zip = tmp.join("out.zip");
        compress(&[&src], &zip, "zip").unwrap();
        assert!(zip.exists());
        let extract_dir = tmp.join("extracted");
        std::fs::create_dir_all(&extract_dir).unwrap();
        extract(&zip, &extract_dir).unwrap();
        let back = extract_dir.join("hello.txt");
        assert!(back.exists(), "extracted file must exist");
        assert_eq!(std::fs::read(&back).unwrap(), b"archive test payload");
        std::fs::remove_dir_all(&tmp).ok();
    }
}
