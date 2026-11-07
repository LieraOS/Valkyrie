// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// "Open in Terminal" support: picks an installed terminal emulator and
// launches it with the directory as working folder.

use std::process::Command;

use gio::prelude::*;

/// Fallback cascade after the user setting and the freedesktop
/// helpers. Ordered by preference on a GTK desktop. pub(crate): the
/// settings dialog lists every detected candidate.
pub(crate) const TERMINAL_CANDIDATES: &[&str] = &[
    "xdg-terminal-exec",
    "x-terminal-emulator",
    "kgx",
    "gnome-terminal",
    "xfce4-terminal",
    "konsole",
    "alacritty",
    "kitty",
    "foot",
    "xterm",
];

/// Open a terminal emulator with `dir` as its working directory.
/// Virtual locations (trash://, admin://, ...) have no filesystem path;
/// for those the terminal opens in the user's home instead.
pub fn spawn(dir: &gio::File) {
    let cwd = dir
        .path()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(glib::home_dir);

    let command = pick_terminal();
    let Some(command) = command else {
        eprintln!("valkyrie: no terminal emulator found");
        return;
    };

    // Terminal emulators inherit the process working directory; most
    // common emulators (kgx, gnome-terminal, kitty, xfce4-terminal, ...)
    // use it for the new window/tab.
    if let Err(e) = Command::new(&command).current_dir(&cwd).spawn() {
        eprintln!("valkyrie: failed to spawn {command}: {e}");
    }
}

/// Resolve which emulator to launch: user setting first, then the
/// detection cascade through PATH.
fn pick_terminal() -> Option<String> {
    let configured = crate::settings::terminal_command();
    if !configured.is_empty() && program_exists(&configured) {
        return Some(configured);
    }
    TERMINAL_CANDIDATES
        .iter()
        .find(|name| program_exists(name))
        .map(|s| s.to_string())
}

/// True when `program` resolves through PATH.
pub(crate) fn program_exists(program: &str) -> bool {
    glib::find_program_in_path(program).is_some()
}
