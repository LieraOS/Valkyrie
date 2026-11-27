// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// GObject wrapper around a (gio::File, gio::FileInfo) pair. GTK4 list
// widgets can only display GObjects, so every directory entry shown in
// the file views is wrapped in a FileObject.

mod imp {
    use std::cell::RefCell;

    use glib::subclass::prelude::*;

    #[derive(Default)]
    pub struct FileObject {
        pub file: RefCell<Option<gio::File>>,
        pub info: RefCell<Option<gio::FileInfo>>,
        /// Lowercase display name computed once at construction —
        /// the sorter compares thousands of pairs and cannot afford
        /// two fresh allocations per comparison.
        pub sort_key: RefCell<String>,
        /// Dotfile flag cached for the same reason: is_hidden() is
        /// called by the filter on every model churn.
        pub is_dot: std::cell::Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for FileObject {
        const NAME: &'static str = "ValkyrieFileObject";
        type Type = super::FileObject;
    }

    impl ObjectImpl for FileObject {}
}

use gio::prelude::*;
use glib::subclass::prelude::*;

glib::wrapper! {
    pub struct FileObject(ObjectSubclass<imp::FileObject>);
}

impl FileObject {
    /// Create a FileObject for `file` carrying the already-loaded `info`.
    /// `info` may be `None` for places whose attributes have not been
    /// queried yet (e.g. unmounted volumes).
    pub fn new(file: &gio::File, info: Option<&gio::FileInfo>) -> Self {
        let obj: Self = glib::Object::new();
        *obj.imp().file.borrow_mut() = Some(file.clone());
        *obj.imp().info.borrow_mut() = info.cloned();
        // Cache the hot-path values the model pipeline reads on every
        // sort/filter pass: the lowercase name (per-comparison) and
        // the dotfile flag (per filter run). Computed once here,
        // they never change for the lifetime of the FileInfo.
        *obj.imp().sort_key.borrow_mut() = obj.display_name().to_lowercase();
        obj.imp().is_dot.set(obj.raw_name().starts_with('.'));
        obj
    }

    pub fn file(&self) -> gio::File {
        self.imp()
            .file
            .borrow()
            .clone()
            .expect("FileObject has no file")
    }

    pub fn info(&self) -> Option<gio::FileInfo> {
        self.imp().info.borrow().clone()
    }

    /// True when the displayed attributes still match `info` — the
    /// dir model's diff relies on it: unchanged rows emit no signal
    /// at all, so refreshes never re-populate the view visibly.
    pub fn same_state(&self, info: &gio::FileInfo) -> bool {
        let Some(old) = self.info() else {
            return false;
        };
        old.attribute_uint64("standard::size") == info.attribute_uint64("standard::size")
            && old.boolean("standard::is-hidden") == info.boolean("standard::is-hidden")
            && (old.has_attribute("standard::type").then(|| old.file_type()))
                == (info
                    .has_attribute("standard::type")
                    .then(|| info.file_type()))
            && old.modification_date_time().map(|d| d.to_unix())
                == info.modification_date_time().map(|d| d.to_unix())
    }

