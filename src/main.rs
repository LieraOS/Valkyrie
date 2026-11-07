// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project

mod app;
mod archive;
mod archive_ui;
mod batch_rename;
mod browser_tab;
mod conflict;
mod context_menu;
mod copy_pool;
mod create_entry;
mod dir_watch;
mod dnd;
mod fast_copy;
mod file_object;
mod file_view;
mod i18n;
mod menu_widgets;
mod menubar;
mod mode_editor;
mod mount_ops;
mod navigation;
mod nextcloud;
mod open_with;
mod operations;
mod pane_view;
mod pathbar;
mod progress;
mod properties;
mod properties_perms;
mod rename;
mod rubberband;
mod search;
mod secrets;
mod secure_wipe;
mod settings;
mod settings_dialog;
mod sidebar;
mod terminal;
mod terminal_panel;
mod thumbnails;
mod trash_actions;
mod tree_ops;
mod undo;
mod view_grid;
mod view_model;
mod view_widgets;
mod window;
mod window_actions;
mod zoom;

fn main() -> glib::ExitCode {
    // Locale + gettext must be up before any widget builds a label.
    i18n::init();
    // Keep the terminal quiet: GTK/GDK emit cosmetic warnings (e.g.
    // the Vulkan VK_SUBOPTIMAL_KHR resize notice) that look alarming
    // but are harmless. Drop everything below Critical — real errors
    // still surface.
    glib::log_set_writer_func(|level, _fields| match level {
        glib::LogLevel::Error | glib::LogLevel::Critical => glib::LogWriterOutput::Unhandled,
        _ => glib::LogWriterOutput::Handled,
    });
    app::run()
}
