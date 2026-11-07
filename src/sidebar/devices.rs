// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// Device rows of the sidebar: mounted volumes with eject buttons and
// right-click mount menus, unmounted mountable volumes. Rebuilt on
// VolumeMonitor signals and after every mount operation.

use std::cell::Cell;
use std::rc::Rc;

use gtk4::gio;
use gtk4::prelude::*;

use super::{Sidebar, SidebarTarget};
use crate::i18n::tr;

impl Sidebar {
    /// Rebuild only the device rows (called on volume/mount changes).
    pub(super) fn rebuild_devices(&self) {
        // Rows tagged "device" are removed and re-added below the header.
        let monitor = gio::VolumeMonitor::get();

        let mut remove: Vec<gtk4::ListBoxRow> = Vec::new();
        self.list.observe_children();
        let mut child = self.list.first_child();
        while let Some(row_widget) = child {
            let next = row_widget.next_sibling();
            if row_widget.widget_name() == "valkyrie-device-row" {
                if let Ok(row) = row_widget.downcast::<gtk4::ListBoxRow>() {
                    remove.push(row);
                }
            }
            child = next;
        }
        for row in &remove {
            self.list.remove(row);
        }
        self.targets
            .borrow_mut()
            .retain(|(r, _)| !remove.contains(r));

        // Mounted volumes and mountable volumes both get rows.
        for mount in monitor.mounts() {
            if mount.is_shadowed() {
                continue;
            }
            let name = mount.name().to_string();
            let icon = mount.icon();
            self.add_device_row(SidebarTarget::Mount(mount), &icon, &name);
        }
        for volume in monitor.volumes() {
            match volume.get_mount() {
                // A visible mount already produced a row above.
                Some(mount) if !mount.is_shadowed() => continue,
                // Mounted but hidden from the mounts list (e.g. the
                // live-boot medium at /run/live/medium — GIO marks
                // mounts outside user-visible paths as shadowed).
                // Show it anyway so the device does not vanish.
                Some(mount) => {
                    let name = volume.name().to_string();
                    let icon = volume.icon();
                    self.add_device_row(SidebarTarget::Mount(mount), &icon, &name);
                }
                // Unmounted: list it when it can be mounted on click.
                None if volume.can_mount() => {
                    let name = volume.name().to_string();
                    let icon = volume.icon();
                    self.add_device_row(SidebarTarget::Volume(volume.clone()), &icon, &name);
                }
                None => continue,
            }
        }
    }

    /// One selectable device row — inserted before the Network header
    /// so rebuilt device lists stay inside their own section. Mounted
    /// volumes get an eject/unmount button; all device rows offer a
    /// right-click menu with the matching mount actions.
    pub(super) fn add_device_row(&self, target: SidebarTarget, icon: &gio::Icon, label: &str) {
        let row = self.make_row(icon, label);
        row.set_widget_name("valkyrie-device-row");
        if let SidebarTarget::Mount(mount) = &target {
            if mount.can_eject() || mount.can_unmount() {
                self.append_eject_button(&row, mount);
            }
        }
        self.attach_device_menu(&row, &target);
        if let Some(header) = self.network_header.borrow().as_ref() {
            self.list.insert(&row, header.index());
        } else {
            self.list.append(&row);
        }
        self.targets.borrow_mut().push((row, target));
    }

    /// The small eject button on the right edge of a mounted device
    /// row. Eject powers removable media down; unmount is the
    /// fallback for volumes that cannot be ejected.
    fn append_eject_button(&self, row: &gtk4::ListBoxRow, mount: &gio::Mount) {
        let Some(box_) = row.child().and_downcast::<gtk4::Box>() else {
            return;
        };
        let eject = mount.can_eject();
        let button = gtk4::Button::builder()
            .icon_name("media-eject-symbolic")
            .has_frame(false)
            .valign(gtk4::Align::Center)
            .tooltip_text(if eject { tr("Eject") } else { tr("Unmount") })
            .build();
        let mount = mount.clone();
        let widget = box_.clone().upcast::<gtk4::Widget>();
        let this = self.clone();
        button.connect_clicked(move |b| {
            // Double clicks must not run two operations at once; the
            // rebuild removes the row on success anyway.
            b.set_sensitive(false);
            let this = this.clone();
            crate::mount_ops::eject_or_unmount(mount.clone(), &widget, move || {
                this.rebuild_devices();
            });
        });
        box_.append(&button);
    }

