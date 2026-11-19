// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// Widget construction for the file views: the details ColumnView, the
// icon GridView and the shared right-click plumbing. Kept separate
// from file_view.rs so each file stays small; these functions only
// touch the public/pub(crate) API of FileView.

use gtk4::prelude::*;

use crate::context_menu;
use crate::file_object::FileObject;
use crate::file_view::FileView;
use crate::i18n::tr;
use crate::view_model::SortField;

/// Marker widget name: the background context menu uses it to detect
/// that a click happened on a row (whose own gesture handles it).
pub(crate) const ROW_MARKER: &str = "valkyrie-file-row";

/// Marker set on the filename label inside every row — the hover
/// expander finds the ellipsized widget through it.
pub(crate) const CAPTION_MARKER: &str = "valkyrie-name-caption";
/// Marker for the shared caption label that lives in each page's
/// overlay (created by file_view::wrap_view_page).
const HOVER_CAPTION: &str = "valkyrie-hover-caption";

/// Details list with the classic columns. Every column carries its
/// own sorter so the headers are clickable; the ColumnViewSorter then
/// drives the SortListModel (under a directories-first sorter).
pub fn build_column_view(view: &FileView, model: &gtk4::SortListModel) -> gtk4::ColumnView {
    let selection = gtk4::MultiSelection::new(Some(model.clone()));
    *view.sel_list.borrow_mut() = Some(selection.clone());
    let v = view.clone();
    selection.connect_selection_changed(move |_, _, _| v.emit_selection_notify());
    let list_view = gtk4::ColumnView::new(Some(selection.clone()));
    list_view.set_show_column_separators(true);
    // Rubber band selection is custom (src/rubberband.rs).
    list_view.set_enable_rubberband(false);

    let columns: [(SortField, gtk4::ColumnViewColumn); 4] = [
        (
            SortField::Name,
            column(
                view,
                SortField::Name,
                &tr("Name"),
                240,
                |box_| {
                    box_.append(&gtk4::Image::new());
                    let label = gtk4::Label::builder()
                        .xalign(0.0)
                        .ellipsize(gtk4::pango::EllipsizeMode::End)
                        .build();
                    label.set_widget_name(CAPTION_MARKER);
                    box_.append(&label);
                },
                |box_, obj| {
                    if let Some(image) = box_.first_child().and_downcast::<gtk4::Image>() {
                        image.set_from_gicon(&obj.icon());
                    }
                    if let Some(label) = box_
                        .first_child()
                        .and_then(|w| w.next_sibling())
                        .and_downcast::<gtk4::Label>()
                    {
                        label.set_label(&obj.display_name());
                    }
                },
            ),
        ),
        (
            SortField::Size,
            column(
                view,
                SortField::Size,
                &tr("Size"),
                90,
                |box_| {
                    box_.append(&gtk4::Label::builder().xalign(1.0).build());
                },
                |box_, obj| {
                    if let Some(label) = box_.first_child().and_downcast::<gtk4::Label>() {
                        label.set_label(&obj.size_text());
                    }
                },
            ),
        ),
        (
            SortField::Type,
            column(
                view,
                SortField::Type,
                &tr("Type"),
                140,
                |box_| {
                    box_.append(&gtk4::Label::builder().xalign(0.0).build());
                },
                |box_, obj| {
                    if let Some(label) = box_.first_child().and_downcast::<gtk4::Label>() {
                        label.set_label(&obj.type_text());
                    }
                },
            ),
        ),
        (
            SortField::Modified,
            column(
                view,
                SortField::Modified,
                &tr("Modified"),
                170,
                |box_| {
                    box_.append(&gtk4::Label::builder().xalign(0.0).build());
                },
                |box_, obj| {
                    if let Some(label) = box_.first_child().and_downcast::<gtk4::Label>() {
                        label.set_label(&obj.modified_text());
                    }
                },
            ),
        ),
    ];
    for (_, col) in &columns {
        list_view.append_column(col);
    }
    register_sort_state(view, &list_view, columns);

    // Enter or double click activates the selected row. The position
    // is resolved through the selection model (not a captured model
    // reference) so it always reflects the current directory — the
    // model is swapped in place on navigation.
    let view2 = view.clone();
    let sel = selection.clone();
    list_view.connect_activate(move |_, pos| {
        if let Some(obj) = sel.item(pos).and_then(|i| i.downcast::<FileObject>().ok()) {
            view2.emit_activate(&obj.file(), obj.is_dir());
        }
    });

    attach_background_menu(view, &list_view.clone().upcast());
    attach_empty_click_clear(view, &list_view.clone().upcast());
    list_view
}

