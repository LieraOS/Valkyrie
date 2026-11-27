// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// The data pipeline behind the file views:
//   DirModel        — live ListStore (enumeration, monitor, minimal diffs)
//   FilterListModel — drops hidden entries unless enabled
//   SortListModel   — directories first, then the chosen field
// Filter and sorter state live in GSettings so they survive restarts.

use std::cell::Cell;
use std::cmp::Ordering;
use std::rc::Rc;

use gtk4::gio;
#[cfg(test)]
use gtk4::glib;
use gtk4::prelude::*;

use crate::file_object::FileObject;

/// Attributes loaded for every directory entry. is-hidden is needed for
/// the hidden-file filter; the rest feeds the list columns and sorting.
/// standard::icon is deliberately absent: FileObject::icon() derives
/// the icon lazily from the content type (per-type cache), which is
/// ~45ms faster per 5000 files than querying it during enumeration.
/// thumbnail::path and access::can-write are likewise absent — the
/// thumbnail path is queried lazily per visible cell and writability
/// only when a context menu or the properties dialog asks for it.
pub const ATTRIBUTES: &str = concat!(
    "standard::name,standard::display-name,",
    "standard::size,standard::content-type,standard::type,",
    "standard::is-hidden,time::modified"
);

/// Fields the user can sort by (context menu "Arrange Items").
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SortField {
    Name,
    Size,
    Type,
    Modified,
}

impl SortField {
    /// Map a GSettings string to the enum; anything unknown is "name".
    pub fn from_str(value: &str) -> Self {
        match value {
            "size" => Self::Size,
            "type" => Self::Type,
            "modified" => Self::Modified,
            _ => Self::Name,
        }
    }

    /// The string persisted in the sort-field GSettings key.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Size => "size",
            Self::Type => "type",
            Self::Modified => "modified",
        }
    }
}

/// The filterable/sortable model for one directory. Recreated per
/// location; toggles mutate the shared filter/sorter in place so the
/// visible view updates without a reload.
#[derive(Clone)]
pub struct ViewModel {
    /// Final model handed to the list widgets.
    pub list: gtk4::SortListModel,
    filter: gtk4::CustomFilter,
    show_hidden: Rc<Cell<bool>>,
    /// Live directory store — keeps the monitor watch alive and owns
    /// the refresh path. None for search results (for_entries):
    /// they do not track a directory.
    dir: Option<crate::dir_model::DirModel>,
}

impl ViewModel {
    /// Build the model pipeline for `location`, seeded from GSettings.
    /// The DirModel fills its store asynchronously and keeps it live
    /// through the directory monitor.
    pub fn new(location: &gio::File) -> Self {
        let dir = crate::dir_model::DirModel::new(location);
        let mut this = Self::from_base(dir.store.clone().upcast());
        this.dir = Some(dir);
        this
    }

    /// Pipeline over an already-filled store of FileObjects — used by
    /// the recursive search, which collects its own entries instead of
    /// enumerating a single directory.
    pub fn for_entries(store: &gio::ListStore) -> Self {
        Self::from_base(store.clone().upcast())
    }

    /// Shared tail of the pipeline: hidden-file filter plus sorting on
    /// top of any FileObject-bearing base model.
    fn from_base(base: gio::ListModel) -> Self {
        let show_hidden = Rc::new(Cell::new(crate::settings::show_hidden()));
        let flag = show_hidden.clone();
        let filter = gtk4::CustomFilter::new(move |obj| {
            flag.get()
                || !obj
                    .downcast_ref::<FileObject>()
                    .map(|o| o.is_hidden())
                    .unwrap_or(false)
        });
        let filtered = gtk4::FilterListModel::new(Some(base), Some(filter.clone()));

        // The real sorter is attached later by attach_view_sorter:
        // directories-first plus the ColumnView's own sorter, so
        // column header clicks and menu actions drive one pipeline.
        let list = gtk4::SortListModel::new(
            Some(filtered),
            Some(dirs_first_sorter().upcast::<gtk4::Sorter>()),
        );

        Self {
            list,
            filter,
            show_hidden,
            dir: None,
        }
    }

    /// Re-enumerate the directory and apply a minimal diff — rows
    /// that vanished drop, rows that changed emit a 1-item signal,
    /// new rows append. Selections, sorter and widgets all stay put.
    /// No-op for search-result models.
    pub fn refresh_contents(&self) {
        if let Some(dir) = &self.dir {
            dir.refresh();
        }
    }