    /// Right-click popover on a device row: Open/Unmount/Eject for
    /// mounted volumes, Mount for unmounted ones. The popover is
    /// built once per row — device rows are few and live until the
    /// VolumeMonitor rebuild replaces them. Its parent is the stable
    /// ListBox (not the row): parenting to a recycled row child made
    /// the popup surface lookup fail.
    fn attach_device_menu(&self, row: &gtk4::ListBoxRow, target: &SidebarTarget) {
        let Some(box_) = row.child().and_downcast::<gtk4::Box>() else {
            return;
        };
        // autohide OFF (same as the file context menu): the menu is
        // opened from a released handler, and GTK would treat the
        // still-finishing sequence as an outside press and hide the
        // popover instantly. Escape and outside presses dismiss it
        // manually instead.
        let popover = gtk4::Popover::builder()
            .has_arrow(true)
            .autohide(false)
            .position(gtk4::PositionType::Right)
            .build();
        popover.set_parent(&self.list);

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

        // Any press landing on the window (outside the popover's own
        // surface) dismisses the menu. The sidebar is built before it
        // enters a window, so the controller is attached lazily when
        // the popover first realizes — root() is valid by then.
        let dismiss = gtk4::GestureClick::new();
        dismiss.set_propagation_phase(gtk4::PropagationPhase::Capture);
        dismiss.set_button(0);
        let p = popover.clone();
        dismiss.connect_pressed(move |_, _, _, _| {
            if p.is_visible() {
                p.popdown();
            }
        });
        let dismiss_wired = Rc::new(Cell::new(false));
        let p = popover.clone();
        popover.connect_realize(move |pop| {
            if dismiss_wired.replace(true) {
                return;
            }
            if let Some(root) = pop.root() {
                root.add_controller(dismiss.clone());
            } else {
                dismiss_wired.set(false);
            }
            let _ = &p;
        });
        let page = crate::menu_widgets::page();
        let widget = box_.clone().upcast::<gtk4::Widget>();
        match target {
            SidebarTarget::Mount(mount) => {
                let callbacks = self.callbacks.clone();
                let root = mount.root();
                crate::menu_widgets::item(&page, &popover, &tr("Open"), move || {
                    for cb in callbacks.borrow().iter() {
                        cb(&root);
                    }
                });
                // Open in new tab / new window — same as place rows.
                let root2 = mount.root();
                let cbs = self.open_in_tab.clone();
                crate::menu_widgets::item(&page, &popover, &tr("Open in New Tab"), move || {
                    for cb in cbs.borrow().iter() {
                        cb(&root2);
                    }
                });
                let root3 = mount.root();
                let cbs = self.open_in_window.clone();
                crate::menu_widgets::item(&page, &popover, &tr("Open in New Window"), move || {
                    for cb in cbs.borrow().iter() {
                        cb(&root3);
                    }
                });
                if mount.can_unmount() {
                    let (m, w, this) = (mount.clone(), widget.clone(), self.clone());
                    crate::menu_widgets::item(&page, &popover, &tr("Unmount"), move || {
                        let this = this.clone();
                        crate::mount_ops::unmount(m.clone(), &w, move || {
                            this.rebuild_devices();
                        });
                    });
                }
                if mount.can_eject() {
                    let (m, w, this) = (mount.clone(), widget.clone(), self.clone());
                    crate::menu_widgets::item(&page, &popover, &tr("Eject"), move || {
                        let this = this.clone();
                        crate::mount_ops::eject_or_unmount(m.clone(), &w, move || {
                            this.rebuild_devices();
                        });
                    });
                }
            }
            SidebarTarget::Volume(volume) => {
                let callbacks = self.callbacks.clone();
                let (v, w, this) = (volume.clone(), widget.clone(), self.clone());
                crate::menu_widgets::item(&page, &popover, &tr("Mount"), move || {
                    let callbacks = callbacks.clone();
                    let this = this.clone();
                    crate::mount_ops::mount_and_open(v.clone(), &w, move |root| {
                        // Rebuild so the volume row becomes a mounted
                        // device row (with its eject button).
                        this.rebuild_devices();
                        for cb in callbacks.borrow().iter() {
                            cb(root);
                        }
                    });
                });
            }
            SidebarTarget::Location(_) | SidebarTarget::Nextcloud => return,
        }
        popover.set_child(Some(&page));
        // Detach the popover before the row is torn down by a device
        // rebuild — GTK warns about finalizing widgets with children.
        let pop = popover.clone();
        row.connect_unrealize(move |_| pop.unparent());

        // Open on release, not press: popping up mid-press lets
        // autohide interpret the still-running sequence as an
        // outside press and closes the menu instantly.
        let gesture = gtk4::GestureClick::new();
        gesture.set_button(3);
        let pop = popover.clone();
        let list = self.list.clone().upcast::<gtk4::Widget>();
        let row_widget = row.clone().upcast::<gtk4::Widget>();
        gesture.connect_released(move |_, _, x, y| {
            // The pointing rect must be expressed in the popover
            // parent's (the list's) coordinate space.
            let p = row_widget
                .compute_point(&list, &gtk4::graphene::Point::new(x as f32, y as f32))
                .unwrap_or_else(|| gtk4::graphene::Point::new(x as f32, y as f32));
            pop.set_pointing_to(Some(&gtk4::gdk::Rectangle::new(
                p.x() as i32,
                p.y() as i32,
                1,
                1,
            )));
            pop.popup();
        });
        row.add_controller(gesture);
    }
}
