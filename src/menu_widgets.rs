// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// Hand-built menu rows for the context popover. GtkPopoverMenu is
// avoided on purpose: it embeds an internal GtkScrolledWindow that
// produces scrollbars whenever the compositor clamps the popover's
// allocation. Plain buttons inside a GtkPopover can never scroll —
// the menu is always fully visible. Submenus are real nested
// popovers attached to their entry row, so each menu sizes to its
// own content like a classic cascading menu.

use gtk4::gio;
use gtk4::prelude::*;

/// A submenu: its vertical page box plus the popovers to close when
/// one of its entries fires (the submenu itself and the outer menu).
pub struct SubMenu {
    page: gtk4::Box,
    closers: Vec<gtk4::Popover>,
}

/// Fresh vertical menu page.
pub fn page() -> gtk4::Box {
    gtk4::Box::builder()
        .orientation(gtk4::Orientation::Vertical)
        .spacing(0)
        .margin_top(4)
        .margin_bottom(4)
        .build()
}

/// Plain menu entry; activation closes `popover` and runs `run`.
pub fn item(page: &gtk4::Box, popover: &gtk4::Popover, label: &str, run: impl Fn() + 'static) {
    item_multi(page, std::slice::from_ref(popover), label, run);
}

/// Toggle-style entry with a check mark when `active`.
pub fn check_item(
    page: &gtk4::Box,
    popover: &gtk4::Popover,
    label: &str,
    active: bool,
    run: impl Fn() + 'static,
) {
    check_item_multi(page, std::slice::from_ref(popover), label, active, run);
}

/// Submenu entry: appends a "label ▸" row to `parent` and returns the
/// SubMenu to fill. The nested popover opens to the right of the row
/// (GTK flips it left when there is no room).
pub fn submenu(parent: &gtk4::Box, outer: &gtk4::Popover, label: &str) -> SubMenu {
    let row = row_widget(label, false, Some("go-next-symbolic"), None);
    parent.append(&row);

    let pop = gtk4::Popover::builder()
        .has_arrow(false)
        .position(gtk4::PositionType::Right)
        .build();
    pop.set_parent(&row);
    let page = page();
    pop.set_child(Some(&page));

    let pop2 = pop.clone();
    row.connect_clicked(move |_| pop2.popup());

    // Cascading menus open on hover: entering the row pops up its
    // nested popover and closes any sibling submenu still open, so
    // only one cascade is visible at a time.
    let motion = gtk4::EventControllerMotion::new();
    let pop3 = pop.clone();
    let row2 = row.clone();
    motion.connect_enter(move |_, _, _| {
        close_sibling_submenus(&row2);
        pop3.popup();
    });
    row.add_controller(motion);

    // The row owns the nested popover as a child widget. When the row
    // is torn down (menu rebuilt on the next click, tab closed,
    // window destroyed) it must detach the popover first — GTK warns
    // otherwise about finalizing a widget that still has children.
    let pop3 = pop.clone();
    row.connect_unrealize(move |_| pop3.unparent());

    SubMenu {
        closers: vec![pop.clone(), outer.clone()],
        page,
    }
}

impl SubMenu {
    /// Entry inside this submenu — closes submenu AND main menu.
    pub fn item(&self, label: &str, run: impl Fn() + 'static) {
        item_multi(&self.page, &self.closers, label, run);
    }

    /// Check entry inside this submenu — closes submenu AND main menu.
    pub fn check_item(&self, label: &str, active: bool, run: impl Fn() + 'static) {
        check_item_multi(&self.page, &self.closers, label, active, run);
    }

    /// Entry with an app icon in the left slot (the "Open With" app
    /// list) — closes submenu AND main menu.
    pub fn icon_item(&self, icon: Option<&gio::Icon>, label: &str, run: impl Fn() + 'static) {
        let button = row_widget(label, false, None, icon);
        let closers = self.closers.clone();
        button.connect_clicked(move |_| {
            for p in &closers {
                p.popdown();
            }
            run();
        });
        let motion = gtk4::EventControllerMotion::new();
        let page = self.page.clone();
        motion.connect_enter(move |_, _, _| close_submenus_in(&page));
        button.add_controller(motion);
        self.page.append(&button);
    }

