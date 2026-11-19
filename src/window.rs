// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// Main window: classic menu bar on top, a GtkNotebook of browser tabs
// and the places sidebar. All menu entries are "win.*" actions whose
// states (view mode, sorting, hidden files) resync whenever the view
// or the active tab changes.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{gio, glib};

mod panel;

use crate::browser_tab::BrowserTab;
use crate::i18n::tr;
use crate::menubar;
use crate::sidebar::Sidebar;

pub struct ValkyrieWindow {
    pub widget: gtk4::ApplicationWindow,
    pub(crate) notebook: gtk4::Notebook,
    /// Live BrowserTab objects, indexed parallel to the notebook pages.
    tabs: Rc<RefCell<Vec<BrowserTab>>>,
    pub(crate) menu_actions: Rc<crate::window_actions::MenuActions>,
    /// Vertical split wrapping the browser area; the bottom child is
    /// the embedded terminal panel (View menu, F4).
    vpaned: gtk4::Paned,
    terminal_panel: Rc<crate::terminal_panel::TerminalPanel>,
    /// The stored panel height is applied once on first show — before
    /// the window is allocated there is no height to split from.
    panel_pos_restored: std::cell::Cell<bool>,
    /// Self-reference keeping this object alive as long as the GTK
    /// window exists. Without it the Rc would drop right after
    /// `new()` returns and every weak upgrade (sidebar navigation,
    /// all win.* menu actions) would silently fail. Cleared on
    /// destroy so the cycle breaks and the object can drop.
    keep_alive: RefCell<Option<Rc<ValkyrieWindow>>>,
    /// Undo journal for copy/move/trash operations (Ctrl+Z).
    pub(crate) journal: crate::undo::JournalHandle,
}

impl ValkyrieWindow {
    /// Create the main window and open `initial` (or $HOME) in the first tab.
    pub fn new(app: &gtk4::Application, initial: Option<&gio::File>) -> Rc<Self> {
        let this = Self::build(app);
        this.open_tab(initial);
        this
    }

