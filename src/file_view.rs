// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// The directory contents area: a ColumnView (details list) and a
// GridView (icons) sharing the filtered/sorted model from view_model.
// Widget construction lives in view_widgets.rs; this file owns the
// state and the public API (navigation target, options, selection).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk4::gio;
use gtk4::prelude::*;

mod selection;

use crate::view_model::{SortField, ViewModel};

pub(crate) type ActivateCallbacks = Rc<RefCell<Vec<Box<dyn Fn(&gio::File, bool)>>>>;
pub(crate) type FileCallbacks = Rc<RefCell<Vec<Box<dyn Fn(&gio::File)>>>>;
pub(crate) type CountCallbacks = Rc<RefCell<Vec<Box<dyn Fn(u32)>>>>;
pub(crate) type OptionsCallbacks = Rc<RefCell<Vec<Box<dyn Fn()>>>>;
/// Selection-change listeners; same shape as OptionsCallbacks but a
/// separate collection — selection churns far more often than view
/// options, and listeners only care about one of them.
pub(crate) type SelCallbacks = Rc<RefCell<Vec<Box<dyn Fn()>>>>;

/// Handles that let the FileView steer column-header sorting: the
/// four columns (to resolve a field to its column) and the view's
/// sorter (the single source of truth for active field + direction).
#[derive(Clone)]
pub(crate) struct SortState {
    pub columns: [(SortField, gtk4::ColumnViewColumn); 4],
    pub view: gtk4::ColumnView,
    pub sorter: gtk4::ColumnViewSorter,
}

#[derive(Clone)]
pub struct FileView {
    pub widget: gtk4::Stack,
    pub(crate) location: Rc<RefCell<Option<gio::File>>>,
    pub(crate) model: Rc<RefCell<Option<ViewModel>>>,
    pub(crate) activate: ActivateCallbacks,
    item_count: Rc<RefCell<u32>>,
    count_notify: CountCallbacks,
    /// Fired when a view option changed (sort, hidden, view mode) so
    /// the menubar can resync its radio/checkbox states.
    options_notify: OptionsCallbacks,
    /// Icon vs. list mode, remembered so a location change reapplies it.
    pub(crate) icon_mode: Rc<Cell<bool>>,
    /// Icon pixel size; changing it rebuilds the grid so cells rebind.
    pub(crate) icon_size: Rc<Cell<i32>>,
    /// Selection models of both views; the visible one answers queries.
    pub(crate) sel_list: Rc<RefCell<Option<gtk4::MultiSelection>>>,
    pub(crate) sel_icons: Rc<RefCell<Option<gtk4::MultiSelection>>>,
    /// "Open in new tab" / "in new window" requests, resolved by the
    /// window owning this view (the view itself cannot reach it).
    pub(crate) open_in_tab: FileCallbacks,
    pub(crate) open_in_window: FileCallbacks,
    /// Single shared context-menu popover. Reused on every right
    /// click — creating a fresh popover each time leaves stale popover
    /// children on the stack and trips GTK's "broken accounting of
    /// active state" bookkeeping.
    pub(crate) ctx_popover: Rc<RefCell<Option<gtk4::Popover>>>,
    /// Column-sort handles of the details view; set once the
    /// ColumnView exists (first apply_model).
    pub(crate) sort_state: Rc<RefCell<Option<SortState>>>,
    /// Fired whenever the selection of either view changes (status
    /// bar updates — the models live in view_widgets, the notify is
    /// forwarded here).
    sel_notify: SelCallbacks,
    /// Parent-directory monitor resurrecting the view when the
    /// displayed folder is deleted and recreated in place (build
    /// scripts, checkouts) — the DirectoryList's own monitor dies
    /// with the old inode and would never fire again.
    dir_watch: Rc<RefCell<Option<gio::FileMonitor>>>,
}

