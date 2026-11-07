// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// Recursive search (Ctrl+F / toolbar button): a SearchBar drops down
// under the toolbar; typing runs a case-insensitive substring match
// over the whole subtree on a worker thread. Hits stream into a
// ListStore shown through the normal view pipeline, so thumbnails,
// sorting and activation keep working. Every keystroke cancels the
// previous enumeration via a shared Cancellable.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use gtk4::prelude::*;
use gtk4::{gio, glib};

use crate::browser_tab::BrowserTab;
use crate::file_object::FileObject;
use crate::view_model::ATTRIBUTES;

/// Results stop streaming past this point — a runaway query over a
/// huge subtree must not grow the model without bound.
const MAX_RESULTS: usize = 2000;

/// Debounce between the last keystroke and the enumeration start.
const DEBOUNCE_MS: u64 = 250;

/// Messages from the worker thread to the UI poller. Only the File
/// crosses threads — gio::FileInfo is not Send, so the poller queries
/// the display attributes on the main thread (a fast local read for
/// files the enumerator just touched).
enum Msg {
    Hit(gio::File),
    Done,
}

/// The tab's search widgets and the bookkeeping for the running job.
#[derive(Clone)]
pub struct SearchUi {
    pub bar: gtk4::SearchBar,
    entry: gtk4::SearchEntry,
    /// True while the view shows search results instead of the dir.
    searching: Rc<Cell<bool>>,
    /// Suppresses the notify handler while we close the bar
    /// programmatically during navigation.
    suppress: Rc<Cell<bool>>,
    cancel: Rc<RefCell<Option<gio::Cancellable>>>,
    /// Debounce generation — every keystroke bumps it, a scheduled
    /// query only runs if its generation is still current. Removing a
    /// fired SourceId panics inside glib, so staleness beats removal.
    generation: Rc<Cell<u64>>,
}

impl SearchUi {
    pub fn new() -> Self {
        let entry = gtk4::SearchEntry::builder().hexpand(true).build();
        let bar = gtk4::SearchBar::builder().show_close_button(true).build();
        bar.set_child(Some(&entry));
        bar.connect_entry(&entry);
        Self {
            bar,
            entry,
            searching: Rc::new(Cell::new(false)),
            suppress: Rc::new(Cell::new(false)),
            cancel: Rc::new(RefCell::new(None)),
            generation: Rc::new(Cell::new(0)),
        }
    }

    /// Ctrl+F / toolbar button — drop the bar and focus the entry.
    pub fn activate(&self) {
        self.bar.set_search_mode(true);
        self.entry.grab_focus();
    }

    /// Close an open search without touching the model — used on
    /// navigation, which swaps in the new directory's model anyway.
    /// The entry is cleared so the next Ctrl+F starts fresh.
    pub fn deactivate(&self) {
        self.suppress.set(true);
        self.bar.set_search_mode(false);
        self.entry.set_text("");
        self.suppress.set(false);
        self.cancel_running();
    }

    /// Invalidate the pending debounce and cancel the running
    /// enumeration.
    fn cancel_running(&self) {
        self.generation.set(self.generation.get() + 1);
        if let Some(c) = self.cancel.borrow_mut().take() {
            c.cancel();
        }
        self.searching.set(false);
    }
}

/// Wire the search entry to the tab's view. Called once per tab.
pub fn wire(tab: &BrowserTab, ui: &SearchUi) {
    // Keystrokes debounce into a fresh recursive search.
    {
        let entry = ui.entry.clone();
        entry.connect_search_changed({
            let tab = tab.clone();
            let ui = ui.clone();
            move |entry| {
                let query = entry.text().to_string();
                let gen = ui.generation.get() + 1;
                ui.generation.set(gen);
                let tab = tab.clone();
                let ui2 = ui.clone();
                glib::timeout_add_local(Duration::from_millis(DEBOUNCE_MS), move || {
                    if ui2.generation.get() == gen {
                        run_query(&tab, &ui2, &query);
                    }
                    glib::ControlFlow::Break
                });
            }
        });
    }
    // Esc in the entry / the close button both end the search.
    {
        let entry = ui.entry.clone();
        let bar = ui.bar.clone();
        entry.connect_stop_search(move |_| {
            bar.set_search_mode(false);
        });
    }
    // Whenever the bar closes (close button, Esc, or an outside
    // action), restore the directory model if results were shown.
    {
        let bar = ui.bar.clone();
        bar.connect_search_mode_enabled_notify({
            let tab = tab.clone();
            let ui = ui.clone();
            move |bar| {
                if bar.is_search_mode() || ui.suppress.get() || !ui.searching.get() {
                    return;
                }
                ui.cancel_running();
                tab.active_view().leave_search();
            }
        });
    }
}

