// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// Window-scoped "win.*" actions, accelerators and the About dialog.
// Split out of window.rs so each file stays focused; everything here
// operates on the pub(crate) API of ValkyrieWindow.

use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{gio, glib};

use crate::i18n::tr;
use crate::operations;
use crate::properties;
use crate::view_model::SortField;
use crate::window::ValkyrieWindow;

/// Handles to the stateful menu actions, kept for resyncing their
/// visible state (radio/checkbox marks) against the active tab.
pub(crate) struct MenuActions {
    pub view_mode: gio::SimpleAction,
    pub arrange: gio::SimpleAction,
    pub arrange_desc: gio::SimpleAction,
    pub show_hidden: gio::SimpleAction,
    /// Window-level toggle for the embedded terminal panel — state
    /// tracks the persisted GSettings flag, not a per-tab property.
    pub terminal_panel: gio::SimpleAction,
    /// Per-tab split view (F3) — resynced to the active tab like the
    /// other per-tab view options.
    pub split_view: gio::SimpleAction,
}

/// Create the stateful actions backing the View menu.
pub(crate) fn menu_actions() -> MenuActions {
    MenuActions {
        view_mode: stateful_string("view-mode", "icons"),
        arrange: stateful_string("arrange", "name"),
        arrange_desc: stateful_bool("arrange-desc", false),
        show_hidden: stateful_bool("show-hidden", false),
        terminal_panel: stateful_bool("terminal-panel", crate::settings::terminal_panel_visible()),
        split_view: stateful_bool("split-view", false),
    }
}

