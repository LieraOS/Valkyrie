// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// Minimal-diff application of a fresh directory listing onto the
// live store: rows that vanished are removed (descending positions
// keep indices valid), rows that changed are spliced for a fresh
// FileObject (GTK4 rebinds only on a new item pointer), and
// brand-new entries append in one batch. Rows that did not change
// emit no signal at all — that is what kills the visible
// re-population flicker the old swap-a-fresh-DirectoryList refresh
// produced on every monitor event.

use std::collections::{HashMap, HashSet};

use gtk4::gio;
use gtk4::prelude::*;

use crate::file_object::FileObject;

/// Reconcile `store` with the freshly enumerated `infos`.
pub fn apply(store: &gio::ListStore, parent: &gio::File, infos: Vec<gio::FileInfo>) {
    // Index the fresh listing by raw name; duplicate names cannot
    // occur inside one directory.
    let mut new_map: HashMap<String, gio::FileInfo> = HashMap::with_capacity(infos.len());
    let mut order = Vec::with_capacity(infos.len());
    for info in infos {
        let Some(name) = row_name(&info) else {
            continue;
        };
        new_map.insert(name.clone(), info);
        order.push(name);
    }
    let new_names: HashSet<&String> = new_map.keys().collect();

    // Pass 1 — removals, highest position first so every stored
    // position stays valid while splices shift the tail down.
    let n = store.n_items();
    for pos in (0..n).rev() {
        let Some(obj) = store.item(pos) else { continue };
        let Some(fo) = obj.downcast_ref::<FileObject>() else {
            continue;
        };
        if !new_names.contains(&fo.raw_name()) {
            store.remove(pos);
        }
    }

    // Pass 2 — re-index the survivors, then update changed rows in
    // place and queue genuinely new entries for one batched append.
    let mut pos_by_name: HashMap<String, u32> = HashMap::with_capacity(store.n_items() as usize);
    for pos in 0..store.n_items() {
        if let Some(fo) = store
            .item(pos)
            .and_then(|o| o.downcast::<FileObject>().ok())
        {
            pos_by_name.insert(fo.raw_name(), pos);
        }
    }
    let mut adds: Vec<FileObject> = Vec::new();
    for name in order {
        let info = &new_map[&name];
        match pos_by_name.get(&name) {
            Some(&pos) => {
                if let Some(fo) = store
                    .item(pos)
                    .and_then(|o| o.downcast::<FileObject>().ok())
                {
                    if !fo.same_state(info) {
                        // Splice a fresh FileObject: GTK4 list views
                        // rebind reliably only when the item pointer
                        // changes — a same-object items_changed can
                        // leave stale text in the row. The sorter and
                        // filter re-evaluate the replaced entry anyway.
                        store.splice(pos, 1, &[FileObject::new(&parent.child(&name), Some(info))]);
                    }
                }
            }
            None => adds.push(FileObject::new(&parent.child(&name), Some(info))),
        }
    }
    // One splice at the tail — a single items_changed for the whole
    // batch of genuinely new entries.
    store.splice(store.n_items(), 0, &adds);
}

/// Raw filename of a freshly enumerated entry — the stable key the
/// store is indexed by. standard::name is always in ATTRIBUTES; a
/// backend leaving it empty yields no row at all.
fn row_name(info: &gio::FileInfo) -> Option<String> {
    info.attribute_byte_string("standard::name")
        .map(|n| n.to_string())
        .filter(|n| !n.is_empty())
}
