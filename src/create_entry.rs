// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// "New Folder" / "New Document" from the view's background menu: a
// small modal dialog asks for the name, then operations.rs performs
// the creation. Files installed in the user's Templates directory are
// offered as document templates.

use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{gio, glib};

use crate::i18n::tr;
use crate::mode_editor::ModeEditor;
use crate::operations;

/// List the user's template files (the XDG Templates directory —
/// "~/Vorlagen" on German systems). Empty when the directory is unset
/// or contains no files; subdirectories are skipped.
pub fn templates() -> Vec<gio::File> {
    let Some(dir) = glib::user_special_dir(glib::UserDirectory::Templates) else {
        return Vec::new();
    };
    let dir = gio::File::for_path(dir);
    let Ok(enumerator) = dir.enumerate_children(
        "standard::name,standard::type",
        gio::FileQueryInfoFlags::NONE,
        gio::Cancellable::NONE,
    ) else {
        return Vec::new();
    };
    let mut files = Vec::new();
    let mut enumerator = enumerator;
    while let Some(Ok(info)) = enumerator.next() {
        // Both getters warn when the backend leaves the attribute
        // unset — guard the type read and take the name via the raw
        // attribute getter.
        let regular =
            info.has_attribute("standard::type") && info.file_type() == gio::FileType::Regular;
        if regular {
            if let Some(name) = info.attribute_byte_string("standard::name") {
                files.push(dir.child(&*name));
            }
        }
    }
    files
}

/// Ask for a folder name, then create it inside `dir`. The collapsed
/// permissions expander offers the POSIX defaults (0755); opening it
/// lets the user pick different bits.
pub fn create_folder(parent: &gtk4::Widget, dir: &gio::File) {
    let dir = dir.clone();
    ask_name(
        parent,
        &tr("New Folder"),
        &tr("New Folder"),
        &tr("Create"),
        Some(0o755),
        move |name, mode| operations::create_folder(&dir, name, mode),
    );
}

/// Ask for a document name, then create it inside `dir` — either as an
/// empty file or by copying `template`. The collapsed permissions
/// expander offers the POSIX defaults (0644).
pub fn create_document(parent: &gtk4::Widget, dir: &gio::File, template: Option<&gio::File>) {
    let dir = dir.clone();
    let template = template.cloned();
    let default = template
        .as_ref()
        .and_then(|t| t.basename())
        .map(|b| b.to_string_lossy().into_owned())
        .unwrap_or_else(|| tr("Untitled Document"));
    ask_name(
        parent,
        &tr("New Document"),
        &default,
        &tr("Create"),
        Some(0o644),
        move |name, mode| {
            operations::create_document(&dir, name, template.as_ref(), mode);
        },
    );
}

/// Minimal modal name dialog: an entry (prefilled + selected) and a
/// confirm/Cancel button row. Enter and Esc work as expected.
/// `mode_default` adds a collapsible permissions editor preset to
/// those bits; the callback then receives `Some(mode)` only when the
/// user expanded it — collapsed means "system defaults". The rename
/// dialog passes `None` and ignores the mode argument.
pub(crate) fn ask_name(
    parent: &gtk4::Widget,
    title: &str,
    default: &str,
    confirm: &str,
    mode_default: Option<u32>,
    on_create: impl Fn(&str, Option<u32>) + 'static,
) {
    let window = gtk4::Window::builder()
        .title(title)
        .modal(false)
        .resizable(false)
        .build();
    if let Some(root) = parent.root() {
        if let Ok(w) = root.downcast::<gtk4::Window>() {
            window.set_transient_for(Some(&w));
        }
    }

    let content = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
    content.set_margin_top(12);
    content.set_margin_bottom(12);
    content.set_margin_start(12);
    content.set_margin_end(12);

    let entry = gtk4::Entry::builder().text(default).build();
    entry.select_region(0, -1);
    content.append(&entry);

    // Optional permissions section (New Folder/Document only).
    let mode_reader = mode_default.map(perms_expander).map(|(expander, reader)| {
        content.append(&expander);
        reader
    });

    let buttons = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    buttons.set_halign(gtk4::Align::End);
    let cancel = gtk4::Button::with_label(&tr("Cancel"));
    let create = gtk4::Button::with_label(confirm);
    create.add_css_class("suggested-action");
    buttons.append(&cancel);
    buttons.append(&create);
    content.append(&buttons);
    window.set_child(Some(&content));

    // Shared commit: trimmed, non-empty names only. The user callback
    // is Fn but not Clone, so it is shared through an Rc.
    let on_create = std::rc::Rc::new(on_create);
    let commit = {
        let window = window.clone();
        let entry = entry.clone();
        move || {
            let name = entry.text().trim().to_string();
            if !name.is_empty() {
                on_create(&name, mode_reader.as_ref().and_then(|r| r()));
            }
            window.close();
        }
    };
    let c = commit.clone();
    entry.connect_activate(move |_| c());
    create.connect_clicked(move |_| commit());
    let w = window.clone();
    cancel.connect_clicked(move |_| w.close());

    // Esc closes the dialog.
    let key = gtk4::EventControllerKey::new();
    let w = window.clone();
    key.connect_key_pressed(move |_, k, _, _| {
        if k == gtk4::gdk::Key::Escape {
            w.close();
            glib::Propagation::Stop
        } else {
            glib::Propagation::Proceed
        }
    });
    window.add_controller(key);

    window.present();
    entry.grab_focus();
}

/// Collapsible permission editor for the create dialogs: the shared
/// ModeEditor (rwx matrix + octal entry) preset to `default_mode`, with
/// a live octal readout in the expander label. The returned reader
/// yields `Some(mode)` only when the section was opened — a collapsed
/// expander means "keep the system defaults" (umask decides).
fn perms_expander(default_mode: u32) -> (gtk4::Expander, Rc<dyn Fn() -> Option<u32>>) {
    let expander = gtk4::Expander::new(Some(&tr("Permissions")));

    // No chmod at create time — the mode is read back on commit.
    let editor = ModeEditor::new(default_mode, None);
    let editor = Rc::new(editor);
    editor.widget.set_margin_top(6);
    editor.widget.set_margin_start(12);

    // Live octal readout in the expander label: the editor's entry and
    // checkboxes are the sources; we mirror the mode on each toggle.
    for (check, _) in &editor.checks {
        let expander = expander.clone();
        let editor = editor.clone();
        check.connect_toggled(move |_| {
            expander.set_label(Some(&format!(
                "{} ({:04o})",
                tr("Permissions"),
                editor.mode()
            )));
        });
    }
    // Entry commits change the mode without toggling checkboxes —
    // refresh the label on every entry text change too.
    {
        let expander = expander.clone();
        let entry = editor.entry.clone();
        let editor = editor.clone();
        entry.connect_changed(move |_| {
            expander.set_label(Some(&format!(
                "{} ({:04o})",
                tr("Permissions"),
                editor.mode()
            )));
        });
    }
    expander.set_child(Some(&editor.widget));

    let reader: Rc<dyn Fn() -> Option<u32>> = {
        let expander = expander.clone();
        let editor = editor.clone();
        Rc::new(move || expander.is_expanded().then(|| editor.mode()))
    };
    (expander, reader)
}
