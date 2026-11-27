// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// File properties dialog: a header with icon,
// name and type, then two notebook pages — "Basic" (type, size,
// location, timestamps, free space, directory contents counted
// asynchronously) and "Permissions" (properties_perms.rs).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use gtk4::prelude::*;
use gtk4::{gio, glib};

use crate::i18n::tr;
use crate::properties_perms;

/// All attributes the dialog displays — including `standard::type`
/// (missing it produced GLib-GIO-CRITICAL warnings before).
const ATTRIBUTES: &str = concat!(
    "standard::type,standard::size,standard::content-type,",
    "standard::icon,standard::display-name,",
    "time::modified,time::access,time::created,",
    "unix::uid,unix::gid,unix::mode,",
    "owner::user,owner::group,",
    "filesystem::free,filesystem::size,",
    "access::can-read,access::can-write,access::can-execute"
);

/// Show the properties dialog for `file`.
pub fn show(parent: &impl IsA<gtk4::Widget>, file: &gio::File) {
    // The window sizes to its content. Two things make that
    // deterministic (they caused "Trying to measure GtkWindow …
    // needs at least N" warnings before):
    //   - value labels ellipsize instead of wrap, so row height does
    //     not depend on width (no height-for-width surprise);
    //   - every notebook page gets the tallest page's natural height,
    //     so switching tabs never makes the content need more than
    //     the allocation.
    // Non-modal transient window: stays above its parent window but does
    // not block input — several properties windows and other Valkyrie
    // windows can all stay usable at the same time.
    let dialog = gtk4::Window::builder()
        .title(tr("Properties"))
        .modal(false)
        .default_width(420)
        .build();
    if let Some(window) = parent
        .ancestor(gtk4::Window::static_type())
        .and_then(|w| w.downcast::<gtk4::Window>().ok())
    {
        dialog.set_transient_for(Some(&window));
    }

    let layout = gtk4::Box::new(gtk4::Orientation::Vertical, 0);

    // Header: large icon + name + type.
    let header = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);
    header.set_margin_top(16);
    header.set_margin_bottom(8);
    header.set_margin_start(16);
    header.set_margin_end(16);

    let info = file
        .query_info(
            ATTRIBUTES,
            gio::FileQueryInfoFlags::NONE,
            gio::Cancellable::NONE,
        )
        .ok();

    let icon = info
        .as_ref()
        .and_then(|i| i.icon())
        .unwrap_or_else(|| gio::ThemedIcon::new("text-x-generic").into());
    let image = gtk4::Image::from_gicon(&icon);
    image.set_pixel_size(48);
    header.append(&image);

    let name_label = gtk4::Label::builder()
        .label(format!("<b>{}</b>", basename_or_uri(file)))
        .use_markup(true)
        .xalign(0.0)
        .ellipsize(gtk4::pango::EllipsizeMode::Middle)
        .build();
    let titles = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
    titles.set_valign(gtk4::Align::Center);
    titles.append(&name_label);
    titles.append(
        &gtk4::Label::builder()
            .label(info.as_ref().map(type_text).unwrap_or_default())
            .xalign(0.0)
            .css_classes(["dim-label"])
            .build(),
    );
    header.append(&titles);
    layout.append(&header);
    layout.append(&gtk4::Separator::new(gtk4::Orientation::Horizontal));

    let notebook = gtk4::Notebook::builder().show_border(false).build();
    notebook.append_page(
        &basic_page(file, info.as_ref(), &name_label),
        Some(&gtk4::Label::new(Some(&tr("Basic")))),
    );
    if let Some(perms) = info.as_ref().and_then(|i| properties_perms::page(file, i)) {
        notebook.append_page(&perms, Some(&gtk4::Label::new(Some(&tr("Permissions")))));
    }

    // Uniform page height: measure every page's natural height and
    // pin them all to the maximum. GtkNotebook's natural size is the
    // visible page only — without this, switching to a taller page
    // re-measures the window below the content's minimum.
    let mut page_height = 0;
    for idx in 0..notebook.n_pages() {
        if let Some(page) = notebook.nth_page(Some(idx)) {
            let (_, natural, ..) = page.measure(gtk4::Orientation::Vertical, -1);
            page_height = page_height.max(natural);
        }
    }
    for idx in 0..notebook.n_pages() {
        if let Some(page) = notebook.nth_page(Some(idx)) {
            page.set_size_request(-1, page_height);
        }
    }
    layout.append(&notebook);

    dialog.set_child(Some(&layout));
    dialog.present();
}

