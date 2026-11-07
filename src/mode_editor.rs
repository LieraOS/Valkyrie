// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// Shared POSIX mode editor: the rwx checkbox matrix plus an octal
// entry ("755", "777") kept in sync in both directions. Typing a
// valid octal value updates the checkboxes; toggling a checkbox
// updates the entry. Used by the properties "Permissions" page and
// the create dialogs' permission expander.

use std::cell::Cell;
use std::rc::Rc;

use gtk4::prelude::*;

use crate::i18n::tr;
use crate::properties_perms::{COL_BITS, ROW_BITS};

/// Parses an octal mode string ("755", "0644", "777") into mode bits.
/// Only the nine rwx permission bits are representable in the UI, so
/// values carrying special bits (setuid/setgid/sticky) are rejected
/// rather than silently masked.
pub(crate) fn parse_octal(text: &str) -> Option<u32> {
    let text = text.trim();
    if text.is_empty() || text.len() > 4 || !text.bytes().all(|b| b.is_ascii_digit() && b < b'8') {
        return None;
    }
    match u32::from_str_radix(text, 8) {
        Ok(v) if v <= 0o777 => Some(v),
        _ => None,
    }
}

/// Formats mode bits as the three-digit octal the entry displays
/// ("755", "644", "000").
fn octal_text(mode: u32) -> String {
    format!("{:03o}", mode & 0o777)
}

/// The rwx matrix + octal entry. `on_change(mode)` is invoked for every
/// committed change — checkbox or entry alike — and reports whether the
/// change was accepted; returning `false` reverts the widgets to the
/// last accepted mode. Pass `None` for a purely visual editor whose
/// value is read back via `ModeEditor::mode` (create dialogs).
pub(crate) struct ModeEditor {
    pub widget: gtk4::Grid,
    pub(crate) checks: Vec<(gtk4::CheckButton, u32)>,
    pub(crate) entry: gtk4::Entry,
    // Last mode the consumer accepted; widgets revert to it when
    // on_change rejects a change (e.g. chmod EPERM).
    last_good: Rc<Cell<u32>>,
    // Re-entry guard: updating checkboxes/entry programmatically must
    // not cascade back into the change handlers.
    syncing: Rc<Cell<bool>>,
    on_change: Option<Rc<dyn Fn(u32) -> bool>>,
}

impl ModeEditor {
    /// Build the editor preset to `initial_mode`. `on_change` may be
    /// `None` (create dialogs read the mode back on commit).
    pub fn new(initial_mode: u32, on_change: Option<Rc<dyn Fn(u32) -> bool>>) -> Self {
        let grid = gtk4::Grid::builder()
            .row_spacing(8)
            .column_spacing(12)
            .build();

        for (col, title) in [tr("Read"), tr("Write"), tr("Execute")].iter().enumerate() {
            grid.attach(
                &gtk4::Label::builder()
                    .label(format!("<b>{title}</b>"))
                    .use_markup(true)
                    .build(),
                1 + col as i32,
                0,
                1,
                1,
            );
        }

        let checks = Vec::new();
        let last_good = Rc::new(Cell::new(initial_mode));
        let syncing = Rc::new(Cell::new(false));

        let mut editor = ModeEditor {
            widget: grid.clone(),
            checks,
            entry: gtk4::Entry::builder()
                .text(octal_text(initial_mode))
                .max_length(4)
                .width_chars(5)
                .input_purpose(gtk4::InputPurpose::Digits)
                .build(),
            last_good: last_good.clone(),
            syncing: syncing.clone(),
            on_change,
        };

        for (row, (role, row_bits)) in [tr("Owner"), tr("Group"), tr("Others")]
            .iter()
            .zip(ROW_BITS)
            .enumerate()
        {
            grid.attach(
                &gtk4::Label::builder()
                    .label(format!("<b>{role}</b>"))
                    .use_markup(true)
                    .xalign(0.0)
                    .build(),
                0,
                1 + row as i32,
                1,
                1,
            );
            for (col, col_bits) in COL_BITS.iter().enumerate() {
                let bit = row_bits & col_bits;
                let check = gtk4::CheckButton::builder()
                    .active(initial_mode & bit != 0)
                    .halign(gtk4::Align::Center)
                    .build();
                editor.connect_check(&check, bit);
                grid.attach(&check, 1 + col as i32, 1 + row as i32, 1, 1);
                editor.checks.push((check, bit));
            }
        }

        // Fourth row: "Octal" label + entry, synced with the matrix.
        grid.attach(
            &gtk4::Label::builder()
                .label(format!("<b>{}</b>", tr("Octal")))
                .use_markup(true)
                .xalign(0.0)
                .build(),
            0,
            4,
            1,
            1,
        );
        grid.attach(&editor.entry, 1, 4, 3, 1);
        editor.connect_entry();

        editor
    }

    /// Current mode as selected by the widgets (checkbox state is the
    /// source of truth; the entry mirrors it on every valid commit).
    pub fn mode(&self) -> u32 {
        self.checks
            .iter()
            .filter(|(check, _)| check.is_active())
            .fold(0u32, |mode, (_, bit)| mode | bit)
    }

