// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// Progress window for long-running file operations (paste, drops,
// permanent deletes). The work runs on a plain worker thread with a
// shared gio::Cancellable; progress flows back over an mpsc channel
// that a main-context timeout drains into the UI. The dialog shows
// the current entry, a real progress bar and a detail line with
// bytes done/total, the measured rate and an ETA. Copy jobs expose
// an integrity-verify checkbox whose flag the worker samples per
// file; verify results stream back as a live counter, mismatches are
// collected and surfaced in one dialog at the end. Tree walking and
// the workers live in tree_ops.rs / jobs.rs; this file only owns
// the window and the message pump.

mod jobs;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::time::Duration;

use gtk4::prelude::*;
use gtk4::{gio, glib};

use crate::conflict;
use crate::i18n::{tr, trn};
use crate::operations;
use crate::tree_ops::{Action, VerifyResult};

pub use jobs::{copy_move, delete};
// Re-export for the module tests (and future callers).
#[allow(unused_imports)]
pub(crate) use jobs::filter_sources;

/// Shared flag between the verify checkbox and the copy worker.
pub(crate) type VerifyFlag = Arc<AtomicBool>;

/// The verify flag starts unset; the checkbox initializes it from
/// the persisted GSettings default when the window is created.
pub(crate) fn new_verify_flag() -> VerifyFlag {
    Arc::new(AtomicBool::new(false))
}

/// Messages from the worker thread to the UI poller.
/// pub(crate): the sync engine reuses this window for its own
/// worker (it never sends Conflict or Verify — syncs resolve
/// non-interactively and do not hash).
pub(crate) enum Msg {
    /// Byte/item progress: `name` is the current entry, `done` the
    /// cumulative amount, `total` the measured batch denominator.
    Progress { name: String, done: i64, total: i64 },
    /// Per-file integrity outcome — copy jobs only, and only for
    /// files started while the toggle was on.
    Verify { name: String, result: VerifyResult },
    /// The whole batch finished (or was cancelled).
    Done,
    /// One file failed; carries a ready-to-show error text.
    Failed(String),
    /// A name collision pauses the worker until the user answers on
    /// `reply` (choice + apply-to-all flag).
    Conflict {
        src: gio::File,
        dst: gio::File,
        reply: Sender<(Action, bool)>,
    },
}

/// Build the progress window for callers that drive their own worker
/// (e.g. Nextcloud sync). Returns (sender, cancellable).
pub(crate) fn spawn_window(parent: &gtk4::Widget, title: &str) -> (Sender<Msg>, gio::Cancellable) {
    spawn_impl(parent, title, None)
}