/// The "Basic" page: standard attributes in a label/value grid.
/// The name row is an editable entry — committing renames the file
/// (classic file managers' behaviour) and updates the dialog header on success.
fn basic_page(
    file: &gio::File,
    info: Option<&gio::FileInfo>,
    name_label: &gtk4::Label,
) -> gtk4::Widget {
    let grid = gtk4::Grid::builder()
        .row_spacing(6)
        .column_spacing(12)
        .margin_top(12)
        .margin_bottom(12)
        .margin_start(16)
        .margin_end(16)
        .build();

    let mut row = 0;
    add_name_row(&grid, row, file, name_label);
    row += 1;
    add_row(&grid, row, &tr("Location"), &location_text(file));
    row += 1;

    if let Some(info) = info {
        let is_dir = info.file_type() == gio::FileType::Directory;
        if is_dir {
            // Contents row gets filled asynchronously (recursive walk
            // on a worker thread — counting large trees must not
            // block the UI).
            let value = count_row(&grid, row, &tr("Contents"));
            start_count(file.clone(), value);
            row += 1;
        } else {
            add_row(
                &grid,
                row,
                &tr("Size"),
                // attribute_uint64 is the safe getter — size() warns
                // when a backend leaves standard::size unset.
                glib::format_size(info.attribute_uint64("standard::size")).as_ref(),
            );
            row += 1;
        }
        for (dt, key) in [
            (info.modification_date_time(), tr("Modified")),
            (info.access_date_time(), tr("Accessed")),
            (info.creation_date_time(), tr("Created")),
        ] {
            if let Some(dt) = dt {
                if let Ok(text) = dt.format("%x %X") {
                    add_row(&grid, row, &key, text.as_ref());
                    row += 1;
                }
            }
        }
        if info.has_attribute("filesystem::free") {
            let free = info.attribute_uint64("filesystem::free");
            add_row(
                &grid,
                row,
                &tr("Free Space"),
                glib::format_size(free).as_ref(),
            );
        }
    }

    grid.upcast()
}

/// Editable "Name" row: an entry pre-filled with the basename.
/// Enter or focus loss commits the rename via `set_display_name`;
/// on success the dialog header follows, on failure the text
/// reverts and an alert explains why.
fn add_name_row(grid: &gtk4::Grid, row: i32, file: &gio::File, name_label: &gtk4::Label) {
    let key_label = gtk4::Label::builder()
        .label(format!("<b>{}</b>", tr("Name")))
        .use_markup(true)
        .xalign(0.0)
        .build();
    let entry = gtk4::Entry::builder()
        .text(basename_or_uri(file))
        .hexpand(true)
        .build();
    grid.attach(&key_label, 0, row, 1, 1);
    grid.attach(&entry, 1, row, 1, 1);

    let file = file.clone();
    let name_label = name_label.clone();
    // Track the committed name so both commit paths (Enter and
    // focus-out after Enter) don't rename twice.
    let committed = std::rc::Rc::new(std::cell::RefCell::new(entry.text().to_string()));
    let commit = {
        let committed = committed.clone();
        move |entry: &gtk4::Entry| {
            let name = entry.text().to_string();
            if name == *committed.borrow() || name.trim().is_empty() {
                return;
            }
            match file.set_display_name(&name, gio::Cancellable::NONE) {
                Ok(renamed) => {
                    *committed.borrow_mut() = name.clone();
                    let shown = renamed
                        .basename()
                        .map(|b| b.to_string_lossy().into_owned())
                        .unwrap_or(name);
                    name_label.set_label(&format!("<b>{shown}</b>"));
                }
                Err(e) => {
                    entry.set_text(&committed.borrow().clone());
                    let window = entry.root().and_then(|r| r.downcast::<gtk4::Window>().ok());
                    gtk4::AlertDialog::builder()
                        .message(tr("Could not rename the file"))
                        .detail(e.message().to_string())
                        .modal(false)
                        .build()
                        .show(window.as_ref());
                }
            }
        }
    };
    entry.connect_activate({
        let commit = commit.clone();
        move |entry| commit(entry)
    });
    entry.connect_has_focus_notify(move |entry| {
        if !entry.has_focus() {
            commit(entry);
        }
    });
}

