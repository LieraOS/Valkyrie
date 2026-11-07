// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// Preferences dialog: a StackSidebar page switcher on the left, the
// selected settings page on the right — the layout stays readable no
// matter how many sections accumulate. Pages write straight into
// GSettings and apply the live-relevant keys (view mode, hidden
// files, sorting) to all open tabs.

use std::rc::Rc;

use gtk4::prelude::*;

use crate::i18n::tr;
use crate::view_model::SortField;
use crate::window::ValkyrieWindow;

/// One labeled form row: title on the left, control on the right.
fn row(page: &gtk4::Box, title: &str, control: &gtk4::Widget) {
    let line = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);
    line.append(
        &gtk4::Label::builder()
            .label(title)
            .xalign(0.0)
            .hexpand(true)
            .build(),
    );
    line.append(control);
    page.append(&line);
}

/// A fresh settings page: vertical, padded box.
fn page() -> gtk4::Box {
    let page = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
    page.set_margin_top(16);
    page.set_margin_bottom(16);
    page.set_margin_start(16);
    page.set_margin_end(16);
    page
}

/// Small gray helper text under a control.
fn hint_label(page: &gtk4::Box, text: &str) {
    let hint = gtk4::Label::builder()
        .label(text)
        .xalign(0.0)
        .wrap(true)
        .build();
    hint.add_css_class("dim-label");
    page.append(&hint);
}

/// A dropdown whose strings come from `labels`; `selected` pre-picks.
fn dropdown(labels: &[String], selected: u32) -> gtk4::DropDown {
    let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
    let list = gtk4::StringList::new(&refs);
    let dd = gtk4::DropDown::new(Some(list), gtk4::Expression::NONE);
    dd.set_selected(selected.min(labels.len().saturating_sub(1) as u32));
    dd
}

/// Build and present the modal preferences window.
pub fn show(this: &Rc<ValkyrieWindow>) {
    let dialog = gtk4::Window::builder()
        .transient_for(&this.widget)
        .modal(true)
        .title(tr("Preferences"))
        .default_width(640)
        .default_height(420)
        .build();

    // Every page scrolls for itself — a long page (Cloud/WebDAV with
    // many sync pairs) never pushes the sidebar or window size.
    let scroll = |page: gtk4::Box| {
        gtk4::ScrolledWindow::builder()
            .child(&page)
            .vexpand(true)
            .propagate_natural_height(true)
            .build()
    };

    let stack = gtk4::Stack::new();
    stack.set_hexpand(true);
    stack.add_titled(&scroll(view_page(this)), Some("view"), &tr("View"));
    stack.add_titled(&scroll(sorting_page(this)), Some("sorting"), &tr("Sorting"));
    stack.add_titled(&scroll(terminal_page()), Some("terminal"), &tr("Terminal"));
    stack.add_titled(
        &scroll(thumbnails_page()),
        Some("thumbnails"),
        &tr("Thumbnails"),
    );
    stack.add_titled(
        &scroll(operations_page()),
        Some("operations"),
        &tr("File operations"),
    );
    stack.add_titled(&scroll(delete_page(this)), Some("delete"), &tr("Delete"));
    stack.add_titled(
        &scroll(context_menu_page()),
        Some("contextmenu"),
        &tr("Context menu"),
    );
    stack.add_titled(
        &scroll(nextcloud_page()),
        Some("nextcloud"),
        &tr("Cloud (WebDAV)"),
    );

    let sidebar = gtk4::StackSidebar::new();
    sidebar.set_stack(&stack);

    let layout = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    layout.append(&sidebar);
    layout.append(&gtk4::Separator::new(gtk4::Orientation::Vertical));
    layout.append(&stack);

    dialog.set_child(Some(&layout));
    dialog.present();
}

// ===================== View page =====================

