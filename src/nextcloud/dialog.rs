// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// Target picker for "Upload to Nextcloud": lists the cloud root's
// folders (enumerated asynchronously — the mount just happened and
// the listing round-trips through gvfsd-dav), lets the user pick one
// and optionally type an additional subfolder that is created before
// the copy starts.

use gtk4::prelude::*;
use gtk4::{gio, glib};

use crate::i18n::tr;

/// Show the modal picker over `parent`; `root` is the mounted cloud
/// root. `on_pick` receives the resolved target directory (created if
/// needed) — it runs when the user confirms or not at all on Cancel.
pub fn pick_target(
    parent: &gtk4::Widget,
    root: &gio::File,
    on_pick: impl Fn(&gio::File) + 'static,
) {
    let dialog = gtk4::Window::builder()
        .title(tr("Upload to Cloud"))
        .modal(false)
        .resizable(false)
        .build();
    if let Some(w) = parent
        .root()
        .and_then(|r| r.downcast::<gtk4::Window>().ok())
    {
        dialog.set_transient_for(Some(&w));
    }

    let content = gtk4::Box::new(gtk4::Orientation::Vertical, 10);
    content.set_margin_top(14);
    content.set_margin_bottom(14);
    content.set_margin_start(14);
    content.set_margin_end(14);
    content.append(
        &gtk4::Label::builder()
            .label(tr("Target folder"))
            .xalign(0.0)
            .build(),
    );

    // Root entry plus one row per cloud folder; "…" as a placeholder
    // while the (network) enumeration is in flight.
    let list = gtk4::ListBox::new();
    list.set_selection_mode(gtk4::SelectionMode::Single);
    list.add_css_class("boxed-list");
    let scroller = gtk4::ScrolledWindow::builder()
        .child(&list)
        .min_content_height(140)
        .max_content_height(220)
        .propagate_natural_height(true)
        .build();
    content.append(&scroller);
    let loading = gtk4::ListBoxRow::builder()
        .child(&gtk4::Label::new(Some(&tr("Loading…"))))
        .selectable(false)
        .activatable(false)
        .build();
    list.append(&loading);
    list.select_row(list.row_at_index(0).as_ref());

    let sub = gtk4::Entry::builder()
        .placeholder_text(tr("New subfolder (optional)"))
        .build();
    content.append(&sub);

    let buttons = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    buttons.set_halign(gtk4::Align::End);
    let cancel = gtk4::Button::with_label(&tr("Cancel"));
    let ok = gtk4::Button::with_label(&tr("Upload"));
    ok.add_css_class("suggested-action");
    buttons.append(&cancel);
    buttons.append(&ok);
    content.append(&buttons);
    dialog.set_child(Some(&content));

    // Fetch the cloud root's top-level directories in the background;
    // the result lands in a shared slot a main-context timeout drains.
    // Failures leave the picker with just the root choice.
    enum Msg {
        Rows(Vec<String>),
        Failed(glib::Error),
    }
    let shared = std::sync::Arc::new(std::sync::Mutex::new(None::<Msg>));
    let shared_w = shared.clone();
    let root_c = root.clone();
    std::thread::spawn(move || {
        let mut names = Vec::new();
        let result = root_c.enumerate_children(
            "standard::name,standard::type",
            gio::FileQueryInfoFlags::NONE,
            gio::Cancellable::NONE,
        );
        let msg = match result {
            Ok(en) => {
                while let Ok(Some(info)) = en.next_file(gio::Cancellable::NONE) {
                    if info.has_attribute("standard::type")
                        && info.file_type() == gio::FileType::Directory
                    {
                        names.push(info.name().to_string_lossy().into_owned());
                    }
                }
                names.sort_unstable_by_key(|a| a.to_lowercase());
                Msg::Rows(names)
            }
            Err(e) => Msg::Failed(e),
        };
        *shared_w.lock().unwrap_or_else(|e| e.into_inner()) = Some(msg);
    });
    let list_c = list.clone();
    let dialog_weak = dialog.downgrade();
    glib::timeout_add_local(std::time::Duration::from_millis(120), move || {
        let Some(msg) = shared.lock().unwrap_or_else(|e| e.into_inner()).take() else {
            // Nothing yet — keep polling while the dialog lives.
            return if dialog_weak.upgrade().is_some() {
                glib::ControlFlow::Continue
            } else {
                glib::ControlFlow::Break
            };
        };
        match msg {
            Msg::Rows(names) => {
                list_c.remove(&loading);
                // First row: the cloud root itself.
                let root_row = gtk4::ListBoxRow::builder().child(&row_label("/")).build();
                list_c.append(&root_row);
                for name in &names {
                    list_c.append(&gtk4::ListBoxRow::builder().child(&row_label(name)).build());
                }
                list_c.select_row(list_c.row_at_index(0).as_ref());
            }
            Msg::Failed(e) => {
                if let Some(l) = loading
                    .child()
                    .and_then(|c| c.downcast::<gtk4::Label>().ok())
                {
                    l.set_text(&format!("{}: {e}", tr("Could not list folders")));
                }
            }
        }
        glib::ControlFlow::Break
    });

    // Commit: resolve selection + optional subfolder into a target.
    let commit = {
        let dialog = dialog.clone();
        let list = list.clone();
        let sub = sub.clone();
        let root = root.clone();
        let on_pick = std::rc::Rc::new(on_pick);
        move || {
            let mut target = root.clone();
            if let Some(row) = list.selected_row() {
                if let Some(label) = row.child().and_then(|c| c.downcast::<gtk4::Label>().ok()) {
                    let text = label.text().to_string();
                    if text != "/" {
                        target = target.child(&text);
                    }
                }
            }
            let extra = sub.text().trim().to_string();
            if !extra.is_empty() {
                target = target.child(&extra);
            }
            // Parent folders must exist before copy_tree writes into
            // them — create the whole chain (same sync-create pattern
            // as the folder/document dialogs), then start the upload.
            if target.query_exists(gio::Cancellable::NONE)
                || target
                    .make_directory_with_parents(gio::Cancellable::NONE)
                    .is_ok()
            {
                on_pick(&target);
            }
            dialog.close();
        }
    };
    let c = commit.clone();
    ok.connect_clicked(move |_| c());
    let w = dialog.clone();
    cancel.connect_clicked(move |_| w.close());

    dialog.present();
}

/// One folder-name row label (left aligned, padded like the other
/// Valkyrie lists).
fn row_label(text: &str) -> gtk4::Label {
    gtk4::Label::builder()
        .label(text)
        .xalign(0.0)
        .margin_top(6)
        .margin_bottom(6)
        .margin_start(8)
        .margin_end(8)
        .build()
}
