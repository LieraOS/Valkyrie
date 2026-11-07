// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// Cross-pane transfer entries for the row context menu: "Copy to Other
// Pane" / "Move to Other Pane", shown only while the owning tab is
// split. The sibling registry resolves the opposite pane's directory.

use gtk4::gio;

use crate::browser_tab::split;
use crate::file_view::FileView;
use crate::i18n::tr;
use crate::menu_widgets;

/// Append the cross-pane transfer items when `view`'s tab is split.
pub(super) fn add_pane_items(
    main: &gtk4::Box,
    popover: &gtk4::Popover,
    widget: &gtk4::Widget,
    targets: &[gio::File],
    view: &FileView,
) {
    let Some(dest) = split::other_location_for(view) else {
        return;
    };

    let files = targets.to_vec();
    let w = widget.clone();
    let d = dest.clone();
    menu_widgets::item(main, popover, &tr("Copy to Other Pane"), move || {
        crate::progress::copy_move(&w, files.clone(), d.clone(), false);
    });

    let files = targets.to_vec();
    let w = widget.clone();
    menu_widgets::item(main, popover, &tr("Move to Other Pane"), move || {
        crate::progress::copy_move(&w, files.clone(), dest.clone(), true);
    });
}
