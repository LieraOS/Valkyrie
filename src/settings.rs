// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// Thin wrapper around GSettings. The schema may not be installed yet
// (development builds), so every accessor degrades gracefully to the
// compiled-in defaults instead of panicking.

use gtk4::prelude::*;

const SCHEMA_ID: &str = "org.lieraos.valkyrie";
const KEY_VIEW_MODE: &str = "view-mode";
const KEY_SHOW_HIDDEN: &str = "show-hidden";
const KEY_SORT_FIELD: &str = "sort-field";
const KEY_SORT_DESCENDING: &str = "sort-descending";
const KEY_ICON_SIZE: &str = "icon-size";
const KEY_TERMINAL: &str = "terminal-command";
const KEY_NC_SERVER: &str = "nextcloud-server";
const KEY_NC_USER: &str = "nextcloud-user";
const KEY_NC_FOLDERS: &str = "nextcloud-sync-folders";
const KEY_VERIFY_COPIES: &str = "verify-copies";
const KEY_COPY_PARALLEL: &str = "copy-parallel";
const KEY_COPY_WORKERS: &str = "copy-workers";
const KEY_TERM_PANEL_VISIBLE: &str = "terminal-panel-visible";
const KEY_TERM_PANEL_HEIGHT: &str = "terminal-panel-height";

/// Return a live Settings object, or None when the schema is absent.
fn settings() -> Option<gio::Settings> {
    let source = gio::SettingsSchemaSource::default()?;
    let schema = source.lookup(SCHEMA_ID, true)?;
    Some(gio::Settings::new_full(
        &schema,
        None::<&gio::SettingsBackend>,
        None,
    ))
}

/// Persisted file-view mode: "list" or "icons".
pub fn view_mode() -> String {
    settings()
        .map(|s| s.string(KEY_VIEW_MODE).to_string())
        .filter(|v| v == "list" || v == "icons")
        .unwrap_or_else(|| "icons".to_string())
}

pub fn set_view_mode(mode: &str) {
    if let Some(s) = settings() {
        let _ = s.set_string(KEY_VIEW_MODE, mode);
    }
}

/// Whether hidden files are shown; persisted until the user toggles it.
pub fn show_hidden() -> bool {
    settings()
        .map(|s| s.boolean(KEY_SHOW_HIDDEN))
        .unwrap_or(false)
}

pub fn set_show_hidden(show: bool) {
    if let Some(s) = settings() {
        let _ = s.set_boolean(KEY_SHOW_HIDDEN, show);
    }
}

/// Sort field: "name", "size", "type" or "modified".
pub fn sort_field() -> String {
    settings()
        .map(|s| s.string(KEY_SORT_FIELD).to_string())
        .unwrap_or_else(|| "name".to_string())
}

pub fn set_sort_field(field: &str) {
    if let Some(s) = settings() {
        let _ = s.set_string(KEY_SORT_FIELD, field);
    }
}

/// Whether the sort order is reversed; directories stay first.
pub fn sort_descending() -> bool {
    settings()
        .map(|s| s.boolean(KEY_SORT_DESCENDING))
        .unwrap_or(false)
}

pub fn set_sort_descending(descending: bool) {
    if let Some(s) = settings() {
        let _ = s.set_boolean(KEY_SORT_DESCENDING, descending);
    }
}

/// Global fallback icon size (pixels) for the icon view; directories
/// may override it via GVFS metadata (per-folder zoom).
pub fn icon_size() -> i32 {
    settings()
        .map(|s| s.int(KEY_ICON_SIZE))
        .map(|v| v.clamp(16, 320))
        .unwrap_or(80)
}

pub fn set_icon_size(px: i32) {
    if let Some(s) = settings() {
        let _ = s.set_int(KEY_ICON_SIZE, px.clamp(16, 320));
    }
}

/// Persisted default of the "Verify integrity" checkbox in the copy
/// progress dialog. Off by default: hashing adds one destination
/// read per file, which only pays off when integrity matters.
pub fn verify_copies() -> bool {
    settings()
        .map(|s| s.boolean(KEY_VERIFY_COPIES))
        .unwrap_or(false)
}

pub fn set_verify_copies(on: bool) {
    if let Some(s) = settings() {
        let _ = s.set_boolean(KEY_VERIFY_COPIES, on);
    }
}

/// Whether file copies run through the worker pool. Default off:
/// sequential copying is the safest choice on slow media.
pub fn copy_parallel() -> bool {
    settings()
        .map(|s| s.boolean(KEY_COPY_PARALLEL))
        .unwrap_or(false)
}

