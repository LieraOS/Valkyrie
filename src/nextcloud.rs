// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// Cloud integration over WebDAV. Nextcloud, ownCloud and generic
// WebDAV providers all speak plain WebDAV, so every file operation
// runs through GVFS's dav backend (gvfsd-dav) exactly like browsing
// sftp:// or smb:// — no custom HTTP layer. The app password lives in
// the system keyring (secrets.rs); a MountOperation hands it to GVFS
// on the first auth request, so mounting and uploading work without
// password prompts.
//
// Naming note: the GSettings keys and this module keep their
// historical `nextcloud` prefix so existing configurations stay valid
// — the user-facing UI speaks of "Cloud" / "WebDAV" instead.

use std::cell::Cell;
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{gio, glib};

use crate::i18n::tr;
use crate::secrets;
use crate::settings;

mod dialog;
pub(crate) mod exec;
pub(crate) mod folders_ui;
pub(crate) mod monitor;
pub(crate) mod pairs;
pub(crate) mod plan;
pub(crate) mod state;
pub(crate) mod sync;

/// Whether server URL and username are configured (the password may
/// still be missing — the mount then falls back to the GTK prompt).
pub fn configured() -> bool {
    !settings::nextcloud_server().is_empty() && !settings::nextcloud_user().is_empty()
}

/// The user's file root on the cloud, as a GVFS dav:// URI.
/// Without a path in the server field the Nextcloud/ownCloud
/// convention `davs://user@host/remote.php/dav/files/user/` applies.
/// An explicit path is used verbatim as the provider's WebDAV
/// endpoint (the `{user}` placeholder expands to the login name),
/// which covers generic WebDAV services. Plain http:// input (local
/// test servers) maps to dav://, everything else is TLS.
pub fn dav_root() -> Option<gio::File> {
    let user = settings::nextcloud_user();
    let raw = settings::nextcloud_server();
    let (host, path) = parse_server(&raw)?;
    let insecure =
        raw.trim_start().starts_with("http://") || raw.trim_start().starts_with("dav://");
    let scheme = if insecure { "dav" } else { "davs" };
    let path = match path {
        Some(p) => format!("/{}/", p.replace("{user}", &user)),
        None => format!("/remote.php/dav/files/{user}/"),
    };
    // URI-escape path and userinfo so spaces, UTF-8 names and
    // email-style logins (user@example.com) survive URI assembly.
    let path = glib::uri_escape_string(&path, Some("/"), false);
    let user_esc = glib::uri_escape_string(&user, None::<&str>, false);
    Some(gio::File::for_uri(&format!(
        "{scheme}://{user_esc}@{host}{path}"
    )))
}

/// Split whatever the user typed into `(host[:port], dav path)`:
/// bare hostnames produce no path, `https://` URLs and pasted WebDAV
/// links keep everything after the authority as the endpoint path.
fn parse_server(raw: &str) -> Option<(String, Option<String>)> {
    let s = raw.trim().trim_end_matches('/');
    if s.is_empty() {
        return None;
    }
    // Strip a known scheme; https/davs both mean TLS.
    let rest = s
        .strip_prefix("https://")
        .or_else(|| s.strip_prefix("davs://"))
        .or_else(|| s.strip_prefix("http://"))
        .or_else(|| s.strip_prefix("dav://"))
        .unwrap_or(s);
    // Authority first (user@host[:port]); everything after the first
    // slash is the provider-specific endpoint path.
    let (authority, path) = match rest.split_once('/') {
        Some((a, p)) => (a, Some(p.trim_matches('/').to_string())),
        None => (rest, None),
    };
    let host = authority.rsplit('@').next().unwrap_or(authority);
    if host.is_empty() {
        return None;
    }
    let path = path.filter(|p| !p.is_empty());
    Some((host.to_string(), path))
}

/// `host[:port]` portion only — the keyring storage key, where the
/// endpoint path deliberately plays no role (credentials are per
/// host and login, like GVFS itself treats them).
fn normalize_server(raw: &str) -> Option<String> {
    parse_server(raw).map(|(host, _)| host)
}

/// Mount the cloud root (or reuse the existing mount) and hand the
/// root file to `on_ready`. Errors surface as an alert dialog.
pub fn mount_and_use(parent: &gtk4::Widget, on_ready: impl Fn(&gio::File) + 'static) {
    mount_and_use_result(parent, on_ready, || {});
}