/// Start (or reset) a recursive search for `query` below the tab's
/// current location. An empty query restores the directory listing.
fn run_query(tab: &BrowserTab, ui: &SearchUi, query: &str) {
    if let Some(c) = ui.cancel.borrow_mut().take() {
        c.cancel();
    }
    let Some(root) = tab.location() else { return };

    if query.is_empty() {
        if ui.searching.replace(false) {
            tab.active_view().leave_search();
        }
        return;
    }
    ui.searching.set(true);

    // Fresh result store swapped into the views immediately — hits
    // stream in live and the status bar reports the growing count.
    let store = gio::ListStore::new::<FileObject>();
    tab.active_view().show_search_results(&store);

    let (tx, rx) = std::sync::mpsc::channel::<Msg>();
    let cancel = gio::Cancellable::new();
    *ui.cancel.borrow_mut() = Some(cancel.clone());
    let needle = query.to_lowercase();
    std::thread::spawn(move || {
        // Device id of the search root — recursion stays on this
        // filesystem (find -xdev semantics) so pseudo-filesystems
        // like /proc, /sys, /run and mounted volumes are skipped.
        // Backends without unix::device (gvfs) report None and keep
        // the previous unrestricted behaviour.
        let root_dev = root
            .query_info(
                "unix::device",
                gio::FileQueryInfoFlags::NONE,
                gio::Cancellable::NONE,
            )
            .ok()
            .filter(|i| i.has_attribute("unix::device"))
            .map(|i| i.attribute_uint32("unix::device"));
        let mut count = 0usize;
        walk(&root, &needle, &cancel, &tx, &mut count, root_dev);
        let _ = tx.send(Msg::Done);
    });

    // Drain hits on the main context — FileObjects are built here
    // because they are GObjects owned by the GTK thread.
    glib::timeout_add_local(Duration::from_millis(30), move || loop {
        match rx.try_recv() {
            Ok(Msg::Hit(file)) => {
                // Entries that vanished between enumeration and this
                // query (procfs/sysfs churn, removed files) get no
                // FileObject at all — without FileInfo they would
                // render as generic files even when they were dirs.
                if let Ok(info) = file.query_info(
                    ATTRIBUTES,
                    gio::FileQueryInfoFlags::NONE,
                    gio::Cancellable::NONE,
                ) {
                    store.append(&FileObject::new(&file, Some(&info)));
                }
            }
            Ok(Msg::Done) => return glib::ControlFlow::Break,
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                return glib::ControlFlow::Continue;
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                return glib::ControlFlow::Break;
            }
        }
    });
}

/// Recursive case-insensitive substring match on name + display name.
/// Runs off-thread; checks the cancellable on every step so a new
/// keystroke or a closed bar stops it quickly.
///
/// NOFOLLOW_SYMLINKS keeps the enumerator honest about what an entry
/// is: a symlinked directory (merged-usr /bin -> /usr/bin etc.) is
/// NOT descended into, so every file is reported once under its real
/// path instead of twice under aliased paths. Hits still resolve for
/// display — the poller queries attributes with the default
/// follow-symlinks behaviour.
/// `root_dev` limits recursion to the device the search started on.
fn walk(
    dir: &gio::File,
    needle: &str,
    cancel: &gio::Cancellable,
    tx: &std::sync::mpsc::Sender<Msg>,
    count: &mut usize,
    root_dev: Option<u32>,
) {
    // The match is case-insensitive; lowercase here so the function
    // stays correct for every caller, not just the debounced entry.
    let needle = needle.to_lowercase();
    let needle = needle.as_str();
    if cancel.is_cancelled() || *count >= MAX_RESULTS {
        return;
    }
    // unix::device is needed for the filesystem boundary check on top
    // of the display attributes.
    let attributes = format!("{ATTRIBUTES},unix::device");
    let Ok(en) = dir.enumerate_children(
        &attributes,
        gio::FileQueryInfoFlags::NOFOLLOW_SYMLINKS,
        Some(cancel),
    ) else {
        return;
    };
    while let Ok(Some(info)) = en.next_file(Some(cancel)) {
        if cancel.is_cancelled() || *count >= MAX_RESULTS {
            return;
        }
        // Child GFile needs standard::name — it is part of ATTRIBUTES.
        let child = en.child(&info);
        let name = info
            .attribute_byte_string("standard::name")
            .map(|n| n.to_string().to_lowercase())
            .unwrap_or_default();
        let display = info
            .attribute_as_string("standard::display-name")
            .map(|n| n.to_string().to_lowercase())
            .unwrap_or_default();
        if name.contains(needle) || display.contains(needle) {
            *count += 1;
            if tx.send(Msg::Hit(child.clone())).is_err() {
                return;
            }
        }
        if info.has_attribute("standard::type") && info.file_type() == gio::FileType::Directory {
            // A child on a different device is a mount point — stop
            // the descent there (find -xdev semantics). Entries whose
            // backend reports no device keep the old behaviour.
            if let Some(root_dev) = root_dev {
                if info.has_attribute("unix::device")
                    && info.attribute_uint32("unix::device") != root_dev
                {
                    continue;
                }
            }
            walk(&child, needle, cancel, tx, count, root_dev);
        }
    }
}

#[cfg(test)]
mod tests;