pub fn set_copy_parallel(on: bool) {
    if let Some(s) = settings() {
        let _ = s.set_boolean(KEY_COPY_PARALLEL, on);
    }
}

/// Worker count for parallel copies, clamped to the machine's CPU
/// cores — the settings dialog already caps the control, this keeps
/// hand-edited values sane as well. Default: 4 or fewer on small CPUs.
pub fn copy_workers() -> i32 {
    let cores = std::thread::available_parallelism()
        .map(|n| n.get() as i32)
        .unwrap_or(4);
    settings()
        .map(|s| s.int(KEY_COPY_WORKERS))
        .unwrap_or(4)
        .clamp(1, cores)
}

pub fn set_copy_workers(n: i32) {
    if let Some(s) = settings() {
        let _ = s.set_int(KEY_COPY_WORKERS, n.clamp(1, 64));
    }
}

/// Whether the embedded terminal panel is shown (View menu, F4).
pub fn terminal_panel_visible() -> bool {
    settings()
        .map(|s| s.boolean(KEY_TERM_PANEL_VISIBLE))
        .unwrap_or(false)
}

pub fn set_terminal_panel_visible(on: bool) {
    if let Some(s) = settings() {
        let _ = s.set_boolean(KEY_TERM_PANEL_VISIBLE, on);
    }
}

/// Pixel height the user last gave the terminal panel; restored on show.
pub fn terminal_panel_height() -> i32 {
    settings()
        .map(|s| s.int(KEY_TERM_PANEL_HEIGHT))
        .unwrap_or(240)
        .clamp(80, 2000)
}

pub fn set_terminal_panel_height(px: i32) {
    if let Some(s) = settings() {
        let _ = s.set_int(KEY_TERM_PANEL_HEIGHT, px.clamp(80, 2000));
    }
}

/// Optional user override for the terminal emulator ("gnome-terminal",
/// "kgx", ...). Empty string = automatic detection.
pub fn terminal_command() -> String {
    settings()
        .map(|s| s.string(KEY_TERMINAL).to_string())
        .unwrap_or_default()
}

pub fn set_terminal_command(command: &str) {
    if let Some(s) = settings() {
        let _ = s.set_string(KEY_TERMINAL, command.trim());
    }
}

/// Nextcloud server as typed by the user (any scheme/path is accepted;
/// nextcloud.rs normalizes it into the WebDAV endpoint). Empty = not
/// configured.
pub fn nextcloud_server() -> String {
    settings()
        .map(|s| s.string(KEY_NC_SERVER).to_string())
        .unwrap_or_default()
}

pub fn set_nextcloud_server(server: &str) {
    if let Some(s) = settings() {
        let _ = s.set_string(KEY_NC_SERVER, server.trim());
    }
}

/// Nextcloud login name. The app password is deliberately NOT a
/// GSettings key — it lives in the system keyring (secrets.rs).
pub fn nextcloud_user() -> String {
    settings()
        .map(|s| s.string(KEY_NC_USER).to_string())
        .unwrap_or_default()
}

pub fn set_nextcloud_user(user: &str) {
    if let Some(s) = settings() {
        let _ = s.set_string(KEY_NC_USER, user.trim());
    }
}

/// Synced folder pairs as raw tuples — (local path, remote path,
/// direction, schedule). nextcloud::pairs interprets the fields.
pub fn nextcloud_sync_folders() -> Vec<(String, String, String, i32)> {
    settings()
        .map(|s| s.get::<Vec<(String, String, String, i32)>>(KEY_NC_FOLDERS))
        .unwrap_or_default()
}

pub fn set_nextcloud_sync_folders(pairs: &[(String, String, String, i32)]) {
    if let Some(s) = settings() {
        let _ = s.set(KEY_NC_FOLDERS, pairs.to_vec());
    }
}

/// Run `f` whenever one of the Nextcloud keys changes (sidebar
/// refreshes its Network row). The returned Settings object MUST be
/// kept alive by the caller — the handler dies with it.
pub fn connect_nextcloud_changed(f: impl Fn() + 'static) -> Option<gio::Settings> {
    let s = settings()?;
    s.connect_changed(None, move |_, key| {
        if key.starts_with("nextcloud-") {
            f();
        }
    });
    Some(s)
}

const KEY_SECURE_DELETE: &str = "secure-delete";
const KEY_SECURE_DELETE_PASSES: &str = "secure-delete-passes";

/// Whether permanent deletion overwrites file contents first.
/// Default off — the user opts in via the Delete settings page.
pub fn secure_delete() -> bool {
    settings()
        .map(|s| s.boolean(KEY_SECURE_DELETE))
        .unwrap_or(false)
}

