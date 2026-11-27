// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// The org.freedesktop.FileManager1 D-Bus interface — the mechanism
// browsers and other apps use for "Show in Folder", distinct from
// the MIME default (inode/directory → double click / xdg-open).
// Without this object the only application answering the well-known
// name on the system is whatever ships a service file (Nemo on the
// developer box), so Firefox's "Show in Folder" opens a foreign
// file manager. While a Valkyrie instance owns the name, calls land
// here and open real Valkyrie windows with the target pre-selected.

use gtk4::gio;
use gtk4::prelude::*;

/// Object path + well-known name per the freedesktop spec.
const PATH: &str = "/org/freedesktop/FileManager1";
const NAME: &str = "org.freedesktop.FileManager1";

/// Introspection for the three spec methods. StartupId is accepted
/// but ignored — labwc focuses presented windows regardless.
const XML: &str = concat!(
    "<node>",
    "<interface name='org.freedesktop.FileManager1'>",
    "<method name='ShowFolders'>",
    "<arg type='as' name='URIs' direction='in'/>",
    "<arg type='s' name='StartupId' direction='in'/>",
    "</method>",
    "<method name='ShowItems'>",
    "<arg type='as' name='URIs' direction='in'/>",
    "<arg type='s' name='StartupId' direction='in'/>",
    "</method>",
    "<method name='ShowItemProperties'>",
    "<arg type='as' name='URIs' direction='in'/>",
    "<arg type='s' name='StartupId' direction='in'/>",
    "</method>",
    "</interface>",
    "</node>",
);

/// Claim the well-known name on the session bus and serve calls.
/// With NON_UNIQUE every Valkyrie process asks for the name — the
/// first one wins and serves the whole desktop session; later
/// instances get name_lost and simply never receive calls. D-Bus
/// activation (when nothing runs) is covered by the service file
/// shipped in the package.
pub fn install(app: &gtk4::Application) {
    let app_acquired = app.clone();
    let app_held = app.clone();
    gio::bus_own_name(
        gio::BusType::Session,
        NAME,
        gio::BusNameOwnerFlags::NONE,
        move |conn, _name| register(&conn, &app_acquired),
        move |_, _| {
            // A `--gapplication-service` instance may idle-quit once
            // activation completes; hold the app while it owns the
            // name so queued callers always reach a live object. The
            // guard intentionally leaks — the hold is meant to last
            // for the process lifetime.
            std::mem::forget(app_held.hold());
        },
        |_conn, _name| {},
    );
}

/// Export the object on the freshly acquired connection — a new
/// connection (after a bus restart) re-registers the same object.
fn register(conn: &gio::DBusConnection, app: &gtk4::Application) {
    let Ok(node) = gio::DBusNodeInfo::for_xml(XML) else {
        return;
    };
    let Some(iface) = node.interfaces().first().cloned() else {
        return;
    };
    let app = app.clone();
    let result = conn
        .register_object(PATH, &iface)
        .method_call(
            move |_conn, _sender, _path, _iface, method, params, invocation| {
                // Every method's signature is (as, s): URI list + the
                // ignored startup id.
                let uris: Vec<String> = params
                    .child_value(0)
                    .get::<Vec<String>>()
                    .unwrap_or_default();
                match method {
                    "ShowItems" => show_items(&app, &uris),
                    "ShowFolders" => show_folders(&app, &uris),
                    "ShowItemProperties" => show_properties(&app, &uris),
                    _ => {}
                }
                invocation.return_value(None);
            },
        )
        .build();
    if let Err(e) = result {
        eprintln!("valkyrie: FileManager1 registration failed: {e}");
    }
}

/// ShowItems — the Firefox "Show in Folder" path: open the parent
/// directory and select each item inside it. Distinct parents get
/// their own window.
fn show_items(app: &gtk4::Application, uris: &[String]) {
    // Group items by parent directory — one window per parent, all
    // of its items selected.
    let mut groups: Vec<(gio::File, Vec<gio::File>)> = Vec::new();
    for uri in uris {
        let file = gio::File::for_uri(uri);
        let Some(parent) = file.parent() else {
            continue;
        };
        match groups.iter_mut().find(|(p, _)| p.equal(&parent)) {
            Some((_, files)) => files.push(file),
            None => groups.push((parent, vec![file])),
        }
    }
    for (parent, files) in groups {
        let window = crate::window::ValkyrieWindow::new(app, Some(&parent));
        window.widget.present();
        if let Some(tab) = window.active_tab() {
            tab.active_view().select_files(&files);
        }
    }
}

/// ShowFolders — open each folder URI. The first one gets its own
/// window; further URIs become tabs in it.
fn show_folders(app: &gtk4::Application, uris: &[String]) {
    let mut window: Option<std::rc::Rc<crate::window::ValkyrieWindow>> = None;
    for uri in uris {
        let dir = gio::File::for_uri(uri);
        match &window {
            None => {
                let w = crate::window::ValkyrieWindow::new(app, Some(&dir));
                w.widget.present();
                window = Some(w);
            }
            Some(w) => w.open_tab(Some(&dir)),
        }
    }
}

/// ShowItemProperties — the properties dialog per URI, parented to
/// a live window (a fresh one when nothing is open yet).
fn show_properties(app: &gtk4::Application, uris: &[String]) {
    let parent_widget: Option<gtk4::Widget> = app
        .windows()
        .into_iter()
        .next()
        .map(|w| w.upcast::<gtk4::Widget>());
    let parent_widget = match parent_widget {
        Some(w) => w,
        // No window open: create one showing the first item's parent
        // so the dialogs are anchored somewhere sensible.
        None => {
            let location = uris
                .first()
                .map(|u| gio::File::for_uri(u))
                .and_then(|f| f.parent());
            let w = crate::window::ValkyrieWindow::new(app, location.as_ref());
            w.widget.present();
            w.widget.clone().upcast()
        }
    };
    for uri in uris {
        crate::properties::show(&parent_widget, &gio::File::for_uri(uri));
    }
}