impl FileView {
    /// Build the view stack for `location`.
    pub fn new(location: &gio::File) -> Self {
        let this = Self {
            widget: gtk4::Stack::builder()
                .transition_type(gtk4::StackTransitionType::Crossfade)
                // Only the visible child takes part in size
                // negotiation — with the default homogeneous stack,
                // the hidden ColumnView (min 640px) gets measured at
                // width 0 on transient layouts and GTK warns about it.
                .hhomogeneous(false)
                .vhomogeneous(false)
                .build(),
            location: Rc::new(RefCell::new(None)),
            model: Rc::new(RefCell::new(None)),
            activate: Rc::new(RefCell::new(Vec::new())),
            item_count: Rc::new(RefCell::new(0)),
            count_notify: Rc::new(RefCell::new(Vec::new())),
            options_notify: Rc::new(RefCell::new(Vec::new())),
            icon_mode: Rc::new(Cell::new(crate::settings::view_mode() == "icons")),
            icon_size: Rc::new(Cell::new(crate::settings::icon_size())),
            sel_list: Rc::new(RefCell::new(None)),
            sel_icons: Rc::new(RefCell::new(None)),
            open_in_tab: Rc::new(RefCell::new(Vec::new())),
            open_in_window: Rc::new(RefCell::new(Vec::new())),
            ctx_popover: Rc::new(RefCell::new(None)),
            sort_state: Rc::new(RefCell::new(None)),
            sel_notify: Rc::new(RefCell::new(Vec::new())),
            dir_watch: Rc::new(RefCell::new(None)),
        };
        this.set_location(location);
        // Break the watch→callback→view reference cycle on teardown:
        // the monitor's callback clones this FileView, so the pair
        // only dies together once the monitor is explicitly dropped.
        let dir_watch = this.dir_watch.clone();
        this.widget.connect_destroy(move |_| {
            *dir_watch.borrow_mut() = None;
        });
        // Files dropped anywhere on the view land in the displayed dir.
        crate::dnd::attach_view_drop(&this);
        this
    }

    /// Currently displayed directory.
    pub fn location(&self) -> Option<gio::File> {
        self.location.borrow().clone()
    }

    /// Re-evaluate clipboard styling on every *visible* row: walks
    /// the realized ListItem widgets (position lives in the
    /// "valkyrie-pos" qdata from the factories) instead of touching
    /// the model — SortListModel does not rebind rows reliably on
    /// a same-size items_changed notification.
    pub fn refresh_clip_styles(&self) {
        let Some(model) = self.model.borrow().clone() else {
            return;
        };
        // Find the GridView/ColumnView anywhere inside the visible
        // page — the wrappers (ScrolledWindow, rubber-band Overlay)
        // differ between view modes, so walk the subtree by type.
        let Some(list) = self
            .widget
            .visible_child()
            .and_then(|w| find_list_widget(&w, 6))
        else {
            return;
        };
        let mut row = list.first_child();
        while let Some(w) = row {
            row = w.next_sibling();
            let Some(cell) = w.first_child().and_downcast::<gtk4::Box>() else {
                continue;
            };
            let pos = unsafe { cell.data::<u32>("valkyrie-pos").map(|p| *p.as_ref()) };
            let Some(pos) = pos else { continue };
            let Some(obj) = model
                .list
                .item(pos)
                .and_downcast::<crate::file_object::FileObject>()
            else {
                continue;
            };
            if let Some(icon) = cell.first_child().and_downcast::<gtk4::Image>() {
                crate::view_widgets::apply_clip_classes(cell.upcast_ref(), &icon, &obj);
            }
        }
    }

    /// Number of rows in the model (status bar).
    pub fn item_count(&self) -> u32 {
        *self.item_count.borrow()
    }

    /// Switch between details list and icon grid. The two views keep
    /// separate MultiSelection models, so the selection is mirrored
    /// across — users expect marked items to stay marked when they
    /// flip views (and the status bar then reflects the visible one).
    pub fn set_icon_mode(&self, icons: bool) {
        let changed = self.icon_mode.get() != icons;
        if changed {
            self.mirror_selection(!icons);
        }
        self.icon_mode.set(icons);
        self.widget
            .set_visible_child_name(if icons { "icons" } else { "list" });
        if changed {
            self.emit_options_notify();
        }
    }

