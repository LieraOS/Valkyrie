// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// Undo journal for file operations. Each reversible operation pushes
// an entry; Ctrl+Z pops and reverses the most recent one. Permanent
// delete (with or without wipe) is not undoable — the data is gone.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::gio;
use gtk4::prelude::*;

/// One recorded operation that can be reversed.
#[derive(Clone)]
pub enum Entry {
    /// Copy: sources were copied into `dest_dir`. Undo deletes the
    /// copies (the children of dest_dir named after each source).
    Copy {
        sources: Vec<gio::File>,
        dest_dir: gio::File,
    },
    /// Move: each source moved into `dest_dir`. Undo moves the
    /// resulting children back to their original paths.
    Move {
        originals: Vec<gio::File>,
        dest_dir: gio::File,
    },
    /// Trash: files sent to trash. Undo restores via trash::orig-path.
    Trash { uris: Vec<String> },
}

impl Entry {
    /// Human-readable label for the undo toast/dialog.
    fn label(&self) -> String {
        use crate::i18n::tr;
        match self {
            Entry::Copy { .. } => tr("Undo Copy"),
            Entry::Move { .. } => tr("Undo Move"),
            Entry::Trash { .. } => tr("Undo Trash"),
        }
    }
}

/// The shared journal — one per window, lives in ValkyrieWindow.
#[derive(Default)]
pub struct Journal {
    entries: RefCell<Vec<Entry>>,
}

impl Journal {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record an operation as undoable.
    pub fn push(&self, entry: Entry) {
        self.entries.borrow_mut().push(entry);
        // Cap history — 50 operations is plenty for daily use.
        let mut list = self.entries.borrow_mut();
        if list.len() > 50 {
            list.remove(0);
        }
    }

    /// Whether there is something to undo right now.
    #[allow(dead_code)]
    pub fn can_undo(&self) -> bool {
        !self.entries.borrow().is_empty()
    }

    /// Pop and execute the reverse of the last operation.
    /// Returns a label for UI feedback, or None if the journal is empty.
    pub fn undo(&self) -> Option<String> {
        let entry = self.entries.borrow_mut().pop()?;
        let label = entry.label();
        match entry {
            Entry::Copy { sources, dest_dir } => {
                undo_copy(sources, dest_dir);
            }
            Entry::Move {
                originals,
                dest_dir,
            } => {
                undo_move(originals, dest_dir);
            }
            Entry::Trash { uris } => {
                undo_trash(uris);
            }
        }
        Some(label)
    }
}

/// Delete the copies: for each source, remove the child of dest_dir
/// that was created by the copy (same basename as the source).
fn undo_copy(sources: Vec<gio::File>, dest_dir: gio::File) {
    for src in &sources {
        let Some(name) = src.basename() else { continue };
        let copy = dest_dir.child(&name);
        // Best-effort: if the user already removed it, skip silently.
        let _ = copy.delete(gio::Cancellable::NONE);
    }
}

/// Move files back: each original was moved into dest_dir; the child
/// there now has the original's basename — move it back to the
/// original's parent path.
fn undo_move(originals: Vec<gio::File>, dest_dir: gio::File) {
    for orig in &originals {
        let Some(name) = orig.basename() else {
            continue;
        };
        let moved = dest_dir.child(&name);
        let Some(parent) = orig.parent() else {
            continue;
        };
        // If the moved file is gone (user deleted it after the move),
        // skip silently — partial undo is better than a crash.
        if moved.query_exists(gio::Cancellable::NONE) {
            let _ = moved.move_(
                &parent.child(&name),
                gio::FileCopyFlags::OVERWRITE,
                gio::Cancellable::NONE,
                None,
            );
        }
    }
}

/// Restore trashed files via the trash backend's orig-path metadata.
/// gio::File for trash:// items carries trash::orig-path; restoring
/// is done by moving the trash item back to that path.
fn undo_trash(uris: Vec<String>) {
    // Enumerate trash:/// and match by orig-path, then move back.
    let trash = gio::File::for_uri("trash:///");
    let Ok(en) = trash.enumerate_children(
        "standard::name,trash::orig-path",
        gio::FileQueryInfoFlags::NONE,
        gio::Cancellable::NONE,
    ) else {
        return;
    };
    let orig_set: Vec<String> = uris;
    while let Ok(Some(info)) = en.next_file(gio::Cancellable::NONE) {
        let orig = info
            .attribute_string("trash::orig-path")
            .map(|s| s.to_string())
            .unwrap_or_default();
        if orig_set.iter().any(|u| {
            // Match by path suffix — the orig-path may be absolute
            // while the stored URI was file://...
            u.ends_with(&orig) || orig.ends_with(u.trim_start_matches("file://"))
        }) {
            let item = en.child(&info);
            let dest = gio::File::for_path(&orig);
            let _ = item.move_(
                &dest,
                gio::FileCopyFlags::OVERWRITE,
                gio::Cancellable::NONE,
                None,
            );
        }
    }
}

/// Shared handle so window actions can reach the journal.
pub type JournalHandle = Rc<Journal>;