fn view_page(this: &Rc<ValkyrieWindow>) -> gtk4::Box {
    let page = page();

    // Default view: icon grid or details list. Applied to every open
    // tab immediately — the persisted key seeds new windows/tabs.
    let view_mode = dropdown(
        &[tr("Icon View"), tr("List View")],
        if crate::settings::view_mode() == "icons" {
            0
        } else {
            1
        },
    );
    {
        let weak = Rc::downgrade(this);
        view_mode.connect_selected_notify(move |dd| {
            let icons = dd.selected() == 0;
            crate::settings::set_view_mode(if icons { "icons" } else { "list" });
            if let Some(this) = weak.upgrade() {
                this.for_each_tab(|tab| tab.set_icon_mode(icons));
            }
        });
    }
    row(&page, &tr("Default View"), view_mode.upcast_ref());

    // Hidden files: same GSettings key the menu toggle uses, applied
    // to all tabs at once.
    let hidden = gtk4::Switch::builder()
        .active(crate::settings::show_hidden())
        .valign(gtk4::Align::Center)
        .build();
    {
        let weak = Rc::downgrade(this);
        hidden.connect_active_notify(move |switch| {
            let show = switch.is_active();
            crate::settings::set_show_hidden(show);
            if let Some(this) = weak.upgrade() {
                this.for_each_tab(|tab| tab.set_show_hidden(show));
            }
        });
    }
    row(&page, &tr("Show Hidden Files"), hidden.upcast_ref());

    // Default icon size for new locations; per-folder zoom metadata
    // and the status-bar slider still take precedence at runtime.
    let icon_size = gtk4::SpinButton::with_range(16.0, 320.0, 8.0);
    icon_size.set_value(crate::settings::icon_size() as f64);
    icon_size.connect_value_changed(|spin| {
        crate::settings::set_icon_size(spin.value() as i32);
    });
    row(&page, &tr("Default Icon Size"), icon_size.upcast_ref());

    page
}

// ===================== Sorting page =====================

/// One checkbox line in the context-menu page: checked = visible.
fn ctx_row(page: &gtk4::Box, id: &str, label: &str) {
    let cb = gtk4::CheckButton::with_label(label);
    cb.set_active(crate::settings::menu_entry_visible(id));
    let id = id.to_string();
    cb.connect_toggled(move |cb| {
        let mut hidden = crate::settings::context_menu_hidden();
        if cb.is_active() {
            hidden.retain(|h| h != &id);
        } else if !hidden.contains(&id) {
            hidden.push(id.clone());
        }
        crate::settings::set_context_menu_hidden(&hidden);
    });
    page.append(&cb);
}

fn context_menu_page() -> gtk4::Box {
    let page = page();
    let h = gtk4::Label::builder()
        .label(tr("On files and folders"))
        .xalign(0.0)
        .build();
    h.add_css_class("heading");
    page.append(&h);
    for (id, label) in [
        ("open-tab", tr("Open in New Tab")),
        ("open-window", tr("Open in New Window")),
        ("open-with", tr("Open With")),
        ("background", tr("Set as Background")),
        ("admin", tr("Open as Administrator")),
        ("terminal", tr("Open in Terminal")),
        ("terminal-panel", tr("Open Terminal Panel")),
        ("cut", tr("Cut")),
        ("copy", tr("Copy")),
        ("copy-path", tr("Copy Path")),
        ("cloud-upload", tr("Upload/Sync with Cloud")),
        ("rename", tr("Rename")),
        ("bookmark", tr("Add/Remove Bookmark")),
        ("extract", tr("Extract Here")),
        ("compress", tr("Compress…")),
        ("trash", tr("Move to Trash")),
        ("delete-permanent", tr("Delete Permanently")),
    ] {
        ctx_row(&page, id, &label);
    }
    let h2 = gtk4::Label::builder()
        .label(tr("On empty space"))
        .xalign(0.0)
        .margin_top(12)
        .build();
    h2.add_css_class("heading");
    page.append(&h2);
    for (id, label) in [
        ("new", tr("New")),
        ("paste", tr("Paste")),
        ("select-all", tr("Select All")),
        ("arrange", tr("Arrange Items")),
        ("show-hidden", tr("Show Hidden Files")),
    ] {
        ctx_row(&page, id, &label);
    }
    hint_label(&page, &tr("\"Open\" and \"Properties\" always stay. Entries apply wherever they exist — file rows and empty space alike."));
    page
}

