// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// The secondary pane of a split view (View → Split View, F3). Fully
// self-contained: own back/forward/up history buttons and its own
// breadcrumb bar, so the pane navigates without touching the primary
// pane's toolbar state. The shared tab-level toolbar (back/forward/up,
// location entry) delegates to whichever pane is active — see
// browser_tab::split.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::gio;
use gtk4::prelude::*;

use crate::browser_tab;
use crate::file_view::FileView;
use crate::i18n::tr;
use crate::pathbar::Pathbar;

/// Callbacks fired after every navigation inside this pane, so the
/// owning tab can refresh title/status/nav sensitivity when this pane
/// is the active one.
type NavNotify = Rc<RefCell<Vec<Box<dyn Fn()>>>>;

pub(crate) struct PaneView {
    pub widget: gtk4::Box,
    pub file_view: FileView,
    pathbar: Pathbar,
    back_button: gtk4::Button,
    forward_button: gtk4::Button,
    back_stack: Rc<RefCell<Vec<gio::File>>>,
    forward_stack: Rc<RefCell<Vec<gio::File>>>,
    nav_notify: NavNotify,
}

impl PaneView {
    /// Build the pane showing `location`.
    pub fn new(location: &gio::File) -> Rc<Self> {
        let file_view = FileView::new(location);
        // Stack pages are scrollable children inside their own
        // ScrolledWindows (see file_view.rs) — no outer scroller.
        file_view.widget.set_hexpand(true);
        file_view.widget.set_vexpand(true);

        let back = nav_button("go-previous-symbolic", &tr("Back"));
        let forward = nav_button("go-next-symbolic", &tr("Forward"));
        let up = nav_button("go-up-symbolic", &tr("Go to parent folder"));
        let pathbar = Pathbar::new(location);
        pathbar.widget.set_hexpand(true);

        let nav = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
        nav.append(&back);
        nav.append(&forward);
        nav.append(&up);
        nav.append(&pathbar.widget);

        let widget = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        widget.append(&nav);
        widget.append(&file_view.widget);

        let this = Rc::new(Self {
            widget,
            file_view: file_view.clone(),
            pathbar,
            back_button: back.clone(),
            forward_button: forward.clone(),
            back_stack: Rc::new(RefCell::new(Vec::new())),
            forward_stack: Rc::new(RefCell::new(Vec::new())),
            nav_notify: Rc::new(RefCell::new(Vec::new())),
        });

        // --- self-contained navigation ---------------------------------
        let t = Rc::downgrade(&this);
        back.connect_clicked(move |_| {
            if let Some(t) = t.upgrade() {
                t.go_back();
            }
        });
        let t = Rc::downgrade(&this);
        forward.connect_clicked(move |_| {
            if let Some(t) = t.upgrade() {
                t.go_forward();
            }
        });
        let t = Rc::downgrade(&this);
        up.connect_clicked(move |_| {
            if let Some(t) = t.upgrade() {
                if let Some(parent) = t.location().and_then(|f| f.parent()) {
                    t.navigate_to(&parent);
                }
            }
        });
        let t = Rc::downgrade(&this);
        this.pathbar.connect_location_selected(move |f| {
            if let Some(t) = t.upgrade() {
                t.navigate_to(f);
            }
        });

        // Activation mirrors the primary pane: dirs navigate, files open
        // with their default application.
        let t = Rc::downgrade(&this);
        file_view.connect_activate(move |file, is_dir| {
            if let Some(t) = t.upgrade() {
                if is_dir {
                    t.navigate_to(file);
                } else {
                    browser_tab::open_with_default(file);
                }
            }
        });

        this.navigate_to(location);
        this
    }

    /// Navigate to `location`, recording history.
    pub fn navigate_to(&self, location: &gio::File) {
        if let Some(current) = self.file_view.location() {
            if current != *location {
                self.back_stack.borrow_mut().push(current);
                self.forward_stack.borrow_mut().clear();
            }
        }
        self.show_location(location);
    }

    /// Navigate without touching history (back/forward reuse this).
    pub fn show_location(&self, location: &gio::File) {
        self.file_view.set_location(location);
        self.pathbar.set_location(location);
        self.update_nav_sensitivity();
        for f in self.nav_notify.borrow().iter() {
            f();
        }
    }

    pub fn location(&self) -> Option<gio::File> {
        self.file_view.location()
    }

    pub fn go_back(&self) {
        let Some(target) = self.back_stack.borrow_mut().pop() else {
            return;
        };
        if let Some(current) = self.location() {
            self.forward_stack.borrow_mut().push(current);
        }
        self.show_location(&target);
    }

    pub fn go_forward(&self) {
        let Some(target) = self.forward_stack.borrow_mut().pop() else {
            return;
        };
        if let Some(current) = self.location() {
            self.back_stack.borrow_mut().push(current);
        }
        self.show_location(&target);
    }

    pub fn can_back(&self) -> bool {
        !self.back_stack.borrow().is_empty()
    }

    pub fn can_forward(&self) -> bool {
        !self.forward_stack.borrow().is_empty()
    }

    /// The owning tab listens here to refresh shared widgets (title,
    /// status line, toolbar sensitivity) when this pane is active.
    pub fn connect_nav_notify<F: Fn() + 'static>(&self, f: F) {
        self.nav_notify.borrow_mut().push(Box::new(f));
    }

    fn update_nav_sensitivity(&self) {
        self.back_button.set_sensitive(self.can_back());
        self.forward_button.set_sensitive(self.can_forward());
    }
}

fn nav_button(icon: &str, tooltip: &str) -> gtk4::Button {
    gtk4::Button::builder()
        .icon_name(icon)
        .tooltip_text(tooltip)
        .build()
}