/// Insert a row whose value label is filled in later; returns it.
fn count_row(grid: &gtk4::Grid, row: i32, key: &str) -> gtk4::Label {
    let key_label = gtk4::Label::builder()
        .label(format!("<b>{key}</b>"))
        .use_markup(true)
        .xalign(0.0)
        .build();
    let value_label = gtk4::Label::builder()
        .label(tr("Counting…"))
        .xalign(0.0)
        .selectable(true)
        .build();
    grid.attach(&key_label, 0, row, 1, 1);
    grid.attach(&value_label, 1, row, 1, 1);
    value_label
}

/// Recursively count a directory on a worker thread; the label is
/// updated once the walk finishes. Stops early when the label (i.e.
/// the dialog) is gone.
fn start_count(dir: gio::File, label: gtk4::Label) {
    let shared = Arc::new(Mutex::new(None::<(u64, u64)>));
    let shared2 = shared.clone();
    std::thread::spawn(move || {
        let mut items = 0u64;
        let mut bytes = 0u64;
        walk(&dir, &mut items, &mut bytes);
        *shared2.lock().unwrap_or_else(|e| e.into_inner()) = Some((items, bytes));
    });

    let weak = label.downgrade();
    glib::timeout_add_local(Duration::from_millis(150), move || {
        let Some(label) = weak.upgrade() else {
            return glib::ControlFlow::Break;
        };
        let result = shared.lock().unwrap_or_else(|e| e.into_inner()).take();
        match result {
            Some((items, bytes)) => {
                label.set_text(&format!(
                    "{} ({})",
                    tr("{items} items").replace("{items}", &items.to_string()),
                    glib::format_size(bytes)
                ));
                glib::ControlFlow::Break
            }
            None => glib::ControlFlow::Continue,
        }
    });
}

/// Recursive sync walk counting entries and their total size.
fn walk(dir: &gio::File, items: &mut u64, bytes: &mut u64) {
    // standard::name is required: FileEnumerator::child() builds the
    // child GFile from it — without it the returned object is invalid
    // and every follow-up call asserts.
    let Ok(en) = dir.enumerate_children(
        "standard::name,standard::type,standard::size",
        gio::FileQueryInfoFlags::NONE,
        gio::Cancellable::NONE,
    ) else {
        return;
    };
    while let Ok(Some(info)) = en.next_file(gio::Cancellable::NONE) {
        *items += 1;
        // file_type()/size() warn when a backend leaves the attribute
        // unset — guard the type read and use the raw size getter.
        let is_dir =
            info.has_attribute("standard::type") && info.file_type() == gio::FileType::Directory;
        if is_dir {
            let child = en.child(&info);
            walk(&child, items, bytes);
        } else {
            *bytes += info.attribute_uint64("standard::size");
        }
    }
}

/// One label/value row.
fn add_row(grid: &gtk4::Grid, row: i32, key: &str, value: &str) {
    let key_label = gtk4::Label::builder()
        .label(format!("<b>{key}</b>"))
        .use_markup(true)
        .xalign(0.0)
        .build();
    let value_label = gtk4::Label::builder()
        .label(value)
        .xalign(0.0)
        .ellipsize(gtk4::pango::EllipsizeMode::Middle)
        .selectable(true)
        .build();
    grid.attach(&key_label, 0, row, 1, 1);
    grid.attach(&value_label, 1, row, 1, 1);
}

fn basename_or_uri(file: &gio::File) -> String {
    file.basename()
        .map(|b| b.to_string_lossy().into_owned())
        .unwrap_or_else(|| file.uri().to_string())
}

fn location_text(file: &gio::File) -> String {
    file.parent()
        .and_then(|p| {
            p.path()
                .map(|x| x.to_string_lossy().into_owned())
                .or_else(|| Some(p.uri().to_string()))
        })
        .unwrap_or_default()
}

fn type_text(info: &gio::FileInfo) -> String {
    if info.file_type() == gio::FileType::Directory {
        return tr("Folder");
    }
    info.content_type()
        .map(|ct| gio::content_type_get_description(&ct).to_string())
        .map(|s| s.to_string())
        .unwrap_or_default()
}
