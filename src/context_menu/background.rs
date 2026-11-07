// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// The background context menu: "New" submenu, location actions
// (terminal/admin/paste), view options and the arrange radios.

use gtk4::gio;
use gtk4::prelude::*;

use super::{menu_widgets, open_admin, tr};
use crate::create_entry;
use crate::file_view::FileView;
use crate::operations;
use crate::properties;
use crate::terminal;
use crate::view_model::SortField;

/// Menu on empty view space — "New ▸" and "Arrange Items ▸" open as
/// nested popovers so the top level stays short.
pub(super) fn build(
    main: &gtk4::Box,
    popover: &gtk4::Popover,
    widget: &gtk4::Widget,
    dir: &gio::File,
    view: &FileView,
) {
    // "New" submenu: folder, empty document, plus ~/Templates files.
    if crate::settings::menu_entry_visible("new") {
        let sub_new = menu_widgets::submenu(main, popover, &tr("New"));
        let w = widget.clone();
        let d = dir.clone();
        sub_new.item(&tr("Folder"), move || {
            create_entry::create_folder(&w, &d);
        });
        let w = widget.clone();
        let d = dir.clone();
        sub_new.item(&tr("Empty Document"), move || {
            create_entry::create_document(&w, &d, None);
        });
        for template in create_entry::templates() {
            let name = template
                .basename()
                .map(|b| b.to_string_lossy().into_owned())
                .unwrap_or_default();
            let w = widget.clone();
            let d = dir.clone();
            let t = template.clone();
            sub_new.item(&name, move || {
                create_entry::create_document(&w, &d, Some(&t));
            });
        }
    }

    // Location actions.
    if crate::settings::menu_entry_visible("terminal") {
        let d = dir.clone();
        menu_widgets::item(main, popover, &tr("Open Terminal Here"), move || {
            terminal::spawn(&d);
        });
    }
    if crate::settings::menu_entry_visible("terminal-panel") {
        let w = widget.clone();
        menu_widgets::item(main, popover, &tr("Open Terminal Panel"), move || {
            let _ = w.activate_action("win.terminal-panel", None);
        });
    }
    if dir.path().is_some() && crate::settings::menu_entry_visible("admin") {
        let view2 = view.clone();
        let dir2 = dir.clone();
        menu_widgets::item(main, popover, &tr("Open as Administrator"), move || {
            open_admin(&view2.widget.clone().upcast(), dir2.clone());
        });
    }
    if operations::clipboard_has_files(widget) && crate::settings::menu_entry_visible("paste") {
        let d = dir.clone();
        let w = widget.clone();
        let v = view.clone();
        menu_widgets::item(main, popover, &tr("Paste"), move || {
            operations::paste(&d, &w);
            // Cut-paste consumed the staged set — drop the ghosting.
            v.refresh_clip_styles();
        });
    }

    if crate::settings::menu_entry_visible("compress") {
        let w = widget.clone();
        let d = dir.clone();
        menu_widgets::item(main, popover, &tr("Compress…"), move || {
            // Compress the whole directory's contents.
            let files = std::fs::read_dir(d.path().unwrap_or_default())
                .ok()
                .into_iter()
                .flatten()
                .filter_map(|e| e.ok())
                .map(|e| gio::File::for_path(e.path()))
                .collect::<Vec<_>>();
            crate::archive_ui::compress_dialog(&w, &files);
        });
    }

    menu_widgets::separator(main);

    // View options: select all, arrange (radio marks), hidden toggle.
    if crate::settings::menu_entry_visible("select-all") {
        let view2 = view.clone();
        menu_widgets::item(main, popover, &tr("Select All"), move || {
            view2.select_all();
        });
    }

    let sub_arrange = if crate::settings::menu_entry_visible("arrange") {
        Some(menu_widgets::submenu(main, popover, &tr("Sort By")))
    } else {
        None
    };

    if crate::settings::menu_entry_visible("show-hidden") {
        let view2 = view.clone();
        let active = view.show_hidden();
        menu_widgets::check_item(main, popover, &tr("Show Hidden Files"), active, move || {
            view2.set_show_hidden(!active);
        });
    }

    menu_widgets::separator(main);

    let w = widget.clone();
    let d = dir.clone();
    menu_widgets::item(main, popover, &tr("Properties"), move || {
        properties::show(&w, &d);
    });

    // Fill the arrange submenu (current sort field is check-marked).
    if let Some(sub_arrange) = sub_arrange {
        let current = view.sort_field();
        for field in [
            SortField::Name,
            SortField::Size,
            SortField::Type,
            SortField::Modified,
        ] {
            let v = view.clone();
            sub_arrange.check_item(&field_label(field), field == current, move || {
                v.set_sort_field(field);
            });
        }
        let desc = view.sort_descending();
        let v = view.clone();
        sub_arrange.check_item(&tr("Reversed Order"), desc, move || {
            v.set_sort_descending(!desc);
        });
    }
}

/// Localized label for a sort field.
fn field_label(field: SortField) -> String {
    match field {
        SortField::Name => tr("Name"),
        SortField::Size => tr("Size"),
        SortField::Type => tr("Type"),
        SortField::Modified => tr("Modified"),
    }
}
