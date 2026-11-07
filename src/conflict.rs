// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// Name-collision dialog for copy/move: shows the existing and the
// incoming item side by side (icon, size, modification date) and lets
// the user overwrite/merge, skip, cancel the whole batch or rename
// the incoming file — the classic file-manager choice instead of
// silently writing "name (2)". "Apply to all" remembers the answer
// for the rest of the batch (rename is always per-file).

use gtk4::prelude::*;
use gtk4::{gio, glib};

use crate::i18n::tr;
use crate::operations;
use crate::tree_ops::Action;

/// Attributes shown for both sides of the conflict.
const INFO_ATTRS: &str = "standard::icon,standard::display-name,standard::size,\
                          time::modified,time::created,time::access,standard::type";

/// Ask how to resolve the collision between `src` and existing `dst`.
/// `src_is_dir` switches the destructive button between Merge and
/// Replace. `on_done` receives the choice plus the apply-to-all flag.
pub fn ask(
    parent: &gtk4::Window,
    src: &gio::File,
    dst: &gio::File,
    src_is_dir: bool,
    on_done: impl FnOnce(Action, bool) + 'static,
) {
    let src_info = src
        .query_info(
            INFO_ATTRS,
            gio::FileQueryInfoFlags::NONE,
            gio::Cancellable::NONE,
        )
        .ok();
    let dst_info = dst
        .query_info(
            INFO_ATTRS,
            gio::FileQueryInfoFlags::NONE,
            gio::Cancellable::NONE,
        )
        .ok();
    // Different kinds (file vs directory) can never overwrite each
    // other — only rename and skip stay valid in that case.
    let dst_is_dir = dst_info
        .as_ref()
        .map(|i| i.file_type() == gio::FileType::Directory)
        .unwrap_or(false);
    let can_overwrite = src_is_dir == dst_is_dir;

    let window = gtk4::Window::builder()
        .title(tr("Name conflict"))
        .modal(true)
        .transient_for(parent)
        .resizable(false)
        .default_width(460)
        .build();

    let heading = gtk4::Label::builder()
        .label(if src_is_dir {
            tr("A folder with this name already exists")
        } else {
            tr("A file with this name already exists")
        })
        .xalign(0.0)
        .build();
    heading.add_css_class("title-3");

    let grid = gtk4::Grid::builder()
        .column_spacing(12)
        .row_spacing(12)
        .build();
    grid.attach(&section_header(&tr("Existing")), 0, 0, 1, 1);
    if let Some(info) = &dst_info {
        grid.attach(&info_row(info, dst), 0, 1, 1, 1);
    }
    grid.attach(&section_header(&tr("New")), 0, 2, 1, 1);
    if let Some(info) = &src_info {
        grid.attach(&info_row(info, src), 0, 3, 1, 1);
    }

    // Rename row: entry prefilled with the first free "name (n)".
    let suggestion = suggested_name(dst);
    let rename_entry = gtk4::Entry::builder()
        .text(&suggestion)
        .hexpand(true)
        .activates_default(false)
        .build();
    let rename_button = gtk4::Button::with_label(&tr("Rename"));

    let apply_all = gtk4::CheckButton::with_label(&tr("Apply to all conflicts"));

    let cancel_button = gtk4::Button::with_label(&tr("Cancel"));
    let skip_button = gtk4::Button::with_label(&tr("Skip"));
    let overwrite_button =
        gtk4::Button::with_label(&tr(if src_is_dir { "Merge" } else { "Replace" }));
    overwrite_button.add_css_class("destructive-action");
    overwrite_button.set_sensitive(can_overwrite);

    let buttons = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    buttons.set_halign(gtk4::Align::End);
    buttons.append(&cancel_button);
    buttons.append(&skip_button);
    buttons.append(&rename_button);
    buttons.append(&overwrite_button);

    let content = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
    content.set_margin_top(16);
    content.set_margin_bottom(16);
    content.set_margin_start(16);
    content.set_margin_end(16);
    content.append(&heading);
    content.append(&grid);
    content.append(&rename_entry);
    content.append(&apply_all);
    content.append(&buttons);
    window.set_child(Some(&content));

    // One-shot dispatch: whichever button fires first delivers the
    // answer and closes the dialog; window close counts as cancel.
    let on_done = std::rc::Rc::new(std::cell::RefCell::new(Some(on_done)));
    let finish = {
        let window = window.clone();
        let all = apply_all.clone();
        let on_done = on_done.clone();
        move |action: Action| {
            if let Some(cb) = on_done.borrow_mut().take() {
                cb(action, all.is_active());
            }
            window.close();
        }
    };

    let f = finish.clone();
    cancel_button.connect_clicked(move |_| f(Action::Cancel));
    let f = finish.clone();
    skip_button.connect_clicked(move |_| f(Action::Skip));
    let f = finish.clone();
    overwrite_button.connect_clicked(move |_| f(Action::Overwrite));
    let entry = rename_entry.clone();
    rename_button.connect_clicked(move |_| {
        let name = entry.text().to_string();
        let name = name.trim();
        // A slash would escape the destination folder via path
        // resolution — refuse instead of silently rewriting the name.
        if name.is_empty() || name.contains('/') || name == "." || name == ".." {
            entry.add_css_class("error");
            entry.grab_focus();
            return;
        }
        finish(Action::Rename(name.to_string()));
    });
    let entry2 = rename_entry.clone();
    rename_entry.connect_changed(move |_| entry2.remove_css_class("error"));
    window.connect_close_request(move |_| {
        // Closed via window manager — same as Cancel.
        if let Some(cb) = on_done.borrow_mut().take() {
            cb(Action::Cancel, false);
        }
        glib::Propagation::Proceed
    });

    window.present();
}

