// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// Places sidebar: static places (home, desktop, recent, root, trash),
// dynamic device section from GVolumeMonitor, and a network section.

use std::cell::RefCell;
use std::rc::Rc;

type FileCallbacks = Rc<RefCell<Vec<Box<dyn Fn(&gio::File)>>>>;

use gtk4::prelude::*;
use gtk4::{gio, glib};

use crate::i18n::tr;

mod devices;

/// What a sidebar row activates: a location to navigate to, a
/// mounted volume (with unmount/eject controls on the row), or an
/// unmounted volume that gets mounted first and then navigated into.
#[derive(Clone)]
enum SidebarTarget {
    Location(gio::File),
    Mount(gio::Mount),
    Volume(gio::Volume),
    /// The Nextcloud row: mount the cloud on demand, then navigate.
    Nextcloud,
}

#[derive(Clone)]
pub struct Sidebar {
    pub widget: gtk4::ScrolledWindow,
    list: gtk4::ListBox,
    /// Row widget → activation target (location or mountable volume).
    targets: Rc<RefCell<Vec<(gtk4::ListBoxRow, SidebarTarget)>>>,
    /// The Network section header — device rows insert before it so
    /// mount/unmount rebuilds keep the visual order.
    network_header: Rc<RefCell<Option<gtk4::ListBoxRow>>>,
    /// The Nextcloud row under Network — present only while a server
    /// + user are configured.
    nextcloud_row: Rc<RefCell<Option<gtk4::ListBoxRow>>>,
    /// "Browse Network" row — the Nextcloud row is inserted before it.
    browse_row: Rc<RefCell<Option<gtk4::ListBoxRow>>>,
    /// Kept alive for the changed-signal that refreshes the row.
    nc_settings: Rc<RefCell<Option<gio::Settings>>>,
    callbacks: FileCallbacks,
    /// Bookmark header + rows for rebuild-on-change.
    bookmark_rows: Rc<RefCell<Vec<gtk4::ListBoxRow>>>,
    /// The "Devices" header — bookmarks insert before it.
    devices_header: Rc<RefCell<Option<gtk4::ListBoxRow>>>,
    /// Kept alive for the bookmarks changed-signal.
    bm_settings: Rc<RefCell<Option<gio::Settings>>>,
    /// Right-click menu targets: a place can be opened in a new tab
    /// or a new window, resolved at window level like the file-view
    /// context menu does.
    open_in_tab: FileCallbacks,
    open_in_window: FileCallbacks,
}

