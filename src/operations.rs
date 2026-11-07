// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// File operations: trash, cut/copy/paste between directories.
// Quick one-shot ops (create, trash, rename) run synchronously; the
// heavy batch transfers (paste, drops) run through the progress
// window in progress.rs so they stay cancellable.

use std::cell::RefCell;

use gtk4::prelude::*;

use crate::i18n::{tr, trn};

// Application-wide file clipboard (source files + cut flag). The same
// list is additionally published to the system clipboard in the
// formats other file managers exchange, so copy/cut/paste works
// across applications — e.g. copy on the desktop, paste in Valkyrie.
thread_local! {
    static CLIPBOARD: RefCell<Vec<gio::File>> = const { RefCell::new(Vec::new()) };
    static CLIPBOARD_CUT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Clipboard MIME types carrying file lists: the GLib/GNOME
/// cut-or-copy format plus the plain URI list (KDE and others).
const FILE_MIMES: &[&str] = &["x-special/gnome-copied-files", "text/uri-list"];

/// Whether the clipboard currently holds file entries — our own
/// in-process record first, then the system clipboard's formats.
pub fn clipboard_has_files(widget: &gtk4::Widget) -> bool {
    if CLIPBOARD.with(|c| !c.borrow().is_empty()) {
        return true;
    }
    let formats = widget.clipboard().formats();
    FILE_MIMES.iter().any(|m| formats.contain_mime_type(m))
}

/// Store `files` in the clipboard; `cut` marks a move on paste.
/// The list is published to the system clipboard right away so
/// other applications can paste our files too.
pub fn set_clipboard(files: &[gio::File], cut: bool) {
    CLIPBOARD.with(|c| *c.borrow_mut() = files.to_vec());
    CLIPBOARD_CUT.with(|c| c.set(cut));
    publish_clipboard(files, cut);
}

/// Clipboard state for `file`: Some(true) = staged for cut,
/// Some(false) = staged for copy, None = not on the clipboard.
/// The views use it to ghost cut items and mark copied ones.
pub fn file_clipboard_state(file: &gio::File) -> Option<bool> {
    let uri = file.uri();
    CLIPBOARD.with(|c| {
        c.borrow()
            .iter()
            .any(|f| f.uri() == uri)
            .then(|| CLIPBOARD_CUT.with(|x| x.get()))
    })
}

/// Put the URI list on the display clipboard as
/// x-special/gnome-copied-files (with the cut/copy leader line) and
/// text/uri-list.
fn publish_clipboard(files: &[gio::File], cut: bool) {
    let Some(display) = gtk4::gdk::Display::default() else {
        return;
    };
    // Join instead of append: a trailing newline after the last URI
    // makes classic file managers parse an empty final entry and fail the whole copy
    // with "Vorgang wird nicht unterstützt".
    let uri_list: Vec<String> = files.iter().map(|f| f.uri().to_string()).collect();
    let gnome = format!(
        "{}{}",
        if cut { "cut\n" } else { "copy\n" },
        uri_list.join("\n")
    );
    // text/uri-list strictly wants CRLF separators — several
    // file managers refuse LF-only lists.
    let uris = uri_list.join("\r\n");
    let gnome_provider = gtk4::gdk::ContentProvider::for_bytes(
        "x-special/gnome-copied-files",
        &glib::Bytes::from_owned(gnome.into_bytes()),
    );
    let uri_provider = gtk4::gdk::ContentProvider::for_bytes(
        "text/uri-list",
        &glib::Bytes::from_owned(uris.into_bytes()),
    );
    let provider = gtk4::gdk::ContentProvider::new_union(&[gnome_provider, uri_provider]);
    let clipboard = display.clipboard();
    let _ = clipboard.set_content(Some(&provider));
    // Persist the offer past application exit: without storing, the
    // clipboard dies with the process and paste elsewhere fails.
    clipboard.store_async(glib::Priority::DEFAULT, gio::Cancellable::NONE, |_| {});
}

/// Paste clipboard contents into `dest`; a cut paste becomes a move.
/// The system clipboard is the single source — our own copy is on it
/// already, and lists copied in other applications come back through
/// the same read. Runs through the progress window so large transfers
/// stay cancellable.
pub fn paste(dest: &gio::File, parent: &gtk4::Widget) {
    let clipboard = parent.clipboard();
    let dest = dest.clone();
    let parent = parent.clone();
    glib::MainContext::default().spawn_local(async move {
        let Ok((stream, mime)) = clipboard
            .read_future(FILE_MIMES, glib::Priority::DEFAULT)
            .await
        else {
            eprintln!("valkyrie: paste: clipboard read failed");
            return;
        };
        let Ok(bytes) = stream.read_bytes(1 << 20, gio::Cancellable::NONE) else {
            eprintln!("valkyrie: paste: read_bytes failed");
            return;
        };
        let text = String::from_utf8_lossy(&bytes);
        eprintln!(
            "valkyrie: paste: read {} bytes (mime={})",
            bytes.len(),
            mime
        );
        let (cut, files) = parse_clipboard(&text, mime.as_str());

        if !files.is_empty() {
            crate::progress::copy_move(&parent, files.clone(), dest, cut);
            // Staging is consumed by the paste — cut AND copy
            // clear the visual marker (the system clipboard keeps
            // its data; a second paste just works, but items must
            // not stay dimmed).
            CLIPBOARD.with(|c| c.borrow_mut().clear());
        }
    });
}

/// Turn clipboard text into (cut flag, file list). The gnome format
/// starts with a "cut"/"copy" leader line; a plain uri-list is always
/// a copy. '#' comment lines are skipped per the uri-list spec.
fn parse_clipboard(text: &str, mime: &str) -> (bool, Vec<gio::File>) {
    let mut lines = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'));
    let mut cut = false;
    if mime == "x-special/gnome-copied-files" {
        cut = matches!(lines.next(), Some("cut"));
    }
    let files = lines.map(gio::File::for_uri).collect();
    (cut, files)
}

/// Move `file` to the trash, with an error dialog on failure.
pub fn trash(file: &gio::File, parent: &impl IsA<gtk4::Widget>) {
    if let Err(e) = file.trash(gio::Cancellable::NONE) {
        error_dialog(parent, &format!("{}: {e}", tr("Could not move to trash")));
    }
}

/// Permanently delete `files` after confirmation. Unlike trash, this
/// cannot be undone — the confirmation dialog makes that explicit.
/// Runs through the progress window so multi-item deletes stay
/// cancellable and show a progress bar.
pub fn delete_permanently(parent: &gtk4::Widget, files: &[gio::File]) {
    let files = files.to_vec();
    let n = files.len();
    let body = if n == 1 {
        tr("This item will be deleted permanently. This cannot be undone.")
    } else {
        trn(
            "This item will be deleted permanently. This cannot be undone.",
            "These items will be deleted permanently. This cannot be undone.",
            n as u32,
        )
    };
    crate::trash_actions::confirm(parent, &tr("Delete permanently?"), &body, move |parent| {
        let wipe = if crate::settings::secure_delete() {
            crate::settings::secure_delete_passes() as u32
        } else {
            0
        };
        crate::progress::delete(parent, files.clone(), wipe);
    });
}

/// Create a folder `name` inside `dir`. Name collisions are resolved
/// automatically ("name", "name (2)", ...); failures surface as a dialog.
/// `mode` carries the permission bits the user picked in the create
/// dialog; `None` leaves the system defaults (umask) untouched.
pub fn create_folder(dir: &gio::File, name: &str, mode: Option<u32>) {
    let target = unique_destination(dir, name);
    if let Err(e) = target.make_directory(gio::Cancellable::NONE) {
        eprintln!("valkyrie: create_folder {}: {e}", target.uri());
        return;
    }
    apply_mode(&target, mode);
}

/// Create a document `name` inside `dir` — an empty file, or a copy of
/// `template` when the user picked one from ~/Templates.
/// `mode` carries the permission bits the user picked in the create
/// dialog; `None` leaves the system defaults (umask) untouched.
pub fn create_document(
    dir: &gio::File,
    name: &str,
    template: Option<&gio::File>,
    mode: Option<u32>,
) {
    let target = unique_destination(dir, name);
    let result = match template {
        Some(t) => t
            .copy(
                &target,
                gio::FileCopyFlags::NONE,
                gio::Cancellable::NONE,
                None::<&mut dyn FnMut(i64, i64)>,
            )
            .map(|_| ()),
        None => target
            .create(gio::FileCreateFlags::NONE, gio::Cancellable::NONE)
            .map(|_| ()),
    };
    if let Err(e) = result {
        eprintln!("valkyrie: create_document {}: {e}", target.uri());
        return;
    }
    apply_mode(&target, mode);
}

/// Write the optional permission bits onto a freshly created entry.
/// Backends without POSIX modes silently refuse — the entry stays
/// usable, so the error is intentionally ignored.
fn apply_mode(file: &gio::File, mode: Option<u32>) {
    let Some(mode) = mode else {
        return;
    };
    let _ = file.set_attribute_uint32(
        "unix::mode",
        mode,
        gio::FileQueryInfoFlags::NONE,
        gio::Cancellable::NONE,
    );
}

/// `dest/name`, or "stem (2).ext", "stem (3).ext", ... when occupied.
/// pub(crate): the progress worker resolves collisions through the
/// same helper so paste and drops behave identically.
pub(crate) fn unique_destination(dest: &gio::File, name: &str) -> gio::File {
    let candidate = dest.child(name);
    if !candidate.query_exists(gio::Cancellable::NONE) {
        return candidate;
    }
    // Insert the counter before the extension: "file.txt" → "file (2).txt".
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{e}")),
        _ => (name.to_string(), String::new()),
    };
    for i in 2.. {
        let candidate = dest.child(format!("{stem} ({i}){ext}"));
        if !candidate.query_exists(gio::Cancellable::NONE) {
            return candidate;
        }
    }
    unreachable!()
}

