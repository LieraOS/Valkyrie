// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// Right-click menu for file rows and the view background. Row entries
// cover the standard actions (open, terminal here, administrator,
// clipboard, trash, properties); background clicks additionally offer
// folder/document creation, sorting and the hidden-file toggle.
//
// The menu is a hand-built GtkPopover (see menu_widgets.rs), not a
// GtkPopoverMenu: the latter embeds a scrolled window and scrolls
// whenever the compositor clamps its allocation — the hand-built
// one physically cannot scroll and is always fully visible.

use gtk4::gio;
use gtk4::prelude::*;

mod background;
mod xfer;

use crate::file_object::FileObject;
use crate::file_view::FileView;
use crate::i18n::tr;
use crate::menu_widgets;
use crate::operations;
use crate::properties;
use crate::terminal;

/// Build and show the context menu at (x, y) in view-stack space.
///
/// `picked` is the row under the pointer (None = background click);
/// `view` provides the displayed directory plus the view options the
/// background entries manipulate (sorting, hidden files).
///
/// One popover per FileView is reused: content, action wiring and
/// pointing rect are refreshed per click.
pub fn show(view: &FileView, x: f64, y: f64, picked: Option<&FileObject>) {
    let widget = view.widget.clone().upcast::<gtk4::Widget>();
    wire_dismiss_once(view, &widget);
    // Retire the previous popover: reusing a popover surface lets GTK
    // repaint the old menu's pixels for a frame before the new page
    // is laid out — the intermittent "menu draws wrong" glitch. A
    // fresh popover per click renders the new content only.
    if let Some(old) = view.ctx_popover.borrow_mut().take() {
        old.popdown();
        old.unparent();
    }
    let popover = gtk4::Popover::builder()
        .has_arrow(false)
        .autohide(false)
        .build();
    popover.set_parent(&widget);

    // Escape closes the menu — the controller dies with the popover.
    let key = gtk4::EventControllerKey::new();
    let p = popover.clone();
    key.connect_key_pressed(move |_, key, _, _| {
        if key == gtk4::gdk::Key::Escape {
            p.popdown();
            gtk4::glib::Propagation::Stop
        } else {
            gtk4::glib::Propagation::Proceed
        }
    });
    popover.add_controller(key);

    let page = menu_widgets::page();

    let in_trash = view
        .location()
        .map(|l| crate::trash_actions::is_trash_location(&l))
        .unwrap_or(false);

    if let Some(obj) = picked {
        build_row_menu(&page, &popover, &widget, obj, view, in_trash);
    } else if in_trash {
        build_trash_menu(&page, &popover, &widget);
    } else if let Some(dir) = view.location() {
        background::build(&page, &popover, &widget, &dir, view);
    }

    // Open towards the larger half of the window: GTK only flips a
    // popover when it would leave the *screen*, so a click low in the
    // window otherwise overflows past the bottom edge and gets
    // clamped by the compositor.
    let opens_upward = y > f64::from(view.widget.height()) / 2.0;
    popover.set_position(if opens_upward {
        gtk4::PositionType::Top
    } else {
        gtk4::PositionType::Bottom
    });
    popover.set_child(Some(&page));
    popover.set_pointing_to(Some(&gtk4::gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
    *view.ctx_popover.borrow_mut() = Some(popover.clone());
    popover.popup();
}

/// Wire the click-outside dismissal on the window — exactly once
/// per FileView.
///
/// autohide stays OFF: nested submenu popovers live on separate
/// surfaces, and GTK would otherwise treat a click into a submenu as
/// "outside the parent" and dismiss the whole menu before the entry
/// could fire. Instead we dismiss manually — a capture-phase click
/// controller on the window pops the menu down for any press that
/// lands on the main window (presses inside a popover surface never
/// reach it). The gesture resolves the live popover through
/// `ctx_popover`, so it keeps working while the menu itself is
/// recreated fresh on every click.
fn wire_dismiss_once(view: &FileView, widget: &gtk4::Widget) {
    if view.ctx_popover.borrow().is_some() {
        return;
    }
    let dismiss = gtk4::GestureClick::new();
    dismiss.set_propagation_phase(gtk4::PropagationPhase::Capture);
    let ctx = view.ctx_popover.clone();
    dismiss.connect_pressed(move |_, _, _, _| {
        if let Some(p) = ctx.borrow().as_ref() {
            if p.is_visible() {
                p.popdown();
            }
        }
    });
    if let Some(root) = widget.root() {
        root.add_controller(dismiss);
    }
}

/// Menu on a file/folder row. Inside trash:/// the destructive and
/// clipboard entries are replaced by restore/permanent delete.
fn build_row_menu(
    main: &gtk4::Box,
    popover: &gtk4::Popover,
    widget: &gtk4::Widget,
    obj: &FileObject,
    view: &FileView,
    in_trash: bool,
) {
    let file = obj.file();
    let is_dir = obj.is_dir();

    // Actions apply to the whole selection when the clicked row is
    // part of it — right-clicking inside a multi-selection must not
    // silently drop the other files (the clipboard would otherwise
    // get only one URI and paste would copy just that one).
    let targets = view.action_targets(obj);

    let view2 = view.clone();
    menu_widgets::item(main, popover, &tr("Open"), move || {
        view2.emit_activate(&file, is_dir);
    });

    if is_dir && !in_trash {
        if crate::settings::menu_entry_visible("open-tab") {
            let f = obj.file();
            let view2 = view.clone();
            menu_widgets::item(main, popover, &tr("Open in New Tab"), move || {
                view2.emit_open_in_tab(&f);
            });
        }
        if crate::settings::menu_entry_visible("open-window") {
            let f = obj.file();
            let view2 = view.clone();
            menu_widgets::item(main, popover, &tr("Open in New Window"), move || {
                view2.emit_open_in_window(&f);
            });
        }
    }

    // Files get an "Open With" submenu: apps registered for the
    // content type, plus the full app-chooser dialog.
    if !is_dir && !in_trash && crate::settings::menu_entry_visible("open-with") {
        let sub = menu_widgets::submenu(main, popover, &tr("Open With"));
        crate::open_with::fill_submenu(&sub, obj, widget);
    }

    if in_trash {
        let f = obj.file();
        let w = widget.clone();
        menu_widgets::item(main, popover, &tr("Restore"), move || {
            crate::trash_actions::restore(&w, &f);
        });
        let files = targets.clone();
        let w = widget.clone();
        menu_widgets::item(main, popover, &tr("Delete Permanently"), move || {
            crate::trash_actions::delete_forever(&w, &files);
        });
        return;
    }

    if is_dir {
        let file = obj.file();
        if file.path().is_some() && crate::settings::menu_entry_visible("admin") {
            let view2 = view.clone();
            menu_widgets::item(main, popover, &tr("Open as Administrator"), move || {
                open_admin(&view2.widget.clone().upcast(), file.clone());
            });
        }

        if crate::settings::menu_entry_visible("terminal") {
            let f = obj.file();
            menu_widgets::item(main, popover, &tr("Open in Terminal"), move || {
                terminal::spawn(&f);
            });
        }
        if crate::settings::menu_entry_visible("terminal-panel") {
            let w = widget.clone();
            menu_widgets::item(main, popover, &tr("Open Terminal Panel"), move || {
                let _ = w.activate_action("win.terminal-panel", None);
            });
        }
    }

    // "Set as Background" — single image file with a real local
    // path; writes the GNOME desktop background keys which the
    // ragnar daemon watches to restart swaybg.
    if !is_dir && crate::settings::menu_entry_visible("background") && is_local_image(&obj.file()) {
        let f = obj.file();
        menu_widgets::item(main, popover, &tr("Set as Background"), move || {
            set_wallpaper(&f);
        });
    }

    menu_widgets::separator(main);

    if crate::settings::menu_entry_visible("cut") {
        let files = targets.clone();
        let v = view.clone();
        menu_widgets::item(main, popover, &tr("Cut"), move || {
            operations::set_clipboard(&files, true);
            v.refresh_clip_styles();
        });
    }

    if crate::settings::menu_entry_visible("copy") {
        let files = targets.clone();
        let v = view.clone();
        menu_widgets::item(main, popover, &tr("Copy"), move || {
            operations::set_clipboard(&files, false);
            v.refresh_clip_styles();
        });
    }

    if crate::settings::menu_entry_visible("copy-path") {
        let files = targets.clone();
        menu_widgets::item(main, popover, &tr("Copy Path"), move || copy_paths(&files));
    }

    // Cross-pane transfer — only while this view's tab is split.
    xfer::add_pane_items(main, popover, widget, &targets, view);

    // Cloud upload only makes sense with a configured account.
    if crate::nextcloud::configured() && crate::settings::menu_entry_visible("cloud-upload") {
        let files = targets.clone();
        let w = widget.clone();
        menu_widgets::item(main, popover, &tr("Upload to Cloud…"), move || {
            crate::nextcloud::upload(&w, files.clone());
        });

        // Folder pairing: a local directory can be bound to the cloud
        // permanently (own direction + schedule), or — when already
        // paired — synced on demand.
        if is_dir {
            if let Some(path) = obj.file().path() {
                let path_str = path.to_string_lossy().into_owned();
                let w = widget.clone();
                menu_widgets::item(main, popover, &tr("Sync with Cloud…"), move || {
                    match crate::nextcloud::pairs::find_by_local(&path_str) {
                        Some(pair) => crate::nextcloud::sync::sync_now(&w, &pair),
                        None => {
                            crate::nextcloud::folders_ui::pair_dialog(&w, Some(&path_str), || {})
                        }
                    }
                });
            }
        }
    }

    if crate::settings::menu_entry_visible("rename") {
        let f = obj.file();
        let w = widget.clone();
        let many = targets.clone();
        menu_widgets::item(main, popover, &tr("Rename…"), move || {
            // Multi-selection opens the pattern-based batch dialog;
            // a single file keeps the plain rename prompt.
            if many.len() > 1 {
                crate::batch_rename::show(&w, &many);
            } else {
                crate::rename::rename(&w, &f);
            }
        });
    }

    if crate::settings::menu_entry_visible("trash") {
        let files = targets.clone();
        let w = widget.clone();
        menu_widgets::item(main, popover, &tr("Move to Trash"), move || {
            for f in &files {
                operations::trash(f, &w);
            }
        });
    }

    if crate::settings::menu_entry_visible("delete-permanent") {
        let files = targets.clone();
        let w = widget.clone();
        menu_widgets::item(main, popover, &tr("Delete Permanently"), move || {
            operations::delete_permanently(&w, &files);
        });
    }

    menu_widgets::separator(main);

    // Bookmark toggle for directories — adds/removes the location
    // in the sidebar's Bookmarks section (live refresh).
    if is_dir && crate::settings::menu_entry_visible("bookmark") {
        let uri = obj.file().uri().to_string();
        let bookmarked = crate::settings::bookmarks().iter().any(|u| u == &uri);
        let label = if bookmarked {
            tr("Remove Bookmark")
        } else {
            tr("Add Bookmark")
        };
        menu_widgets::item(main, popover, &label, move || {
            if bookmarked {
                crate::settings::remove_bookmark(&uri);
            } else {
                crate::settings::add_bookmark(&uri);
            }
        });
    }

    // Archive operations: "Extract Here" on archives, "Compress…"
    // on any selection. Both are toggleable in Preferences → Context
    // menu. Extraction runs via 7z/tar in the current directory;
    // compression offers a small format dialog (zip/tar.gz/7z).
    if crate::settings::menu_entry_visible("extract")
        && crate::archive::is_archive(&obj.content_type())
    {
        let f = obj.file();
        let w = widget.clone();
        menu_widgets::item(main, popover, &tr("Extract Here"), move || {
            crate::archive_ui::extract_here(&w, &f);
        });
    }
    if crate::settings::menu_entry_visible("compress") {
        let files: Vec<gio::File> = targets.to_vec();
        let w = widget.clone();
        menu_widgets::item(main, popover, &tr("Compress…"), move || {
            crate::archive_ui::compress_dialog(&w, &files);
        });
    }

    // Properties of the clicked entry.
    let f = obj.file();
    let w = widget.clone();
    menu_widgets::item(main, popover, &tr("Properties"), move || {
        properties::show(&w, &f);
    });
}

/// Background menu inside trash:/// — the only useful action there is
/// emptying it (creation/paste make no sense in the trash).
fn build_trash_menu(main: &gtk4::Box, popover: &gtk4::Popover, widget: &gtk4::Widget) {
    let w = widget.clone();
    menu_widgets::item(main, popover, &tr("Empty Trash…"), move || {
        crate::trash_actions::empty(&w);
    });
}

/// Local image file (image/* content type + real path) — the
/// "Set as Background" entry only makes sense for these.
fn is_local_image(file: &gio::File) -> bool {
    if file.path().is_none() {
        return false;
    }
    file.query_info(
        "standard::content-type",
        gio::FileQueryInfoFlags::NONE,
        gio::Cancellable::NONE,
    )
    .ok()
    .and_then(|i| i.content_type().map(|c| c.to_string()))
    .is_some_and(|ct| ct.starts_with("image/"))
}

/// Set the image as the desktop background: the ragnar daemon
/// watches org.gnome.desktop.background and relaunches swaybg on
/// every change — a gsettings write is all this needs.
fn set_wallpaper(file: &gio::File) {
    let Some(path) = file.path() else { return };
    // Guard the lookup: on systems without the schema
    // gio::Settings::new aborts instead of returning an error.
    let Some(source) = gio::SettingsSchemaSource::default() else {
        return;
    };
    if source
        .lookup("org.gnome.desktop.background", true)
        .is_none()
    {
        return;
    }
    let uri = format!("file://{}", path.to_string_lossy());
    let s = gio::Settings::new("org.gnome.desktop.background");
    let _ = s.set_string("picture-uri", &uri);
    let _ = s.set_string("picture-uri-dark", &uri);
}

/// Open the directory in a fresh Valkyrie instance running as root.
/// `pkexec` triggers the session's polkit dialog every time — a new
/// process is a new subject, so nothing is cached. Closing the
/// window kills the root process; the authorization dies with it.
pub(crate) fn open_admin(parent: &gtk4::Widget, file: gio::File) {
    let Some(path) = file.path() else {
        return;
    };
    let exe = std::env::current_exe().unwrap_or_else(|_| "valkyrie".into());
    let mut cmd = std::process::Command::new("pkexec");
    // pkexec wipes the environment; `env` re-arms what a GTK client
    // needs — display socket, session bus, locale catalogs and the
    // user's gsettings schema directory.
    cmd.arg("env").arg("VALKYRIE_ELEVATED=1");
    for key in [
        "WAYLAND_DISPLAY",
        "XDG_RUNTIME_DIR",
        "DBUS_SESSION_BUS_ADDRESS",
        "DISPLAY",
        "XAUTHORITY",
        "LANG",
        "LOCALEDIR",
        "GSETTINGS_SCHEMA_DIR",
        // The elevated instance reads the user's own config/data
        // dirs — dconf, theme, icon theme and fonts then match the
        // desktop instead of root's defaults.
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
    ] {
        if let Ok(v) = std::env::var(key) {
            cmd.arg(format!("{key}={v}"));
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        if std::env::var_os("GSETTINGS_SCHEMA_DIR").is_none() {
            cmd.arg(format!(
                "GSETTINGS_SCHEMA_DIR={home}/.local/share/glib-2.0/schemas"
            ));
        }
        if std::env::var_os("XDG_CONFIG_HOME").is_none() {
            cmd.arg(format!("XDG_CONFIG_HOME={home}/.config"));
        }
        if std::env::var_os("XDG_DATA_HOME").is_none() {
            cmd.arg(format!("XDG_DATA_HOME={home}/.local/share"));
        }
    }
    cmd.arg(exe).arg(&path);
    let window = parent
        .root()
        .and_then(|r| r.downcast::<gtk4::Window>().ok());
    if let Err(e) = cmd.spawn() {
        gtk4::AlertDialog::builder()
            .message(crate::i18n::tr("Could not open as administrator"))
            .detail(e.to_string())
            .modal(true)
            .build()
            .show(window.as_ref());
    }
}

/// Put the file path/URI on the clipboard.
/// Copy the target paths to the clipboard — one per line for a
/// multi-selection, matching what paste-capable tools expect.
fn copy_paths(files: &[gio::File]) {
    let text = files
        .iter()
        .map(|f| {
            f.path()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|| f.uri().to_string())
        })
        .collect::<Vec<_>>()
        .join("\n");
    if let Some(display) = gtk4::gdk::Display::default() {
        let clipboard = display.clipboard();
        clipboard.set_text(&text);
        clipboard.store_async(glib::Priority::DEFAULT, gio::Cancellable::NONE, |_| {});
    }
}