/// Window-scoped actions and accelerators backing the menubar.
pub(crate) fn install(this: &Rc<ValkyrieWindow>) {
    let group = gio::SimpleActionGroup::new();

    // Register the pre-created stateful actions first.
    group.add_action(&this.menu_actions.view_mode);
    group.add_action(&this.menu_actions.arrange);
    group.add_action(&this.menu_actions.arrange_desc);
    group.add_action(&this.menu_actions.show_hidden);
    group.add_action(&this.menu_actions.terminal_panel);
    group.add_action(&this.menu_actions.split_view);

    // --- File ---
    action(&group, "new-tab", this, |this| this.open_tab(None));
    action(&group, "new-window", this, |this| {
        let location = this.active_tab().and_then(|t| t.location());
        if let Some(app) = this.widget.application() {
            let window = ValkyrieWindow::new(&app, location.as_ref());
            window.widget.present();
        }
    });
    action(&group, "new-folder", this, |this| {
        if let Some(tab) = this.active_tab() {
            tab.create_folder();
        }
    });
    action(&group, "new-document", this, |this| {
        if let Some(tab) = this.active_tab() {
            tab.create_document();
        }
    });
    action(&group, "properties", this, |this| {
        if let Some(dir) = this.active_tab().and_then(|t| t.location()) {
            properties::show(&this.widget, &dir);
        }
    });
    action(&group, "quit", this, |this| {
        if let Some(app) = this.widget.application() {
            app.quit();
        }
    });

    // --- Edit ---
    action(&group, "cut", this, |this| {
        if let Some(tab) = this.active_tab() {
            let files = tab.selected_files();
            if !files.is_empty() {
                operations::set_clipboard(&files, true);
            }
        }
    });
    action(&group, "copy", this, |this| {
        if let Some(tab) = this.active_tab() {
            let files = tab.selected_files();
            if !files.is_empty() {
                operations::set_clipboard(&files, false);
            }
        }
    });
    action(&group, "paste", this, |this| {
        if let Some(dir) = this.active_tab().and_then(|t| t.location()) {
            operations::paste(&dir, this.widget.upcast_ref());
        }
    });
    // Cross-pane transfer: selection of the focused pane lands in the
    // other pane's directory. No-op while the tab is not split.
    action(&group, "copy-to-other-pane", this, |this| {
        if let Some(tab) = this.active_tab() {
            let files = tab.selected_files();
            if let Some(dest) = tab.other_pane_location() {
                if !files.is_empty() {
                    let entry = if false {
                        None
                    } else {
                        Some(crate::undo::Entry::Copy {
                            sources: files.clone(),
                            dest_dir: dest.clone(),
                        })
                    };
                    crate::progress::copy_move(this.widget.upcast_ref(), files, dest, false);
                    if let Some(e) = entry {
                        this.journal.push(e);
                    }
                }
            }
        }
    });
    action(&group, "move-to-other-pane", this, |this| {
        if let Some(tab) = this.active_tab() {
            let files = tab.selected_files();
            if let Some(dest) = tab.other_pane_location() {
                if !files.is_empty() {
                    let originals = files.clone();
                    let dest_dir = dest.clone();
                    crate::progress::copy_move(this.widget.upcast_ref(), files, dest, true);
                    this.journal.push(crate::undo::Entry::Move {
                        originals,
                        dest_dir,
                    });
                }
            }
        }
    });
    action(&group, "select-all", this, |this| {
        if let Some(tab) = this.active_tab() {
            tab.select_all();
        }
    });
    action(&group, "rename", this, |this| {
        if let Some(tab) = this.active_tab() {
            // Rename acts on a single entry — the first selected one.
            let files = tab.selected_files();
            if let Some(file) = files.first() {
                crate::rename::rename(this.widget.upcast_ref(), file);
            }
        }
    });
    action(&group, "search", this, |this| {
        if let Some(tab) = this.active_tab() {
            tab.search_ui.activate();
        }
    });
    action(&group, "undo", this, |this| {
        if let Some(label) = this.journal.undo() {
            let d = gtk4::AlertDialog::builder()
                .message(&label)
                .buttons([crate::i18n::tr("OK")])
                .build();
            if let Some(win) = this.widget.root().and_downcast::<gtk4::Window>() {
                d.show(Some(&win));
            }
        }
    });
    action(&group, "preferences", this, |this| {
        crate::settings_dialog::show(this);
    });
    action(&group, "trash", this, |this| {
        if let Some(tab) = this.active_tab() {
            let w = this.widget.clone().upcast::<gtk4::Widget>();
            let files = tab.selected_files();
            let uris: Vec<String> = files.iter().map(|f| f.uri().to_string()).collect();
            for f in &files {
                operations::trash(f, &w);
            }
            this.journal.push(crate::undo::Entry::Trash { uris });
        }
    });
    action(&group, "delete-permanently", this, |this| {
        if let Some(tab) = this.active_tab() {
            let w = this.widget.clone().upcast::<gtk4::Widget>();
            let files = tab.selected_files();
            operations::delete_permanently(&w, &files);
        }
    });

    // --- View ---
    let weak = Rc::downgrade(this);
    this.menu_actions
        .view_mode
        .connect_activate(move |action, param| {
            let Some(mode) = param.and_then(|p| p.str()) else {
                return;
            };
            action.set_state(&mode.to_variant());
            if let Some(this) = weak.upgrade() {
                if let Some(tab) = this.active_tab() {
                    tab.set_view_mode(crate::file_view::ViewMode::from_str(mode));
                }
            }
        });
    let weak = Rc::downgrade(this);
    this.menu_actions
        .arrange
        .connect_activate(move |action, param| {
            let Some(field) = param.and_then(|p| p.str()).map(SortField::from_str) else {
                return;
            };
            action.set_state(&field.as_str().to_variant());
            if let Some(this) = weak.upgrade() {
                if let Some(tab) = this.active_tab() {
                    tab.set_sort_field(field);
                }
            }
        });
    // Per-tab split view toggle (F3): flips the action state and the
    // active tab's secondary pane together.
    let weak = Rc::downgrade(this);
    this.menu_actions
        .split_view
        .connect_activate(move |action, _| {
            let on = !action
                .state()
                .and_then(|s| s.get::<bool>())
                .unwrap_or(false);
            action.set_state(&on.to_variant());
            if let Some(this) = weak.upgrade() {
                if let Some(tab) = this.active_tab() {
                    tab.set_split(on);
                }
            }
        });
    let weak = Rc::downgrade(this);
    this.menu_actions
        .terminal_panel
        .connect_activate(move |action, _| {
            let on = !action
                .state()
                .and_then(|s| s.get::<bool>())
                .unwrap_or(false);
            action.set_state(&on.to_variant());
            if let Some(this) = weak.upgrade() {
                this.set_terminal_panel(on);
            }
        });
    let weak = Rc::downgrade(this);
    this.menu_actions
        .arrange_desc
        .connect_activate(move |action, _| {
            let desc = !action
                .state()
                .and_then(|s| s.get::<bool>())
                .unwrap_or(false);
            action.set_state(&desc.to_variant());
            if let Some(this) = weak.upgrade() {
                if let Some(tab) = this.active_tab() {
                    tab.set_sort_descending(desc);
                }
            }
        });
    let weak = Rc::downgrade(this);
    this.menu_actions
        .show_hidden
        .connect_activate(move |action, _| {
            let show = !action
                .state()
                .and_then(|s| s.get::<bool>())
                .unwrap_or(false);
            action.set_state(&show.to_variant());
            if let Some(this) = weak.upgrade() {
                if let Some(tab) = this.active_tab() {
                    tab.set_show_hidden(show);
                }
            }
        });
    action(&group, "reload", this, |this| {
        if let Some(tab) = this.active_tab() {
            tab.reload();
        }
    });

    // --- Help ---
    action(&group, "about", this, |this| show_about(&this.widget));

    // --- Tab / location shortcuts ---
    action(&group, "close-tab", this, |this| {
        if this.notebook.n_pages() <= 1 {
            this.widget.close();
        } else if let Some(w) = this.notebook.nth_page(this.notebook.current_page()) {
            this.notebook.detach_tab(&w);
        }
    });
    action(&group, "focus-location", this, |this| {
        if let Some(tab) = this.active_tab() {
            tab.focus_location_entry();
        }
    });
    action(&group, "toggle-hidden", this, |this| {
        if let Some(tab) = this.active_tab() {
            tab.toggle_hidden();
        }
    });

    this.widget.insert_action_group("win", Some(&group));

    if let Some(app) = this.widget.application() {
        app.set_accels_for_action("win.new-tab", &["<Ctrl>t"]);
        app.set_accels_for_action("win.new-window", &["<Ctrl>n"]);
        app.set_accels_for_action("win.close-tab", &["<Ctrl>w"]);
        app.set_accels_for_action("win.focus-location", &["<Ctrl>l"]);
        app.set_accels_for_action("win.new-folder", &["<Ctrl><Shift>n"]);
        app.set_accels_for_action("win.cut", &["<Ctrl>x"]);
        app.set_accels_for_action("win.copy", &["<Ctrl>c"]);
        app.set_accels_for_action("win.paste", &["<Ctrl>v"]);
        app.set_accels_for_action("win.select-all", &["<Ctrl>a"]);
        app.set_accels_for_action("win.trash", &["Delete"]);
        app.set_accels_for_action("win.delete-permanently", &["<Shift>Delete"]);
        app.set_accels_for_action("win.rename", &["F2"]);
        app.set_accels_for_action("win.undo", &["<Ctrl>z"]);
        app.set_accels_for_action("win.search", &["<Ctrl>f"]);
        app.set_accels_for_action("win.preferences", &["<Ctrl>comma"]);
        app.set_accels_for_action("win.toggle-hidden", &["<Ctrl>h"]);
        app.set_accels_for_action("win.reload", &["F5", "<Ctrl>r"]);
        app.set_accels_for_action("win.terminal-panel", &["F4"]);
        app.set_accels_for_action("win.split-view", &["F3"]);
        app.set_accels_for_action("win.view-mode::icons", &["<Ctrl>1"]);
        app.set_accels_for_action("win.view-mode::list", &["<Ctrl>2"]);
        app.set_accels_for_action("win.view-mode::compact", &["<Ctrl>3"]);
        app.set_accels_for_action("win.quit", &["<Ctrl>q"]);
    }
}

