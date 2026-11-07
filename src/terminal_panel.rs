// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// Embedded terminal panel: a real VTE shell docked at the
// bottom of the window. Toggled from the View menu (F4); follows the
// active tab's directory by feeding `cd` into the running shell.
//
// The separate "Open in Terminal" context-menu entry (module `terminal`)
// still launches a standalone terminal window — this panel complements
// it, it does not replace it.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;

use gtk4::prelude::*;
use vte4::prelude::*;

use crate::i18n::tr;

/// Quote `s` for POSIX shells: wrap in single quotes, escaping embedded
/// quotes as '\''. Needed because the panel feeds literal command lines
/// into the running shell.
pub(crate) fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// One embedded shell widget plus its slim header (label + close button).
/// The panel follows the *active* tab: window code calls `set_location`
/// whenever the active tab navigates or the selection flips tabs.
pub(crate) struct TerminalPanel {
    pub widget: gtk4::Box,
    term: vte4::Terminal,
    /// False once the shell exits; the next show/location respawns it.
    alive: Rc<Cell<bool>>,
    /// Spawn request in flight — keeps a second location change from
    /// double-spawning while spawn_async is pending.
    spawning: Rc<Cell<bool>>,
    /// Directory the panel should sit in.
    dir: Rc<RefCell<Option<gio::File>>>,
    /// Directory actually reached, so repeated navigations to the same
    /// folder do not stack `cd` lines.
    synced: Rc<RefCell<Option<PathBuf>>>,
}

impl TerminalPanel {
    pub fn new() -> Rc<Self> {
        let term = vte4::Terminal::builder()
            .hexpand(true)
            .vexpand(true)
            .scrollback_lines(10_000)
            .build();

        // Header: name on the left, close button on the right — the
        // button toggles the same window action the View menu uses.
        let title = gtk4::Label::builder()
            .label(tr("Terminal"))
            .xalign(0.0)
            .hexpand(true)
            .build();
        let close = gtk4::Button::builder()
            .icon_name("window-close-symbolic")
            .has_frame(false)
            .tooltip_text(tr("Close Terminal Panel"))
            .build();
        close.connect_clicked(|b| {
            let _ = b.activate_action("win.terminal-panel", None);
        });
        let header = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
        header.append(&title);
        header.append(&close);

        let widget = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        widget.append(&header);
        widget.append(&term);

        let this = Rc::new(Self {
            widget: widget.clone(),
            term,
            alive: Rc::new(Cell::new(false)),
            spawning: Rc::new(Cell::new(false)),
            dir: Rc::new(RefCell::new(None)),
            synced: Rc::new(RefCell::new(None)),
        });

        let weak = Rc::downgrade(&this);
        this.term.connect_child_exited(move |_, _| {
            if let Some(this) = weak.upgrade() {
                this.alive.set(false);
                this.spawning.set(false);
            }
        });

        this
    }

    /// Show/hide the panel. Showing spawns the shell lazily and moves
    /// keyboard focus into the terminal.
    pub fn set_visible(&self, on: bool) {
        self.widget.set_visible(on);
        if !on {
            return;
        }
        if !self.alive.get() && !self.spawning.get() {
            self.spawn();
        }
        self.sync_location();
        self.term.grab_focus();
    }

    /// Directory the panel should track. Remote locations without a
    /// local path (trash://, sftp://) are stored but never cd'd to.
    pub fn set_location(&self, location: Option<&gio::File>) {
        *self.dir.borrow_mut() = location.cloned();
        self.sync_location();
        if self.widget.is_visible() && !self.alive.get() && !self.spawning.get() {
            self.spawn();
        }
    }

    /// Feed `cd` into the shell when the tracked local path changed.
    /// Skipped while the shell is dead or not running yet (the spawn's
    /// working directory already lands on the right place).
    fn sync_location(&self) {
        if !self.widget.is_visible() || !self.alive.get() {
            return;
        }
        let path = self.dir.borrow().as_ref().and_then(|f| f.path());
        let Some(path) = path else { return };
        if self.synced.borrow().as_ref() == Some(&path) {
            return;
        }
        *self.synced.borrow_mut() = Some(path.clone());
        let cmd = format!("cd {}\n", shell_quote(&path.to_string_lossy()));
        self.term.feed_child(cmd.as_bytes());
    }

    /// Spawn the user's $SHELL (falling back to /bin/sh) in the tracked
    /// directory or, when that has no local path, in the home folder.
    fn spawn(&self) {
        let cwd = self
            .dir
            .borrow()
            .as_ref()
            .and_then(|f| f.path())
            .or_else(dirs_home);
        let shell = std::env::var("SHELL")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "/bin/sh".to_string());

        // envv must carry the full environment: an empty slice would
        // hand the child an empty environment (no PATH, no HOME).
        let env: Vec<String> = std::env::vars().map(|(k, v)| format!("{k}={v}")).collect();
        let argv: Vec<&str> = vec![shell.as_str()];
        let envv: Vec<&str> = env.iter().map(String::as_str).collect();

        self.spawning.set(true);
        let alive = self.alive.clone();
        let spawning = self.spawning.clone();
        let synced = self.synced.clone();
        let target = cwd.clone();
        self.term.spawn_async(
            vte4::PtyFlags::DEFAULT,
            cwd.as_ref().and_then(|p| p.to_str()),
            &argv,
            &envv,
            glib::SpawnFlags::DEFAULT,
            || {},
            -1,
            None::<&gio::Cancellable>,
            move |result| {
                spawning.set(false);
                match result {
                    Ok(_) => {
                        alive.set(true);
                        // The spawn already lands in `dir` — remember it so
                        // the first sync does not feed a redundant `cd`.
                        *synced.borrow_mut() = target;
                    }
                    Err(e) => log_spawn_error(&e),
                }
            },
        );
    }
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// Keep the failure one line and free of paths — terminals spawn from a
/// user shell whose env may carry values we must not log.
fn log_spawn_error(e: &glib::Error) {
    eprintln!("valkyrie: terminal panel spawn failed: {e}");
}

#[cfg(test)]
#[path = "terminal_panel/tests.rs"]
mod tests;
