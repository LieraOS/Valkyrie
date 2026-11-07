// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// Synced-folder management: the list inside the preferences
// ("Nextcloud" section) and the add-pair dialog. The same dialog is
// also opened by the context menu's "Sync with Nextcloud…" on an
// unpaired folder, pre-filled with that folder's path.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::gio;
use gtk4::prelude::*;

use super::pairs::{self, Direction, Schedule, SyncPair};
use crate::i18n::tr;

/// The synced-folders section appended to the preferences page:
/// one row per pair (endpoints + mode + schedule, with Sync-now and
/// remove controls) plus the "Add" button.
pub fn section(page: &gtk4::Box) {
    let title = gtk4::Label::builder()
        .label(format!("<b>{}</b>", tr("Synced Folders")))
        .use_markup(true)
        .xalign(0.0)
        .margin_top(6)
        .build();
    page.append(&title);

    let list = gtk4::Box::new(gtk4::Orientation::Vertical, 6);
    page.append(&list);

    let rebuild = Rc::new(RefCell::new(Box::new(|| {}) as Box<dyn Fn()>));
    *rebuild.borrow_mut() = {
        let list = list.clone();
        let rebuild = rebuild.clone();
        Box::new(move || {
            while let Some(child) = list.first_child() {
                list.remove(&child);
            }
            for pair in pairs::all() {
                list.append(&pair_row(&pair, {
                    let rebuild = rebuild.clone();
                    move || (rebuild.borrow())()
                }));
            }
            if pairs::all().is_empty() {
                let hint = gtk4::Label::builder()
                    .label(tr("No synced folders yet"))
                    .xalign(0.0)
                    .build();
                hint.add_css_class("dim-label");
                list.append(&hint);
            }
        }) as Box<dyn Fn()>
    };
    (rebuild.borrow())();

    let add = gtk4::Button::with_label(&tr("Add Synced Folder…"));
    add.set_halign(gtk4::Align::Start);
    {
        let rebuild = rebuild.clone();
        add.connect_clicked(move |b| {
            let rebuild = rebuild.clone();
            pair_dialog(b.upcast_ref(), None, move || (rebuild.borrow())());
        });
    }
    page.append(&add);
}

/// One pair row: "local → remote", a dim mode/schedule line, and
/// per-pair actions (sync now, remove).
fn pair_row(pair: &SyncPair, on_change: impl Fn() + 'static) -> gtk4::Widget {
    let outer = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
    let line = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);

    let remote = if pair.remote.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", pair.remote)
    };
    line.append(
        &gtk4::Label::builder()
            .label(format!("{} → Cloud:{}", pair.local, remote))
            .xalign(0.0)
            .hexpand(true)
            .ellipsize(gtk4::pango::EllipsizeMode::Middle)
            .build(),
    );

    let anchor = line.clone().upcast::<gtk4::Widget>();
    let sync = gtk4::Button::with_label(&tr("Sync Now"));
    {
        let pair = pair.clone();
        let anchor = anchor.clone();
        sync.connect_clicked(move |_| {
            super::sync::sync_now(&anchor, &pair);
        });
    }
    line.append(&sync);

    let remove = gtk4::Button::with_label(&tr("Remove"));
    {
        let key = pair.key();
        let local = pair.local.clone();
        remove.connect_clicked(move |_| {
            pairs::remove(&local);
            // Drop the journal too — a later re-pair must not
            // resurrect phantom deletions from the old snapshot.
            super::state::discard(&key);
            on_change();
        });
    }
    line.append(&remove);
    outer.append(&line);

    let dir_label = match pair.direction {
        Direction::UploadOnly => tr("Upload only"),
        Direction::TwoWay => tr("Two-way"),
    };
    let sched_label = match pair.schedule {
        Schedule::Manual => tr("Manual"),
        Schedule::Live => tr("On change"),
        Schedule::Interval(m) => format!("{} {}", tr("Every"), format_minutes(m)),
    };
    let sub = gtk4::Label::builder()
        .label(format!("{dir_label} · {sched_label}"))
        .xalign(0.0)
        .build();
    sub.add_css_class("dim-label");
    outer.append(&sub);
    outer.upcast()
}

/// Human interval label: "15 min", "1 h", "2 h".
fn format_minutes(minutes: u32) -> String {
    if minutes.is_multiple_of(60) {
        let h = minutes / 60;
        format!("{h} h")
    } else {
        format!("{minutes} min")
    }
}