impl Sidebar {
    /// Build the sidebar and populate all three sections.
    pub fn new() -> Self {
        let list = gtk4::ListBox::new();
        list.add_css_class("navigation-sidebar");
        list.set_selection_mode(gtk4::SelectionMode::Single);

        let this = Self {
            widget: gtk4::ScrolledWindow::builder()
                .child(&list)
                .vexpand(true)
                .build(),
            list,
            targets: Rc::new(RefCell::new(Vec::new())),
            network_header: Rc::new(RefCell::new(None)),
            nextcloud_row: Rc::new(RefCell::new(None)),
            browse_row: Rc::new(RefCell::new(None)),
            nc_settings: Rc::new(RefCell::new(None)),
            bookmark_rows: Rc::new(RefCell::new(Vec::new())),
            devices_header: Rc::new(RefCell::new(None)),
            bm_settings: Rc::new(RefCell::new(None)),
            callbacks: Rc::new(RefCell::new(Vec::new())),
            open_in_tab: Rc::new(RefCell::new(Vec::new())),
            open_in_window: Rc::new(RefCell::new(Vec::new())),
        };

        this.populate();

        // Show/hide the Nextcloud row live when the credentials in the
        // preferences change.
        {
            let refresh_owner = this.clone();
            let settings_slot = this.nc_settings.clone();
            *settings_slot.borrow_mut() = crate::settings::connect_nextcloud_changed(move || {
                refresh_owner.refresh_nextcloud();
            });
        }

        // Bookmarks section refreshes live when the list changes.
        {
            let owner = this.clone();
            let slot = this.bm_settings.clone();
            *slot.borrow_mut() = crate::settings::connect_bookmarks_changed(move || {
                owner.rebuild_bookmarks();
            });
        }

        // Click on a place row navigates the active tab; a volume row
        // mounts the volume first and then navigates into its root.
        let targets = this.targets.clone();
        let callbacks = this.callbacks.clone();
        let this_row_owner = this.clone();
        this.list.connect_row_activated(move |_, row| {
            let target = targets
                .borrow()
                .iter()
                .find(|(r, _)| r == row)
                .map(|(_, t)| t.clone());
            match target {
                Some(SidebarTarget::Location(file)) => {
                    for cb in callbacks.borrow().iter() {
                        cb(&file);
                    }
                }
                Some(SidebarTarget::Mount(mount)) => {
                    let root = mount.root();
                    for cb in callbacks.borrow().iter() {
                        cb(&root);
                    }
                }
                Some(SidebarTarget::Nextcloud) => {
                    let callbacks = callbacks.clone();
                    let row = row.clone();
                    crate::nextcloud::mount_and_use(row.upcast_ref(), move |root| {
                        let root = root.clone();
                        for cb in callbacks.borrow().iter() {
                            cb(&root);
                        }
                    });
                }
                Some(SidebarTarget::Volume(volume)) => {
                    // Mount asynchronously; on success open the root.
                    let callbacks = callbacks.clone();
                    let this = this_row_owner.clone();
                    crate::mount_ops::mount_and_open(volume, row.upcast_ref(), move |root| {
                        this.rebuild_devices();
                        for cb in callbacks.borrow().iter() {
                            cb(root);
                        }
                    });
                }
                None => {}
            }
        });

        // Repopulate the device rows when mounts change.
        let this2 = this.clone();
        let monitor = gio::VolumeMonitor::get();
        monitor.connect_mount_added(move |_, _| this2.rebuild_devices());
        let this3 = this.clone();
        monitor.connect_mount_removed(move |_, _| this3.rebuild_devices());
        let this4 = this.clone();
        monitor.connect_volume_added(move |_, _| this4.rebuild_devices());
        let this5 = this.clone();
        monitor.connect_volume_removed(move |_, _| this5.rebuild_devices());

        this
    }

