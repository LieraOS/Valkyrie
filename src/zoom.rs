// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// Icon zoom: the status-bar slider resizes the icon view live and the
// chosen size is remembered per directory in GVFS metadata
// (metadata::valkyrie-icon-size), so a folder keeps its own zoom
// level across restarts.

use gtk4::prelude::*;
use gtk4::{gio, glib};

use crate::browser_tab::BrowserTab;

/// GVFS metadata key carrying this folder's icon size in pixels.
const ZOOM_ATTRIBUTE: &str = "metadata::valkyrie-icon-size";

/// Wire the status-bar slider: movement resizes the icons and stores
/// the size on the current directory.
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
/// Runs async — slow or remote mounts must not block navigation.
pub fn load(tab: &BrowserTab, location: &gio::File) {
    let tab = tab.clone();
    let location = location.clone();
    glib::MainContext::default().spawn_local(async move {
        let px = location
            .query_info_future(
                ZOOM_ATTRIBUTE,
                gio::FileQueryInfoFlags::NONE,
                glib::Priority::LOW,
            )
            .await
            .ok()
            .filter(|i| i.has_attribute(ZOOM_ATTRIBUTE))
            .and_then(|i| i.attribute_string(ZOOM_ATTRIBUTE))
            .and_then(|s| s.parse::<i32>().ok())
            .unwrap_or_else(crate::settings::icon_size);
        // Guard: applying the loaded value must not trigger write-back.
        tab.zoom_loading.set(true);
        tab.zoom_scale.set_value(px as f64);
        tab.zoom_loading.set(false);
        tab.set_icon_size(px);
    });
}

/// Persist the zoom on the directory itself. GVFS metadata only
/// accepts strings/string lists — int32 writes fail silently, so the
/// size travels as a decimal string. Failures are expected for
/// virtual locations (trash://, recent:///) — silently ignored.
fn store(tab: &BrowserTab, px: i32) {
    let Some(location) = tab.location() else {
        return;
    };
    let info = gio::FileInfo::new();
    info.set_attribute_string(ZOOM_ATTRIBUTE, &px.to_string());
    location.set_attributes_async(
        &info,
        gio::FileQueryInfoFlags::NONE,
        glib::Priority::LOW,
        gio::Cancellable::NONE,
        |_| {},
    );
}
