// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// Application object: owns actions, keyboard shortcuts and the
// primary window.

use gtk4::prelude::*;

use crate::window::ValkyrieWindow;

pub const APP_ID: &str = "org.lieraos.valkyrie";

/// Icon name the desktop file and window decoration resolve through
/// the installed icon theme (hicolor fallback included).
const APP_ICON: &str = "valkyrie";

/// Build the application object, wire actions and run the main loop.
pub fn run() -> glib::ExitCode {
    let app = gtk4::Application::new(
        Some(APP_ID),
        gio::ApplicationFlags::HANDLES_OPEN | gio::ApplicationFlags::NON_UNIQUE,
    );

    app.connect_startup(|_| {
        apply_elevated_theme();
        load_stylesheet();
        gtk4::Window::set_default_icon_name(APP_ICON);
        apply_icon_theme();
        // Schedule drivers for synced cloud folders (interval timers
        // + live watchers) — starts once, rebuilds on every settings
        // change. In connect_startup so both activate and open
        // (CLI path) run the watches; anchors resolve at fire time.
        crate::nextcloud::monitor::start();
    });

    app.connect_activate(|app| {
        let window = ValkyrieWindow::new(app, None);
        window.widget.present();
    });

    // `valkyrie /some/dir` opens that location in a new window.
    app.connect_open(|app, files, _hint| {
        let file = files.first().cloned();
        let window = ValkyrieWindow::new(app, file.as_ref());
        window.widget.present();
    });

    app.run()
}

/// Register the bundled application stylesheet (icon-view selection
/// shape and other tweaks) on the default display.
fn load_stylesheet() {
    let provider = gtk4::CssProvider::new();
    provider.load_from_data(include_str!("../data/valkyrie.css"));
    if let Some(display) = gtk4::gdk::Display::default() {
        gtk4::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

/// Activate the bundled "Valkyrie" icon theme when it is installed in
/// a standard icon location — the per-user data dir first, then the
/// XDG system data dirs. The theme inherits Adwaita/hicolor, so icons
/// Valkyrie does not ship fall back automatically; without an
/// installed theme the default icon set stays active.
fn apply_icon_theme() {
    let installed = std::iter::once(glib::user_data_dir())
        .chain(glib::system_data_dirs())
        .map(|d| d.join("icons/Valkyrie/index.theme"))
        .any(|p| p.exists());
    if !installed {
        return;
    }
    // The per-display IconTheme is a singleton — set_theme_name is
    // private to the toolkit. Apps switch themes via the GtkSettings
    // property the singleton listens to.
    if let Some(settings) = gtk4::Settings::default() {
        settings.set_property("gtk-icon-theme-name", "Valkyrie");
    }
}

/// Elevated (pkexec) instances: the XDG settings portal answers
/// root clients with defaults, so the window would come up light.
/// Instead read the user's color-scheme/theme from the forwarded
/// dconf and apply it straight onto GtkSettings.
fn apply_elevated_theme() {
    if std::env::var_os("VALKYRIE_ELEVATED").is_none() {
        return;
    }
    let Some(src) = gio::SettingsSchemaSource::default() else {
        return;
    };
    if src.lookup("org.gnome.desktop.interface", true).is_none() {
        return;
    }
    let iface = gio::Settings::new("org.gnome.desktop.interface");
    if let Some(settings) = gtk4::Settings::default() {
        settings
            .set_gtk_application_prefer_dark_theme(iface.string("color-scheme") == "prefer-dark");
        settings.set_gtk_theme_name(Some(&iface.string("gtk-theme")));
    }
}
