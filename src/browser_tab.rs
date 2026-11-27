// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// One browser tab: navigation toolbar, breadcrumb pathbar, location
// entry, the file view stack and a status bar with history management.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk4::gio;
use gtk4::glib;
use gtk4::prelude::*;

pub(crate) type TitleCallbacks = Rc<RefCell<Vec<Box<dyn Fn(&BrowserTab)>>>>;

pub(crate) mod close;
pub(crate) mod inflight;
mod nav_state;
pub(crate) mod split;
mod status_text;

use crate::file_view::FileView;
use crate::i18n::tr;
use crate::pathbar::Pathbar;

/// Everything that lives inside a single notebook page.
#[derive(Clone)]
pub struct BrowserTab {
    pub widget: gtk4::Box,
    /// pub(crate): the wiring modules (navigation, zoom) are sibling
    /// files that operate on these internals.
    pub(crate) file_view: FileView,
    pub(crate) pathbar: Pathbar,
    pub(crate) location_entry: gtk4::Entry,
    /// View switcher: icons, list, compact — the pressed button
    /// shows the active mode at a glance.
    pub(crate) view_switcher: [gtk4::ToggleButton; 3],
    pub(crate) status: gtk4::Label,
    /// Zoom slider in the status row; `zoom_loading` suppresses the
    /// write-back while a per-directory value is being applied.
    pub(crate) zoom_scale: gtk4::Scale,
    pub(crate) zoom_loading: Rc<Cell<bool>>,
    pub(crate) back_button: gtk4::Button,
    pub(crate) forward_button: gtk4::Button,
    /// Ctrl+F search bar; deactivated on every navigation.
    pub(crate) search_ui: crate::search::SearchUi,
    pub(crate) title: Rc<RefCell<String>>,
    pub(crate) title_notify: TitleCallbacks,
    pub(crate) back_stack: Rc<RefCell<Vec<gio::File>>>,
    pub(crate) forward_stack: Rc<RefCell<Vec<gio::File>>>,
    /// Split view (F3): the horizontal paned hosting the primary view
    /// plus — while split — a secondary PaneView.
    pub(crate) split_paned: gtk4::Paned,
    /// Banner shown when directory enumeration fails (e.g. no read
    /// permission) — replaces the misleadingly empty view.
    pub(crate) error_banner: gtk4::Box,
    /// The banner's message label — kept beside the banner so
    /// `set_enum_error` can retext it for each new failure.
    pub(crate) error_label: gtk4::Label,
    /// Rc-shared like title/back_stack: BrowserTab is cloned freely
    /// (action handlers, callbacks) and every clone must observe the
    /// same split state — a per-clone cell would drop the pane with
    /// the temporary clone that toggled it.
    pub(crate) secondary: Rc<RefCell<Option<Rc<crate::pane_view::PaneView>>>>,
    pub(crate) active_pane: Rc<Cell<split::ActivePane>>,
}