fn delete_page(this: &Rc<ValkyrieWindow>) -> gtk4::Box {
    let page = page();

    // Secure delete: overwrite file contents before unlinking.
    // Enabling on unreliable media (SSD, COW filesystems) raises a
    // one-time warning — never per deletion, that would be noise.
    let wipe = gtk4::Switch::builder()
        .active(crate::settings::secure_delete())
        .valign(gtk4::Align::Center)
        .build();
    {
        let weak = Rc::downgrade(this);
        let wipe_state = wipe.clone();
        wipe.connect_active_notify(move |switch| {
            let on = switch.is_active();
            if on {
                let home = std::env::var("HOME").unwrap_or_else(|_| "/".to_string());
                if !crate::secure_wipe::medium_is_reliable(std::path::Path::new(&home)) {
                    let d = gtk4::AlertDialog::builder()
                        .message(tr("Limited effectiveness on this storage"))
                        .detail(tr("This medium (SSD or copy-on-write filesystem) cannot guarantee that overwritten data is unrecoverable — wear leveling and snapshots may keep stale copies. The feature stays enabled, but treat it as best-effort, not a security promise."))
                        .buttons([tr("Understood")])
                        .build();
                    if let Some(this) = weak.upgrade() {
                        d.show(Some(&this.widget));
                    }
                }
            }
            let _ = &wipe_state;
            crate::settings::set_secure_delete(on);
        });
    }
    row(
        &page,
        &tr("Overwrite files when deleting permanently"),
        wipe.upcast_ref(),
    );

    let passes = gtk4::SpinButton::with_range(1.0, 7.0, 1.0);
    passes.set_value(crate::settings::secure_delete_passes() as f64);
    passes.set_sensitive(crate::settings::secure_delete());
    passes.connect_value_changed(|spin| {
        crate::settings::set_secure_delete_passes(spin.value() as i32);
    });
    {
        let passes = passes.clone();
        wipe.connect_active_notify(move |s| passes.set_sensitive(s.is_active()));
    }
    row(&page, &tr("Overwrite passes"), passes.upcast_ref());

    hint_label(
        &page,
        &tr("Only applies to permanent deletion (Shift+Delete). The trash stays untouched."),
    );
    page
}

fn sorting_page(this: &Rc<ValkyrieWindow>) -> gtk4::Box {
    let page = page();

    // Default sort field — applied to every open tab like the view
    // options, so the dialog feels live instead of deferred.
    let fields = [
        SortField::Name,
        SortField::Size,
        SortField::Type,
        SortField::Modified,
    ];
    let current = SortField::from_str(&crate::settings::sort_field());
    let pos = fields.iter().position(|f| *f == current).unwrap_or(0) as u32;
    let sort_field = dropdown(
        &[tr("Name"), tr("Size"), tr("Type"), tr("Modification Date")],
        pos,
    );
    {
        let weak = Rc::downgrade(this);
        sort_field.connect_selected_notify(move |dd| {
            let field = fields[dd.selected() as usize];
            crate::settings::set_sort_field(field.as_str());
            if let Some(this) = weak.upgrade() {
                this.for_each_tab(|tab| tab.set_sort_field(field));
            }
        });
    }
    row(&page, &tr("Default Sort Field"), sort_field.upcast_ref());

    // Default direction; directories stay pinned to the top.
    let descending = gtk4::Switch::builder()
        .active(crate::settings::sort_descending())
        .valign(gtk4::Align::Center)
        .build();
    {
        let weak = Rc::downgrade(this);
        descending.connect_active_notify(move |switch| {
            let desc = switch.is_active();
            crate::settings::set_sort_descending(desc);
            if let Some(this) = weak.upgrade() {
                this.for_each_tab(|tab| tab.set_sort_descending(desc));
            }
        });
    }
    row(&page, &tr("Sort Descending"), descending.upcast_ref());

    page
}

// ===================== Terminal page =====================