/// Same as mount_and_use but also reports failure to the caller —
/// the sync engine needs to release its in-flight guard when the
/// mount never materialized.
pub(crate) fn mount_and_use_result(
    parent: &gtk4::Widget,
    on_ready: impl Fn(&gio::File) + 'static,
    on_fail: impl FnOnce() + 'static,
) {
    let Some(root) = dav_root() else {
        return;
    };

    // An earlier mount in this session is still registered — reuse it
    // instead of asking GVFS to mount the same root again. The mount
    // must answer a cheap probe first: a mount left behind by a dead
    // process or a dropped connection still shows up in the monitor
    // but fails on every access, which silently broke sync runs.
    let uri = root.uri();
    for mount in gio::VolumeMonitor::get().mounts() {
        if mount.root().uri() != uri {
            continue;
        }
        // Probe: does the mount still serve a one-item listing?
        if let Ok(en) = mount.root().enumerate_children(
            "standard::name",
            gio::FileQueryInfoFlags::NONE,
            gio::Cancellable::NONE,
        ) {
            let _ = en.next_file(gio::Cancellable::NONE);
            on_ready(&mount.root());
            return;
        }
        // Stale mount: drop the gvfsd registration, then fall through
        // to a fresh mount below.
        mount.unmount_with_operation(
            gio::MountUnmountFlags::NONE,
            gio::MountOperation::NONE,
            gio::Cancellable::NONE,
            |_| {},
        );
        break;
    }

    let window = parent
        .root()
        .and_then(|r| r.downcast::<gtk4::Window>().ok());
    let op = gtk4::MountOperation::new(window.as_ref());
    op.set_username(Some(&settings::nextcloud_user()));

    // Answer the first credential request from the keyring; if GVFS
    // asks again (wrong/stored password), the default GTK dialog
    // takes over — it can also write the corrected password back to
    // the keyring via PasswordSave.
    let answered = Rc::new(Cell::new(false));
    let answered2 = answered.clone();
    op.connect_ask_password(move |op, _msg, _def_user, _def_domain, _flags| {
        if answered2.replace(true) {
            op.reply(gio::MountOperationResult::Unhandled);
            return;
        }
        let server = settings::nextcloud_server();
        let user = settings::nextcloud_user();
        if let Some(password) = secrets::lookup(&host_key(&server), &user) {
            op.set_username(Some(&user));
            op.set_password(Some(&password));
            op.set_password_save(gio::PasswordSave::Permanently);
            op.reply(gio::MountOperationResult::Handled);
        } else {
            op.reply(gio::MountOperationResult::Unhandled);
        }
    });

    let parent = parent.clone();
    glib::MainContext::default().spawn_local(async move {
        match root
            .mount_enclosing_volume_future(gio::MountMountFlags::NONE, Some(&op))
            .await
        {
            Ok(()) => on_ready(&root),
            Err(e) => {
                operations_fail(&parent, &tr("Could not connect to cloud server"), &e);
                on_fail();
            }
        }
    });
}

/// Right-click upload entry point: ensure the mount, let the user
/// pick a cloud target folder, then hand the batch to the shared
/// copy engine (progress + conflict dialogs included).
pub fn upload(parent: &gtk4::Widget, sources: Vec<gio::File>) {
    if sources.is_empty() {
        return;
    }
    let mount_parent = parent.clone();
    let parent_owned = parent.clone();
    mount_and_use(&mount_parent, move |root| {
        let root = root.clone();
        let sources = sources.clone();
        let pick_parent = parent_owned.clone();
        let copy_parent = parent_owned.clone();
        dialog::pick_target(&pick_parent, &root, move |target| {
            crate::progress::copy_move(&copy_parent, sources.clone(), target.clone(), false);
        });
    });
}

/// Keyring storage key for `server`: secrets are stored against the
/// normalized host so "https://x/y" and "x" share one entry.
/// pub(crate): the preferences dialog uses the same key shape.
pub(crate) fn host_key(server: &str) -> String {
    normalize_server(server).unwrap_or_else(|| server.trim().to_string())
}

/// User-facing mount error.
fn operations_fail(widget: &gtk4::Widget, message: &str, error: &glib::Error) {
    let window = widget
        .root()
        .and_then(|r| r.downcast::<gtk4::Window>().ok());
    let dialog = gtk4::AlertDialog::builder()
        .message(message)
        .detail(error.message())
        .modal(false)
        .build();
    dialog.show(window.as_ref());
}

#[cfg(test)]
mod tests {
    use super::{normalize_server, parse_server};

    #[test]
    fn normalize_bare_host() {
        assert_eq!(
            normalize_server("cloud.example.com").as_deref(),
            Some("cloud.example.com")
        );
    }

    #[test]
    fn normalize_full_url() {
        assert_eq!(
            normalize_server("https://cloud.example.com/nextcloud/").as_deref(),
            Some("cloud.example.com")
        );
    }

    #[test]
    fn normalize_pasted_dav_link() {
        assert_eq!(
            normalize_server("https://cloud.example.com/remote.php/dav/files/joerg/").as_deref(),
            Some("cloud.example.com")
        );
    }

    #[test]
    fn normalize_userinfo_and_port() {
        assert_eq!(
            normalize_server("https://joerg@cloud.example.com:8443/").as_deref(),
            Some("cloud.example.com:8443")
        );
    }

    #[test]
    fn normalize_empty() {
        assert_eq!(normalize_server("   "), None);
    }

    fn parsed(raw: &str) -> (String, Option<String>) {
        parse_server(raw).unwrap_or_default()
    }

    #[test]
    fn parse_bare_host_has_no_path() {
        assert_eq!(
            parsed("cloud.example.com"),
            ("cloud.example.com".into(), None)
        );
    }

    #[test]
    fn parse_generic_dav_path_is_kept() {
        assert_eq!(
            parsed("https://webdav.example.com/shared/root"),
            ("webdav.example.com".into(), Some("shared/root".into()))
        );
    }

    #[test]
    fn parse_pasted_dav_link_keeps_full_endpoint() {
        assert_eq!(
            parsed("https://cloud.example.com/remote.php/dav/files/joerg/"),
            (
                "cloud.example.com".into(),
                Some("remote.php/dav/files/joerg".into())
            )
        );
    }

    #[test]
    fn parse_userinfo_and_port_keep_path() {
        assert_eq!(
            parsed("davs://joerg@cloud.example.com:8443/dav"),
            ("cloud.example.com:8443".into(), Some("dav".into()))
        );
    }
}
