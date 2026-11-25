// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// The compact view: small icons with a single-line label to their
// right, packed into as many columns as the pane width allows.
// Same recycling contract as the icon grid — widgets are built once
// in connect_setup, only data changes in connect_bind.

use gtk4::prelude::*;

use crate::file_object::FileObject;
use crate::file_view::FileView;
use crate::view_widgets;

/// Compact-icon pixel size — small enough for density, large enough
/// for the thumbnail types to stay recognizable.
const COMPACT_ICON_PX: i32 = 22;

/// Minimum pixel width of one compact cell. The GridView fits as
/// many of these columns into the pane as possible.
const COMPACT_CELL_MIN_WIDTH: i32 = 220;

/// Compact list alternative — same selection/model contract as the
/// icon grid, horizontal cells instead of the big vertical cards.
/// The grid derives its column count from the widest cell: keeping
/// the request tight is what makes the dense multi-column look.
pub fn build_compact_view(view: &FileView, model: &gtk4::SortListModel) -> gtk4::GridView {
    let this = view.clone();

    let factory = gtk4::SignalListItemFactory::new();

    // --- setup: build the row shell once per recyclable item ---
    let this_setup = this.clone();
    factory.connect_setup(move |_, item| {
        let Some(item) = item.downcast_ref::<gtk4::ListItem>() else {
            return;
        };

        let box_ = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
        box_.set_size_request(COMPACT_CELL_MIN_WIDTH, -1);
        box_.set_widget_name(view_widgets::ROW_MARKER);
        box_.set_halign(gtk4::Align::Fill);
        box_.set_margin_start(4);
        box_.set_margin_end(4);
        box_.set_margin_top(2);
        box_.set_margin_bottom(2);

        let image = gtk4::Image::new();
        image.set_pixel_size(COMPACT_ICON_PX);
        box_.append(&image);
        // Single-line caption, ellipsized at the end — the full name
        // stays in the tooltip and the file object itself.
        // FxLabel: same geometry as the old Label, plus the
        // optional filename effect (see crate::fxcfg).
        let caption = crate::fxlabel::FxLabel::new();
        caption.set_xalign(0.0);
        caption.set_single_line_mode(true);
        caption.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        caption.set_hexpand(true);
        caption.set_widget_name(view_widgets::CAPTION_MARKER);
        box_.append(&caption);

        // Selection paints on the row box — the CSS keeps the
        // stretched grid cell transparent so the highlight hugs the
        // row instead of covering dead cell space (same trick as the
        // icon grid).
        let cell = box_.clone();
        item.connect_selected_notify(move |i| {
            if i.is_selected() {
                cell.add_css_class("selected");
            } else {
                cell.remove_css_class("selected");
            }
        });

        view_widgets::attach_row_controllers(&box_, item, &this_setup);
        item.set_child(Some(&box_));
    });

    // --- bind: fill in the data; runs on every recycled row ---
    factory.connect_bind(move |_, item| {
        let Some(item) = item.downcast_ref::<gtk4::ListItem>() else {
            return;
        };
        let Some(obj) = item.item().and_then(|i| i.downcast::<FileObject>().ok()) else {
            return;
        };
        let Some(box_) = item.child().and_downcast::<gtk4::Box>() else {
            return;
        };
        // Model position for the rubber-band hit test — the widget
        // has no public position getter (see view_grid.rs).
        unsafe { box_.set_data("valkyrie-pos", item.position()) };
        // A recycled row under a static pointer would show a stale
        // expanded caption — drop it; motion re-shows if needed.
        crate::view_widgets::hide_caption_for(box_.upcast_ref());
        let (Some(image), Some(caption)) = (
            box_.first_child().and_downcast::<gtk4::Image>(),
            box_.first_child()
                .and_then(|w| w.next_sibling())
                .and_downcast::<crate::fxlabel::FxLabel>(),
        ) else {
            return;
        };

        image.set_from_gicon(&obj.icon());
        let name = obj.display_name();
        caption.set_label(&name);
        caption.set_tooltip_text(Some(&name));
        crate::view_widgets::apply_clip_classes(box_.upcast_ref(), &image, &obj);
    });

    let selection = gtk4::MultiSelection::new(Some(model.clone()));
    *view.sel_compact.borrow_mut() = Some(selection.clone());
    let v = view.clone();
    selection.connect_selection_changed(move |_, _, _| v.emit_selection_notify());
    let grid_view = gtk4::GridView::new(Some(selection.clone()), Some(factory));
    grid_view.add_css_class("valkyrie-icons");
    grid_view.set_min_columns(1);
    // Custom rubber-band controller — see view_grid.rs.
    grid_view.set_enable_rubberband(false);

    let view2 = view.clone();
    let sel = selection.clone();
    grid_view.connect_activate(move |_, pos| {
        if let Some(obj) = sel.item(pos).and_then(|i| i.downcast::<FileObject>().ok()) {
            view2.emit_activate(&obj.file(), obj.is_dir());
        }
    });

    grid_view
}