impl BrowserTab {
    /// Build a tab showing `location`.
    pub fn new(location: &gio::File) -> Self {
        let container = gtk4::Box::new(gtk4::Orientation::Vertical, 0);

        // --- Navigation toolbar: back, forward, up, pathbar, view toggle ---
        let toolbar = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
        toolbar.add_css_class("toolbar");

        let back = nav_button("go-previous-symbolic", &tr("Back"));
        let forward = nav_button("go-next-symbolic", &tr("Forward"));
        let up = nav_button("go-up-symbolic", &tr("Go to parent folder"));
        toolbar.append(&back);
        toolbar.append(&forward);
        toolbar.append(&up);

        let location_entry = gtk4::Entry::builder().hexpand(true).visible(false).build();
        let pathbar = Pathbar::new(location);
        // Deep paths produce many segment buttons — without the
        // scroller the toolbar's minimum width grows past a usable
        // window size and the whole view clips instead of reflowing.
        let path_scroll = gtk4::ScrolledWindow::builder()
            .child(&pathbar.widget)
            .hscrollbar_policy(gtk4::PolicyType::Automatic)
            .vscrollbar_policy(gtk4::PolicyType::Never)
            .propagate_natural_height(true)
            .build();
        // Keep the current (rightmost) segment visible as the bar grows.
        path_scroll.hadjustment().connect_changed(|a| {
            a.set_value(a.upper() - a.page_size());
        });
        let path_stack = gtk4::Stack::builder()
            .transition_type(gtk4::StackTransitionType::Crossfade)
            .build();
        path_stack.add_named(&path_scroll, Some("path"));
        path_stack.add_named(&location_entry, Some("entry"));
        path_stack.set_hexpand(true);
        toolbar.append(&path_stack);

        // Three linked toggle buttons — the pressed one marks the
        // active renderer without opening a menu.
        let view_switcher = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
        view_switcher.add_css_class("linked");
        let b_icons = gtk4::ToggleButton::builder()
            .icon_name("view-grid-symbolic")
            .tooltip_text(tr("Icon View"))
            .build();
        let b_list = gtk4::ToggleButton::builder()
            .icon_name("view-list-symbolic")
            .tooltip_text(tr("List View"))
            .group(&b_icons)
            .build();
        let b_compact = gtk4::ToggleButton::builder()
            .icon_name("view-compact-symbolic")
            .tooltip_text(tr("Compact View"))
            .group(&b_icons)
            .build();
        view_switcher.append(&b_icons);
        view_switcher.append(&b_list);
        view_switcher.append(&b_compact);
        toolbar.append(&view_switcher);

        // Search toggle mirrors the search bar's visibility both ways.
        let search_ui = crate::search::SearchUi::new();
        let search_toggle = gtk4::ToggleButton::builder()
            .icon_name("edit-find-symbolic")
            .tooltip_text(tr("Search"))
            .build();
        search_toggle
            .bind_property("active", &search_ui.bar, "search-mode-enabled")
            .bidirectional()
            .sync_create()
            .build();
        toolbar.append(&search_toggle);
        container.append(&toolbar);
        container.append(&search_ui.bar);

        // Enumeration-error banner: hidden until the directory model
        // reports a failed listing (e.g. no read permission) — a
        // successful pass hides it again.
        let error_banner = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        error_banner.add_css_class("error-banner");
        error_banner.set_visible(false);
        let warn_icon = gtk4::Image::from_icon_name("dialog-warning-symbolic");
        let error_label = gtk4::Label::builder()
            .xalign(0.0)
            .hexpand(true)
            .ellipsize(gtk4::pango::EllipsizeMode::End)
            .build();
        let error_close = gtk4::Button::from_icon_name("window-close-symbolic");
        error_close.add_css_class("flat");
        {
            let banner = error_banner.clone();
            error_close.connect_clicked(move |_| banner.set_visible(false));
        }
        error_banner.append(&warn_icon);
        error_banner.append(&error_label);
        error_banner.append(&error_close);
        container.append(&error_banner);

        // --- File views ---
        // The stack's pages are scrollable widgets inside their own
        // ScrolledWindows (see file_view.rs): no outer scroller here —
        // nested scrolled containers break GTK's height negotiation
        // and make large directories lay out rows across a fake
        // inflated height.
        let file_view = FileView::new(location);
        file_view.widget.set_hexpand(true);
        file_view.widget.set_vexpand(true);
        // The primary view always occupies the start slot; set_split
        // drops the secondary pane into the end slot (F3).
        let split_paned = gtk4::Paned::builder()
            .orientation(gtk4::Orientation::Horizontal)
            .start_child(&file_view.widget)
            .build();
        split_paned.set_resize_start_child(true);
        split_paned.set_resize_end_child(true);
        container.append(&split_paned);

        let status = gtk4::Label::builder()
            .xalign(0.0)
            .ellipsize(gtk4::pango::EllipsizeMode::End)
            .margin_start(8)
            .margin_end(8)
            .margin_top(2)
            .margin_bottom(2)
            .build();
        status.add_css_class("dim-label");
        status.set_hexpand(true);

        // Zoom slider in the status row — per-directory
        // size persisted in GVFS metadata, fallback in GSettings.
        // The value is not drawn (a permanent readout would make the
        // bar taller); zoom.rs keeps the current level in the tooltip.
        let zoom_scale = gtk4::Scale::with_range(gtk4::Orientation::Horizontal, 16.0, 320.0, 8.0);
        zoom_scale.set_width_request(140);
        zoom_scale.set_valign(gtk4::Align::Center);
        zoom_scale.set_margin_end(8);
        zoom_scale.set_tooltip_text(Some(&tr("Zoom")));
        let status_bar = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
        status_bar.append(&status);
        status_bar.append(&zoom_scale);
        container.append(&status_bar);

        let tab = Self {
            widget: container,
            file_view,
            pathbar,
            location_entry,
            view_switcher: [b_icons.clone(), b_list.clone(), b_compact.clone()],
            status,
            zoom_scale: zoom_scale.clone(),
            zoom_loading: Rc::new(Cell::new(false)),
            back_button: back.clone(),
            forward_button: forward.clone(),
            search_ui: search_ui.clone(),
            title: Rc::new(RefCell::new(String::new())),
            title_notify: Rc::new(RefCell::new(Vec::new())),
            back_stack: Rc::new(RefCell::new(Vec::new())),
            forward_stack: Rc::new(RefCell::new(Vec::new())),
            split_paned,
            error_banner,
            error_label,
            secondary: Rc::new(RefCell::new(None)),
            active_pane: Rc::new(Cell::new(split::ActivePane::Primary)),
        };

        crate::navigation::wire(&tab, &back, &forward, &up, &path_stack);
        crate::navigation::wire_view_switcher(&tab, [&b_icons, &b_list, &b_compact]);
        crate::zoom::wire(&tab, &zoom_scale);
        crate::search::wire(&tab, &search_ui);

        // Refresh the status bar whenever enumeration produces rows
        // and whenever the selection changes in either view.
        let tab2 = tab.clone();
        tab.file_view
            .connect_count_notify(move |_| tab2.update_status());
        let tab2 = tab.clone();
        tab.file_view
            .connect_selection_notify(move || tab2.update_status());
        // Mode/sort/hidden switches can change what the status line
        // must show (e.g. the other view's selection becomes active).
        let tab2 = tab.clone();
        tab.file_view
            .connect_options_notify(move || tab2.update_status());
        // Enumeration failures show the banner; a later successful
        // pass (None) hides it again.
        {
            let tab2 = tab.clone();
            tab.file_view
                .connect_error_notify(move |err| tab2.set_enum_error(err));
        }

        tab.navigate_to(location);
        tab
    }