/// Minimal modal error dialog attached to `parent`'s window.
/// pub(crate): rename, trash restore and the progress runner surface
/// their failures through the same dialog.
pub(crate) fn error_dialog(parent: &impl IsA<gtk4::Widget>, message: &str) {
    let window = parent
        .ancestor(gtk4::Window::static_type())
        .and_then(|w| w.downcast::<gtk4::Window>().ok());
    let dialog = gtk4::AlertDialog::builder()
        .message(tr("File operation failed"))
        .detail(message)
        .build();
    dialog.show(window.as_ref());
}

/// Modal informational dialog attached to `parent`'s window — used
/// for outcomes that succeeded but deserve a confirmation, e.g. the
/// copy integrity verification summary.
pub(crate) fn info_dialog(parent: &impl IsA<gtk4::Widget>, message: &str) {
    let window = parent
        .ancestor(gtk4::Window::static_type())
        .and_then(|w| w.downcast::<gtk4::Window>().ok());
    let dialog = gtk4::AlertDialog::builder()
        .message(tr("File operation finished"))
        .detail(message)
        .build();
    dialog.show(window.as_ref());
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh empty directory inside the system temp dir; each test
    /// gets a unique path so parallel runs never interfere.
    fn temp_dir(tag: &str) -> gio::File {
        let path = std::env::temp_dir().join(format!("valkyrie-test-{}-{tag}", std::process::id()));
        let dir = gio::File::for_path(&path);
        let _ = dir.make_directory(gio::Cancellable::NONE);
        dir
    }

    fn touch(dir: &gio::File, name: &str) {
        dir.child(name)
            .create(gio::FileCreateFlags::NONE, gio::Cancellable::NONE)
            .unwrap();
    }

    fn cleanup(dir: &gio::File) {
        // Tests only ever create flat dirs with plain files.
        if let Ok(en) = dir.enumerate_children(
            "standard::name",
            gio::FileQueryInfoFlags::NONE,
            gio::Cancellable::NONE,
        ) {
            while let Ok(Some(info)) = en.next_file(gio::Cancellable::NONE) {
                let _ = en.child(&info).delete(gio::Cancellable::NONE);
            }
        }
        let _ = dir.delete(gio::Cancellable::NONE);
    }

    #[test]
    fn unique_destination_free_name() {
        let dir = temp_dir("unique-free");
        let target = unique_destination(&dir, "neu.txt");
        assert_eq!(target.basename().unwrap().to_str().unwrap(), "neu.txt");
        cleanup(&dir);
    }

    #[test]
    fn unique_destination_collisions_get_counters() {
        let dir = temp_dir("unique-collide");
        touch(&dir, "neu.txt");
        touch(&dir, "neu (2).txt");
        assert_eq!(
            unique_destination(&dir, "neu.txt")
                .basename()
                .unwrap()
                .to_str()
                .unwrap(),
            "neu (3).txt"
        );
        cleanup(&dir);
    }

    #[test]
    fn unique_destination_extensionless() {
        let dir = temp_dir("unique-noext");
        touch(&dir, "ordner");
        assert_eq!(
            unique_destination(&dir, "ordner")
                .basename()
                .unwrap()
                .to_str()
                .unwrap(),
            "ordner (2)"
        );
        cleanup(&dir);
    }

    /// Read the POSIX permission bits of a created entry.
    fn mode_of(file: &gio::File) -> u32 {
        file.query_info(
            "unix::mode",
            gio::FileQueryInfoFlags::NONE,
            gio::Cancellable::NONE,
        )
        .unwrap()
        .attribute_uint32("unix::mode")
            & 0o777
    }

    #[test]
    fn create_folder_applies_requested_mode() {
        let dir = temp_dir("create-mode");
        create_folder(&dir, "privat", Some(0o700));
        assert_eq!(mode_of(&dir.child("privat")), 0o700);
        cleanup(&dir);
    }

    #[test]
    fn create_folder_none_mode_uses_defaults() {
        let dir = temp_dir("create-default");
        create_folder(&dir, "normal", None);
        let mode = mode_of(&dir.child("normal"));
        // Any sane umask leaves owner bits fully set.
        assert!(mode & 0o700 == 0o700 || mode & 0o700 != 0);
        assert!(dir.child("normal").query_exists(gio::Cancellable::NONE));
        cleanup(&dir);
    }

    #[test]
    fn create_document_applies_requested_mode() {
        let dir = temp_dir("create-docmode");
        create_document(&dir, "skript.sh", None, Some(0o755));
        assert_eq!(mode_of(&dir.child("skript.sh")), 0o755);
        cleanup(&dir);
    }

    #[test]
    fn clipboard_gnome_cut_parses_as_move() {
        let (cut, files) = parse_clipboard(
            "cut\nfile:///tmp/a.txt\nfile:///tmp/b.txt\n",
            "x-special/gnome-copied-files",
        );
        assert!(cut);
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].uri().as_str(), "file:///tmp/a.txt");
    }

    #[test]
    fn clipboard_gnome_copy_parses_as_copy() {
        let (cut, files) =
            parse_clipboard("copy\nfile:///tmp/a.txt\n", "x-special/gnome-copied-files");
        assert!(!cut);
        assert_eq!(files.len(), 1);
    }

    #[test]
    fn clipboard_uri_list_is_always_copy() {
        let (cut, files) = parse_clipboard(
            "# comment\nfile:///tmp/a.txt\nfile:///tmp/b.txt\n",
            "text/uri-list",
        );
        assert!(!cut);
        assert_eq!(files.len(), 2);
    }

    #[test]
    fn clipboard_malformed_input_yields_no_panic() {
        let (cut, files) = parse_clipboard("", "x-special/gnome-copied-files");
        assert!(!cut);
        assert!(files.is_empty());
        let (_, files) = parse_clipboard("not-a-uri\n", "text/uri-list");
        assert_eq!(files.len(), 1); // for_uri never fails, odd schemes keep their gio::File
    }
}
