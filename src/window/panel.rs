// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// Window-level wiring for the embedded terminal panel: the toggle entry
// point and the position watch that persists the user-chosen height.

use std::rc::Rc;

use gtk4::prelude::*;

use super::ValkyrieWindow;

impl ValkyrieWindow {
    /// Show or hide the embedded terminal panel (View menu / F4 / the
    /// panel's own close button). Persists the visibility, restores the
    /// user-chosen height once the window has an allocation to split,
    /// and points the shell at the active tab's directory.
    pub(crate) fn set_terminal_panel(&self, on: bool) {
        self.terminal_panel.set_visible(on);
        crate::settings::set_terminal_panel_visible(on);
        if !on {
            return;
        }

        if !self.panel_pos_restored.replace(true) {
            // Position counts from the top: allocation minus panel height.
            let h = self.vpaned.allocated_height();
            let want = crate::settings::terminal_panel_height();
            if h > want {
                self.vpaned.set_position(h - want);
            }
        }
        if let Some(tab) = self.active_tab() {
            self.terminal_panel.set_location(tab.location().as_ref());
        }
    }
}

/// Persist the user-chosen panel height on every drag of the split
/// handle. Only while the panel is shown — a hidden end child gets zero
/// allocation and would otherwise overwrite the stored value.
pub(crate) fn watch_position(this: &Rc<ValkyrieWindow>) {
    let weak = Rc::downgrade(this);
    this.vpaned.connect_position_notify(move |p| {
        if let Some(this) = weak.upgrade() {
            if !this.terminal_panel.widget.is_visible() {
                return;
            }
            let h = p.allocated_height();
            let pos = p.position();
            if h > 0 && pos > 0 && pos < h {
                crate::settings::set_terminal_panel_height(h - pos);
            }
        }
    });
}