    /// Navigate to `location`, recording history. While the secondary
    /// pane is focused it owns the navigation instead.
    pub fn navigate_to(&self, location: &gio::File) {
        if let Some(pane) = self.secondary_if_active() {
            pane.navigate_to(location);
            return;
        }
        // Push the current location before switching away from it.
        if let Some(current) = self.file_view.location() {
            if current != *location {
                self.back_stack.borrow_mut().push(current);
                self.forward_stack.borrow_mut().clear();
            }
        }
        self.show_location(location);
    }

    /// Navigate without touching history (back/forward reuse this).
    pub(crate) fn show_location(&self, location: &gio::File) {
        // An open search ends on every navigation — the new location
        // loads its own directory model.
        self.search_ui.deactivate();
        // A stale error banner must not bleed into the new location —
        // its model reports success/failure on its own.
        self.error_banner.set_visible(false);
        self.error_label.set_text("");
        self.file_view.set_location(location);
        self.pathbar.set_location(location);
        self.update_title();
        self.update_nav_sensitivity();
        self.update_status();
        crate::zoom::load(self, location);
    }

    /// Show or hide the enumeration-error banner: `Some(err)` after a
    /// failed listing, `None` after a successful one. Shared between
    /// the primary view and — while split — the secondary pane.
    pub(crate) fn set_enum_error(&self, err: Option<&glib::Error>) {
        match err {
            Some(e) => {
                let text = if e.matches(gio::IOErrorEnum::PermissionDenied) {
                    tr("No read permission for this folder").to_string()
                } else {
                    format!("{}: {e}", tr("Cannot display the folder contents"))
                };
                self.error_label.set_text(&text);
                self.error_banner.set_visible(true);
            }
            None => self.error_banner.set_visible(false),
        }
    }

    /// Tab header text: the basename of the current folder.
    pub fn title(&self) -> String {
        self.title.borrow().clone()
    }