    /// Copy the selected positions from the currently visible view's
    /// selection model into the other one (used on view-mode switch).
    fn mirror_selection(&self, to_icons: bool) {
        let (src, dst) = if to_icons {
            (
                self.sel_list.borrow().clone(),
                self.sel_icons.borrow().clone(),
            )
        } else {
            (
                self.sel_icons.borrow().clone(),
                self.sel_list.borrow().clone(),
            )
        };
        let (Some(src), Some(dst), Some(model)) = (src, dst, self.model.borrow().clone()) else {
            return;
        };
        dst.unselect_all();
        for pos in 0..model.list.n_items() {
            if src.is_selected(pos) {
                dst.select_item(pos, false);
            }
        }
    }

    /// Whether the icon grid is currently shown.
    pub fn is_icon_mode(&self) -> bool {
        self.icon_mode.get()
    }

    /// Current icon pixel size.
    pub fn icon_size(&self) -> i32 {
        self.icon_size.get()
    }

    /// Change the icon size and rebuild the grid so every cell rebinds
    /// with the new pixel size (the signal factory caches no state).
    pub fn set_icon_size(&self, px: i32) {
        if self.icon_size.replace(px) == px {
            return;
        }
        let Some(model) = self.model.borrow().clone() else {
            return;
        };
        if let Some(old) = self.widget.child_by_name("icons") {
            self.widget.remove(&old);
        }
        *self.sel_icons.borrow_mut() = None;
        let grid = crate::view_grid::build_grid_view(self, &model.list);
        let page = wrap_view_page(
            &grid,
            self.sel_icons.borrow().as_ref().expect("icons selection"),
        );
        self.widget.add_named(&page, Some("icons"));
        if self.icon_mode.get() {
            self.widget.set_visible_child_name("icons");
        }
    }

    /// Forward a file activation (double click, Enter, context menu
    /// "Open") to the tab's registered handlers. Deferred one main
    /// loop iteration: activation arrives inside a pointer sequence
    /// and navigation recycles the row widgets it ran on — tearing
    /// them down mid-sequence trips GTK's "broken accounting of
    /// active state" bookkeeping. Pending release events always run
    /// before idle, so by the time this fires the sequence has ended.
    pub fn emit_activate(&self, file: &gio::File, is_dir: bool) {
        let activate = self.activate.clone();
        let file = file.clone();
        gtk4::glib::idle_add_local_once(move || {
            for cb in activate.borrow().iter() {
                cb(&file, is_dir);
            }
        });
    }

    /// "Open in New Tab" / middle click: resolved by the window, so
    /// it fires immediately (it never mutates this view's model).
    pub fn emit_open_in_tab(&self, file: &gio::File) {
        for cb in self.open_in_tab.borrow().iter() {
            cb(file);
        }
    }

    /// "Open in New Window" context menu entry.
    pub fn emit_open_in_window(&self, file: &gio::File) {
        for cb in self.open_in_window.borrow().iter() {
            cb(file);
        }
    }