    /// Forward enumeration-error notifications to `f` — search
    /// result models have no directory and never fire.
    pub fn connect_error_notify<F: Fn(Option<&glib::Error>) + 'static>(&self, f: F) {
        if let Some(dir) = &self.dir {
            dir.connect_error_notify(f);
        }
    }

    /// Plug the ColumnView's sorter under a directories-first sorter.
    /// Called for every fresh model (navigation, search) while the
    /// ColumnView persists — its sorter keeps the active column and
    /// direction across model swaps for free.
    pub fn attach_view_sorter(&self, view_sorter: &gtk4::Sorter) {
        let multi = gtk4::MultiSorter::new();
        multi.append(dirs_first_sorter());
        multi.append(view_sorter.clone());
        self.list.set_sorter(Some(&multi.upcast::<gtk4::Sorter>()));
    }

    pub fn show_hidden(&self) -> bool {
        self.show_hidden.get()
    }

    /// Toggle hidden-file visibility and persist the choice.
    pub fn set_show_hidden(&self, show: bool) {
        self.show_hidden.set(show);
        crate::settings::set_show_hidden(show);
        self.filter.changed(gtk4::FilterChange::Different);
    }
}

/// Leading sorter of the model chain: directories always stay above
/// files, unaffected by the column sort direction.
pub(crate) fn dirs_first_sorter() -> gtk4::CustomSorter {
    gtk4::CustomSorter::new(|a, b| {
        match (
            a.downcast_ref::<FileObject>(),
            b.downcast_ref::<FileObject>(),
        ) {
            (Some(a), Some(b)) => match (a.is_dir(), b.is_dir()) {
                (true, false) => gtk4::Ordering::Smaller,
                (false, true) => gtk4::Ordering::Larger,
                _ => gtk4::Ordering::Equal,
            },
            _ => gtk4::Ordering::Equal,
        }
    })
}

/// Per-column sorter for `field` with a case-insensitive name
/// tiebreak so equal values never order randomly. The ColumnView
/// inverts the whole comparison when the direction flips — same
/// semantics the previous custom sorter had.
pub(crate) fn field_sorter(field: SortField) -> gtk4::CustomSorter {
    gtk4::CustomSorter::new(move |a, b| {
        match (
            a.downcast_ref::<FileObject>(),
            b.downcast_ref::<FileObject>(),
        ) {
            (Some(a), Some(b)) => compare_field(a, b, field),
            _ => gtk4::Ordering::Equal,
        }
    })
}

/// Compare two entries on one field, tie-broken by natural name order.
fn compare_field(a: &FileObject, b: &FileObject, field: SortField) -> gtk4::Ordering {
    let by_field = match field {
        // Name field: compare with natural sort directly (no tiebreak
        // needed — natural compare is the primary sort).
        SortField::Name => {
            return a.with_sort_key(|ka| {
                b.with_sort_key(|kb| match natural_cmp(ka, kb) {
                    Ordering::Less => gtk4::Ordering::Smaller,
                    Ordering::Equal => gtk4::Ordering::Equal,
                    Ordering::Greater => gtk4::Ordering::Larger,
                })
            });
        }
        SortField::Size => a.size_raw().cmp(&b.size_raw()),
        SortField::Type => a.content_type().cmp(&b.content_type()),
        SortField::Modified => a.modified_raw().cmp(&b.modified_raw()),
    };
    // Tiebreak for non-name fields: natural name sort.
    let order =
        by_field.then_with(|| a.with_sort_key(|ka| b.with_sort_key(|kb| natural_cmp(ka, kb))));
    match order {
        Ordering::Less => gtk4::Ordering::Smaller,
        Ordering::Equal => gtk4::Ordering::Equal,
        Ordering::Greater => gtk4::Ordering::Larger,
    }
}