/// Build the progress window, wire cancel and start the pump.
/// `verify` adds the integrity checkbox + status counter (copy jobs).
fn spawn_impl(
    parent: &gtk4::Widget,
    title: &str,
    verify: Option<VerifyFlag>,
) -> (Sender<Msg>, gio::Cancellable) {
    // Deliberately NOT modal: a copy can run for minutes and the
    // user must stay free to keep working in every window — modal
    // grabs (labwc redirects focus inside the transient tree) made
    // long transfers block the parent window outright.
    let window = gtk4::Window::builder()
        .title(title)
        .resizable(false)
        .default_width(420)
        .build();
    if let Some(root) = parent.root() {
        if let Ok(w) = root.downcast::<gtk4::Window>() {
            window.set_transient_for(Some(&w));
        }
    }

    let content = gtk4::Box::new(gtk4::Orientation::Vertical, 10);
    content.set_margin_top(16);
    content.set_margin_bottom(16);
    content.set_margin_start(16);
    content.set_margin_end(16);
    window.set_child(Some(&content));

    // Header: operation icon + title and the current entry below.
    let head = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);
    let icon = gtk4::Image::from_icon_name("folder-copy-symbolic");
    icon.set_pixel_size(32);
    icon.set_valign(gtk4::Align::Start);
    head.append(&icon);
    let head_text = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
    head.append(&head_text);
    let title_l = gtk4::Label::builder().label(title).xalign(0.0).build();
    title_l.add_css_class("heading");
    head_text.append(&title_l);
    let file_l = gtk4::Label::builder()
        .xalign(0.0)
        .ellipsize(gtk4::pango::EllipsizeMode::Middle)
        .build();
    file_l.add_css_class("dim-label");
    head_text.append(&file_l);
    content.append(&head);

    let bar = gtk4::ProgressBar::builder().hexpand(true).build();
    bar.set_show_text(true);
    content.append(&bar);

    // Detail line: "1.2 GB / 4.8 GB · 85 MB/s · ~35 s left".
    let detail_l = gtk4::Label::builder().xalign(0.0).build();
    detail_l.add_css_class("dim-label");
    content.append(&detail_l);

    // Integrity row for copies: toggle + live "2/3 verified" tally.
    // `flush_hint` remembers whether this window wraps a copy job —
    // only those can sit at 100 % while the tail is still flushing.
    let verify_status = gtk4::Label::builder().hexpand(true).xalign(1.0).build();
    verify_status.add_css_class("dim-label");
    let flush_hint = verify.is_some();
    if let Some(flag) = verify {
        let check = gtk4::CheckButton::with_label(&tr("Verify integrity"));
        check.set_tooltip_text(Some(&tr(
            "Hash each copied file and compare it with the source (BLAKE3)",
        )));
        let stored = crate::settings::verify_copies();
        flag.store(stored, std::sync::atomic::Ordering::Relaxed);
        check.set_active(stored);
        let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        row.append(&check);
        row.append(&verify_status);
        content.append(&row);
        check.connect_toggled(move |cb| {
            flag.store(cb.is_active(), std::sync::atomic::Ordering::Relaxed);
            crate::settings::set_verify_copies(cb.is_active());
        });
    }

    let cancel_button = gtk4::Button::with_label(&tr("Cancel"));
    cancel_button.set_halign(gtk4::Align::End);
    content.append(&cancel_button);

    let cancellable = gio::Cancellable::new();

    // Cancel button and window close both cancel the job.
    let c = cancellable.clone();
    cancel_button.connect_clicked(move |_| c.cancel());
    let c = cancellable.clone();
    window.connect_close_request(move |_| {
        c.cancel();
        glib::Propagation::Proceed
    });

    // Pump state: byte counters for rate/ETA plus the verify tally
    // that must survive until the result is shown.
    let state = Rc::new(RefCell::new(PumpState::default()));

    let (tx, rx) = std::sync::mpsc::channel::<Msg>();
    let parent_widget = parent.clone();
    let w = window.clone();
    let st = state.clone();
    glib::timeout_add_local(Duration::from_millis(50), move || {
        loop {
            match rx.try_recv() {
                Ok(Msg::Progress { name, done, total }) => {
                    file_l.set_text(&name);
                    let frac = if total > 0 {
                        done as f64 / total as f64
                    } else {
                        0.0
                    };
                    bar.set_fraction(frac.clamp(0.0, 1.0));
                    bar.set_text(Some(&format!("{:.0}%", frac * 100.0)));
                    let mut s = st.borrow_mut();
                    s.tick(done, total);
                    s.render(&detail_l);
                    // All bytes are in the page cache but the batch-end
                    // flush is still running — name the phase instead of
                    // leaving a frozen 100 % bar with a stale file name.
                    if flush_hint && total > 0 && done >= total {
                        file_l.set_text(&tr("Writing to device…"));
                    }
                }
                Ok(Msg::Verify { name, result }) => {
                    let mut s = st.borrow_mut();
                    match result {
                        VerifyResult::Verified | VerifyResult::Reflinked => {
                            s.verify_ok += 1;
                            s.verify_total += 1;
                        }
                        VerifyResult::Mismatch => {
                            s.verify_total += 1;
                            s.mismatches.push(name.clone());
                        }
                        VerifyResult::Unverifiable => s.unverifiable += 1,
                    }
                    drop(s);
                    let s = st.borrow();
                    verify_status.set_text(&s.verify_label());
                    file_l.set_text(&tr_fmt_verifying(&name));
                }
                Ok(Msg::Conflict { src, dst, reply }) => {
                    // The worker thread is parked on reply.recv() —
                    // the dialog answers it asynchronously.
                    let is_dir = src
                        .query_info(
                            "standard::type",
                            gio::FileQueryInfoFlags::NONE,
                            gio::Cancellable::NONE,
                        )
                        .map(|i| i.file_type() == gio::FileType::Directory)
                        .unwrap_or(false);
                    conflict::ask(&w, &src, &dst, is_dir, move |action, all| {
                        let _ = reply.send((action, all));
                    });
                }
                Ok(done @ (Msg::Done | Msg::Failed(_))) => {
                    let s = st.borrow();
                    let mismatches = s.mismatches.clone();
                    let verified = s.verify_total;
                    drop(s);
                    match done {
                        Msg::Failed(e) => {
                            operations::error_dialog(&parent_widget, &e);
                            w.close();
                            return glib::ControlFlow::Break;
                        }
                        _ => {
                            w.close();
                            if !mismatches.is_empty() {
                                let text = trn(
                                    "Integrity check failed for %d file",
                                    "Integrity check failed for %d files",
                                    mismatches.len() as u32,
                                )
                                .replace("%d", &mismatches.len().to_string())
                                    + "\n"
                                    + &mismatches.join("\n");
                                operations::error_dialog(&parent_widget, &text);
                            } else if verified > 0 {
                                let text = trn(
                                    "Integrity verified for %d file",
                                    "Integrity verified for %d files",
                                    verified,
                                )
                                .replace("%d", &verified.to_string());
                                operations::info_dialog(&parent_widget, &text);
                            }
                            return glib::ControlFlow::Break;
                        }
                    }
                }
                Err(_) => break,
            }
        }
        glib::ControlFlow::Continue
    });

    window.present();
    (tx, cancellable)
}