/// First free "name (n)" variant for dst's name in its parent — the
/// prefill for the rename entry.
fn suggested_name(dst: &gio::File) -> String {
    let Some(parent) = dst.parent() else {
        return String::new();
    };
    let Some(name) = dst.basename().map(|b| b.to_string_lossy().into_owned()) else {
        return String::new();
    };
    let unique = operations::unique_destination(&parent, &name);
    unique
        .basename()
        .map(|b| b.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Small bold caption over each file card.
fn section_header(text: &str) -> gtk4::Label {
    let label = gtk4::Label::builder().label(text).xalign(0.0).build();
    label.add_css_class("heading");
    label
}

/// One file card: icon plus name, size and modification date.
fn info_row(info: &gio::FileInfo, file: &gio::File) -> gtk4::Widget {
    let icon = info
        .icon()
        .map(|i| gtk4::Image::from_gicon(&i))
        .unwrap_or_default();
    icon.set_pixel_size(32);

    let name = gtk4::Label::builder()
        .label(
            info.attribute_as_string("standard::display-name")
                .map(|s| s.to_string())
                .unwrap_or_else(|| file.uri().to_string()),
        )
        .xalign(0.0)
        .ellipsize(gtk4::pango::EllipsizeMode::Middle)
        .build();
    name.add_css_class("heading");

    let size = if info.has_attribute("standard::size") {
        glib::format_size(info.size() as u64).to_string()
    } else {
        String::new()
    };
    let modified = info
        .modification_date_time()
        .and_then(|dt| dt.format("%d.%m.%Y %H:%M").ok().map(|s| s.to_string()))
        .unwrap_or_default();
    let meta = gtk4::Label::builder()
        .label(format!("{size}  ·  {} {modified}", tr("Modified:")))
        .xalign(0.0)
        .ellipsize(gtk4::pango::EllipsizeMode::End)
        .build();
    meta.add_css_class("dim-label");

    let texts = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
    texts.set_valign(gtk4::Align::Center);
    texts.append(&name);
    texts.append(&meta);

    // Creation time is only available on some backends/filesystems —
    // show the row only when the attribute is actually present.
    if let Some(created) = info
        .creation_date_time()
        .and_then(|dt| dt.format("%d.%m.%Y %H:%M").ok().map(|s| s.to_string()))
    {
        let created_label = gtk4::Label::builder()
            .label(format!("{} {created}", tr("Created:")))
            .xalign(0.0)
            .build();
        created_label.add_css_class("dim-label");
        texts.append(&created_label);
    }

    let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);
    row.append(&icon);
    row.append(&texts);
    row.upcast()
}
