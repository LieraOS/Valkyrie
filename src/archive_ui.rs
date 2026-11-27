// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// UI glue for archive operations: "Extract Here" runs extraction
// in the current directory with a progress-aware error dialog;
// "Compress…" opens a small format chooser (zip / tar.gz / 7z) and
// creates the archive next to the selection.

use gtk4::prelude::*;
use gtk4::{gio, glib};

use crate::i18n::tr;

/// Extract `archive` into its parent directory. Errors surface as a
/// simple message dialog — extraction is fast enough that a full
/// progress window is not warranted for the common case.
pub fn extract_here(parent: &gtk4::Widget, archive: &gio::File) {
    let Some(path) = archive.path() else { return };
    let Some(parent_path) = path.parent() else {
        return;
    };
    let archive_path = path.clone();
    let dest = parent_path.to_path_buf();
    // The archive stem (e.g. "Y" from "Y.zip") — used to rename the
    // extracted top-level directory so it matches the archive name
    // instead of the original source folder name.
    let archive_stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let result_slot = std::sync::Arc::new(std::sync::Mutex::new(None::<Result<(), String>>));
    let slot2 = result_slot.clone();
    std::thread::spawn(move || {
        // Extract into a temp dir on the SAME filesystem as dest so
        // rename works (no EXDEV). We create a hidden dir next to dest.
        let tmp = dest.join(format!(".valkyrie-extract-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        if std::fs::create_dir_all(&tmp).is_err() {
            *slot2.lock().unwrap() = Some(Err("cannot create temp dir".to_string()));
            return;
        }
        let result = crate::archive::extract(&archive_path, &tmp)
            .and_then(|()| move_extracted(&tmp, &dest, &archive_stem));
        let _ = std::fs::remove_dir_all(&tmp);
        *slot2.lock().unwrap() = Some(result);
    });
    let root = parent.root();
    glib::source::timeout_add_local(std::time::Duration::from_millis(100), move || {
        let r = result_slot.lock().unwrap().take();
        match r {
            Some(Ok(())) => {
                if let Some(win) = root.clone().and_downcast::<gtk4::Window>() {
                    let _ = win.activate_action("win.reload", None);
                }
                glib::ControlFlow::Break
            }
            Some(Err(e)) => {
                let d = gtk4::AlertDialog::builder()
                    .message(tr("Extraction failed"))
                    .detail(&e)
                    .buttons([tr("OK")])
                    .build();
                if let Some(win) = root.clone().and_downcast::<gtk4::Window>() {
                    d.show(Some(&win));
                }
                glib::ControlFlow::Break
            }
            None => glib::ControlFlow::Continue,
        }
    });
}

/// Compress `files` into a new archive. A small dialog asks for the
/// archive name and format (zip, tar.gz, 7z). The archive is created
/// in the same directory as the first source.
pub fn compress_dialog(parent: &gtk4::Widget, files: &[gio::File]) {
    if files.is_empty() {
        return;
    }
    let first = files[0].clone();
    let Some(dir) = first.parent() else {
        return;
    };
    let dir = dir.clone();

    let dialog = gtk4::Window::builder()
        .modal(false)
        .title(tr("Create Archive"))
        .default_width(360)
        .build();
    if let Some(win) = parent
        .ancestor(gtk4::Window::static_type())
        .and_downcast::<gtk4::Window>()
    {
        dialog.set_transient_for(Some(&win));
    }

    let page = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
    page.set_margin_top(12);
    page.set_margin_bottom(12);
    page.set_margin_start(12);
    page.set_margin_end(12);

    let name_label = gtk4::Label::builder()
        .label(tr("Archive name:"))
        .xalign(0.0)
        .build();
    page.append(&name_label);

    let name_entry = gtk4::Entry::new();
    // Pre-fill with the first source's basename so the user only
    // needs to confirm — not type from scratch.
    let suggested = files
        .first()
        .and_then(|f| f.basename())
        .map(|b| b.to_string_lossy().into_owned())
        .unwrap_or_default();
    name_entry.set_text(&suggested);
    name_entry.set_placeholder_text(Some("archive.zip"));
    page.append(&name_entry);

    let fmt_label = gtk4::Label::builder()
        .label(tr("Format:"))
        .xalign(0.0)
        .margin_top(6)
        .build();
    page.append(&fmt_label);

    let fmt_list = gtk4::StringList::new(&[".zip", ".tar.gz", ".7z"]);
    let fmt_dd = gtk4::DropDown::new(Some(fmt_list), gtk4::Expression::NONE);
    page.append(&fmt_dd);

    let buttons = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    buttons.set_halign(gtk4::Align::End);
    buttons.set_margin_top(6);

    let cancel = gtk4::Button::with_label(&tr("Cancel"));
    let create = gtk4::Button::with_label(&tr("Create"));
    create.add_css_class("suggested-action");
    buttons.append(&cancel);
    buttons.append(&create);
    page.append(&buttons);

    dialog.set_child(Some(&page));

    let win = dialog.clone();
    cancel.connect_clicked(move |_| win.close());

    let win = dialog.clone();
    let name_entry = name_entry.clone();
    let fmt_dd = fmt_dd.clone();
    let dir = dir.clone();
    let files: Vec<gio::File> = files.to_vec();
    let parent = parent.clone();
    create.connect_clicked(move |_| {
        let base = name_entry.text().trim().to_string();
        if base.is_empty() {
            return;
        }
        let ext = match fmt_dd.selected() {
            1 => "tar.gz",
            2 => "7z",
            _ => "zip",
        };
        // Strip any existing extension from the name, then append.
        let stem = base
            .trim_end_matches(".zip")
            .trim_end_matches(".tar.gz")
            .trim_end_matches(".7z");
        let filename = format!("{stem}.{ext}");
        let dest = dir.child(&filename);

        // Collect source paths — skip non-local files silently.
        let sources: Vec<std::path::PathBuf> = files.iter().filter_map(|f| f.path()).collect();
        if sources.is_empty() {
            return;
        }
        let dest_path = dest.path().unwrap_or_default();
        let result_slot = std::sync::Arc::new(std::sync::Mutex::new(None::<Result<(), String>>));
        let slot2 = result_slot.clone();
        std::thread::spawn(move || {
            let src_refs: Vec<&std::path::Path> = sources.iter().map(|p| p.as_path()).collect();
            *slot2.lock().unwrap() = Some(crate::archive::compress(&src_refs, &dest_path, ext));
        });
        let root = parent.root();
        let win2 = win.clone();
        glib::source::timeout_add_local(std::time::Duration::from_millis(100), move || {
            let r = result_slot.lock().unwrap().take();
            match r {
                Some(Ok(())) => {
                    win2.close();
                    // Reload the file view so the new archive appears
                    // with the correct content-type immediately.
                    if let Some(win) = root.clone().and_downcast::<gtk4::Window>() {
                        let _ = win.activate_action("win.reload", None);
                    }
                    glib::ControlFlow::Break
                }
                Some(Err(e)) => {
                    win2.close();
                    let d = gtk4::AlertDialog::builder()
                        .message(tr("Compression failed"))
                        .detail(&e)
                        .buttons([tr("OK")])
                        .build();
                    if let Some(win) = root.clone().and_downcast::<gtk4::Window>() {
                        d.show(Some(&win));
                    }
                    glib::ControlFlow::Break
                }
                None => glib::ControlFlow::Continue,
            }
        });
    });

    dialog.present();
}

/// Move the extracted content from `tmp` into `dest`. If the archive
/// contained exactly one top-level directory, rename it to
/// `archive_stem` — so "Y.zip" extracts to "Y/" instead of the
/// original source folder name "X/". Handles name collisions by
/// appending a counter. Uses copy+remove because `tmp` may be on a
/// different filesystem (tmpfs) than `dest` — `rename` fails with
/// EXDEV across filesystem boundaries.
fn move_extracted(
    tmp: &std::path::Path,
    dest: &std::path::Path,
    archive_stem: &str,
) -> Result<(), String> {
    let Ok(entries) = std::fs::read_dir(tmp) else {
        return Err("extraction produced no output".to_string());
    };
    let items: Vec<_> = entries.filter_map(|e| e.ok()).collect();
    if items.is_empty() {
        return Err("archive is empty".to_string());
    }
    // Single top-level directory → rename to archive_stem
    if items.len() == 1 && items[0].file_type().map(|t| t.is_dir()).unwrap_or(false) {
        let src = items[0].path();
        let target = unique_path(dest, archive_stem);
        move_across_fs(&src, &target)?;
    } else {
        // Multiple items or a single file → move each into dest
        for item in &items {
            let name = item.file_name();
            let target = unique_path(dest, &name.to_string_lossy());
            move_across_fs(&item.path(), &target)?;
        }
    }
    let _ = std::fs::remove_dir_all(tmp);
    Ok(())
}

/// Move a file or directory across filesystem boundaries. Tries
/// `rename` first (fast, same-fs), falls back to copy+remove on EXDEV.
fn move_across_fs(src: &std::path::Path, dst: &std::path::Path) -> Result<(), String> {
    match std::fs::rename(src, dst) {
        Ok(()) => Ok(()),
        Err(e) if e.raw_os_error() == Some(18) => {
            // EXDEV: cross-device link — copy then remove.
            let meta = std::fs::symlink_metadata(src).map_err(|e| format!("stat: {e}"))?;
            if meta.is_dir() {
                copy_dir_recursive(src, dst)?;
            } else {
                std::fs::copy(src, dst).map_err(|e| format!("copy: {e}"))?;
            }
            std::fs::remove_file(src)
                .or_else(|_| std::fs::remove_dir_all(src))
                .map_err(|e| format!("remove: {e}"))?;
            Ok(())
        }
        Err(e) => Err(format!("rename: {e}")),
    }
}

/// Recursively copy a directory tree. Used as the EXDEV fallback for
/// `move_across_fs` — `std::fs::copy` only handles files, not dirs.
fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> Result<(), String> {
    std::fs::create_dir_all(dst).map_err(|e| format!("mkdir: {e}"))?;
    for entry in std::fs::read_dir(src).map_err(|e| format!("readdir: {e}"))? {
        let entry = entry.map_err(|e| format!("readdir: {e}"))?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        let meta = entry.metadata().map_err(|e| format!("stat: {e}"))?;
        if meta.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else if meta.is_symlink() {
            let target = std::fs::read_link(&from).map_err(|e| format!("readlink: {e}"))?;
            std::os::unix::fs::symlink(&target, &to).map_err(|e| format!("symlink: {e}"))?;
        } else {
            std::fs::copy(&from, &to).map_err(|e| format!("copy: {e}"))?;
        }
    }
    Ok(())
}

/// Find a path in `dir` that doesn't collide — append " (1)", " (2)"
/// etc. if `name` already exists.
fn unique_path(dir: &std::path::Path, name: &str) -> std::path::PathBuf {
    let base = dir.join(name);
    if !base.exists() {
        return base;
    }
    // Split name into stem + extension for counter suffix
    let path = std::path::Path::new(name);
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = path
        .extension()
        .map(|s| format!(".{}", s.to_string_lossy()))
        .unwrap_or_default();
    for i in 1..=999 {
        let candidate = dir.join(format!("{stem} ({i}){ext}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    base
}
