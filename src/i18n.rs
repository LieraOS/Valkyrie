// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// Translation support. Valkyrie ships German and English catalogs via
// gettext; English is the msgid and therefore the automatic fallback.

use gettextrs::{bind_textdomain_codeset, bindtextdomain, textdomain};

/// Name of the gettext catalog (matches data/po/*.po and the .mo files).
const TEXTDOMAIN: &str = "valkyrie";

/// Initialize gettext for the current process locale.
///
/// Catalog lookup order: the `LOCALEDIR` environment override first,
/// then the per-user directory (`~/.local/share/locale`, used by local
/// installs), finally the system directory.
pub fn init() {
    let locale_dir = std::env::var("LOCALEDIR").unwrap_or_else(|_| {
        let user_dir = glib::user_data_dir().join("locale");
        if user_dir.join("de/LC_MESSAGES/valkyrie.mo").exists()
            || user_dir.join("en/LC_MESSAGES/valkyrie.mo").exists()
        {
            user_dir.to_string_lossy().into_owned()
        } else {
            "/usr/share/locale".to_string()
        }
    });

    // All three calls must succeed for translations to load; gettext
    // silently falls back to msgids on failure, which is acceptable.
    let _ = bindtextdomain(TEXTDOMAIN, locale_dir);
    let _ = bind_textdomain_codeset(TEXTDOMAIN, "UTF-8");
    let _ = textdomain(TEXTDOMAIN);
}

/// Translate a message id for the current locale.
///
/// Kept as a tiny wrapper so call sites stay short (`tr("Open")`)
/// and the gettext backend can be swapped in one place if needed.
pub fn tr(msgid: &str) -> String {
    gettextrs::gettext(msgid)
}

/// Plural-aware translation: `n` picks between `msgid` (singular)
/// and `msgid_plural`. The returned format still contains `%d` —
/// callers substitute the number.
pub fn trn(msgid: &str, msgid_plural: &str, n: u32) -> String {
    gettextrs::ngettext(msgid, msgid_plural, n)
}