/// The pair editor. `local` pre-fills the folder when the dialog is
/// opened from a folder's context menu; `on_done` lets the caller
/// refresh its list after a save.
pub fn pair_dialog(parent: &gtk4::Widget, local: Option<&str>, on_done: impl Fn() + 'static) {
    let window = parent
        .root()
        .and_then(|r| r.downcast::<gtk4::Window>().ok());
    let dialog = gtk4::Window::builder()
        .modal(true)
        .title(tr("Sync Folder with Cloud"))
        .default_width(420)
        .resizable(false)
        .build();
    dialog.set_transient_for(window.as_ref());

    let form = gtk4::Box::new(gtk4::Orientation::Vertical, 10);
    form.set_margin_top(16);
    form.set_margin_bottom(16);
    form.set_margin_start(16);
    form.set_margin_end(16);

    // Local folder: entry + a native folder picker.
    let local_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    let local_entry = gtk4::Entry::builder()
        .text(local.unwrap_or_default())
        .hexpand(true)
        .build();
    local_row.append(&local_entry);
    let browse = gtk4::Button::with_label(&tr("Browse…"));
    {
        let local_entry = local_entry.clone();
        let dialog = dialog.clone();
        browse.connect_clicked(move |_| {
            let picker = gtk4::FileDialog::builder()
                .title(tr("Choose local folder"))
                .build();
            let entry = local_entry.clone();
            picker.select_folder(Some(&dialog), gio::Cancellable::NONE, move |result| {
                if let Ok(file) = result {
                    if let Some(path) = file.path() {
                        entry.set_text(&path.to_string_lossy());
                    }
                }
            });
        });
    }
    local_row.append(&browse);
    form.append(&labeled(&tr("Local folder"), &local_row));

    // Remote path, relative to the user's cloud root.
    let remote_entry = gtk4::Entry::builder()
        .placeholder_text(tr("e.g. Documents/Work — empty = cloud root"))
        .build();
    form.append(&labeled(&tr("Cloud folder"), &remote_entry));

    let direction = dropdown(&[tr("Upload only"), tr("Two-way (mirror both sides)")], 0);
    form.append(&labeled(&tr("Direction"), &direction));

    let schedule = dropdown(
        &[
            tr("Manual"),
            tr("On change"),
            tr("Every 5 minutes"),
            tr("Every 15 minutes"),
            tr("Every hour"),
        ],
        0,
    );
    form.append(&labeled(&tr("Synchronize"), &schedule));

    // Save row.
    let buttons = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    buttons.set_halign(gtk4::Align::End);
    let cancel = gtk4::Button::with_label(&tr("Cancel"));
    {
        let dialog = dialog.clone();
        cancel.connect_clicked(move |_| dialog.close());
    }
    buttons.append(&cancel);
    let save = gtk4::Button::with_label(&tr("Save"));
    save.add_css_class("suggested-action");
    {
        let dialog = dialog.clone();
        let parent = parent.clone();
        save.connect_clicked(move |_| {
            let local = local_entry.text().to_string();
            if local.is_empty() || !gio::File::for_path(&local).query_exists(gio::Cancellable::NONE)
            {
                return; // require an existing local folder
            }
            let pair = SyncPair {
                local,
                remote: remote_entry.text().to_string(),
                direction: if direction.selected() == 1 {
                    Direction::TwoWay
                } else {
                    Direction::UploadOnly
                },
                schedule: match schedule.selected() {
                    1 => Schedule::Live,
                    2 => Schedule::Interval(5),
                    3 => Schedule::Interval(15),
                    4 => Schedule::Interval(60),
                    _ => Schedule::Manual,
                },
            };
            pairs::upsert(pair.clone());
            dialog.close();
            on_done();
            // Fresh pairs deserve an immediate first sync so the
            // user sees the link working.
            super::sync::sync_now(&parent, &pair);
        });
    }
    buttons.append(&save);
    form.append(&buttons);

    dialog.set_child(Some(&form));
    dialog.present();
}

/// "Label: control" — one form line.
fn labeled(title: &str, control: &impl IsA<gtk4::Widget>) -> gtk4::Widget {
    let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 10);
    row.append(
        &gtk4::Label::builder()
            .label(title)
            .xalign(0.0)
            .width_request(110)
            .build(),
    );
    row.append(control);
    row.upcast()
}

/// StringList-backed dropdown, mirroring settings_dialog's helper.
fn dropdown(labels: &[String], selected: u32) -> gtk4::DropDown {
    let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
    let list = gtk4::StringList::new(&refs);
    let dd = gtk4::DropDown::new(Some(list), gtk4::Expression::NONE);
    dd.set_selected(selected.min(labels.len().saturating_sub(1) as u32));
    dd
}
