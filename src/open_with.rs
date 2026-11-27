// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// "Open With…" support: the context-menu submenu lists the
// applications registered for the file's content type, and the
// "Other Application…" dialog offers every installed app with an
// optional "always use for this type" switch (GAppInfo default
// handler machinery).

use std::rc::Rc;

use gtk4::gio;
use gtk4::prelude::*;

use crate::file_object::FileObject;
use crate::i18n::tr;
use crate::menu_widgets;

/// Fill the "Open With" submenu: one entry per recommended
/// application (icon + name), then a divider and the app-chooser
/// dialog entry. The chooser entry always remains — even for unknown
/// content types the user can pick any installed application.
pub fn fill_submenu(sub: &menu_widgets::SubMenu, obj: &FileObject, parent: &gtk4::Widget) {
    let file = obj.file();
    let content_type = obj.content_type();

    let apps = if content_type.is_empty() {
        Vec::new()
    } else {
        gio::AppInfo::recommended_for_type(&content_type)
    };

    for app in apps.into_iter().filter(|a| a.should_show()) {
        let f = file.clone();
        let parent = parent.clone();
        sub.icon_item(app.icon().as_ref(), &app.display_name(), move || {
            launch(&app, &f, &parent);
        });
    }

    sub.separator();
    let parent = parent.clone();
    sub.item(&tr("Other Application…"), move || {
        chooser_dialog(&parent, &file, &content_type);
    });
}

/// Actually start the application with the file. `AppInfo::launch`
/// runs the desktop entry through the session's launch context so
/// startup notification and focus behave like other launches.
pub fn launch(app: &gio::AppInfo, file: &gio::File, parent: &gtk4::Widget) {
    let ctx = parent.display().app_launch_context();
    if let Err(e) = app.launch(std::slice::from_ref(file), Some(&ctx)) {
        crate::operations::error_dialog(parent, &format!("{e}"));
    }
}

/// The "Other Application…" dialog: every installed, visible
/// application, filterable, with an optional "set as default" switch
/// when the file has a usable content type.
fn chooser_dialog(parent: &gtk4::Widget, file: &gio::File, content_type: &str) {
    let window = parent
        .root()
        .and_then(|r| r.downcast::<gtk4::Window>().ok());
    let dialog = gtk4::Window::builder()
        .modal(false)
        .title(tr("Open With"))
        .default_width(380)
        .default_height(460)
        .build();
    dialog.set_transient_for(window.as_ref());

    let page = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
    page.set_margin_top(12);
    page.set_margin_bottom(12);
    page.set_margin_start(12);
    page.set_margin_end(12);

    let search = gtk4::SearchEntry::new();
    page.append(&search);

    // All visible applications, sorted by name.
    let apps: Vec<gio::AppInfo> = gio::AppInfo::all()
        .into_iter()
        .filter(|a| a.should_show())
        .collect();

    let list = gtk4::ListBox::new();
    list.set_selection_mode(gtk4::SelectionMode::Single);
    list.set_sort_func(|a, b| {
        a.widget_name()
            .to_lowercase()
            .cmp(&b.widget_name().to_lowercase())
            .into()
    });
    // Row → application association is kept explicitly: sorting and
    // filtering change ListBoxRow::index(), so the visible position
    // can never be used to look the app up.
    let mut row_apps: Vec<(gtk4::ListBoxRow, gio::AppInfo)> = Vec::with_capacity(apps.len());
    for app in apps {
        let row_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 10);
        row_box.set_margin_top(8);
        row_box.set_margin_bottom(8);
        row_box.set_margin_start(10);
        row_box.set_margin_end(10);
        let image = match app.icon() {
            Some(icon) => gtk4::Image::from_gicon(&icon),
            None => gtk4::Image::from_icon_name("application-x-executable-symbolic"),
        };
        image.set_pixel_size(24);
        row_box.append(&image);
        row_box.append(
            &gtk4::Label::builder()
                .label(app.display_name())
                .xalign(0.0)
                .hexpand(true)
                .ellipsize(gtk4::pango::EllipsizeMode::End)
                .build(),
        );
        // The widget name doubles as the sort/filter key (lowercased
        // app name).
        let row = gtk4::ListBoxRow::builder().child(&row_box).build();
        row.set_widget_name(&app.display_name());
        list.append(&row);
        row_apps.push((row, app));
    }
    let row_apps = Rc::new(row_apps);

    // Live filter: hide rows whose app name lacks the search text.
    {
        let search = search.clone();
        list.set_filter_func(move |row| {
            let needle = search.text().to_lowercase();
            needle.is_empty() || row.widget_name().to_lowercase().contains(&needle)
        });
    }
    search.connect_search_changed({
        let list = list.clone();
        move |_| list.invalidate_filter()
    });

    let scrolled = gtk4::ScrolledWindow::builder()
        .vexpand(true)
        .child(&list)
        .build();
    page.append(&scrolled);

    // Optional: make the chosen app the handler for this file type.
    let set_default = gtk4::CheckButton::builder()
        .label(format!(
            "{} {}",
            tr("Always use for"),
            file_type_label(content_type)
        ))
        .active(false)
        .build();
    if !content_type.is_empty() {
        page.append(&set_default);
    }

    // Buttons.
    let buttons = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    buttons.set_halign(gtk4::Align::End);
    let cancel = gtk4::Button::with_label(&tr("Cancel"));
    {
        let dialog = dialog.clone();
        cancel.connect_clicked(move |_| dialog.close());
    }
    buttons.append(&cancel);
    let open = gtk4::Button::with_label(&tr("Open"));
    open.add_css_class("suggested-action");
    buttons.append(&open);
    page.append(&buttons);

    let do_open = {
        let dialog = dialog.clone();
        let file = file.clone();
        let ct = content_type.to_string();
        let row_apps = row_apps.clone();
        Rc::new(move |row: &gtk4::ListBoxRow| {
            let Some((_, app)) = row_apps.iter().find(|(r, _)| r == row) else {
                return;
            };
            if set_default.is_active() && !ct.is_empty() {
                let _ = app.set_as_default_for_type(&ct);
                let _ = app.set_as_last_used_for_type(&ct);
            }
            launch(app, &file, dialog.upcast_ref());
            dialog.close();
        }) as Rc<dyn Fn(&gtk4::ListBoxRow)>
    };
    {
        let do_open = do_open.clone();
        let list = list.clone();
        open.connect_clicked(move |_| {
            if let Some(row) = list.selected_row() {
                do_open(&row);
            }
        });
    }
    list.connect_row_activated(move |_, row| do_open(row));

    dialog.set_child(Some(&page));
    dialog.present();
}

/// Short label for the default-app checkbox — the human-readable
/// content type ("PNG image"), or the raw type when unknown.
fn file_type_label(content_type: &str) -> String {
    if content_type.is_empty() {
        return tr("this file type");
    }
    let desc = gio::content_type_get_description(content_type);
    if desc.is_empty() {
        content_type.to_string()
    } else {
        desc.to_string()
    }
}
