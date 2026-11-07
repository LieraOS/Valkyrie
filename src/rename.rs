// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// Rename (F2 / context menu): a small modal prompt prefilled with the
// current name, committed through GFile::set_display_name — the
// portable rename operation that works for local files and every GVFS
// backend that supports renaming.

use gtk4::gio;
use gtk4::prelude::*;

use crate::create_entry;
use crate::i18n::tr;
use crate::operations;

/// Prompt for a new name and rename `file`. No-op on cancel, an
/// unchanged name or when the name cannot be derived.
pub fn rename(parent: &gtk4::Widget, file: &gio::File) {
    let Some(current) = file.basename().map(|b| b.to_string_lossy().into_owned()) else {
        return;
    };
    let dialog_parent = parent.clone();
    let current_name = current.clone();
    let file = file.clone();
    create_entry::ask_name(
        parent,
        &tr("Rename"),
        &current,
        &tr("Rename"),
        None,
        move |new_name, _mode| {
            if new_name == current_name.as_str() {
                return;
            }
            if let Err(e) = file.set_display_name(new_name, gio::Cancellable::NONE) {
                operations::error_dialog(
                    &dialog_parent,
                    &format!("{}: {e}", tr("Could not rename")),
                );
            }
        },
    );
}