fn terminal_page() -> gtk4::Box {
    let page = page();

    // Which emulator "Open in Terminal" launches: Automatic runs the
    // built-in detection cascade; every emulator found on PATH is a
    // selectable entry. A configured custom command joins the list so
    // the current value stays visible and selectable.
    let mut names: Vec<String> = crate::terminal::TERMINAL_CANDIDATES
        .iter()
        .filter(|name| crate::terminal::program_exists(name))
        .map(|s| s.to_string())
        .collect();
    let configured = crate::settings::terminal_command();
    if !configured.is_empty() && !names.contains(&configured) {
        names.push(configured.clone());
    }
    let mut labels = vec![tr("Automatic detection")];
    labels.extend(names.iter().cloned());
    let selected = names
        .iter()
        .position(|n| *n == configured)
        .map(|p| p as u32 + 1)
        .unwrap_or(0);
    let terminal = dropdown(&labels, selected);
    {
        let names = names.clone();
        terminal.connect_selected_notify(move |dd| {
            let sel = dd.selected() as usize;
            let value = sel.checked_sub(1).and_then(|i| names.get(i));
            crate::settings::set_terminal_command(value.map(String::as_str).unwrap_or(""));
        });
    }
    row(&page, &tr("Terminal Emulator"), terminal.upcast_ref());

    hint_label(
        &page,
        &tr("Used by \"Open Terminal Here\" — Automatic picks the first installed emulator."),
    );

    page
}

// ===================== File operations page =====================

/// Copy/move behaviour: whether file payloads are copied by a worker
/// pool, and with how many threads. The SpinButton is capped at the
/// machine's CPU core count and only enabled when parallel copying
/// is on — sequential is the safe default for slow media.
fn operations_page() -> gtk4::Box {
    let page = page();

    let cores = std::thread::available_parallelism()
        .map(|n| n.get() as i32)
        .unwrap_or(4);

    let parallel = gtk4::CheckButton::with_label(&tr("Copy files in parallel"));
    parallel.set_active(crate::settings::copy_parallel());
    page.append(&parallel);

    let workers = gtk4::SpinButton::with_range(1.0, f64::from(cores), 1.0);
    workers.set_value(f64::from(crate::settings::copy_workers()));
    workers.set_sensitive(parallel.is_active());
    workers.connect_value_changed(|s| {
        crate::settings::set_copy_workers(s.value() as i32);
    });
    row(&page, &tr("Copy workers"), workers.upcast_ref());

    let spin_ref = workers.clone();
    parallel.connect_toggled(move |cb| {
        crate::settings::set_copy_parallel(cb.is_active());
        spin_ref.set_sensitive(cb.is_active());
    });

    hint_label(
        &page,
        &tr("This machine has %d CPU cores — 2–4 workers are usually optimal on SSDs; on hard disks or network drives parallel copying can be slower.")
            .replace("%d", &cores.to_string()),
    );

    page
}

// ===================== Cloud (WebDAV) page =====================

