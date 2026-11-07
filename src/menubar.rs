// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// The classic menu bar model (File / Edit / View / Help). All entries
// point at "win.*" actions registered by the main window; stateful
// actions (view mode, sorting, hidden files) render as radio/checkbox
// entries automatically.

use gtk4::gio;

use crate::i18n::tr;

/// Build the complete menubar model. Stateless entries trigger
/// "win.<name>" actions; ":<value>" targets feed stateful actions.
pub fn build_model() -> gio::Menu {
    let bar = gio::Menu::new();
    bar.append_submenu(Some(&tr("_File")), &file_menu());
    bar.append_submenu(Some(&tr("_Edit")), &edit_menu());
    bar.append_submenu(Some(&tr("_View")), &view_menu());
    bar.append_submenu(Some(&tr("_Help")), &help_menu());
    bar
}

/// File: tabs/windows, creation, properties, quit.
fn file_menu() -> gio::Menu {
    let menu = gio::Menu::new();

    let s = gio::Menu::new();
    s.append(Some(&tr("New Tab")), Some("win.new-tab"));
    s.append(Some(&tr("New Window")), Some("win.new-window"));
    menu.append_section(None, &s);

    let s = gio::Menu::new();
    s.append(Some(&tr("New Folder")), Some("win.new-folder"));
    s.append(Some(&tr("New Document")), Some("win.new-document"));
    menu.append_section(None, &s);

    let s = gio::Menu::new();
    s.append(Some(&tr("Properties")), Some("win.properties"));
    menu.append_section(None, &s);

    let s = gio::Menu::new();
    s.append(Some(&tr("Quit")), Some("win.quit"));
    menu.append_section(None, &s);

    menu
}

/// Edit: clipboard operations on the selection, select-all, trash.
fn edit_menu() -> gio::Menu {
    let menu = gio::Menu::new();

    let s = gio::Menu::new();
    s.append(Some(&tr("Cut")), Some("win.cut"));
    s.append(Some(&tr("Copy")), Some("win.copy"));
    s.append(Some(&tr("Paste")), Some("win.paste"));
    menu.append_section(None, &s);

    let s = gio::Menu::new();
    s.append(
        Some(&tr("Copy to Other Pane")),
        Some("win.copy-to-other-pane"),
    );
    s.append(
        Some(&tr("Move to Other Pane")),
        Some("win.move-to-other-pane"),
    );
    menu.append_section(None, &s);

    let s = gio::Menu::new();
    s.append(Some(&tr("Select All")), Some("win.select-all"));
    s.append(Some(&tr("Search")), Some("win.search"));
    menu.append_section(None, &s);

    let s = gio::Menu::new();
    s.append(Some(&tr("Rename")), Some("win.rename"));
    s.append(Some(&tr("Move to Trash")), Some("win.trash"));
    menu.append_section(None, &s);

    let s = gio::Menu::new();
    s.append(Some(&tr("Preferences")), Some("win.preferences"));
    menu.append_section(None, &s);

    menu
}

/// View: icons/list radio, arrange submenu, hidden toggle, reload.
fn view_menu() -> gio::Menu {
    let menu = gio::Menu::new();

    let s = gio::Menu::new();
    s.append(Some(&tr("Icon View")), Some("win.view-mode::icons"));
    s.append(Some(&tr("List View")), Some("win.view-mode::list"));
    menu.append_section(None, &s);

    let arrange = gio::Menu::new();
    arrange.append(Some(&tr("Name")), Some("win.arrange::name"));
    arrange.append(Some(&tr("Size")), Some("win.arrange::size"));
    arrange.append(Some(&tr("Type")), Some("win.arrange::type"));
    arrange.append(Some(&tr("Modified")), Some("win.arrange::modified"));
    arrange.append(Some(&tr("Reversed Order")), Some("win.arrange-desc"));
    let s = gio::Menu::new();
    s.append_submenu(Some(&tr("Sort By")), &arrange);
    menu.append_section(None, &s);

    let s = gio::Menu::new();
    s.append(Some(&tr("Show Hidden Files")), Some("win.show-hidden"));
    menu.append_section(None, &s);

    let s = gio::Menu::new();
    s.append(Some(&tr("Split View")), Some("win.split-view"));
    s.append(Some(&tr("Terminal Panel")), Some("win.terminal-panel"));
    menu.append_section(None, &s);

    let s = gio::Menu::new();
    s.append(Some(&tr("Reload")), Some("win.reload"));
    menu.append_section(None, &s);

    menu
}

/// Help: only "About" for now — a real help section lands once there
/// is content worth pointing at.
fn help_menu() -> gio::Menu {
    let menu = gio::Menu::new();
    menu.append(Some(&tr("About Valkyrie")), Some("win.about"));
    menu
}