    /// Register the navigation callback (sidebar → active tab).
    pub fn connect_location_selected<F: Fn(&gio::File) + 'static>(&self, f: F) {
        self.callbacks.borrow_mut().push(Box::new(f));
    }

    /// Register "Open in New Tab" for the place context menu.
    pub fn connect_open_in_tab<F: Fn(&gio::File) + 'static>(&self, f: F) {
        self.open_in_tab.borrow_mut().push(Box::new(f));
    }

    /// Register "Open in New Window" for the place context menu.
    pub fn connect_open_in_window<F: Fn(&gio::File) + 'static>(&self, f: F) {
        self.open_in_window.borrow_mut().push(Box::new(f));
    }

    /// Fill all three sections once.
    fn populate(&self) {
        self.add_header(&tr("Places"));
        let home = gio::File::for_path(glib::home_dir());
        self.add_place(&home, "user-home-symbolic", &tr("Home"));

        if let Some(desktop) = glib::user_special_dir(glib::UserDirectory::Desktop) {
            self.add_place(
                &gio::File::for_path(desktop),
                "user-desktop-symbolic",
                &tr("Desktop"),
            );
        }

        self.add_place(
            &gio::File::for_uri("recent:///"),
            "document-open-recent-symbolic",
            &tr("Recent"),
        );
        self.add_place(
            &gio::File::for_path("/"),
            "drive-harddisk-symbolic",
            &tr("File System"),
        );
        self.add_place(
            &gio::File::for_uri("trash:///"),
            "user-trash-symbolic",
            &tr("Trash"),
        );

        // User bookmarks live between static places and devices and
        // refresh live when the GSettings list changes.
        self.add_header(&tr("Bookmarks"));
        self.rebuild_bookmarks();

        *self.devices_header.borrow_mut() = Some(self.add_header(&tr("Devices")));
        self.rebuild_devices();

        *self.network_header.borrow_mut() = Some(self.add_header(&tr("Network")));
        let browse = self.make_row(
            &gio::ThemedIcon::new("network-workgroup-symbolic").into(),
            &tr("Browse Network"),
        );
        self.list.append(&browse);
        self.targets.borrow_mut().push((
            browse.clone(),
            SidebarTarget::Location(gio::File::for_uri("network:///")),
        ));
        *self.browse_row.borrow_mut() = Some(browse);
        self.refresh_nextcloud();
    }

    /// Insert the cloud row before "Browse Network" while
    /// credentials exist — remove it when they are cleared. Clicking
    /// mounts the cloud (keyring-backed auth) and navigates into it.
    /// The row is labelled with the configured host, so any WebDAV
    /// provider is recognizable — no vendor branding.
    fn refresh_nextcloud(&self) {
        // Remove the previous row first.
        if let Some(old) = self.nextcloud_row.borrow_mut().take() {
            self.targets.borrow_mut().retain(|(r, _)| r != &old);
            self.list.remove(&old);
        }
        if !crate::nextcloud::configured() {
            return;
        }
        let host = crate::nextcloud::host_key(&crate::settings::nextcloud_server());
        let label = if host.is_empty() { tr("Cloud") } else { host };
        let row = self.make_row(
            &gio::ThemedIcon::new("network-server-symbolic").into(),
            &label,
        );
        let index = self
            .browse_row
            .borrow()
            .as_ref()
            .map(|b| b.index())
            .unwrap_or(-1);
        self.list.insert(&row, index);
        self.targets
            .borrow_mut()
            .push((row.clone(), SidebarTarget::Nextcloud));
        *self.nextcloud_row.borrow_mut() = Some(row);
    }

    /// Rebuild the Bookmarks section from the GSettings list. Rows
    /// are inserted directly before the Devices header so the
    /// section order stays stable on every refresh.
    fn rebuild_bookmarks(&self) {
        for row in self.bookmark_rows.borrow_mut().drain(..) {
            self.targets.borrow_mut().retain(|(r, _)| r != &row);
            self.list.remove(&row);
        }
        let Some(dev) = self.devices_header.borrow().clone() else {
            // First call from populate() — devices header not stored
            // yet; append normally in order.
            for uri in crate::settings::bookmarks() {
                let f = gio::File::for_uri(&uri);
                let name = f
                    .basename()
                    .map(|b| b.to_string_lossy().into_owned())
                    .unwrap_or(uri);
                self.add_place(&f, "user-bookmarks-symbolic", &name);
            }
            return;
        };
        for (pos, uri) in (dev.index()..).zip(crate::settings::bookmarks()) {
            let f = gio::File::for_uri(&uri);
            let name = f
                .basename()
                .map(|b| b.to_string_lossy().into_owned())
                .unwrap_or_else(|| uri.clone());
            let row = self.make_row(
                &gio::ThemedIcon::new("user-bookmarks-symbolic").into(),
                &name,
            );
            self.attach_place_menu(&row, &f);
            self.list.insert(&row, pos);
            self.targets
                .borrow_mut()
                .push((row.clone(), SidebarTarget::Location(f)));
            self.bookmark_rows.borrow_mut().push(row);
        }
    }

    /// One selectable place row. Right-click opens a small menu with
    /// new-tab/new-window/terminal actions — the same affordances the
    /// file view's context menu offers for directories.
    fn add_place(&self, file: &gio::File, icon: &str, label: &str) {
        let row = self.make_row(&gio::ThemedIcon::new(icon).into(), label);
        self.attach_place_menu(&row, file);
        self.list.append(&row);
        self.targets
            .borrow_mut()
            .push((row, SidebarTarget::Location(file.clone())));
    }

    /// Right-click gesture on a place row → popup menu.
    fn attach_place_menu(&self, row: &gtk4::ListBoxRow, file: &gio::File) {
        let gesture = gtk4::GestureClick::new();
        gesture.set_button(3);
        let this = self.clone();
        let row2 = row.clone();
        let file = file.clone();
        // Claim on press, pop up on release (X11 menu convention):
        // opening mid-press breaks GTK's active-state accounting
        // when the pointer moves before release — GTK bug #4563.
        gesture.connect_pressed(move |g, _, _, _| {
            g.set_state(gtk4::EventSequenceState::Claimed);
        });
        gesture.connect_released(move |_, _, x, y| {
            this.show_place_menu(&row2, &file, x, y);
        });
        row.add_controller(gesture);
    }

    /// The place context menu itself — a plain hand-built popover
    /// (same idiom as the file-view menus, no scrolling menus).
    fn show_place_menu(&self, row: &gtk4::ListBoxRow, file: &gio::File, x: f64, y: f64) {
        let popover = gtk4::Popover::builder().has_arrow(false).build();
        popover.set_parent(row);
        let page = crate::menu_widgets::page();

        let f = file.clone();
        let cbs = self.open_in_tab.clone();
        crate::menu_widgets::item(&page, &popover, &tr("Open in New Tab"), move || {
            for cb in cbs.borrow().iter() {
                cb(&f);
            }
        });

        let f = file.clone();
        let cbs = self.open_in_window.clone();
        crate::menu_widgets::item(&page, &popover, &tr("Open in New Window"), move || {
            for cb in cbs.borrow().iter() {
                cb(&f);
            }
        });

        // A terminal only makes sense on real filesystem paths —
        // recent:/// and trash:/// have none.
        if file.path().is_some() {
            let f = file.clone();
            crate::menu_widgets::item(&page, &popover, &tr("Open in Terminal"), move || {
                crate::terminal::spawn(&f);
            });
        }

        // Trash gets an "Empty Trash" entry alongside Open.
        if file.uri() == "trash:///" {
            let w = row.clone().upcast::<gtk4::Widget>();
            crate::menu_widgets::item(&page, &popover, &tr("Empty Trash…"), move || {
                crate::trash_actions::empty(&w);
            });
        }

        popover.set_child(Some(&page));
        popover.set_pointing_to(Some(&gtk4::gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
        popover.popup();
    }

    /// Non-selectable section header.
    fn add_header(&self, title: &str) -> gtk4::ListBoxRow {
        let label = gtk4::Label::builder()
            .label(title)
            .xalign(0.0)
            .margin_top(12)
            .margin_bottom(4)
            .margin_start(8)
            .build();
        label.add_css_class("dim-label");
        let row = gtk4::ListBoxRow::builder()
            .child(&label)
            .selectable(false)
            .activatable(false)
            .build();
        self.list.append(&row);
        row
    }

    /// Row widget with icon + label.
    fn make_row(&self, icon: &gio::Icon, label: &str) -> gtk4::ListBoxRow {
        let box_ = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        box_.set_margin_top(4);
        box_.set_margin_bottom(4);
        box_.set_margin_start(8);
        box_.set_margin_end(8);
        box_.append(&gtk4::Image::from_gicon(icon));
        box_.append(
            &gtk4::Label::builder()
                .label(label)
                .xalign(0.0)
                .hexpand(true)
                .ellipsize(gtk4::pango::EllipsizeMode::End)
                .build(),
        );
        // activatable is required for row-activated; its GTK4 default
        // is false, so rows without it silently ignore clicks.
        gtk4::ListBoxRow::builder()
            .child(&box_)
            .activatable(true)
            .build()
    }
}