/// One details-list column. `setup` builds the row's widgets once per
/// recyclable ListItem, `bind` fills them with data on every (re)bind —
/// the GTK4 recycling contract that keeps scrolling cheap. The column
/// sorter makes the header clickable — the ColumnViewSorter picks it
/// up and applies the direction.
fn column(
    view: &FileView,
    field: SortField,
    title: &str,
    width: i32,
    setup: impl Fn(&gtk4::Box) + 'static,
    bind: impl Fn(&gtk4::Box, &FileObject) + 'static,
) -> gtk4::ColumnViewColumn {
    let this = view.clone();

    let factory = gtk4::SignalListItemFactory::new();
    let this_setup = this.clone();
    factory.connect_setup(move |_, item| {
        let Some(item) = item.downcast_ref::<gtk4::ListItem>() else {
            return;
        };
        let box_ = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
        box_.set_widget_name(ROW_MARKER);
        setup(&box_);
        // Controllers resolve item.item() at event time — they stay
        // correct no matter which object the row is recycled onto.
        attach_row_controllers(&box_, item, &this_setup);
        item.set_child(Some(&box_));
    });
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
        // See view_grid.rs — position needed by the rubber-band.
        unsafe { box_.set_data("valkyrie-pos", item.position()) };
        // A recycled row under a static pointer would show a stale
        // expanded caption — drop it; motion re-shows if needed.
        crate::view_widgets::hide_caption_for(box_.upcast_ref());
        bind(&box_, &obj);
        // The row's leading widget is the icon — the dim effect
        // lives on it, text stays readable.
        if let Some(icon) = box_.first_child().and_downcast::<gtk4::Image>() {
            apply_clip_classes(box_.upcast_ref(), &icon, &obj);
        }
    });

    gtk4::ColumnViewColumn::builder()
        .title(title)
        .factory(&factory)
        .sorter(&crate::view_model::field_sorter(field))
        // fixed_width is the *initial* width — resizable puts a drag
        // handle on the header separator so the user can widen or
        // shrink every column afterwards.
        .fixed_width(width)
        .resizable(true)
        .build()
}

/// Store the sort handles on the FileView (field setters resolve the
/// column to sort by), seed the persisted sort once and persist every
/// later change — whether it came from a header click or the menu.
fn register_sort_state(
    view: &FileView,
    list_view: &gtk4::ColumnView,
    columns: [(SortField, gtk4::ColumnViewColumn); 4],
) {
    let sorter = list_view
        .sorter()
        .and_then(|s| s.downcast::<gtk4::ColumnViewSorter>().ok());
    let Some(sorter) = sorter else { return };

    let persist_columns = columns.clone();
    sorter.connect_changed(move |sorter, _| {
        let Some(sorter) = sorter.downcast_ref::<gtk4::ColumnViewSorter>() else {
            return;
        };
        let field = sorter
            .primary_sort_column()
            .and_then(|col| {
                persist_columns
                    .iter()
                    .find(|(_, c)| *c == col)
                    .map(|(f, _)| *f)
            })
            .unwrap_or(SortField::Name);
        let descending = sorter.primary_sort_order() == gtk4::SortType::Descending;
        crate::settings::set_sort_field(field.as_str());
        crate::settings::set_sort_descending(descending);
    });

    view.sort_state
        .borrow_mut()
        .replace(crate::file_view::SortState {
            columns,
            view: list_view.clone(),
            sorter,
        });

    // Seed the persisted choice once; the sorter keeps it across
    // model swaps, so this must not run again on navigation.
    let field = SortField::from_str(&crate::settings::sort_field());
    let descending = crate::settings::sort_descending();
    view.set_sort(field, descending);
}

/// Right click on empty view area shows the directory menu. Clicks
/// on rows are skipped: the row's own gesture already handled them.
pub(crate) fn attach_background_menu(view: &FileView, widget: &gtk4::Widget) {
    let this = view.clone();
    let gesture = gtk4::GestureClick::new();
    gesture.set_button(3);
    let target = widget.clone();
    // Open on release, not press (X11 menu convention): popping up
    // mid-press breaks GTK's active-state accounting when the pointer
    // moves before release — GTK bug #4563, fixed upstream in 4.21.6.
    gesture.connect_released(move |_, _, x, y| {
        if click_is_on_row(&target, x, y) {
            return;
        }
        // Translate into view-stack space: the shared popover lives on
        // the stack so it always has the full window height available.
        let stack = this.widget.clone().upcast::<gtk4::Widget>();
        let p = target
            .compute_point(&stack, &gtk4::graphene::Point::new(x as f32, y as f32))
            .unwrap_or_else(|| gtk4::graphene::Point::new(x as f32, y as f32));
        context_menu::show(&this, p.x() as f64, p.y() as f64, None);
    });
    widget.add_controller(gesture);
}