/// Mark the current file as "verifying" in the header line.
fn tr_fmt_verifying(name: &str) -> String {
    format!("{} — {}", name, tr("verifying…"))
}

/// Live pump counters; the rate is an exponentially smoothed bytes/s
/// sampled on every 50 ms tick, the ETA derives from the remainder.
#[derive(Default)]
struct PumpState {
    total: i64,
    done: i64,
    prev_done: i64,
    prev_tick_us: i64,
    rate: f64,
    verify_ok: u32,
    verify_total: u32,
    unverifiable: u32,
    mismatches: Vec<String>,
}

impl PumpState {
    /// Fold a new Progress message into the counters. The rate uses
    /// an EMA so single stutter ticks do not produce wild ETAs.
    fn tick(&mut self, done: i64, total: i64) {
        let now = glib::monotonic_time();
        if self.prev_tick_us > 0 && now > self.prev_tick_us {
            let dt = (now - self.prev_tick_us) as f64 / 1e6;
            let inst = (done - self.prev_done) as f64 / dt;
            if inst.is_finite() && inst >= 0.0 {
                self.rate = if self.rate > 0.0 {
                    0.7 * self.rate + 0.3 * inst
                } else {
                    inst
                };
            }
        }
        self.prev_done = done;
        self.prev_tick_us = now;
        self.done = done;
        self.total = total;
    }

    /// "1.2 GB / 4.8 GB · 85 MB/s · ~35 s" — empty while nothing is
    /// measured, rate/ETA appended once a rate estimate exists.
    fn render(&self, label: &gtk4::Label) {
        if self.total <= 0 {
            label.set_text("");
            return;
        }
        let mut text = format!(
            "{} / {}",
            glib::format_size(self.done as u64),
            glib::format_size(self.total as u64)
        );
        if self.rate > 0.0 {
            text += &format!(" · {}/s", glib::format_size(self.rate as u64));
            let left = (self.total - self.done) as f64 / self.rate;
            text += &format!(" · {}", format_eta(left));
        }
        label.set_text(&text);
    }

    /// "2/3 verified" for the status label next to the checkbox.
    fn verify_label(&self) -> String {
        tr("%d/%d verified")
            .replacen("%d", &self.verify_ok.to_string(), 1)
            .replacen("%d", &self.verify_total.to_string(), 1)
    }
}

/// Compact ETA: seconds stay seconds, beyond a minute it degrades to
/// minutes — the dialog is not a stopwatch.
fn format_eta(secs: f64) -> String {
    if !secs.is_finite() || secs < 0.0 {
        return "–".to_string();
    }
    if secs < 60.0 {
        tr("~%d s").replace("%d", &(secs.round() as u64).to_string())
    } else {
        tr("~%d min").replace("%d", &((secs / 60.0).ceil() as u64).to_string())
    }
}

#[cfg(test)]
mod tests;
