// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// Selection queries and notifications for FileView, split out so the
// parent module stays under the file-size limit. The child module sees
// the parent's private fields — the API stays identical.

use gio::prelude::{FileExt, ListModelExt};
use glib::prelude::Cast;
use gtk4::prelude::SelectionModelExt;

use super::FileView;

impl FileView {
    /// The MultiSelection model of the currently visible view.
    fn active_selection(&self) -> Option<gtk4::MultiSelection> {
        if self.icon_mode.get() {
            self.sel_icons.borrow().clone()
        } else {
            self.sel_list.borrow().clone()
        }
    }

    /// All selected FileObjects of the currently visible view.
    pub fn selected_objects(&self) -> Vec<crate::file_object::FileObject> {
        let (Some(sel), Some(model)) = (self.active_selection(), self.model.borrow().clone())
        else {
            return Vec::new();
        };
        // Walk positions directly — Bitset::iter is not exposed by the
        // binding, is_selected is the portable query.
        let mut out = Vec::new();
        for pos in 0..model.list.n_items() {
            if sel.is_selected(pos) {
                if let Some(obj) = model
                    .list
                    .item(pos)
                    .and_then(|i| i.downcast::<crate::file_object::FileObject>().ok())
                {
                    out.push(obj);
                }
            }
        }
        out
    }

    /// Files an action on `picked` should apply to: the whole
    /// selection when the picked row is part of it, otherwise just
    /// that row. Standard file-manager rule — right-clicking inside
    /// a multi-selection must not silently drop the other files.
    pub fn action_targets(&self, picked: &crate::file_object::FileObject) -> Vec<gtk4::gio::File> {
        let picked_file = picked.file();
        let selected: Vec<gtk4::gio::File> =
            self.selected_objects().iter().map(|o| o.file()).collect();
        if selected.iter().any(|f| f.equal(&picked_file)) {
            selected
        } else {
            vec![picked_file]
        }
    }

    /// Select every visible row (Ctrl+A).
    pub fn select_all(&self) {
        if let Some(sel) = self.active_selection() {
            sel.select_all();
        }
    }

    /// Clear the selection — a click on empty background deselects.
    pub fn unselect_all(&self) {
        if let Some(sel) = self.active_selection() {
            sel.unselect_all();
        }
    }

    /// Register a listener fired on every selection change — the tab's
    /// status bar recomputes count and size of the marked items.
    pub fn connect_selection_notify<F: Fn() + 'static>(&self, f: F) {
        self.sel_notify.borrow_mut().push(Box::new(f));
    }

    /// Both MultiSelection models report their selection-changed
    /// signal here (wired in view_widgets on view construction).
    pub(crate) fn emit_selection_notify(&self) {
        for cb in self.sel_notify.borrow().iter() {
            cb();
        }
    }

    /// (marked item count, combined byte size) of the current
    /// selection. Directory sizes are not counted — summing them
    /// would require a recursive scan per selection change.
    pub fn selection_summary(&self) -> (u32, u64) {
        let mut count = 0u32;
        let mut bytes = 0u64;
        for obj in self.selected_objects() {
            count += 1;
            if !obj.is_dir() {
                bytes = bytes.saturating_add(obj.size_raw().max(0) as u64);
            }
        }
        (count, bytes)
    }
}