    /// Checkbox toggled: recompute the mode, ask the consumer, revert
    /// on rejection, mirror to the entry on success.
    fn connect_check(&self, check: &gtk4::CheckButton, bit: u32) {
        let syncing = self.syncing.clone();
        let last_good = self.last_good.clone();
        let entry = self.entry.clone();
        let on_change = self.on_change.clone();
        check.connect_toggled(move |check| {
            if syncing.get() {
                return;
            }
            let mut mode = last_good.get();
            if check.is_active() {
                mode |= bit;
            } else {
                mode &= !bit;
            }
            let accepted = on_change.as_ref().map(|f| f(mode)).unwrap_or(true);
            if accepted {
                last_good.set(mode);
                syncing.set(true);
                entry.set_text(&octal_text(mode));
                entry.remove_css_class("error");
                syncing.set(false);
            } else {
                syncing.set(true);
                check.set_active(!check.is_active());
                syncing.set(false);
            }
        });
    }

    /// Entry committed (Enter or focus-out): parse, ask the consumer,
    /// then sync the matrix — or restore the last accepted text.
    fn connect_entry(&self) {
        let ctx = Rc::new(EntryCtx {
            entry: self.entry.clone(),
            checks: self.checks.clone(),
            syncing: self.syncing.clone(),
            last_good: self.last_good.clone(),
            on_change: self.on_change.clone(),
        });
        let c = ctx.clone();
        self.entry.connect_activate(move |_| commit_entry(&c, true));
        let c = ctx.clone();
        let focus = gtk4::EventControllerFocus::new();
        focus.connect_leave(move |_| commit_entry(&c, true));
        self.entry.add_controller(focus);
        let ctx = ctx.clone();
        self.entry.connect_changed(move |entry| {
            let text = entry.text();
            if parse_octal(&text).is_some() {
                entry.remove_css_class("error");
            }
            // Live-apply as soon as the input is a complete octal mode
            // (3 digits like "755"); shorter fragments stay pending
            // until Enter/focus-out so intermediate keystrokes cannot
            // flip permissions.
            if text.len() >= 3 {
                commit_entry(&ctx, false);
            }
        });
    }
}

/// Shared widget+state context for entry commits.
struct EntryCtx {
    entry: gtk4::Entry,
    checks: Vec<(gtk4::CheckButton, u32)>,
    syncing: Rc<Cell<bool>>,
    last_good: Rc<Cell<u32>>,
    on_change: Option<Rc<dyn Fn(u32) -> bool>>,
}

/// Apply the entry text: valid octal → consumer decides; accepted →
/// checkboxes mirror it, rejected → `error` styling. When
/// `restore_on_invalid` is set (Enter/focus-out commit), the text is
/// additionally reset to the last accepted mode; during live typing it
/// is left alone so the input is not wiped mid-keystroke.
fn commit_entry(ctx: &EntryCtx, restore_on_invalid: bool) {
    let EntryCtx {
        entry,
        checks,
        syncing,
        last_good,
        on_change,
    } = ctx;
    let text = entry.text().to_string();
    if text == octal_text(last_good.get()) {
        return; // unchanged — nothing to commit
    }
    match parse_octal(&text) {
        Some(mode) => {
            let accepted = on_change.as_ref().map(|f| f(mode)).unwrap_or(true);
            if accepted {
                last_good.set(mode);
                syncing.set(true);
                for (check, bit) in checks {
                    check.set_active(mode & bit != 0);
                }
                syncing.set(false);
                entry.set_text(&octal_text(mode));
                entry.remove_css_class("error");
            } else {
                entry.set_text(&octal_text(last_good.get()));
                entry.add_css_class("error");
            }
        }
        None => {
            if restore_on_invalid {
                entry.set_text(&octal_text(last_good.get()));
            }
            entry.add_css_class("error");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_octal_accepts_common_modes() {
        assert_eq!(parse_octal("755"), Some(0o755));
        assert_eq!(parse_octal("777"), Some(0o777));
        assert_eq!(parse_octal("0644"), Some(0o644));
        assert_eq!(parse_octal("0"), Some(0));
        assert_eq!(parse_octal("000"), Some(0));
    }

    #[test]
    fn parse_octal_rejects_bad_input() {
        assert_eq!(parse_octal(""), None);
        assert_eq!(parse_octal("888"), None);
        assert_eq!(parse_octal("abc"), None);
        assert_eq!(parse_octal("12a"), None);
        // Special bits (setuid/sticky) are not representable here.
        assert_eq!(parse_octal("1777"), None);
        assert_eq!(parse_octal("1000"), None);
        // More than four digits.
        assert_eq!(parse_octal("00000"), None);
    }

    #[test]
    fn octal_text_formats_three_digits() {
        assert_eq!(octal_text(0o755), "755");
        assert_eq!(octal_text(0o644), "644");
        assert_eq!(octal_text(0), "000");
        // Special bits are masked out of the display.
        assert_eq!(octal_text(0o4755), "755");
    }
}