pub fn set_secure_delete(on: bool) {
    if let Some(s) = settings() {
        let _ = s.set_boolean(KEY_SECURE_DELETE, on);
    }
}

/// Overwrite passes for secure delete (schema range 1..=7, default 3).
pub fn secure_delete_passes() -> i32 {
    settings()
        .map(|s| s.int(KEY_SECURE_DELETE_PASSES))
        .unwrap_or(3)
        .clamp(1, 7)
}

pub fn set_secure_delete_passes(n: i32) {
    if let Some(s) = settings() {
        let _ = s.set_int(KEY_SECURE_DELETE_PASSES, n.clamp(1, 7));
    }
}

const KEY_CTX_HIDDEN: &str = "context-menu-hidden";

/// Entry ids the user chose to hide from the context menus.
pub fn context_menu_hidden() -> Vec<String> {
    settings()
        .map(|s| s.get::<Vec<String>>(KEY_CTX_HIDDEN))
        .unwrap_or_default()
}

pub fn set_context_menu_hidden(ids: &[String]) {
    if let Some(s) = settings() {
        let _ = s.set(KEY_CTX_HIDDEN, ids.to_vec());
    }
}

/// Whether context-menu entry `id` is shown right now.
pub fn menu_entry_visible(id: &str) -> bool {
    !context_menu_hidden().iter().any(|h| h == id)
}

const KEY_BOOKMARKS: &str = "bookmarks";

/// User bookmarks as URIs in display order.
pub fn bookmarks() -> Vec<String> {
    settings()
        .map(|s| s.get::<Vec<String>>(KEY_BOOKMARKS))
        .unwrap_or_default()
}

fn write_bookmarks(list: Vec<String>) {
    if let Some(s) = settings() {
        let _ = s.set(KEY_BOOKMARKS, list);
    }
}

/// Append `uri` to bookmarks when not already present.
pub fn add_bookmark(uri: &str) {
    let mut list = bookmarks();
    if !list.iter().any(|u| u == uri) {
        list.push(uri.to_string());
        write_bookmarks(list);
    }
}

/// Drop `uri` from bookmarks (no-op when absent).
pub fn remove_bookmark(uri: &str) {
    let list = bookmarks();
    let kept: Vec<String> = list.into_iter().filter(|u| u != uri).collect();
    if kept.len() != bookmarks().len() {
        write_bookmarks(kept);
    }
}

/// Run `f` whenever the bookmarks key changes — sidebar refreshes.
/// The returned Settings object MUST be kept alive by the caller.
pub fn connect_bookmarks_changed(f: impl Fn() + 'static) -> Option<gio::Settings> {
    let s = settings()?;
    s.connect_changed(None, move |_, key| {
        if key == KEY_BOOKMARKS {
            f();
        }
    });
    Some(s)
}

/// Thumbnail categories matching the thumbnails-* GSettings keys.
fn thumb_key(cat: &str, suffix: &str) -> String {
    format!("thumbnails-{cat}-{suffix}")
}

/// Thumbnail policy for a category ("images", "videos", "pdfs"):
/// "always" | "limit" | "never".
pub fn thumbnail_mode(cat: &str) -> String {
    settings()
        .map(|s| s.string(&thumb_key(cat, "mode")).to_string())
        .unwrap_or_else(|| "always".to_string())
}

pub fn set_thumbnail_mode(cat: &str, mode: &str) {
    if let Some(s) = settings() {
        let _ = s.set_string(&thumb_key(cat, "mode"), mode);
    }
}

/// Size cap in MB applied when the mode is "limit".
pub fn thumbnail_max_mb(cat: &str) -> u32 {
    settings()
        .map(|s| s.uint(&thumb_key(cat, "max-mb")))
        .unwrap_or(20)
}

pub fn set_thumbnail_max_mb(cat: &str, mb: u32) {
    if let Some(s) = settings() {
        let _ = s.set_uint(&thumb_key(cat, "max-mb"), mb);
    }
}

/// Whether a thumbnail may be generated for this content type and
/// file size — the single gate the thumbnail pool consults.
pub fn thumbnails_allowed(content_type: &str, size: u64) -> bool {
    let cat = if content_type.starts_with("image/") {
        "images"
    } else if content_type == "application/pdf" {
        "pdfs"
    } else if content_type.starts_with("video/") {
        "videos"
    } else {
        return false;
    };
    match thumbnail_mode(cat).as_str() {
        "always" => true,
        "limit" => size <= thumbnail_max_mb(cat) as u64 * 1024 * 1024,
        _ => false,
    }
}