/// A button-1 press on empty background clears the selection — the
/// file-manager convention. Capture phase: the list cells claim their
/// own gestures before bubble ever sees the event, so the background
/// check must run on the way down. The gesture always denies the
/// sequence — item clicks, rubber-band drags and the background menu
/// all keep working exactly as before.
pub(crate) fn attach_empty_click_clear(view: &FileView, widget: &gtk4::Widget) {
    let this = view.clone();
    let target = widget.clone();
    let gesture = gtk4::GestureClick::new();
    gesture.set_button(1);
    gesture.set_propagation_phase(gtk4::PropagationPhase::Capture);
    let g = gesture.clone();
    gesture.connect_pressed(move |_, _, x, y| {
        g.set_state(gtk4::EventSequenceState::Denied);
        if !click_is_on_row(&target, x, y) {
            this.unselect_all();
        }
    });
    widget.add_controller(gesture);
}

/// Attach the row gestures (right-click menu, middle-click open) and
/// DnD plumbing to a recycled row. Called once in connect_setup —
/// every controller resolves `item.item()` at event time, so it
/// always acts on the object the row is *currently* bound to rather
/// than whatever it showed at setup.
///
/// The popover is parented to the view's stack widget, not the row:
/// a popover parented to a small row box gets vertically clamped and
/// gains an internal scrollbar. Coordinates are translated from row
/// space into stack space so the menu still opens at the pointer.
pub(crate) fn attach_row_controllers(row: &gtk4::Box, item: &gtk4::ListItem, view: &FileView) {
    let gesture = gtk4::GestureClick::new();
    gesture.set_button(3);
    let target = row.clone();
    {
        let view = view.clone();
        let item = item.clone();
        // Release, not press — see attach_background_menu.
        gesture.connect_released(move |_, _, x, y| {
            let Some(obj) = item.item().and_then(|i| i.downcast::<FileObject>().ok()) else {
                return;
            };
            let stack = view.widget.clone().upcast::<gtk4::Widget>();
            let point = target
                .compute_point(&stack, &gtk4::graphene::Point::new(x as f32, y as f32))
                .unwrap_or_else(|| gtk4::graphene::Point::new(x as f32, y as f32));
            context_menu::show(&view, point.x() as f64, point.y() as f64, Some(&obj));
        });
    }
    row.add_controller(gesture);

    // Middle click on a folder opens it in a new tab — the standard
    // shortcut every tabbed file manager offers. Directory check
    // happens at click time (the row may have been recycled onto a
    // plain file).
    {
        let v = view.clone();
        let item = item.clone();
        let middle = gtk4::GestureClick::new();
        middle.set_button(2);
        middle.connect_released(move |g, _, _, _| {
            let Some(obj) = item.item().and_then(|i| i.downcast::<FileObject>().ok()) else {
                return;
            };
            if obj.is_dir() {
                g.set_state(gtk4::EventSequenceState::Claimed);
                v.emit_open_in_tab(&obj.file());
            }
        });
        row.add_controller(middle);
    }

    // Folder rows accept drops; every row can be dragged out — to
    // another folder, tab or window. Both resolve the bound object
    // at event time as well.
    crate::dnd::attach_row_drop(row, item, view);
    crate::dnd::attach_row_source(row, item, view);

    // Hover expander: a truncated caption grows downward over the
    // neighbours (Nemo-style). The text is painted by a shared label
    // inside the page overlay — the row itself keeps its size, so the
    // grid never reflows on hover.
    {
        let target = row.clone().upcast::<gtk4::Widget>();
        let motion = gtk4::EventControllerMotion::new();
        {
            let target = target.clone();
            motion.connect_enter(move |_, _, _| show_hover_caption(&target));
        }
        {
            let target = target.clone();
            // motion re-shows after a scroll/rebind hid the caption
            // while the pointer stayed inside the row.
            motion.connect_motion(move |_, _, _| show_hover_caption(&target));
        }
        motion.connect_leave(move |_| hide_caption_for(&target));
        row.add_controller(motion);
    }
}

/// The shared hover caption one page overlay hosts. It must not be a
/// pointer target — events have to pass through to the rows below.
pub(crate) fn hover_caption() -> gtk4::Label {
    let label = gtk4::Label::builder()
        .halign(gtk4::Align::Start)
        .valign(gtk4::Align::Start)
        .wrap(true)
        .wrap_mode(gtk4::pango::WrapMode::WordChar)
        .visible(false)
        .can_target(false)
        .css_classes(["valkyrie-hover-caption"])
        .build();
    label.set_widget_name(HOVER_CAPTION);
    label
}