    /// Window skeleton: widgets, actions, sidebar wiring — but no
    /// tab. `new` opens the initial tab; the tab-drag create-window
    /// path builds a skeleton and lets GTK move the dragged page in.
    fn build(app: &gtk4::Application) -> Rc<Self> {
        let window = gtk4::ApplicationWindow::builder()
            .application(app)
            .title(tr("Valkyrie"))
            .default_width(1024)
            .default_height(640)
            .build();
        // Elevated (pkexec-spawned) instances get the `elevated`
        // class → red frame + tinted menu bar so a root window is
        // visually unmistakable.
        if std::env::var_os("VALKYRIE_ELEVATED").is_some() {
            window.add_css_class("elevated");
        }

        let sidebar = Sidebar::new();
        let notebook = gtk4::Notebook::builder()
            .scrollable(true)
            .show_border(false)
            .build();
        notebook.set_group_name(Some("valkyrie")); // tabs can be dragged between windows

        // Horizontal split: sidebar on the left, tabbed browsers on the right.
        let paned = gtk4::Paned::builder()
            .orientation(gtk4::Orientation::Horizontal)
            .start_child(&sidebar.widget)
            .end_child(&notebook)
            .position(220)
            .build();
        paned.set_resize_start_child(false);

        // Embedded terminal: vertical split, file area on top, shell
        // below. The terminal child keeps its pixel height on resize —
        // only the file area grows/shrinks.
        let terminal_panel = crate::terminal_panel::TerminalPanel::new();
        let vpaned = gtk4::Paned::builder()
            .orientation(gtk4::Orientation::Vertical)
            .start_child(&paned)
            .end_child(&terminal_panel.widget)
            .build();
        vpaned.set_resize_end_child(false);
        terminal_panel
            .widget
            .set_visible(crate::settings::terminal_panel_visible());

        // Classic menu bar above the paned area.
        let menubar = gtk4::PopoverMenuBar::from_model(Some(&menubar::build_model()));
        let layout = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        if std::env::var_os("VALKYRIE_ELEVATED").is_some() {
            layout.add_css_class("elevated-frame");
        }
        layout.append(&menubar);
        layout.append(&vpaned);
        window.set_child(Some(&layout));

        let this = Rc::new(Self {
            widget: window,
            notebook,
            vpaned,
            terminal_panel,
            panel_pos_restored: std::cell::Cell::new(false),
            tabs: Rc::new(RefCell::new(Vec::new())),
            menu_actions: Rc::new(crate::window_actions::menu_actions()),
            keep_alive: RefCell::new(None),
            journal: Rc::new(crate::undo::Journal::new()),
        });

        // Dragging the split handle stores the user-chosen height.
        panel::watch_position(&this);

        // Self-keep-alive for the whole window lifetime: the Rc cycle
        // is broken when GTK destroys the window.
        *this.keep_alive.borrow_mut() = Some(this.clone());
        let weak = Rc::downgrade(&this);
        this.widget.connect_destroy(move |_| {
            if let Some(this) = weak.upgrade() {
                *this.keep_alive.borrow_mut() = None;
            }
        });

        crate::window_actions::install(&this);

        // Sidebar selection navigates the active tab.
        let weak = Rc::downgrade(&this);
        sidebar.connect_location_selected(move |file| {
            if let Some(this) = weak.upgrade() {
                if let Some(tab) = this.active_tab() {
                    tab.navigate_to(file);
                }
            }
        });

        // The sidebar's right-click menu resolves "Open in New Tab" /
        // "Open in New Window" at window level, just like the
        // file-view context menu does.
        let weak = Rc::downgrade(&this);
        sidebar.connect_open_in_tab(move |file| {
            if let Some(this) = weak.upgrade() {
                this.open_tab(Some(file));
            }
        });
        let weak = Rc::downgrade(&this);
        sidebar.connect_open_in_window(move |file| {
            if let Some(this) = weak.upgrade() {
                if let Some(app) = this.widget.application() {
                    let window = ValkyrieWindow::new(&app, Some(file));
                    window.widget.present();
                }
            }
        });

        // Keep tabs vec and the auto-hiding tab bar in sync when a
        // page is added, reordered or detached.
        let weak = Rc::downgrade(&this);
        this.notebook.connect_page_removed(move |_, page, _| {
            if let Some(this) = weak.upgrade() {
                // Stash the departing tab before our vec drops the
                // last object reference — the window receiving the
                // drag claims it in its page-added handler.
                if let Some(tab) = this
                    .tabs
                    .borrow()
                    .iter()
                    .find(|t| &t.widget == page)
                    .cloned()
                {
                    crate::browser_tab::inflight::stash_in_flight(page, tab);
                }
                this.sync_tabs();
                this.update_tab_bar();
            }
        });
        let weak = Rc::downgrade(&this);
        this.notebook.connect_page_added(move |_, page, _| {
            if let Some(this) = weak.upgrade() {
                // A page we do not own arrived: a tab dragged from
                // another window. Adopt its BrowserTab and re-point
                // the window-level handlers to this window.
                let known = this.tabs.borrow().iter().any(|t| &t.widget == page);
                if !known {
                    if let Some(tab) = crate::browser_tab::inflight::take_in_flight(page) {
                        this.adopt_tab(&tab);
                        this.tabs.borrow_mut().push(tab);
                    }
                }
                this.update_tab_bar();
            }
        });
        // Menu states and the terminal panel's working directory follow
        // the active tab.
        let weak = Rc::downgrade(&this);
        this.notebook.connect_switch_page(move |_, _, _| {
            if let Some(this) = weak.upgrade() {
                this.sync_view_actions();
                if let Some(tab) = this.active_tab() {
                    this.terminal_panel.set_location(tab.location().as_ref());
                }
            }
        });

        // Tab close interactions (X button, middle click) live at
        // notebook level — see browser_tab::close for why.
        crate::browser_tab::close::attach(&this.notebook);

        // A tab dragged outside every window opens a new one that
        // hosts it — without this handler GTK would silently discard
        // the page. The new window adopts the tab through the
        // page-added handler above, like any other drag target.
        let weak = Rc::downgrade(&this);
        this.notebook.connect_create_window(move |_, _| {
            let this = weak.upgrade()?;
            let app = this.widget.application()?;
            let window = ValkyrieWindow::build(&app);
            window.widget.present();
            Some(window.notebook.clone())
        });

        this
    }

    /// Open a new browser tab at `location` (falls back to the home dir).
    pub fn open_tab(self: &Rc<Self>, location: Option<&gio::File>) {
        let home = gio::File::for_path(glib::home_dir());
        let file = location.cloned().unwrap_or(home);

        let tab = BrowserTab::new(&file);
        // Own the tab before it enters the notebook: the page-added
        // handler must see it as ours and not double-adopt it.
        self.tabs.borrow_mut().push(tab.clone());
        self.wire_tab(&tab);
        let header = self.tab_header(&tab);

        let page_num = self.notebook.append_page(&tab.widget, Some(&header));
        self.notebook.set_tab_reorderable(&tab.widget, true);
        self.notebook.set_tab_detachable(&tab.widget, true);
        self.notebook.set_current_page(Some(page_num));
        self.update_tab_bar();
        self.sync_view_actions();
    }

