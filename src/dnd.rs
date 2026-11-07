// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// Drag & drop plumbing. Rows offer a gdk::FileList payload (understood
// by other applications, other Valkyrie windows and our own targets);
// folder rows, the view background and notebook tab labels accept it.
// A plain drop moves, Ctrl held at drop time copies — the classic
// file-manager convention. Transfers run through the progress dialog,
// so large drops stay cancellable.

use gtk4::prelude::*;
use gtk4::{gdk, gio, glib};

use crate::file_view::FileView;

/// Drag source on a file row: exposes the **full selection** as a
/// FileList so dragging one selected file moves all selected files —
/// the standard file-manager convention. When the dragged row is not
/// part of the selection, only that single file is dragged. The file
/// list is resolved from the selection model at drag time — rows are
/// recycled by the list views, so capturing the bound object here
/// would drag whatever file the row showed at setup time.
///
/// A drag icon is set in `drag_begin` so the user sees the file's
/// icon "in hand" while dragging — matching classic file managers' behavior.
/// GTK4 DragSource shows nothing by default, which made it look as
/// if the file was not actually grabbed.
pub fn attach_row_source(row: &gtk4::Box, item: &gtk4::ListItem, view: &FileView) {
    let source = gtk4::DragSource::new();
    source.set_actions(gdk::DragAction::MOVE | gdk::DragAction::COPY);
    // Claim the event early (capture phase) so the GridView's
    // rubberband gesture does not steal the drag when the user
    // starts dragging on a file icon. Without this, clicking a file
    // and moving the mouse often starts the rubberband instead of
    // the drag — the row's DragSource lost the race to the grid's
    // built-in rubberband gesture.
    source.set_propagation_phase(gtk4::PropagationPhase::Capture);
    let item = item.clone();
    let item_for_icon = item.clone();
    let view = view.clone();
    let view_for_size = view.clone();
    source.connect_prepare(move |_, _, _| {
        let obj = item
            .item()?
            .downcast::<crate::file_object::FileObject>()
            .ok()?;
        // Collect all selected files — but only if the dragged row
        // is part of the selection. Dragging an unselected row drags
        // only that file (matching classic file managers' behavior).
        let files: Vec<gio::File> = if is_row_selected(&view, &obj) {
            selected_files(&view)
        } else {
            vec![obj.file()]
        };
        if files.is_empty() {
            return None;
        }
        let list = gdk::FileList::from_array(&files);
        Some(gdk::ContentProvider::for_value(&list.to_value()))
    });

    // Drag icon: set on the DragSource (not the Drag) so the user
    // sees "the file in hand" while dragging — matching classic file managers
    // behavior. We build a Snapshot with the file icon on top and
    // the filename below, then convert to a Paintable. This gives the
    // user a "card" showing both icon and name, like classic file managers does.
    //
    // The Drag object under Wayland (GdkWaylandDrag) has no "icon"
    // property — setting it panics. DragSource::set_icon is the
    // documented way and works because it stores the Paintable and
    // applies it when the drag begins.
    let item_for_icon2 = item_for_icon.clone();
    let source_for_icon = source.clone();
    let view_size = view_for_size.clone();
    source.connect_drag_begin(move |_, _drag| {
        let Some(obj) = item_for_icon2
            .item()
            .and_then(|i| i.downcast::<crate::file_object::FileObject>().ok())
        else {
            return;
        };
        // Use the view's current icon size so the drag icon matches
        // the size of the icon the user clicked on — not a fixed size.
        let icon_px = view_size.icon_size.get();
        if let Some(paintable) = build_drag_icon(&obj, icon_px) {
            // Hotspot at center of the drag icon card.
            let w = paintable.intrinsic_width();
            let h = paintable.intrinsic_height();
            source_for_icon.set_icon(Some(&paintable), w / 2, h / 2);
        }
    });

    row.add_controller(source);
}

/// Check whether the dragged file's row is in the current selection.
fn is_row_selected(view: &FileView, obj: &crate::file_object::FileObject) -> bool {
    let target = obj.file();
    for f in selected_files(view) {
        if f.equal(&target) {
            return true;
        }
    }
    false
}

/// Collect all selected files from whichever selection model is
/// active (icon grid or list view).
fn selected_files(view: &FileView) -> Vec<gio::File> {
    let mut out = Vec::new();
    let sel = view
        .sel_icons
        .borrow()
        .as_ref()
        .cloned()
        .or_else(|| view.sel_list.borrow().as_ref().cloned());
    let Some(sel) = sel else { return out };
    let model = sel.model();
    let Some(model) = model else { return out };
    let n = model.n_items();
    for i in 0..n {
        if sel.is_selected(i) {
            if let Some(obj) = model
                .item(i)
                .and_then(|o| o.downcast::<crate::file_object::FileObject>().ok())
            {
                out.push(obj.file());
            }
        }
    }
    out
}

/// Drop target on a folder row — files land inside that directory.
/// The target directory is resolved from the ListItem at drop time
/// (rows are recycled; see attach_row_source). Rows bound to plain
/// files reject the drop — folders accept it.
pub fn attach_row_drop(row: &gtk4::Box, item: &gtk4::ListItem, view: &FileView) {
    let target = drop_target();
    let item = item.clone();
    let view = view.clone();
    target.connect_drop(move |t, value, _, _| {
        let Some(obj) = item
            .item()
            .and_then(|i| i.downcast::<crate::file_object::FileObject>().ok())
        else {
            return false;
        };
        if !obj.is_dir() {
            return false;
        }
        transfer_at(t, value, &obj.file(), view.widget.upcast_ref())
    });
    row.add_controller(target);
}