/// Locate the shared caption inside `scope` — accepts the page
/// overlay itself or any widget inside it.
fn caption_in(scope: &gtk4::Widget) -> Option<gtk4::Label> {
    let root = if scope.is::<gtk4::Overlay>() {
        Some(scope.clone())
    } else {
        scope.ancestor(gtk4::Overlay::static_type())
    }?;
    find_by_name(&root, HOVER_CAPTION, 2).and_downcast()
}

/// Hide the page's hover caption. Called on pointer leave, scroll,
/// view-mode switch and row rebind — anything that can stale the
/// floating text's position or content.
pub(crate) fn hide_caption_for(w: &gtk4::Widget) {
    if let Some(c) = caption_in(w) {
        c.set_visible(false);
    }
}

/// Expand an ellipsized row caption over the space below it. Copies
/// text, width and alignment from the real label onto the shared
/// overlay caption, so the first line sits exactly where the
/// truncated one is and only the continuation lines float.
fn show_hover_caption(row: &gtk4::Widget) {
    let Some(label) = find_by_name(row, CAPTION_MARKER, 4).and_downcast::<gtk4::Label>() else {
        return;
    };
    // Only expand when the name is actually truncated — short names
    // already read in full and need no overlay.
    if !label.layout().is_ellipsized() {
        hide_caption_for(row);
        return;
    }
    let Some(caption) = caption_in(row) else {
        return;
    };
    let Some(overlay) = caption
        .ancestor(gtk4::Overlay::static_type())
        .or_else(|| caption.parent().map(|p| p.upcast::<gtk4::Widget>()))
    else {
        return;
    };
    let Some(p) = label.compute_point(&overlay, &gtk4::graphene::Point::new(0.0, 0.0)) else {
        return;
    };
    caption.set_text(&label.label());
    caption.set_wrap_mode(label.wrap_mode());
    caption.set_justify(label.justify());
    caption.set_xalign(label.xalign());
    // Force the wrap width to the truncated label's allocation:
    // width_request is only a minimum (the label could still take its
    // unwrapped natural width and grow sideways), while
    // max_width_chars caps the natural size and makes pango wrap.
    // char width comes from the font metrics — px to chars is an
    // approximation, the line raggedness hardly matters.
    let width = (overlay.width() as f32 - p.x()).min(label.width() as f32);
    let metrics = caption
        .create_pango_context()
        .metrics(None::<&gtk4::pango::FontDescription>, None);
    let char_px = metrics.approximate_char_width() / gtk4::pango::SCALE;
    if char_px > 0 {
        caption.set_max_width_chars((width / char_px as f32).max(4.0) as i32);
    }
    caption.set_margin_start(p.x() as i32);
    caption.set_margin_top(p.y() as i32);
    caption.set_visible(true);
}

/// Depth-first child search by widget name — the recycling factories
/// tag their caption labels and row boxes instead of exporting them.
fn find_by_name(w: &gtk4::Widget, name: &str, depth: u8) -> Option<gtk4::Widget> {
    if w.widget_name() == name {
        return Some(w.clone());
    }
    if depth == 0 {
        return None;
    }
    let mut child = w.first_child();
    while let Some(ch) = child {
        if let Some(found) = find_by_name(&ch, name, depth - 1) {
            return Some(found);
        }
        child = ch.next_sibling();
    }
    None
}

/// True when the widget under (x, y) is inside a file row — detected
/// by walking ancestors for the row marker name.
fn click_is_on_row(widget: &gtk4::Widget, x: f64, y: f64) -> bool {
    let Some(picked) = widget.pick(x, y, gtk4::PickFlags::DEFAULT) else {
        return false;
    };
    let mut current = Some(picked);
    while let Some(w) = current {
        if w.widget_name() == ROW_MARKER {
            return true;
        }
        current = w.parent();
    }
    false
}

/// Paint the clipboard state on a bound row — programmatic opacity,
/// not CSS: theme stylesheet properties like -gtk-icon-effect are
/// not honoured in GTK4, so classes stayed invisible. Cut ghosts
/// icon + cell; copy fades the icon only ("staged, stays put").
pub fn apply_clip_classes(cell: &gtk4::Widget, icon: &gtk4::Image, obj: &FileObject) {
    match crate::operations::file_clipboard_state(&obj.file()) {
        Some(true) => {
            icon.set_opacity(0.4);
            cell.set_opacity(0.55);
        }
        Some(false) => {
            icon.set_opacity(0.55);
            cell.set_opacity(1.0);
        }
        None => {
            icon.set_opacity(1.0);
            cell.set_opacity(1.0);
        }
    }
}
