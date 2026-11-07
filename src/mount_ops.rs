// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// Async volume/mount operations for the sidebar device rows:
// mount a volume, unmount or eject a mounted one. All three run
// through a GtkMountOperation so GVFS may ask for credentials, and
// failures surface as an AlertDialog instead of a log line.

use gtk4::prelude::*;
use gtk4::{gio, glib};

use crate::i18n::tr;

/// Transient parent for mount dialogs, derived from the sidebar
/// widget the user interacted with.
fn window_of(widget: &gtk4::Widget) -> Option<gtk4::Window> {
    widget
        .root()
        .and_then(|r| r.downcast::<gtk4::Window>().ok())
}

/// Find the live Mount for `volume`. First via the volume object
/// itself, then by matching the unix-device identifier against the
/// monitor's mount list — sidebar rows can hold a stale GVolume on
/// which get_mount() never reports the mount the automounter just
/// created (the monitor swaps volume objects on state changes).
fn find_mount(volume: &gio::Volume) -> Option<gio::Mount> {
    if let Some(mount) = volume.get_mount() {
        return Some(mount);
    }
    let device = volume.identifier(gio::VOLUME_IDENTIFIER_KIND_UNIX_DEVICE)?;
    gio::VolumeMonitor::get().mounts().iter().find_map(|mount| {
        mount
            .volume()
            .filter(|v| {
                v.identifier(gio::VOLUME_IDENTIFIER_KIND_UNIX_DEVICE)
                    .as_deref()
                    == Some(device.as_str())
            })
            .map(|_| mount.clone())
    })
}

/// Mount `volume` (asks for credentials when needed) and hand the
/// resulting root directory to `on_mounted` so the caller can
/// navigate into it. The automounter races this call on every
/// startup: a volume that is already mounted — or still being
/// mounted — must open the existing mount instead of erroring with
/// "location is already mounted".
pub fn mount_and_open(
    volume: gio::Volume,
    widget: &gtk4::Widget,
    on_mounted: impl Fn(&gio::File) + 'static,
) {
    // Already mounted (automounter won the race or the row went
    // stale): navigate straight into the existing mount.
    if let Some(mount) = find_mount(&volume) {
        on_mounted(&mount.root());
        return;
    }
    let window = window_of(widget);
    let op = gtk4::MountOperation::new(window.as_ref());
    let widget = widget.clone();
    glib::MainContext::default().spawn_local(async move {
        let failure = volume
            .mount_future(gio::MountMountFlags::NONE, Some(&op))
            .await
            .err();
        let mut mount = find_mount(&volume);
        if mount.is_none() {
            // The automounter's request can still be finishing while
            // ours already failed — give the volume monitor a short
            // grace period before reporting an error.
            for _ in 0..12 {
                glib::timeout_future(std::time::Duration::from_millis(250)).await;
                mount = find_mount(&volume);
                if mount.is_some() {
                    break;
                }
            }
        }
        match mount {
            Some(m) => on_mounted(&m.root()),
            None => {
                if let Some(e) = failure {
                    fail(&widget, &tr("Could not mount the volume"), &e);
                }
            }
        }
    });
}

/// Unmount without ejecting (media stays present, e.g. a partition
/// on a disk that keeps spinning). `on_done` runs on success — the
/// caller rebuilds its device list, because the VolumeMonitor does
/// not reliably emit mount-removed for daemon mounts.
pub fn unmount(mount: gio::Mount, widget: &gtk4::Widget, on_done: impl Fn() + 'static) {
    let window = window_of(widget);
    let op = gtk4::MountOperation::new(window.as_ref());
    let widget = widget.clone();
    glib::MainContext::default().spawn_local(async move {
        match mount
            .unmount_with_operation_future(gio::MountUnmountFlags::NONE, Some(&op))
            .await
        {
            Ok(()) => on_done(),
            Err(e) => fail(&widget, &tr("Could not unmount the volume"), &e),
        }
    });
}

/// Eject preferred, unmount as fallback — eject powers down removable
/// media (USB sticks, optical drives) so they can be pulled safely.
pub fn eject_or_unmount(mount: gio::Mount, widget: &gtk4::Widget, on_done: impl Fn() + 'static) {
    if mount.can_eject() {
        let window = window_of(widget);
        let op = gtk4::MountOperation::new(window.as_ref());
        let widget = widget.clone();
        glib::MainContext::default().spawn_local(async move {
            // gio's eject_with_operation reuses the unmount flag enum.
            match mount
                .eject_with_operation_future(gio::MountUnmountFlags::NONE, Some(&op))
                .await
            {
                Ok(()) => on_done(),
                Err(e) => fail(&widget, &tr("Could not eject the volume"), &e),
            }
        });
    } else {
        unmount(mount, widget, on_done);
    }
}

/// User-visible error report for failed mount operations.
fn fail(widget: &gtk4::Widget, message: &str, error: &glib::Error) {
    let dialog = gtk4::AlertDialog::builder()
        .message(message)
        .detail(error.message())
        .modal(true)
        .build();
    dialog.show(window_of(widget).as_ref());
}
