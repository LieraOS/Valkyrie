// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// External selection requests — FileManager1's ShowItems is the
// caller: a fresh window opens the parent directory and the target
// file gets selected the moment the async enumeration (or a later
// diff) surfaces its row. Names that never appear stay pending and
// are re-checked on every items_changed at negligible cost.

use gtk4::gio;
use gtk4::prelude::*;

use super::{find_list_widget, FileView};
use crate::file_object::FileObject;

impl FileView {
    /// Select `files` — immediately for rows already loaded, pending
    /// for ones the enumeration has not produced yet (a fresh window
    /// still filling its model takes this path).
    pub fn select_files(&self, files: &[gio::File]) {
        let names: Vec<String> = files
            .iter()
            .filter_map(|f| f.basename())
            .map(|b| b.to_string_lossy().into_owned())
            .collect();
        self.pending_select.borrow_mut().extend(names);
        self.apply_pending_select();
    }

    /// Consume whatever pending selections the current model can
    /// already satisfy — runs on every items_changed so late-arriving
    /// rows get picked up as they appear.
    pub(super) fn apply_pending_select(&self) {
        let wanted: Vec<String> = self.pending_select.borrow().clone();
        if wanted.is_empty() {
            return;
        }
        let Some(model) = self.model.borrow().clone() else {
            return;
        };
        let mut found: Vec<u32> = Vec::new();
        let mut hits: Vec<String> = Vec::new();
        for pos in 0..model.list.n_items() {
            let Some(fo) = model
                .list
                .item(pos)
                .and_then(|o| o.downcast::<FileObject>().ok())
            else {
                continue;
            };
            if let Some(i) = wanted.iter().position(|n| *n == fo.raw_name()) {
                found.push(pos);
                hits.push(wanted[i].clone());
            }
        }
        if found.is_empty() {
            return;
        }
        // Keep the misses pending — a later diff may still surface
        // them (file being written, slow enumeration page).
        self.pending_select
            .borrow_mut()
            .retain(|n| !hits.contains(n));
        for (i, pos) in found.iter().enumerate() {
            for sel in [
                self.sel_list.borrow().clone(),
                self.sel_icons.borrow().clone(),
                self.sel_compact.borrow().clone(),
            ]
            .into_iter()
            .flatten()
            {
                // The first hit replaces the selection; additional
                // hits accumulate (ShowItems with several URIs).
                sel.select_item(*pos, i == 0);
            }
        }
        self.scroll_to_row(found[0]);
    }

    /// Bring `pos` into view on whichever list widget is currently
    /// visible — icon grid, column list and compact share the same
    /// model positions, so the row index transfers directly.
    fn scroll_to_row(&self, pos: u32) {
        let Some(page) = self.widget.visible_child() else {
            return;
        };
        let Some(w) = find_list_widget(&page, 6) else {
            return;
        };
        if let Some(grid) = w.downcast_ref::<gtk4::GridView>() {
            grid.scroll_to(pos, gtk4::ListScrollFlags::FOCUS, None);
        } else if let Some(list) = w.downcast_ref::<gtk4::ColumnView>() {
            list.scroll_to(
                pos,
                None::<&gtk4::ColumnViewColumn>,
                gtk4::ListScrollFlags::FOCUS,
                None,
            );
        }
    }
}
