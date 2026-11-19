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
use std::os::fd::AsRawFd;
use std::path::PathBuf;
use std::rc::{Rc, Weak};
use std::time::Duration;

use gtk4::prelude::*;
use vte4::prelude::*;

use crate::i18n::tr;

/// True when the committed bytes end or wipe the shell's edit line —
/// Enter (\r/\n), Ctrl+C (SIGINT → fresh prompt) or Ctrl+U (kill whole
/// line). Everything else may leave the buffer holding typed text.
fn commit_clears_line(bytes: &[u8]) -> bool {
    bytes
        .iter()
        .any(|b| matches!(*b, b'\r' | b'\n' | 0x03 | 0x15))
}

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
    /// True while the shell's edit line may hold typed text — feeding
    /// `cd` then would land mid-line. Tracked from VTE `commit` data.
    line_dirty: Rc<Cell<bool>>,
    /// Queued `cd` target: set when navigation happens while the line
    /// is dirty or a foreground child (vim, htop) owns the pty. Latest
    /// navigation wins; flushed when the prompt is clean again.
    pending_dir: Rc<RefCell<Option<PathBuf>>>,
    /// Shell process id — the shell is the session leader, so its pid
    /// equals the pty's foreground process group when no child (vim,
    /// htop, a running pipeline) is in front.
    shell_pid: Rc<Cell<i32>>,
    /// Back-reference so commit callbacks can schedule delayed work.
    weak_self: RefCell<Weak<Self>>,
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
            line_dirty: Rc::new(Cell::new(false)),
            pending_dir: Rc::new(RefCell::new(None)),
            shell_pid: Rc::new(Cell::new(0)),
            weak_self: RefCell::new(Weak::new()),
        });
        *this.weak_self.borrow_mut() = Rc::downgrade(&this);

        let weak = Rc::downgrade(&this);
        this.term.connect_child_exited(move |_, _| {
            if let Some(this) = weak.upgrade() {
                this.alive.set(false);
                this.spawning.set(false);
                this.shell_pid.set(0);
                this.line_dirty.set(false);
                // A queued cd dies with the shell — the respawn starts
                // in `dir` directly.
                this.pending_dir.borrow_mut().take();
            }
        });

        // VTE emits `commit` for every chunk written to the pty — key
        // presses, pastes and our own feeds alike. Use it to keep
        // `line_dirty` current: any bytes that do not clear the line
        // might be edit-buffer content.
        let weak = Rc::downgrade(&this);
        this.term.connect_commit(move |_, text, _| {
            let Some(this) = weak.upgrade() else { return };
            let bytes = text.as_bytes();
            if commit_clears_line(bytes) {
                this.line_dirty.set(false);
                // The line just ended — usually Enter. Give the shell a
                // moment to fork its command, then flush a queued `cd`
                // only if the shell still owns the pty foreground.
                this.schedule_flush();
            } else if !bytes.is_empty() {
                this.line_dirty.set(true);
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
    /// working directory already lands on the right place). When the
    /// edit line may hold typed text or a foreground child owns the
    /// pty, the target is queued instead of corrupting user input.
    fn sync_location(&self) {
        if !self.widget.is_visible() || !self.alive.get() {
            return;
        }
        let path = self.dir.borrow().as_ref().and_then(|f| f.path());
        let Some(path) = path else { return };
        if self.synced.borrow().as_ref() == Some(&path) {
            return;
        }
        if self.line_dirty.get() || !self.shell_is_foreground() {
            *self.pending_dir.borrow_mut() = Some(path);
            return;
        }
        self.feed_cd(path);
    }

    /// Write the `cd` line and remember the reached directory.
    fn feed_cd(&self, path: PathBuf) {
        *self.synced.borrow_mut() = Some(path.clone());
        let cmd = format!("cd {}\n", shell_quote(&path.to_string_lossy()));
        self.term.feed_child(cmd.as_bytes());
    }

    /// True while the shell itself holds the pty's foreground process
    /// group — no vim/htop/pipeline running in front. tcgetpgrp on a
    /// dead pty fails and reports not-foreground, which is the safe
    /// side to err on.
    fn shell_is_foreground(&self) -> bool {
        let pid = self.shell_pid.get();
        let Some(pty) = self.term.pty() else {
            return false;
        };
        pid > 0 && unsafe { libc::tcgetpgrp(pty.fd().as_raw_fd()) } == pid
    }

    /// Re-check the queued `cd` after a short delay — long enough for
    /// the shell to have forked whatever the user launched, so the
    /// foreground check reflects the new owner.
    fn schedule_flush(&self) {
        if self.pending_dir.borrow().is_none() {
            return;
        }
        let weak = self.weak_self.borrow().clone();
        glib::timeout_add_local_once(Duration::from_millis(120), move || {
            if let Some(this) = weak.upgrade() {
                this.flush_pending();
            }
        });
    }

    /// Feed the queued `cd` if the prompt is clean and the shell still
    /// owns the foreground. A failed check keeps the entry queued —
    /// the next line-clearing commit retries.
    fn flush_pending(&self) {
        let path = self.pending_dir.borrow().clone();
        let Some(path) = path else { return };
        if !self.widget.is_visible()
            || !self.alive.get()
            || self.line_dirty.get()
            || !self.shell_is_foreground()
        {
            return;
        }
        self.pending_dir.borrow_mut().take();
        self.feed_cd(path);
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
        let shell_pid = self.shell_pid.clone();
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
                    Ok(pid) => {
                        alive.set(true);
                        shell_pid.set(pid.0);
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
