// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// Batch rename dialog for multi-selections: an optional find/replace
// step followed by a name pattern with placeholders — {name} is the
// original stem, {ext} the extension, {n} a counter starting at the
// chosen number. A live preview shows the first rows; applying runs
// the renames and reports any failures.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::gio;
use gtk4::prelude::*;

use crate::i18n::tr;

/// Open the batch rename dialog for `files` (the multi-selection).
pub fn show(parent: &impl IsA<gtk4::Widget>, files: &[gio::File]) {
    let dialog = gtk4::Window::builder()
        .title(tr("Rename Files"))
        .modal(false)
        .default_width(480)
        .build();
    if let Some(window) = parent
        .ancestor(gtk4::Window::static_type())
        .and_then(|w| w.downcast::<gtk4::Window>().ok())
    {
        dialog.set_transient_for(Some(&window));
    }

    let layout = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
    layout.set_margin_top(14);
    layout.set_margin_bottom(14);
    layout.set_margin_start(14);
    layout.set_margin_end(14);

    let find = gtk4::Entry::builder().placeholder_text(tr("Find")).build();
    let replace = gtk4::Entry::builder()
        .placeholder_text(tr("Replace with"))
        .build();
    layout.append(&label(&tr("Find / Replace in original name")));
    layout.append(&find);
    layout.append(&replace);

    layout.append(&label(&tr(
        "Pattern  ({name} = name, {ext} = extension, {n} = counter)",
    )));
    let pattern = gtk4::Entry::builder().text("{name}_{n}").build();
    layout.append(&pattern);

    let start = gtk4::SpinButton::with_range(0.0, 999999.0, 1.0);
    start.set_value(1.0);
    let start_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    start_row.append(&label(&tr("Start counter at")));
    start_row.append(&start);
    layout.append(&start_row);

    let preview = gtk4::Label::builder()
        .xalign(0.0)
        .selectable(true)
        .css_classes(["monospace"])
        .build();
    let scroll = gtk4::ScrolledWindow::builder()
        .child(&preview)
        .min_content_height(140)
        .build();
    layout.append(&label(&tr("Preview")));
    layout.append(&scroll);

    let buttons = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    buttons.set_halign(gtk4::Align::End);
    let cancel = gtk4::Button::with_label(&tr("Cancel"));
    let apply = gtk4::Button::with_label(&tr("Rename"));
    apply.add_css_class("suggested-action");
    buttons.append(&cancel);
    buttons.append(&apply);
    layout.append(&buttons);
    dialog.set_child(Some(&layout));

    // Live preview of the first rows whenever a field changes.
    let files_rc = Rc::new(files.to_vec());
    let rebuild = {
        let preview = preview.clone();
        let files = files_rc.clone();
        let find = find.clone();
        let replace = replace.clone();
        let pattern = pattern.clone();
        let start = start.clone();
        Rc::new(move || {
            let start_at = start.value() as u64;
            let lines: Vec<String> = files
                .iter()
                .take(10)
                .enumerate()
                .map(|(i, f)| {
                    let old = basename(f);
                    let new = render_name(
                        f,
                        &find.text(),
                        &replace.text(),
                        &pattern.text(),
                        start_at + i as u64,
                    );
                    format!("{old}  →  {new}")
                })
                .collect();
            let extra = if files.len() > 10 {
                format!("\n… +{} {}", files.len() - 10, tr("more"))
            } else {
                String::new()
            };
            preview.set_text(&(lines.join("\n") + &extra));
        })
    };
    for w in [&find, &replace, &pattern] {
        let r = rebuild.clone();
        w.connect_changed(move |_| r());
    }
    let r = rebuild.clone();
    start.connect_value_changed(move |_| r());
    rebuild();

    cancel.connect_clicked({
        let dialog = dialog.clone();
        move |_| dialog.close()
    });

    // Apply: rename every file, counting successes; failures are
    // collected and reported once at the end.
    let dialog_cell = Rc::new(RefCell::new(dialog.clone()));
    let parent_window = parent
        .root()
        .and_then(|r| r.downcast::<gtk4::Window>().ok());
    apply.connect_clicked(move |_| {
        let start_at = start.value() as u64;
        let mut failed: Vec<String> = Vec::new();
        for (i, f) in files_rc.iter().enumerate() {
            let new = render_name(
                f,
                &find.text(),
                &replace.text(),
                &pattern.text(),
                start_at + i as u64,
            );
            if new.is_empty() || f.set_display_name(&new, gio::Cancellable::NONE).is_err() {
                failed.push(basename(f));
            }
        }
        dialog_cell.borrow().close();
        if !failed.is_empty() {
            gtk4::AlertDialog::builder()
                .message(tr("Some files could not be renamed"))
                .detail(failed.join("\n"))
                .modal(false)
                .build()
                .show(parent_window.as_ref());
        }
    });

    dialog.present();
}

/// Compute the new name: optional find/replace on the stem, then the
/// pattern with {name}, {ext} and {n} expanded.
fn render_name(file: &gio::File, find: &str, replace: &str, pattern: &str, n: u64) -> String {
    let base = basename(file);
    let (stem, ext) = split_ext(&base);
    let stem = if find.is_empty() {
        stem
    } else {
        stem.replace(find, replace)
    };
    pattern
        .replace("{name}", &stem)
        .replace("{ext}", &ext)
        .replace("{n}", &n.to_string())
}

/// Basename split at the last dot — dotfiles keep their full name
/// (`.hidden` has no extension).
fn split_ext(name: &str) -> (String, String) {
    match name.rfind('.') {
        Some(i) if i > 0 => (name[..i].to_string(), name[i..].to_string()),
        _ => (name.to_string(), String::new()),
    }
}

fn basename(file: &gio::File) -> String {
    file.basename()
        .map(|b| b.to_string_lossy().into_owned())
        .unwrap_or_else(|| file.uri().to_string())
}

fn label(text: &str) -> gtk4::Label {
    gtk4::Label::builder().label(text).xalign(0.0).build()
}

#[cfg(test)]
mod tests {
    use super::split_ext;

    #[test]
    fn split_ext_cases() {
        assert_eq!(
            split_ext("a.tar.gz"),
            ("a.tar".to_string(), ".gz".to_string())
        );
        assert_eq!(split_ext("noext"), ("noext".to_string(), String::new()));
        assert_eq!(split_ext(".hidden"), (".hidden".to_string(), String::new()));
    }
}