/// Shortcut for the stateless-action boilerplate: create, wire, add.
fn action(
    group: &gio::SimpleActionGroup,
    name: &str,
    this: &Rc<ValkyrieWindow>,
    run: impl Fn(&Rc<ValkyrieWindow>) + 'static,
) {
    let action = gio::SimpleAction::new(name, None);
    let weak = Rc::downgrade(this);
    action.connect_activate(move |_, _| {
        if let Some(this) = weak.upgrade() {
            run(&this);
        }
    });
    group.add_action(&action);
}

/// Stateful string action (radio group entries like win.arrange::name).
fn stateful_string(name: &str, initial: &str) -> gio::SimpleAction {
    gio::SimpleAction::new_stateful(name, Some(glib::VariantTy::STRING), &initial.to_variant())
}

/// Stateful bool action (checkbox entries).
fn stateful_bool(name: &str, initial: bool) -> gio::SimpleAction {
    gio::SimpleAction::new_stateful(name, None, &initial.to_variant())
}

/// "Help → About Valkyrie": version, authorship and license.
/// The author attribution is a licensing condition (GPLv3 §7(b)) —
/// keep it in sync with the NOTICE/README attribution.
fn show_about(window: &gtk4::ApplicationWindow) {
    let about = gtk4::AboutDialog::builder()
        .modal(true)
        .transient_for(window)
        .program_name("Valkyrie")
        .version(env!("CARGO_PKG_VERSION"))
        .logo_icon_name("valkyrie")
        .comments(tr(
            "The LieraOS file manager.\n\nAttribution notice (GPLv3 §7(b)): \
             The author attribution \"Jörg Schröder\" must be preserved in \
             source file headers and in this About dialog or equivalent \
             legal notices.",
        ))
        .copyright("© 2026 Jörg Schröder")
        .authors(vec!["Jörg Schröder"])
        .website("https://www.lieraos.com")
        .license_type(gtk4::License::Gpl30)
        .build();
    about.present();
}