fn nextcloud_page() -> gtk4::Box {
    let page = page();

    // Server and username persist in GSettings; the app password goes
    // straight into the encrypted system keyring (secrets.rs) and is
    // never written to a settings key or file.
    let server = gtk4::Entry::builder()
        .text(crate::settings::nextcloud_server())
        .placeholder_text("cloud.example.com")
        .build();
    server.connect_changed(|e| {
        crate::settings::set_nextcloud_server(&e.text());
    });
    row(&page, &tr("Server"), server.upcast_ref());
    hint_label(
        &page,
        &tr("Nextcloud/ownCloud: host only is enough — the endpoint /remote.php/dav/files/<user> is assumed. Other WebDAV providers: enter the full URL including the path."),
    );

    let user = gtk4::Entry::builder()
        .text(crate::settings::nextcloud_user())
        .build();
    user.connect_changed(|e| {
        crate::settings::set_nextcloud_user(&e.text());
    });
    row(&page, &tr("Username"), user.upcast_ref());

    // Password row: entry stays empty even when a password exists —
    // the status line below reports the stored state instead.
    let pass = gtk4::PasswordEntry::builder().show_peek_icon(true).build();
    let status = gtk4::Label::builder().xalign(0.0).wrap(true).build();
    status.add_css_class("dim-label");
    let refresh_status = {
        let status = status.clone();
        move || {
            let server = crate::settings::nextcloud_server();
            let user = crate::settings::nextcloud_user();
            let text = if server.is_empty() || user.is_empty() {
                tr("Enter server and username first")
            } else if crate::secrets::has(&crate::nextcloud::host_key(&server), &user) {
                tr("Password stored — encrypted in the system keyring")
            } else {
                tr("No password stored yet")
            };
            status.set_text(&text);
        }
    };
    refresh_status();
    {
        let refresh = refresh_status.clone();
        pass.connect_changed(move |e| {
            let text = e.text().to_string();
            if text.is_empty() {
                return; // untouched field keeps the stored password
            }
            let server = crate::settings::nextcloud_server();
            let user = crate::settings::nextcloud_user();
            let _ = crate::secrets::store(&crate::nextcloud::host_key(&server), &user, &text);
            refresh();
        });
    }
    row(&page, &tr("App Password"), pass.upcast_ref());

    // Status line + a way to forget the stored password entirely.
    let status_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    status.set_hexpand(true);
    status_row.append(&status);
    let forget = gtk4::Button::with_label(&tr("Forget Password"));
    forget.add_css_class("flat");
    {
        let refresh = refresh_status.clone();
        forget.connect_clicked(move |_| {
            let server = crate::settings::nextcloud_server();
            let user = crate::settings::nextcloud_user();
            crate::secrets::clear(&crate::nextcloud::host_key(&server), &user);
            refresh();
        });
    }
    status_row.append(&forget);
    page.append(&status_row);
    hint_label(
        &page,
        &tr("The password is stored encrypted in the system keyring, never as plain text."),
    );

    // Synced folder pairs live in their own module — the same editor
    // dialog is also reachable from a folder's context menu.
    crate::nextcloud::folders_ui::section(&page);

    page
}

/// "Thumbnails" page: per content type (images, videos, PDFs) a
/// mode dropdown (Always / Only up to size / Never) plus an MB
/// spin button that only activates in limit mode.
fn thumbnails_page() -> gtk4::Box {
    let page = page();
    hint_label(
        &page,
        &tr("Generate preview thumbnails per file type. In \"Up to\" mode files above the size limit show the plain type icon instead."),
    );
    for (cat, label) in [
        ("images", tr("Images")),
        ("videos", tr("Videos")),
        ("pdfs", tr("PDFs")),
    ] {
        thumb_row(&page, cat, &label);
    }
    page
}

/// One thumbnail-policy row: mode dropdown + size spin, wired to the
/// thumbnails-<cat>-mode / thumbnails-<cat>-max-mb GSettings keys.
fn thumb_row(page: &gtk4::Box, cat: &str, label: &str) {
    let modes = ["always", "limit", "never"];
    let initial = modes
        .iter()
        .position(|m| *m == crate::settings::thumbnail_mode(cat))
        .unwrap_or(0) as u32;
    let mode_dd = dropdown(&[tr("Always"), tr("Only up to"), tr("Never")], initial);
    let spin = gtk4::SpinButton::with_range(1.0, 10240.0, 1.0);
    spin.set_value(crate::settings::thumbnail_max_mb(cat) as f64);
    spin.set_sensitive(initial == 1);
    let suffix = gtk4::Label::new(Some("MB"));

    let cat_owned = cat.to_string();
    {
        let spin = spin.clone();
        mode_dd.connect_selected_notify(move |dd| {
            let mode = modes[dd.selected() as usize];
            crate::settings::set_thumbnail_mode(&cat_owned, mode);
            spin.set_sensitive(mode == "limit");
        });
    }
    let cat_owned = cat.to_string();
    spin.connect_value_changed(move |s| {
        crate::settings::set_thumbnail_max_mb(&cat_owned, s.value() as u32);
    });

    let line = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    line.append(&mode_dd);
    line.append(&spin);
    line.append(&suffix);
    row(page, label, &line.upcast());
}
