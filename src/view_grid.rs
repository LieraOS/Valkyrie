// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// The icon grid: themed icons at the current zoom level with wrapped
// captions, thumbnails on top where available. The factory follows
// the GTK4 recycling contract — widget construction in connect_setup
// (once per reusable row), data updates only in connect_bind, which
// runs on every scroll step and therefore must stay cheap.

use gtk4::prelude::*;

use crate::file_object::FileObject;
use crate::file_view::FileView;
use crate::view_widgets;

/// Icon grid alternative — themed icons at the current zoom level,
/// wrapped labels. Rebuilt by set_icon_size when the zoom changes.
/// The grid comes wrapped in its own ScrolledWindow so it is a
/// direct scrollable child: GTK then allocates the real viewport
/// height and the grid manages its own scroll range natively.
pub fn build_grid_view(view: &FileView, model: &gtk4::SortListModel) -> gtk4::GridView {
    let this = view.clone();
    let px = view.icon_size.get();

    let factory = gtk4::SignalListItemFactory::new();

    // --- setup: build the card once; everything below survives ---
    let this_setup = this.clone();
    factory.connect_setup(move |_, item| {
        let Some(item) = item.downcast_ref::<gtk4::ListItem>() else {
            return;
        };

        let box_ = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
        // The card hugs icon + caption as tightly as possible: its
        // minimum width is just the icon plus the CSS padding. Every
        // extra pixel counts twice — the column count is derived from
        // the widest card, and GtkGridView spreads leftover window
        // width evenly across all columns, so a fat card request
        // translates directly into huge gaps between items.
        // The height stays natural (a fixed height smaller than the
        // content squeezed the icon). Everything outside the card —
        // cell padding and the grid's border-spacing — is dead
        // background: clicks there clear the selection.
        box_.set_size_request(px + 14, -1);
        box_.set_widget_name(view_widgets::ROW_MARKER);
        box_.set_halign(gtk4::Align::Center);
        box_.set_valign(gtk4::Align::Center);
        box_.add_css_class("file-cell");

        let image = gtk4::Image::new();
        image.set_pixel_size(px);
        image.set_margin_top(4);
        box_.append(&image);
        // Caption: wraps at word or character boundaries (so a single
        // very long name can never blow up the whole grid layout),
        // max two lines, ellipsized at the end — the real filename is
        // untouched; the tooltip still shows it in full.
        // FxLabel: same geometry as the old Label, plus the optional
        // filename effect (see crate::fxcfg).
        let caption = crate::fxlabel::FxLabel::new();
        caption.set_max_width_chars(11);
        caption.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        caption.set_lines(2);
        caption.set_wrap(true);
        caption.set_wrap_mode(gtk4::pango::WrapMode::WordChar);
        caption.set_justify(gtk4::Justification::Center);
        caption.set_widget_name(view_widgets::CAPTION_MARKER);
        // GtkGridView gives every cell the height of the tallest item,
        // so a single long filename (wrapped to two lines) inflated
        // every row in that directory and looked like random extra
        // spacing. Reserving exactly two text lines for every caption
        // makes the cell height identical in every folder; yalign 0
        // keeps a single-line caption glued under the icon instead of
        // floating centered inside the reserved space.
        let metrics = caption
            .create_pango_context()
            .metrics(None::<&gtk4::pango::FontDescription>, None);
        let line_px = (metrics.ascent() + metrics.descent()) / gtk4::pango::SCALE;
        if line_px > 0 {
            caption.set_size_request(-1, line_px * 2 + 2);
            caption.set_yalign(0.0);
        }
        box_.append(&caption);

        // Selection paints on the item box only — the app stylesheet
        // keeps the stretched grid cell transparent so the highlight
        // hugs icon + label instead of covering dead cell space.
        // Connected once here: the ListItem keeps this handler across
        // every rebind (the old per-bind connect leaked one handler
        // per scroll step).
        let cell = box_.clone();
        item.connect_selected_notify(move |i| {
            if i.is_selected() {
                cell.add_css_class("selected");
            } else {
                cell.remove_css_class("selected");
            }
        });

        // Gestures and DnD resolve item.item() at event time, so the
        // same controllers serve every object this row is bound to.
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
        // Stash the model position on the row widget for the
        // rubber-band hit test (src/rubberband.rs) — GtkListItemWidget
        // exposes no public position getter.
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
        // May upgrade the icon to a thumbnail asynchronously — bumps
        // the image's job generation first, so late results can never
        // write back over a newer bind.
        crate::thumbnails::attach(&obj, &image, px);
        crate::view_widgets::apply_clip_classes(box_.upcast_ref(), &image, &obj);
    });

    let selection = gtk4::MultiSelection::new(Some(model.clone()));
    *view.sel_icons.borrow_mut() = Some(selection.clone());
    let v = view.clone();
    selection.connect_selection_changed(move |_, _, _| v.emit_selection_notify());
    let grid_view = gtk4::GridView::new(Some(selection.clone()), Some(factory));
    grid_view.add_css_class("valkyrie-icons");
    grid_view.set_max_columns(12);
    // min_columns 1: a higher minimum would clamp the column width
    // on narrow panes without helping anything — the grid computes
    // the column count from the real allocated width itself.
    grid_view.set_min_columns(1);
    // Built-in rubberband explicitly OFF — the custom controller in
    // src/rubberband.rs replaces it (live marking + no race with the
    // row DragSources). Just removing set_enable_rubberband(true)
    // left it enabled on GTK versions where the default is true.
    grid_view.set_enable_rubberband(false);

    let view2 = view.clone();
    let sel = selection.clone();
    grid_view.connect_activate(move |_, pos| {
        if let Some(obj) = sel.item(pos).and_then(|i| i.downcast::<FileObject>().ok()) {
            view2.emit_activate(&obj.file(), obj.is_dir());
        }
    });

    view_widgets::attach_background_menu(view, &grid_view.clone().upcast());
    view_widgets::attach_empty_click_clear(view, &grid_view.clone().upcast());
    grid_view
}