/// Natural string comparison: "file_2" < "file_10" < "file_100".
/// Splits the string into alternating alphabetic and numeric chunks,
/// compares alphabetic chunks lexically and numeric chunks by value.
/// This matches the sort order users expect from file managers
/// (classic file managers use the same approach via g_utf8_collate_key_for_filename).
fn natural_cmp(a: &str, b: &str) -> Ordering {
    let mut ai = a.chars().peekable();
    let mut bi = b.chars().peekable();

    loop {
        match (ai.peek(), bi.peek()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(&ac), Some(&bc)) => {
                let a_digit = ac.is_ascii_digit();
                let b_digit = bc.is_ascii_digit();

                if a_digit && b_digit {
                    // Both digits: extract and compare the full number.
                    let mut an: u64 = 0;
                    let mut bn: u64 = 0;
                    while let Some(&c) = ai.peek() {
                        if c.is_ascii_digit() {
                            an = an
                                .saturating_mul(10)
                                .saturating_add((c as u8 - b'0') as u64);
                            ai.next();
                        } else {
                            break;
                        }
                    }
                    while let Some(&c) = bi.peek() {
                        if c.is_ascii_digit() {
                            bn = bn
                                .saturating_mul(10)
                                .saturating_add((c as u8 - b'0') as u64);
                            bi.next();
                        } else {
                            break;
                        }
                    }
                    match an.cmp(&bn) {
                        Ordering::Equal => continue,
                        ord => return ord,
                    }
                } else {
                    // At least one is non-digit: compare char-by-char.
                    match ac.cmp(&bc) {
                        Ordering::Equal => {
                            ai.next();
                            bi.next();
                        }
                        ord => return ord,
                    }
                }
            }
        }
    }
}
// Quick reproduction: does a monitored GtkDirectoryList notice an: does a monitored GtkDirectoryList notice an
// externally created file without being recreated?
#[test]
fn dirlist_monitor_picks_up_new_file() {
    let dir = std::env::temp_dir().join(format!("vky-dlmon-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let probe = dir.clone();
    // GTK objects only live on the init thread — run the body on the
    // shared GTK worker; None means "no display", i.e. skip.
    let Some(count) = crate::testkit::on_gtk(move || {
        let location = gio::File::for_path(&probe);
        let target = probe.join("new-file.txt");
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(400));
            std::fs::write(&target, b"hello").unwrap();
        });

        // Own thread-default context: the shared default context
        // would steal each other's wakeups. The DirectoryList is
        // built inside so its internal monitor attaches here.
        let ctx = glib::MainContext::new();
        ctx.with_thread_default(|| {
            let dl = gtk4::DirectoryList::new(Some("standard::name"), Some(&location));
            dl.set_monitored(true);
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(4);
            let mut count = 0u32;
            while std::time::Instant::now() < deadline {
                ctx.iteration(false);
                count = dl.n_items();
                if count > 0 {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            count
        })
        .unwrap_or(0)
    }) else {
        eprintln!("no display — skipping");
        std::fs::remove_dir_all(&dir).ok();
        return;
    };
    std::fs::remove_dir_all(&dir).ok();
    assert_eq!(count, 1, "monitored DirectoryList must see the new file");
}

/// The FileView refresh path: refresh_contents diffs a fresh
/// enumeration into the live DirModel store — renamed-in files
/// (browser downloads) must surface in the final sorted pipeline.
/// Guards the fix for the stale-view bug where
/// GtkDirectoryList-based monitoring dropped moved-in events.
#[test]
fn refresh_contents_picks_up_renamed_file() {
    let dir = std::env::temp_dir().join(format!("vky-vm-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let probe = dir.clone();
    let Some(names) = crate::testkit::on_gtk(move || {
        let location = gio::File::for_path(&probe);
        let ctx = glib::MainContext::new();
        ctx.with_thread_default(|| {
            // Model construction inside the scope: DirModel's
            // enumeration and monitor attach to the thread-default
            // context — this private ctx, not the global default.
            let vm = ViewModel::new(&location);
            std::fs::write(probe.join("file.part"), b"payload").unwrap();
            std::fs::rename(probe.join("file.part"), probe.join("file.bin")).unwrap();
            vm.refresh_contents();

            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            let mut names = Vec::new();
            while std::time::Instant::now() < deadline {
                ctx.iteration(false);
                names.clear();
                for i in 0..vm.list.n_items() {
                    let obj = vm.list.item(i).unwrap();
                    let fo = obj.downcast_ref::<FileObject>().unwrap();
                    names.push(fo.display_name());
                }
                if names.iter().any(|n| n == "file.bin") {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            names
        })
        .unwrap_or_default()
    }) else {
        eprintln!("no display — skipping");
        std::fs::remove_dir_all(&dir).ok();
        return;
    };
    std::fs::remove_dir_all(&dir).ok();
    assert!(
        names.iter().any(|n| n == "file.bin"),
        "refresh_contents must surface renamed file; got {names:?}"
    );
}

/// Raw GFileMonitor sanity check: renamed-into-dir must produce a
/// usable event (this is what browsers/downloads actually do).
#[test]
fn raw_monitor_sees_rename_in() {
    let dir = std::env::temp_dir().join(format!("vky-rawmon-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let location = gio::File::for_path(&dir);
    let events = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));

    let part = dir.join("g.part");
    let fin = dir.join("g.bin");
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(300));
        std::fs::write(&part, b"x").unwrap();
        std::fs::rename(&part, &fin).unwrap();
    });

    let ctx = glib::MainContext::new();
    let mon = std::rc::Rc::new(std::cell::RefCell::new(None::<gio::FileMonitor>));
    let _ = ctx.with_thread_default(|| {
        let m = location
            .monitor_directory(gio::FileMonitorFlags::NONE, gio::Cancellable::NONE)
            .unwrap();
        let ev = events.clone();
        m.connect_changed(move |_, f, _, e| {
            ev.borrow_mut()
                .push((f.basename().unwrap().to_string_lossy().into_owned(), e));
        });
        *mon.borrow_mut() = Some(m);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while std::time::Instant::now() < deadline {
            ctx.iteration(false);
            if events.borrow().iter().any(|(n, _)| n == "g.bin") {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    });
    let evs = events.borrow().clone();
    std::fs::remove_dir_all(&dir).ok();
    assert!(
        evs.iter().any(|(n, _)| n == "g.bin"),
        "raw monitor must report the renamed file; events: {evs:?}"
    );
}