/// Drop target on the whole view area — files land in the displayed
/// directory.
pub fn attach_view_drop(view: &FileView) {
    let target = drop_target();
    let this = view.clone();
    target.connect_drop(move |t, value, _, _| {
        let Some(dest) = this.location() else {
            return false;
        };
        transfer_at(t, value, &dest, this.widget.upcast_ref())
    });
    view.widget.add_controller(target);
}

/// Drop target on a notebook tab label — files land in that tab's
/// current directory (`dest` is queried at drop time).
pub fn attach_location_drop(widget: &gtk4::Widget, dest: impl Fn() -> Option<gio::File> + 'static) {
    let target = drop_target();
    target.connect_drop(move |t, value, _, _| {
        let (Some(dest), Some(parent)) = (dest(), t.widget()) else {
            return false;
        };
        transfer_at(t, value, &dest, &parent)
    });
    widget.add_controller(target);
}

/// Shared drop handling: decode the FileList, pick copy vs. move from
/// the modifier state at drop time, hand it to the progress runner.
fn transfer_at(
    target: &gtk4::DropTarget,
    value: &glib::Value,
    dest: &gio::File,
    parent: &gtk4::Widget,
) -> bool {
    let Ok(list) = value.get::<gdk::FileList>() else {
        return false;
    };
    let files = list.files();
    if files.is_empty() {
        return false;
    }
    // Ctrl held at the moment of the drop = copy, else move.
    let copy = target
        .current_event_state()
        .contains(gdk::ModifierType::CONTROL_MASK);
    crate::progress::copy_move(parent, files, dest.clone(), !copy);
    true
}

fn drop_target() -> gtk4::DropTarget {
    gtk4::DropTarget::new(
        gdk::FileList::static_type(),
        gdk::DragAction::MOVE | gdk::DragAction::COPY,
    )
}

/// Build a drag icon "card" showing the file's icon and name, like
/// classic file managers do. Uses a GtkSnapshot to render an icon (48px) on
/// top and the filename below, then converts the snapshot to a
/// Paintable via gtk_snapshot_to_paintable.
///
/// The snapshot approach avoids the vte4 trait conflict on
/// gdk::Paintable — Snapshot::to_paintable returns a concrete
/// Paintable that DragSource::set_icon accepts.
fn build_drag_icon(obj: &crate::file_object::FileObject, icon_px: i32) -> Option<gdk::Paintable> {
    use gtk4::gdk;
    let display = gdk::Display::default()?;
    let theme = gtk4::IconTheme::for_display(&display);

    // Icon size from the view's current zoom level — matches the
    // size of the icon the user clicked on.
    let icon_size = icon_px.max(32) as f32;
    let icon_paintable = theme.lookup_by_gicon(
        &obj.icon(),
        icon_px.max(32),
        1,
        gtk4::TextDirection::None,
        gtk4::IconLookupFlags::empty(),
    );

    // Layout: icon + gap + text + padding.
    let text_height = 18.0f32;
    let gap = 4.0f32;
    let padding = 6.0f32;
    let card_width = icon_size + padding * 2.0;
    let card_height = icon_size + gap + text_height + padding * 2.0;

    let snapshot = gtk4::Snapshot::new();

    // Background: rounded rectangle with a semi-transparent fill so
    // the card is visible against any wallpaper.
    snapshot.append_color(
        &gdk::RGBA::new(0.13, 0.25, 0.55, 0.85),
        &gtk4::graphene::Rect::new(0.0, 0.0, card_width, card_height),
    );

    // Icon: centered horizontally, at the top with padding.
    let icon_x = (card_width - icon_size) / 2.0;
    let icon_y = padding;
    snapshot.save();
    snapshot.translate(&gtk4::graphene::Point::new(icon_x, icon_y));
    use gtk4::prelude::PaintableExt;
    icon_paintable.snapshot(&snapshot, icon_size as f64, icon_size as f64);
    snapshot.restore();

    // Filename text: create a PangoLayout and render it into the
    // snapshot with white color.
    let name = obj.display_name();
    let display_name = if name.chars().count() > 28 {
        let truncated: String = name.chars().take(25).collect();
        format!("{}…", truncated)
    } else {
        name
    };

    // Use a temporary Label's Pango context to create the layout.
    let label = gtk4::Label::new(Some(&display_name));
    let ctx = label.pango_context();
    let layout = gtk4::pango::Layout::new(&ctx);
    layout.set_text(&display_name);
    layout.set_width(((card_width as i32) - 8) * gtk4::pango::SCALE);
    layout.set_alignment(gtk4::pango::Alignment::Center);

    let text_y = icon_y + icon_size + gap;
    snapshot.save();
    snapshot.translate(&gtk4::graphene::Point::new(padding, text_y));
    snapshot.append_layout(&layout, &gdk::RGBA::new(1.0, 1.0, 1.0, 1.0));
    snapshot.restore();

    // Convert the snapshot to a Paintable with the card dimensions.
    let size = gtk4::graphene::Size::new(card_width, card_height);
    snapshot.to_paintable(Some(&size))
}