    /// Point a tab's window-level handlers at this window — the menu
    /// state sync plus "Open in New Tab/Window" from the context menu
    /// or a middle click, which resolve at window level.
    fn wire_tab(self: &Rc<Self>, tab: &BrowserTab) {
        let weak = Rc::downgrade(self);
        tab.connect_options_notify(move || {
            if let Some(this) = weak.upgrade() {
                this.sync_view_actions();
            }
        });
        // title_notify fires on every navigation (update_title calls it
        // unconditionally) — reuse it to keep the embedded terminal in
        // the active tab's directory.
        let weak = Rc::downgrade(self);
        tab.connect_title_notify(move |tab| {
            if let Some(this) = weak.upgrade() {
                let is_active = this
                    .active_tab()
                    .map(|a| a.widget == tab.widget)
                    .unwrap_or(false);
                if is_active {
                    this.terminal_panel.set_location(tab.location().as_ref());
                }
            }
        });
        let weak = Rc::downgrade(self);
        tab.file_view.connect_open_in_tab(move |file| {
            if let Some(this) = weak.upgrade() {
                this.open_tab(Some(file));
            }
        });
        let weak = Rc::downgrade(self);
        tab.file_view.connect_open_in_window(move |file| {
            if let Some(this) = weak.upgrade() {
                if let Some(app) = this.widget.application() {
                    let window = ValkyrieWindow::new(&app, Some(file));
                    window.widget.present();
                }
            }
        });
    }

    /// A tab page arrived from another window (drag & drop): drop the
    /// handlers still pointing at the sending window and wire ours.
    fn adopt_tab(self: &Rc<Self>, tab: &BrowserTab) {
        tab.file_view.clear_window_handlers();
        self.wire_tab(tab);
    }

    /// Tab header: title label plus a small close button. Middle-click
    /// closes the tab, and files dropped on the header land in the
    /// tab's directory. The close interactions are NOT wired here:
    /// GtkNotebook processes tab-strip presses through its own
    /// capture-phase gesture, so tab-header children do not reliably
    /// receive button events — the notebook-level controller in
    /// `build()` resolves them via pick(). The button stays as the
    /// visible affordance (and pick target, by widget name).
    fn tab_header(self: &Rc<Self>, tab: &BrowserTab) -> gtk4::Widget {
        let header = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
        header.set_widget_name("tab-header");
        let tab_label = gtk4::Label::new(Some(&tab.title()));

        // Keep the tab header in sync with the current folder name.
        let label = tab_label.clone();
        tab.connect_title_notify(move |t| label.set_label(&t.title()));
        header.append(&tab_label);

        let close = gtk4::Button::builder()
            .icon_name("window-close-symbolic")
            .valign(gtk4::Align::Center)
            .has_frame(false)
            .build();
        // Fixed hit area: an icon-only button collapses to zero width
        // when the icon theme cannot resolve the symbolic name.
        close.set_size_request(22, 22);
        close.set_widget_name("tab-close");
        header.append(&close);

        let tab_ref = tab.clone();
        crate::dnd::attach_location_drop(header.upcast_ref(), move || tab_ref.location());

        header.upcast()
    }

    /// Show the tab bar only when more than one tab exists — a single
    /// tab's bar wastes space and would confuse users.
    fn update_tab_bar(&self) {
        self.notebook.set_show_tabs(self.notebook.n_pages() > 1);
    }

    /// Push the active tab's view options into the menu action states
    /// so radio/checkbox marks always match what the view actually does.
    fn sync_view_actions(&self) {
        let Some(tab) = self.active_tab() else { return };
        let m = &*self.menu_actions;
        m.view_mode
            .set_state(&tab.view_mode().as_str().to_variant());
        m.arrange.set_state(&tab.sort_field().as_str().to_variant());
        m.arrange_desc
            .set_state(&tab.sort_descending().to_variant());
        m.show_hidden.set_state(&tab.show_hidden().to_variant());
        m.split_view.set_state(&tab.is_split().to_variant());
    }

    /// The BrowserTab currently shown in the notebook, if any.
    pub(crate) fn active_tab(&self) -> Option<BrowserTab> {
        let page = self.notebook.nth_page(self.notebook.current_page())?;
        self.tabs
            .borrow()
            .iter()
            .find(|t| t.widget == page)
            .cloned()
    }

    /// Run `f` for every open tab — the preferences dialog uses it to
    /// apply app-wide options (view mode, hidden files) live.
    pub(crate) fn for_each_tab(&self, f: impl Fn(&BrowserTab)) {
        for tab in self.tabs.borrow().iter() {
            f(tab);
        }
    }

    /// Drop tab objects whose widget was removed (reorder/detach/close).
    fn sync_tabs(&self) {
        let mut tabs = self.tabs.borrow_mut();
        tabs.retain(|t| self.notebook.page_num(&t.widget).is_some());
    }
}
