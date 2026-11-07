// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// "Permissions" page of the properties dialog: owner, group and the
// shared ModeEditor (rwx matrix + octal entry), applied immediately
// through GIO's unix::mode attribute (works for local files and GVFS
// backends).

use std::rc::Rc;

use gtk4::gio;
use gtk4::prelude::*;

use crate::i18n::tr;
use crate::mode_editor::ModeEditor;

/// POSIX permission bits we manage (rwx for owner/group/others).
/// pub(crate): the shared mode editor renders the same matrix.
pub(crate) const ROW_BITS: [u32; 3] = [
    0o700, // owner
    0o070, // group
    0o007, // others
];
pub(crate) const COL_BITS: [u32; 3] = [
    0o444, // read
    0o222, // write
    0o111, // execute
];

/// Build the permissions page, or `None` when the location does not
/// expose POSIX mode bits (e.g. virtual GVFS locations).
pub fn page(file: &gio::File, info: &gio::FileInfo) -> Option<gtk4::Widget> {
    if !info.has_attribute("unix::mode") {
        return None;
    }
    let mode = info.attribute_uint32("unix::mode");

    let grid = gtk4::Grid::builder()
        .row_spacing(8)
        .column_spacing(12)
        .margin_top(12)
        .margin_bottom(12)
        .margin_start(16)
        .margin_end(16)
        .build();

    // Owner and group — editable dropdowns like classic file managers: names from
    // /etc/passwd and /etc/group, applied via unix::uid/unix::gid.
    // A non-privileged chown/chgrp fails and reverts the choice.
    let uid = info.attribute_uint32("unix::uid");
    let gid = info.attribute_uint32("unix::gid");
    let owner_name = info
        .attribute_as_string("owner::user")
        .map(|s| s.to_string())
        .unwrap_or_else(|| uid.to_string());
    let group_name = info
        .attribute_as_string("owner::group")
        .map(|s| s.to_string())
        .unwrap_or_else(|| gid.to_string());
    // Owner is editable for everyone — unprivileged chown retries
    // via pkexec (graphical password prompt), going beyond classic file managers
    // which hides the combo for non-root users.
    add_owner_row(
        &grid,
        0,
        file,
        &tr("Owner"),
        (owner_name, uid),
        system_users(),
        "unix::uid",
    );
    // Group: the file owner may only chgrp to their own groups
    // without privileges (classic file managers' behaviour); anyone else gets the full
    // list with pkexec fallback.
    let euid = self_euid();
    let group_entries = if euid == 0 || euid != uid {
        system_groups()
    } else {
        self_groups()
    };
    add_owner_row(
        &grid,
        1,
        file,
        &tr("Group"),
        (group_name, gid),
        group_entries,
        "unix::gid",
    );

    // The shared editor: checkboxes and the octal entry both call the
    // consumer, which performs the chmod. Returning false reverts the
    // widgets (e.g. on EPERM).
    let file = file.clone();
    let on_change: Rc<dyn Fn(u32) -> bool> = Rc::new(move |mode| apply_mode(&file, mode));
    let editor = ModeEditor::new(mode, Some(on_change));
    grid.attach(&editor.widget, 0, 3, 4, 4);

    Some(grid.upcast())
}

/// Apply new rwx bits via chmod on the backend. The current mode is
/// re-read first so bits the editor cannot represent (file type,
/// setuid/setgid/sticky) are preserved. Returns false when the read or
/// write is refused so the editor can revert its widgets.
fn apply_mode(file: &gio::File, mode: u32) -> bool {
    let Ok(info) = file.query_info(
        "unix::mode",
        gio::FileQueryInfoFlags::NONE,
        gio::Cancellable::NONE,
    ) else {
        return false;
    };
    let merged = (info.attribute_uint32("unix::mode") & !0o777) | (mode & 0o777);
    file.set_attribute_uint32(
        "unix::mode",
        merged,
        gio::FileQueryInfoFlags::NONE,
        gio::Cancellable::NONE,
    )
    .is_ok()
}

/// One bold-label + DropDown row for owner/group. `entries` is the
/// (name, id) list shown; the entry matching `current_name` is
/// selected. Changing applies `attr` via set_attribute_uint32; on
/// failure the selection snaps back and an alert shows the error.
fn add_owner_row(
    grid: &gtk4::Grid,
    row: i32,
    file: &gio::File,
    key: &str,
    current: (String, u32),
    entries: Vec<(String, u32)>,
    attr: &str,
) {
    let (current_name, current_id) = current;
    let key_label = gtk4::Label::builder()
        .label(format!("<b>{key}</b>"))
        .use_markup(true)
        .xalign(0.0)
        .build();
    grid.attach(&key_label, 0, row, 1, 1);

    // Guarantee the current owner/group is in the list even if it is
    // absent from /etc/passwd|group (foreign uid on a mount etc.).
    let mut entries = entries;
    if !entries.iter().any(|(_, id)| *id == current_id) {
        entries.insert(0, (current_name.clone(), current_id));
    }
    let names: Vec<&str> = entries.iter().map(|(n, _)| n.as_str()).collect();
    let model = gtk4::StringList::new(&names);
    let dd = gtk4::DropDown::new(Some(model), gtk4::Expression::NONE);
    dd.set_hexpand(true);
    let selected = entries
        .iter()
        .position(|(_, id)| *id == current_id)
        .unwrap_or(0);
    dd.set_selected(selected as u32);

    let committed = Rc::new(std::cell::Cell::new(selected as u32));
    let file = file.clone();
    let attr = attr.to_string();
    dd.connect_selected_notify(move |dd| {
        let idx = dd.selected() as usize;
        if idx == committed.get() as usize {
            return;
        }
        let (_, id) = entries[idx];
        match file.set_attribute_uint32(
            &attr,
            id,
            gio::FileQueryInfoFlags::NONE,
            gio::Cancellable::NONE,
        ) {
            Ok(()) => committed.set(idx as u32),
            Err(e) => {
                // Elevated retry: pkexec shows the graphical sudo
                // password prompt. Works only for real local paths.
                let Some(path) = file.path() else {
                    let msg = e.message().to_string();
                    revert_and_alert(dd, &committed, &attr, &msg);
                    return;
                };
                let dd2 = dd.clone();
                let committed2 = committed.clone();
                let attr2 = attr.clone();
                elevated_apply(path, attr.clone(), id, move |status| match status {
                    0 => committed2.set(idx as u32),
                    126 => dd2.set_selected(committed2.get()), // auth dismissed
                    _ => {
                        dd2.set_selected(committed2.get());
                        let window = dd2.root().and_then(|r| r.downcast::<gtk4::Window>().ok());
                        gtk4::AlertDialog::builder()
                            .message(if attr2 == "unix::uid" {
                                tr("Could not change owner")
                            } else {
                                tr("Could not change group")
                            })
                            .detail(format!("pkexec exit status {status}"))
                            .modal(true)
                            .build()
                            .show(window.as_ref());
                    }
                });
            }
        }
    });
    grid.attach(&dd, 1, row, 1, 1);
}