    /// Register the window-level "open in new tab" handler.
    pub fn connect_open_in_tab<F: Fn(&gio::File) + 'static>(&self, f: F) {
        self.open_in_tab.borrow_mut().push(Box::new(f));
    }

    /// Register the window-level "open in new window" handler.
    pub fn connect_open_in_window<F: Fn(&gio::File) + 'static>(&self, f: F) {
        self.open_in_window.borrow_mut().push(Box::new(f));
    }

    /// Drop every window-level handler (open-in-tab/window, options
    /// sync). Called when a tab is adopted by another window after a
    /// cross-window drag — the old closures still point at the window
    /// that gave the tab up.
    pub fn clear_window_handlers(&self) {
        self.open_in_tab.borrow_mut().clear();
        self.open_in_window.borrow_mut().clear();
        self.options_notify.borrow_mut().clear();
    }

    // --- View options, forwarded to the model (no-op before first load) ---

    pub fn show_hidden(&self) -> bool {
        self.model
            .borrow()
            .as_ref()
            .map(|m| m.show_hidden())
            .unwrap_or(false)
    }

    pub fn set_show_hidden(&self, show: bool) {
        if let Some(m) = self.model.borrow().as_ref() {
            m.set_show_hidden(show);
        }
        self.emit_options_notify();
    }

    /// Active sort field — resolved from the ColumnViewSorter's
    /// primary column so header clicks and menu actions agree.
    pub fn sort_field(&self) -> SortField {
        self.sort_state
            .borrow()
            .as_ref()
            .and_then(|s| {
                s.sorter
                    .primary_sort_column()
                    .and_then(|col| s.columns.iter().find(|(_, c)| *c == col).map(|(f, _)| *f))
            })
            .unwrap_or(SortField::Name)
    }

    pub fn set_sort_field(&self, field: SortField) {
        self.set_sort(field, self.sort_descending());
        self.emit_options_notify();
    }

    /// Active sort direction from the ColumnViewSorter.
    pub fn sort_descending(&self) -> bool {
        self.sort_state
            .borrow()
            .as_ref()
            .map(|s| s.sorter.primary_sort_order() == gtk4::SortType::Descending)
            .unwrap_or(false)
    }

    pub fn set_sort_descending(&self, descending: bool) {
        self.set_sort(self.sort_field(), descending);
        self.emit_options_notify();
    }

    /// Point the ColumnView at the column for `field` in the given
    /// direction — same state a header click produces.
    pub fn set_sort(&self, field: SortField, descending: bool) {
        let state = self.sort_state.borrow().clone();
        let Some(s) = state else { return };
        let column = s
            .columns
            .iter()
            .find(|(f, _)| *f == field)
            .map(|(_, c)| c.clone())
            .unwrap_or_else(|| s.columns[0].1.clone());
        s.view.sort_by_column(
            Some(&column),
            if descending {
                gtk4::SortType::Descending
            } else {
                gtk4::SortType::Ascending
            },
        );
    }

    /// Register a listener fired when view options change — the
    /// menubar uses it to resync its stateful entries.
    pub fn connect_options_notify<F: Fn() + 'static>(&self, f: F) {
        self.options_notify.borrow_mut().push(Box::new(f));
    }

    fn emit_options_notify(&self) {
        for cb in self.options_notify.borrow().iter() {
            cb();
        }
    }

    /// Load `location` into both views.
    pub fn set_location(&self, location: &gio::File) {
        *self.location.borrow_mut() = Some(location.clone());
        // The model's own monitor dies silently when the displayed
        // directory is deleted+recreated in place (its watch is bound
        // to the old inode). Watch the parent — which survives the
        // swap — and reload the view when the directory reappears.
        let view = self.clone();
        *self.dir_watch.borrow_mut() = crate::dir_watch::watch_recreation(location, move || {
            if let Some(loc) = view.location() {
                view.set_location(&loc);
            }
        });
        self.apply_model(ViewModel::new(location));
    }

    /// Show pre-built search results. The displayed location stays
    /// untouched — `leave_search` restores it.
    pub fn show_search_results(&self, store: &gio::ListStore) {
        self.apply_model(ViewModel::for_entries(store));
    }

    /// Restore the real directory model after a search was cancelled
    /// or closed.
    pub fn leave_search(&self) {
        if let Some(location) = self.location.borrow().clone() {
            self.apply_model(ViewModel::new(&location));
        }
    }

    /// Swap a fresh ViewModel into the existing views: the selection
    /// models are rebound (never rebuilt — tearing widgets down
    /// mid-click breaks GTK's active-sequence accounting).
    fn apply_model(&self, model: ViewModel) {
        *self.item_count.borrow_mut() = 0;

        // Async enumeration finishes after navigation — listeners
        // (status bar) must be refreshed whenever rows arrive.
        let count = self.item_count.clone();
        let count_notify = self.count_notify.clone();
        model.list.connect_items_changed(move |m, _, _, _| {
            let n = m.n_items();
            *count.borrow_mut() = n;
            for cb in count_notify.borrow().iter() {
                cb(n);
            }
        });

        if self.widget.child_by_name("list").is_some() {
            if let Some(sel) = self.sel_list.borrow().as_ref() {
                sel.set_model(Some(&model.list));
            }
            if let Some(sel) = self.sel_icons.borrow().as_ref() {
                sel.set_model(Some(&model.list));
            }
        } else {
            let column_view = crate::view_widgets::build_column_view(self, &model.list);
            let grid_view = crate::view_grid::build_grid_view(self, &model.list);
            let list_page = wrap_view_page(
                &column_view,
                self.sel_list.borrow().as_ref().expect("list selection"),
            );
            let icons_page = wrap_view_page(
                &grid_view,
                self.sel_icons.borrow().as_ref().expect("icons selection"),
            );
            self.widget.add_named(&list_page, Some("list"));
            self.widget.add_named(&icons_page, Some("icons"));
        }
        // Every fresh model gets the view sorter chain — the
        // ColumnViewSorter survives navigation, so the chosen column
        // and direction persist across directories.
        if let Some(s) = self.sort_state.borrow().as_ref() {
            model.attach_view_sorter(&s.sorter.clone().upcast());
        }
        *self.model.borrow_mut() = Some(model);
        self.set_icon_mode(self.icon_mode.get());
    }

    /// Register a listener fired whenever the visible row count changes
    /// (async enumeration, folder monitoring, filtering).
    pub fn connect_count_notify<F: Fn(u32) + 'static>(&self, f: F) {
        self.count_notify.borrow_mut().push(Box::new(f));
    }

    /// Register the caller's activation handler (dir → navigate,
    /// file → open with default app).
    pub fn connect_activate<F: Fn(&gio::File, bool) + 'static>(&self, f: F) {
        self.activate.borrow_mut().push(Box::new(f));
    }
}

