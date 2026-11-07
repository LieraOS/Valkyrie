// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// The data pipeline behind the file views:
//   DirectoryList   — async enumeration + folder monitoring
//   MapListModel    — wraps each FileInfo in a FileObject (carries the File)
//   FilterListModel — drops hidden entries unless enabled
//   SortListModel   — directories first, then the chosen field
// Filter and sorter state live in GSettings so they survive restarts.

use std::cell::Cell;
use std::cmp::Ordering;
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{gio, glib};

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
}

impl ViewModel {
    /// Build the model pipeline for `location`, seeded from GSettings.
    pub fn new(location: &gio::File) -> Self {
        let dir_list = gtk4::DirectoryList::new(Some(ATTRIBUTES), Some(location));
        dir_list.set_monitored(true);

        let parent = location.clone();
        let mapped = gtk4::MapListModel::new(Some(dir_list), move |item| {
            // Every item is a FileInfo by DirectoryList contract.
            let info = item
                .downcast_ref::<gio::FileInfo>()
                .expect("DirectoryList item");
            // name() warns when a backend leaves standard::name unset;
            // fall back through the safe attribute getters.
            let name = info
                .attribute_byte_string("standard::name")
                .map(|n| n.to_string())
                .or_else(|| {
                    info.attribute_as_string("standard::display-name")
                        .map(|n| n.to_string())
                })
                .unwrap_or_default();
            FileObject::new(&parent.child(&name), Some(info)).upcast::<glib::Object>()
        });
        Self::from_base(mapped.upcast())
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
