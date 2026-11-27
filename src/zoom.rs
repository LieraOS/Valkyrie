// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// Icon zoom: the status-bar slider resizes the icon view live and the
// chosen size is remembered per directory URI in
// ~/.config/ragnar-settings/valkyrie-zoom.ini, so a folder keeps its
// own zoom level across restarts.
//
// History: this used GVFS metadata (metadata::valkyrie-icon-size) —
// dropped because gvfsd-metadata can spin into a runaway loop under
// write load, blocking every app's synchronous metadata D-Bus call.
// A private file has no such failure mode.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use gtk4::prelude::*;
use gtk4::{gio, glib};

use crate::browser_tab::BrowserTab;

thread_local! {
    /// Lazily loaded URI → icon-size map; the file is read once.
    static LEVELS: RefCell<Option<HashMap<String, i32>>> = const { RefCell::new(None) };
    /// Coalesces slider drags into one write per quiet second.
    static SAVE_QUEUED: Cell<bool> = const { Cell::new(false) };
}

/// Persistent store: one `uri<TAB>pixels` line per directory.
fn ini_path() -> PathBuf {
    let mut p = glib::user_config_dir();
    p.push("ragnar-settings");
    p.push("valkyrie-zoom.ini");
    p
}

/// Run `f` against the level map, loading the file on first use.
fn with_levels<R>(f: impl FnOnce(&mut HashMap<String, i32>) -> R) -> R {
    LEVELS.with(|l| {
        let mut guard = l.borrow_mut();
        let map = guard.get_or_insert_with(read_levels);
        f(map)
    })
}

/// Parse the ini back into the map; malformed lines are skipped —
/// a hand-edited or half-synced file must never break navigation.
fn read_levels() -> HashMap<String, i32> {
    let Ok(text) = std::fs::read_to_string(ini_path()) else {
        return HashMap::new();
    };
    text.lines()
        .filter_map(|line| {
            let (uri, px) = line.split_once('\t')?;
            Some((uri.to_string(), px.parse::<i32>().ok()?))
        })
        .collect()
}

/// Flush the map to disk. Writes go through a temp file + rename so
/// a crash mid-write leaves the previous complete file, never a
/// truncated one.
fn write_levels() {
    with_levels(|map| {
        let mut out = String::with_capacity(map.len() * 48);
        for (uri, px) in map.iter() {
            out.push_str(uri);
            out.push('\t');
            out.push_str(&px.to_string());
            out.push('\n');
        }
        let path = ini_path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let tmp = path.with_extension("tmp");
        if std::fs::write(&tmp, out).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    });
}

/// Queue a delayed flush: repeated slider ticks refresh the map but
/// only the trailing quiet second touches the disk.
fn queue_save() {
    if SAVE_QUEUED.replace(true) {
        return;
    }
    glib::timeout_add_local_once(Duration::from_secs(1), || {
        SAVE_QUEUED.set(false);
        write_levels();
    });
}

/// Wire the status-bar slider: movement resizes the icons and stores
/// the size for the current directory URI.
pub fn wire(tab: &BrowserTab, scale: &gtk4::Scale) {
    scale.set_value(tab.file_view.icon_size() as f64);
    let tab = tab.clone();
    scale.connect_value_changed(move |s| {
        let px = s.value().round() as i32;
        // The slider draws no value (keeps the status bar slim) — the
        // current zoom lives in the tooltip instead.
        s.set_tooltip_text(Some(&format!("{}: {}", crate::i18n::tr("Zoom"), px)));
        if tab.zoom_loading.get() {
            return;
        }
        // Icon size is a shared option — both panes follow the slider.
        tab.set_icon_size(px);
        store(&tab, px);
    });
}

/// Read the per-directory zoom; falls back to the GSettings default.
/// Local file read — fast enough to stay synchronous on navigation.
pub fn load(tab: &BrowserTab, location: &gio::File) {
    let px = with_levels(|map| map.get(&location.uri().to_string()).copied())
        .unwrap_or_else(crate::settings::icon_size);
    // Guard: applying the loaded value must not trigger write-back.
    tab.zoom_loading.set(true);
    tab.zoom_scale.set_value(px as f64);
    tab.zoom_loading.set(false);
    tab.set_icon_size(px);
}

/// Remember the zoom for the directory URI. Any URI works —
/// virtual locations (trash://, recent:///) get zoom too.
fn store(tab: &BrowserTab, px: i32) {
    let Some(location) = tab.location() else {
        return;
    };
    with_levels(|map| {
        map.insert(location.uri().to_string(), px);
    });
    queue_save();
}
