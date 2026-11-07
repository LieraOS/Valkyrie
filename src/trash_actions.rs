// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// Trash-specific actions shown while browsing trash:///: restore an
// item to its original location (trash::orig-path), permanently
// delete a single item, and empty the whole trash. Destructive
// actions ask for confirmation first; mass deletes run through the
// progress window.

use gtk4::prelude::*;
use gtk4::{gio, glib};

use crate::i18n::tr;
use crate::operations;
use crate::progress;

/// Wipe passes for permanent deletes, from the user's Delete
/// settings. 0 = secure delete disabled.
fn wipe_passes() -> u32 {
    if crate::settings::secure_delete() {
        crate::settings::secure_delete_passes() as u32
    } else {
        0
    }
}

/// Whether `location` is inside the trash (row menus and the
/// background menu use it to swap clipboard actions for restore).
pub fn is_trash_location(location: &gio::File) -> bool {
    location.uri().starts_with("trash:")
}

/// Restore `file` to the location stored in trash::orig-path; name
/// collisions resolve through the shared unique_destination helper.
pub fn restore(parent: &gtk4::Widget, file: &gio::File) {
    let result = file
        .query_info(
            "trash::orig-path",
            gio::FileQueryInfoFlags::NONE,
            gio::Cancellable::NONE,
        )
        .and_then(|info| {
            info.attribute_byte_string("trash::orig-path")
                .ok_or_else(|| glib_error("no orig-path"))
        });
    let Ok(orig) = result else {
        operations::error_dialog(parent, &tr("Could not restore the item"));
        return;
    };
    let orig = gio::File::for_path(orig.as_str());
    let Some(dest_dir) = orig.parent() else {
        return;
    };
    let name = orig
        .basename()
        .map(|b| b.to_string_lossy().into_owned())
        .unwrap_or_default();
    let target = operations::unique_destination(&dest_dir, &name);
    if let Err(e) = file.move_(
        &target,
        gio::FileCopyFlags::NONE,
        gio::Cancellable::NONE,
        None::<&mut dyn FnMut(i64, i64)>,
    ) {
        operations::error_dialog(
            parent,
            &format!("{}: {e}", tr("Could not restore the item")),
        );
    }
}

/// Permanently delete the given trashed items after confirmation.
pub fn delete_forever(parent: &gtk4::Widget, files: &[gio::File]) {
    let files = files.to_vec();
    confirm(
        parent,
        &tr("Delete permanently?"),
        &tr("This item will be gone for good."),
        move |parent| {
            progress::delete(parent, files.clone(), wipe_passes());
        },
    );
}

/// Empty the whole trash after confirmation; the batch runs through
/// the progress window so it can be cancelled.
pub fn empty(parent: &gtk4::Widget) {
    confirm(
        parent,
        &tr("Empty the trash?"),
        &tr("All items in the trash will be deleted permanently."),
        move |parent| {
            let trash = gio::File::for_uri("trash:///");
            let mut items = Vec::new();
            if let Ok(en) = trash.enumerate_children(
                "standard::name",
                gio::FileQueryInfoFlags::NONE,
                gio::Cancellable::NONE,
            ) {
                while let Ok(Some(info)) = en.next_file(gio::Cancellable::NONE) {
                    items.push(en.child(&info));
                }
            }
            progress::delete(parent, items, wipe_passes());
        },
    );
}

/// Modal yes/no confirmation via GtkAlertDialog; `run` receives the
/// parent widget so follow-up work can attach further dialogs.
/// pub(crate): operations::delete_permanently reuses the same dialog.
pub(crate) fn confirm(
    parent: &gtk4::Widget,
    heading: &str,
    body: &str,
    run: impl Fn(&gtk4::Widget) + 'static,
) {
    let dialog = gtk4::AlertDialog::builder()
        .message(heading)
        .detail(body)
        .buttons([tr("Cancel"), tr("Delete")])
        .cancel_button(0)
        .default_button(1)
        .build();
    let window = parent
        .ancestor(gtk4::Window::static_type())
        .and_then(|w| w.downcast::<gtk4::Window>().ok());
    let parent = parent.clone();
    dialog.choose(window.as_ref(), gio::Cancellable::NONE, move |result| {
        if result == Ok(1) {
            run(&parent);
        }
    });
}

/// gio::Error for an attribute miss — query_info succeeded but the
/// backend did not fill trash::orig-path.
fn glib_error(msg: &str) -> glib::Error {
    glib::Error::new(gio::IOErrorEnum::NotFound, msg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trash_location_detection() {
        assert!(is_trash_location(&gio::File::for_uri("trash:///")));
        assert!(is_trash_location(&gio::File::for_uri(
            "trash:///some-file.txt"
        )));
        assert!(!is_trash_location(&gio::File::for_path("/tmp")));
        assert!(!is_trash_location(&gio::File::for_uri("recent:///")));
        assert!(!is_trash_location(&gio::File::for_uri("sftp://host/x")));
    }
}