    /// Display name suitable for the UI (falls back to the raw filename).
    /// The raw attribute getter is used deliberately: display_name()
    /// warns when a backend leaves standard::display-name unset.
    pub fn display_name(&self) -> String {
        if let Some(info) = self.info() {
            if let Some(name) = info.attribute_as_string("standard::display-name") {
                if !name.is_empty() {
                    return name.to_string();
                }
            }
        }
        self.file()
            .basename()
            .map(|b| b.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    /// Raw filename — used for stable, locale-independent sorting.
    pub fn raw_name(&self) -> String {
        self.file()
            .basename()
            .map(|b| b.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    /// Icon for this entry, derived lazily from the content type
    /// instead of the `standard::icon` attribute. Querying the icon
    /// attribute during enumeration costs ~9µs per file (~45ms in a
    /// 5000-file folder); the icon only depends on the content type
    /// anyway, so a per-type cache collapses thousands of lookups
    /// into one per type present in the directory.
    pub fn icon(&self) -> gio::Icon {
        use std::collections::HashMap;
        thread_local! {
            static CACHE: std::cell::RefCell<HashMap<String, gio::Icon>> =
                std::cell::RefCell::new(HashMap::new());
        }
        let key = if self.is_dir() {
            "inode/directory".to_string()
        } else {
            self.content_type()
        };
        if key.is_empty() {
            return gio::ThemedIcon::new("text-x-generic").upcast();
        }
        if let Some(icon) = CACHE.with(|c| c.borrow().get(&key).cloned()) {
            return icon;
        }
        let icon = gio::content_type_get_icon(&key);
        CACHE.with(|c| c.borrow_mut().insert(key, icon.clone()));
        icon
    }

    /// Path of a cached thumbnail (freedesktop.org spec), reported by
    /// GIO only when a fresh entry exists in the shared cache.
    /// Queried lazily on demand — the attribute is not part of the
    /// directory enumeration set anymore (it cost ~45ms per 5000
    /// files); only the ~100 visible cells need it.
    pub fn thumbnail_path(&self) -> Option<std::path::PathBuf> {
        if let Some(path) = self
            .info()
            .and_then(|i| i.attribute_byte_string("thumbnail::path"))
        {
            return Some(std::path::PathBuf::from(path.to_string()));
        }
        let info = self
            .file()
            .query_info(
                "thumbnail::path",
                gio::FileQueryInfoFlags::NONE,
                gio::Cancellable::NONE,
            )
            .ok()?;
        info.attribute_byte_string("thumbnail::path")
            .map(|s| std::path::PathBuf::from(s.to_string()))
    }

    /// File type, or None when the backend did not report
    /// standard::type — file_type() itself warns on missing attributes,
    /// so every caller goes through this guarded accessor.
    fn file_type(&self) -> Option<gio::FileType> {
        self.info()
            .and_then(|i| i.has_attribute("standard::type").then(|| i.file_type()))
    }

    pub fn is_dir(&self) -> bool {
        self.file_type() == Some(gio::FileType::Directory)
    }

    /// Whether the entry is hidden — GVFS flag, or a dotfile fallback.
    /// Uses the raw attribute getter: network:// and similar backends
    /// do not fill standard::is-hidden, and calling is_hidden() on a
    /// FileInfo that lacks the attribute raises a GLib critical.
    /// The dotfile check uses the flag cached at construction.
    pub fn is_hidden(&self) -> bool {
        let flagged = self
            .info()
            .map(|i| i.boolean("standard::is-hidden"))
            .unwrap_or(false);
        flagged || self.imp().is_dot.get()
    }

    /// Zero-allocation access to the cached lowercase sort key — the
    /// sorter calls this inside tight comparison loops, so it borrows
    /// the key for the duration of `f` instead of cloning it.
    pub fn with_sort_key<R>(&self, f: impl FnOnce(&str) -> R) -> R {
        f(self.imp().sort_key.borrow().as_str())
    }

    /// Raw content type string — stable, locale-independent key for
    /// "sort by type" (unlike the localized description text).
    pub fn content_type(&self) -> String {
        self.info()
            .and_then(|i| i.content_type())
            .map(|ct| ct.to_string())
            .unwrap_or_default()
    }

    /// Human-readable size, empty for directories and unknown sizes.
    pub fn size_text(&self) -> String {
        if self.file_type() == Some(gio::FileType::Directory) {
            return String::new();
        }
        let Some(info) = self.info() else {
            return String::new();
        };
        glib::format_size(info.attribute_uint64("standard::size")).to_string()
    }

    /// Numeric size for sorting (directories sort by 0 — grouping is
    /// handled by the directory-first comparator). The raw attribute
    /// getter returns 0 without warning when a backend does not report
    /// standard::size (e.g. network:// listings).
    pub fn size_raw(&self) -> i64 {
        self.info()
            .map(|i| i.attribute_uint64("standard::size") as i64)
            .unwrap_or(0)
    }

    /// Localized modification time string.
    pub fn modified_text(&self) -> String {
        let Some(info) = self.info() else {
            return String::new();
        };
        let Some(dt) = info.modification_date_time() else {
            return String::new();
        };
        dt.format("%x %X")
            .map(|s| s.to_string())
            .unwrap_or_default()
    }

    /// Unix timestamp for sorting by modification time.
    pub fn modified_raw(&self) -> i64 {
        self.info()
            .and_then(|i| i.modification_date_time())
            .map(|d| d.to_unix())
            .unwrap_or(0)
    }

    /// Content type description ("JPEG image", "folder", ...).
    pub fn type_text(&self) -> String {
        if self.file_type() == Some(gio::FileType::Directory) {
            return crate::i18n::tr("Folder");
        }
        self.info()
            .and_then(|i| i.content_type())
            .map(|ct| gio::content_type_get_description(&ct).to_string())
            .unwrap_or_default()
    }
}