/// Wrap a scrollable file view (GridView or ColumnView) in its own
/// ScrolledWindow + Overlay. The scrollable child is direct so GTK
/// allocates the real viewport size; the overlay hosts the custom
/// rubber-band controller (src/rubberband.rs) that draws the drag
/// rectangle and updates the selection live.
fn wrap_view_page(
    child: &(impl IsA<gtk4::Widget> + IsA<gtk4::Scrollable>),
    sel: &gtk4::MultiSelection,
) -> gtk4::Widget {
    child.set_vscroll_policy(gtk4::ScrollablePolicy::Minimum);
    child.set_hscroll_policy(gtk4::ScrollablePolicy::Minimum);
    let scrolled = gtk4::ScrolledWindow::builder()
        .child(child)
        .hexpand(true)
        .vexpand(true)
        .build();
    // Keep a narrow floor under the scrollable so the window can
    // shrink well below any single row's width — the grid then
    // reflows instead of its parent being pinned by a wide page.
    scrolled.set_min_content_width(60);
    let overlay = gtk4::Overlay::builder().child(&scrolled).build();
    crate::rubberband::attach(&overlay, child.upcast_ref(), &scrolled, sel);
    overlay.upcast()
}

/// Depth-first search for the GridView or ColumnView inside a
/// view-mode page (ScrolledWindow → Overlay → view, or any other
/// nesting order).
fn find_list_widget(w: &gtk4::Widget, depth: u8) -> Option<gtk4::Widget> {
    if w.is::<gtk4::GridView>() || w.is::<gtk4::ColumnView>() {
        return Some(w.clone());
    }
    if depth == 0 {
        return None;
    }
    let mut child = w.first_child();
    while let Some(ch) = child {
        if let Some(l) = find_list_widget(&ch, depth - 1) {
            return Some(l);
        }
        child = ch.next_sibling();
    }
    None
}
