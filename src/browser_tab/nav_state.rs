// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// Per-tab navigation state and status reporting. All methods operate on
// the *active* pane: in a split tab the secondary pane's own history and
// counts take over (see browser_tab::split).

use gtk4::gio;
use gtk4::prelude::*;

use crate::i18n::tr;

use super::status_text;
use super::BrowserTab;

impl BrowserTab {
    /// History back — called by the navigation wiring.
    pub(crate) fn go_back(&self) {
        if let Some(pane) = self.secondary_if_active() {
            pane.go_back();
            return;
        }
        let Some(target) = self.back_stack.borrow_mut().pop() else {
            return;
        };
        if let Some(current) = self.location() {
            self.forward_stack.borrow_mut().push(current);
        }
        self.show_location(&target);
    }

    /// History forward — called by the navigation wiring.
    pub(crate) fn go_forward(&self) {
        if let Some(pane) = self.secondary_if_active() {
            pane.go_forward();
            return;
        }
        let Some(target) = self.forward_stack.borrow_mut().pop() else {
            return;
        };
        if let Some(current) = self.location() {
            self.back_stack.borrow_mut().push(current);
        }
        self.show_location(&target);
    }

    pub(crate) fn update_nav_sensitivity(&self) {
        let (can_back, can_forward) = match self.secondary_if_active() {
            Some(pane) => (pane.can_back(), pane.can_forward()),
            None => (
                !self.back_stack.borrow().is_empty(),
                !self.forward_stack.borrow().is_empty(),
            ),
        };
        self.back_button.set_sensitive(can_back);
        self.forward_button.set_sensitive(can_forward);
    }

    /// Status line: total row count, plus marked count and combined
    /// size while a selection is active.
    pub(crate) fn update_status(&self) {
        let view = self.active_view();
        let (sel, bytes) = view.selection_summary();
        let mut text = status_text::status_text(view.item_count(), sel, bytes);
        // Free space of the current filesystem — what classic file managers shows
        // permanently on the right. Silently skipped where the
        // backend does not report it (trash://, network:///).
        if let Some(info) = view.location().and_then(|dir| {
            dir.query_filesystem_info("filesystem::free", gio::Cancellable::NONE)
                .ok()
        }) {
            let free = info.attribute_uint64("filesystem::free");
            if free > 0 {
                text = format!(
                    "{text} — {}",
                    tr("{size} free").replace("{size}", &glib::format_size(free))
                );
            }
        }
        self.status.set_text(&text);
    }
}