    /// Horizontal divider inside this submenu.
    pub fn separator(&self) {
        separator(&self.page);
    }
}

/// Horizontal section divider between groups of entries.
pub fn separator(page: &gtk4::Box) {
    page.append(&gtk4::Separator::new(gtk4::Orientation::Horizontal));
}

/// Popdown every nested submenu popover in `page` — entering a plain
/// menu entry ends any open cascade on that page. A submenu popover
/// is parented to its entry row, so it shows up as a child of a row.
fn close_submenus_in(page: &gtk4::Box) {
    let mut sib = page.first_child();
    while let Some(s) = sib {
        let mut child = s.first_child();
        while let Some(c) = child {
            if let Ok(pop) = c.clone().downcast::<gtk4::Popover>() {
                pop.popdown();
            }
            child = c.next_sibling();
        }
        sib = s.next_sibling();
    }
}

/// Same as `close_submenus_in` but skips `row` itself — entering a
/// submenu row closes the other cascades while its own pops up.
fn close_sibling_submenus(row: &gtk4::Button) {
    let Some(page) = row.parent() else { return };
    let Ok(page) = page.downcast::<gtk4::Box>() else {
        return;
    };
    let mut sib = page.first_child();
    while let Some(s) = sib {
        if s != *row.upcast_ref::<gtk4::Widget>() {
            let mut child = s.first_child();
            while let Some(c) = child {
                if let Ok(pop) = c.clone().downcast::<gtk4::Popover>() {
                    pop.popdown();
                }
                child = c.next_sibling();
            }
        }
        sib = s.next_sibling();
    }
}

/// Row that closes every popover in `closers`, then runs.
fn item_multi(page: &gtk4::Box, closers: &[gtk4::Popover], label: &str, run: impl Fn() + 'static) {
    let button = row_widget(label, false, None, None);
    let closers: Vec<gtk4::Popover> = closers.to_vec();
    button.connect_clicked(move |_| {
        for p in &closers {
            p.popdown();
        }
        run();
    });
    // Hovering a plain entry closes any open submenu on this page —
    // classic cascading-menu behaviour.
    let motion = gtk4::EventControllerMotion::new();
    let page2 = page.clone();
    motion.connect_enter(move |_, _, _| close_submenus_in(&page2));
    button.add_controller(motion);
    page.append(&button);
}

/// Check-mark row variant of item_multi.
fn check_item_multi(
    page: &gtk4::Box,
    closers: &[gtk4::Popover],
    label: &str,
    active: bool,
    run: impl Fn() + 'static,
) {
    let button = row_widget(label, active, None, None);
    let closers: Vec<gtk4::Popover> = closers.to_vec();
    button.connect_clicked(move |_| {
        for p in &closers {
            p.popdown();
        }
        run();
    });
    let motion = gtk4::EventControllerMotion::new();
    let page2 = page.clone();
    motion.connect_enter(move |_, _, _| close_submenus_in(&page2));
    button.add_controller(motion);
    page.append(&button);
}

/// A frameless full-width button styled like a menu row: fixed-size
/// left slot (check mark when `checked`, or `icon` when given —
/// icons win over check marks), label, optional trailing icon
/// (submenu arrow).
fn row_widget(
    label: &str,
    checked: bool,
    trailing: Option<&str>,
    icon: Option<&gio::Icon>,
) -> gtk4::Button {
    let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    row.set_margin_start(8);
    row.set_margin_end(8);

    // Left slot always reserves the same width so labels line up.
    let slot = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    slot.set_size_request(16, 16);
    let image = match icon {
        Some(icon) => gtk4::Image::from_gicon(icon),
        None => gtk4::Image::from_icon_name("object-select-symbolic"),
    };
    image.set_pixel_size(16);
    image.set_visible(icon.is_some() || checked);
    slot.append(&image);
    row.append(&slot);

    let text = gtk4::Label::builder()
        .label(label)
        .xalign(0.0)
        .hexpand(true)
        .ellipsize(gtk4::pango::EllipsizeMode::End)
        .build();
    row.append(&text);

    if let Some(icon) = trailing {
        row.append(&gtk4::Image::from_icon_name(icon));
    }

    let button = gtk4::Button::builder().has_frame(false).build();
    button.set_child(Some(&row));
    button
}