/// (name, uid) pairs from /etc/passwd, sorted by name.
fn system_users() -> Vec<(String, u32)> {
    let mut v: Vec<(String, u32)> = std::fs::read_to_string("/etc/passwd")
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            let mut f = l.split(':');
            let name = f.next()?.to_string();
            let uid: u32 = f.nth(1)?.parse().ok()?;
            (!name.is_empty() && !name.starts_with('#')).then_some((name, uid))
        })
        .collect();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v
}

/// (name, gid) pairs from /etc/group, sorted by name.
fn system_groups() -> Vec<(String, u32)> {
    let mut v: Vec<(String, u32)> = std::fs::read_to_string("/etc/group")
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            let mut f = l.split(':');
            let name = f.next()?.to_string();
            let gid: u32 = f.nth(1)?.parse().ok()?;
            (!name.is_empty() && !name.starts_with('#')).then_some((name, gid))
        })
        .collect();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v
}

/// Effective uid of this process, read from /proc/self/status
/// ("Uid:" lists ruid euid suid fsuid). No libc dependency needed.
fn self_euid() -> u32 {
    std::fs::read_to_string("/proc/self/status")
        .unwrap_or_default()
        .lines()
        .find(|l| l.starts_with("Uid:"))
        .and_then(|l| l.split_whitespace().nth(2).and_then(|v| v.parse().ok()))
        .unwrap_or(u32::MAX)
}

/// (name, gid) pairs for the groups this process belongs to:
/// the "Groups:" supplementary list plus the effective gid, mapped
/// through /etc/group for names (classic file managers shows exactly this set).
fn self_groups() -> Vec<(String, u32)> {
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    let mut gids: Vec<u32> = Vec::new();
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("Groups:") {
            gids.extend(
                rest.split_whitespace()
                    .filter_map(|v| v.parse::<u32>().ok()),
            );
        } else if let Some(rest) = line.strip_prefix("Gid:") {
            if let Some(egid) = rest.split_whitespace().nth(1).and_then(|v| v.parse().ok()) {
                gids.push(egid);
            }
        }
    }
    let all = system_groups();
    let mut v: Vec<(String, u32)> = gids
        .into_iter()
        .map(|gid| {
            let name = all
                .iter()
                .find(|(_, id)| *id == gid)
                .map(|(n, _)| n.clone())
                .unwrap_or_else(|| gid.to_string());
            (name, gid)
        })
        .collect();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v.dedup_by(|a, b| a.1 == b.1);
    v
}

/// Run `pkexec chown|chgrp id path` — shows the polkit password
/// dialog for elevation. Async: `on_done` gets the exit status
/// (0 = ok, 126 = authentication dismissed, other = error).
fn elevated_apply(
    path: std::path::PathBuf,
    attr: String,
    id: u32,
    on_done: impl Fn(i32) + 'static,
) {
    let prog: &str = if attr == "unix::uid" {
        "chown"
    } else {
        "chgrp"
    };
    let path_str = path.to_string_lossy().into_owned();
    let argv: Vec<std::ffi::OsString> = vec![
        "pkexec".into(),
        prog.into(),
        "--".into(),
        id.to_string().into(),
        path_str.into(),
    ];
    let argv_refs: Vec<&std::ffi::OsStr> = argv.iter().map(|a| a.as_os_str()).collect();
    let Ok(sub) = gio::Subprocess::newv(&argv_refs, gio::SubprocessFlags::NONE) else {
        on_done(127);
        return;
    };
    glib::MainContext::default().spawn_local(async move {
        let _ = sub.wait_future().await;
        on_done(sub.exit_status());
    });
}

/// Revert the dropdown to the committed entry and show an alert.
fn revert_and_alert(
    dd: &gtk4::DropDown,
    committed: &Rc<std::cell::Cell<u32>>,
    attr: &str,
    detail: &str,
) {
    dd.set_selected(committed.get());
    let window = dd.root().and_then(|r| r.downcast::<gtk4::Window>().ok());
    gtk4::AlertDialog::builder()
        .message(if attr == "unix::uid" {
            tr("Could not change owner")
        } else {
            tr("Could not change group")
        })
        .detail(detail)
        .modal(true)
        .build()
        .show(window.as_ref());
}