    /// Register a callback fired whenever the tab title changes.
    pub fn connect_title_notify<F: Fn(&BrowserTab) + 'static>(&self, f: F) {
        self.title_notify.borrow_mut().push(Box::new(f));
    }

    /// Focus the text location entry (Ctrl+L).
    pub fn focus_location_entry(&self) {
        self.location_entry
            .set_text(&self.location().map(|f| display_uri(&f)).unwrap_or_default());
        self.location_entry.set_visible(true);
        self.location_entry.grab_focus();
        self.location_entry.select_region(0, -1);
    }

    /// Current directory shown in this tab — the focused pane's folder
    /// while split.
    pub fn location(&self) -> Option<gio::File> {
        self.secondary_if_active()
            .and_then(|p| p.location())
            .or_else(|| self.file_view.location())
    }

    /// Flip hidden-file visibility for this tab (Ctrl+H); persisted.
    /// Applies to both panes — view options are shared, only location,
    /// history and selection are per-pane.
    pub fn toggle_hidden(&self) {
        let show = !self.file_view.show_hidden();
        self.for_each_view(|v| v.set_show_hidden(show));
    }

    // --- Accessors used by the menubar actions -----------------------

    /// Switch view mode through the switcher so the pressed button
    /// and the shown renderer always agree.
    pub fn set_view_mode(&self, mode: crate::file_view::ViewMode) {
        let idx = match mode {
            crate::file_view::ViewMode::Icons => 0,
            crate::file_view::ViewMode::List => 1,
            crate::file_view::ViewMode::Compact => 2,
        };
        if !self.view_switcher[idx].is_active() {
            self.view_switcher[idx].set_active(true);
        } else {
            self.for_each_view(|v| v.set_view_mode(mode));
        }
    }

    /// Which renderer is currently shown.
    pub fn view_mode(&self) -> crate::file_view::ViewMode {
        self.file_view.view_mode()
    }

    pub fn show_hidden(&self) -> bool {
        self.file_view.show_hidden()
    }

    pub fn sort_field(&self) -> crate::view_model::SortField {
        self.file_view.sort_field()
    }

    pub fn sort_descending(&self) -> bool {
        self.file_view.sort_descending()
    }

    /// Files currently selected in the focused pane's view.
    pub fn selected_files(&self) -> Vec<gio::File> {
        self.active_view()
            .selected_objects()
            .iter()
            .map(|o| o.file())
            .collect()
    }

    /// Ctrl+A — select every row in the focused pane's view.
    pub fn select_all(&self) {
        self.active_view().select_all();
    }

    /// Re-enumerate the current directory (F5) — refreshes both
    /// panes as a diff into their live stores instead of a rebuild.
    pub fn reload(&self) {
        self.for_each_view(|v| v.refresh());
    }

    /// "New Folder" from the File menu — same dialog as the context menu.
    pub fn create_folder(&self) {
        if let Some(dir) = self.location() {
            crate::create_entry::create_folder(self.widget.upcast_ref(), &dir);
        }
    }

    /// "New Document" from the File menu — empty file via name dialog.
    pub fn create_document(&self) {
        if let Some(dir) = self.location() {
            crate::create_entry::create_document(self.widget.upcast_ref(), &dir, None);
        }
    }

    /// Forward view-option changes so the menubar can resync states.
    pub fn connect_options_notify<F: Fn() + 'static>(&self, f: F) {
        self.file_view.connect_options_notify(f);
    }

    /// Setters used by the stateful menubar actions — shared options,
    /// applied to every pane.
    pub fn set_show_hidden(&self, show: bool) {
        self.for_each_view(|v| v.set_show_hidden(show));
    }

    pub fn set_sort_field(&self, field: crate::view_model::SortField) {
        self.for_each_view(|v| v.set_sort_field(field));
    }

    pub fn set_sort_descending(&self, descending: bool) {
        self.for_each_view(|v| v.set_sort_descending(descending));
    }

    /// Icon size applies to every pane — the zoom slider is shared.
    pub(crate) fn set_icon_size(&self, px: i32) {
        self.for_each_view(|v| v.set_icon_size(px));
    }

    fn update_title(&self) {
        let name = self
            .location()
            .map(|f| {
                f.basename()
                    .map(|b| b.to_string_lossy().into_owned())
                    .unwrap_or_else(|| f.uri().to_string())
            })
            .unwrap_or_else(|| tr("Valkyrie"));
        *self.title.borrow_mut() = name;
        for f in self.title_notify.borrow().iter() {
            f(self);
        }
    }
}

/// Shared "activate a file" handler: dirs are navigated by the caller,
/// plain files open with their default application.
pub fn open_with_default(file: &gio::File) {
    // launch_default_for_uri resolves the handler for the file's URI
    // scheme and mime type itself — no explicit AppInfo query needed.
    let _ = gio::AppInfo::launch_default_for_uri(&file.uri(), None::<&gio::AppLaunchContext>);
}

/// Render a gio::File for the location entry: local paths stay paths,
/// virtual locations keep their URI.
fn display_uri(file: &gio::File) -> String {
    file.path()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| file.uri().to_string())
}

/// Small helper for identical navigation buttons.
fn nav_button(icon: &str, tooltip: &str) -> gtk4::Button {
    gtk4::Button::builder()
        .icon_name(icon)
        .tooltip_text(tooltip)
        .build()
}
