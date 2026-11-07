// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// Breadcrumb pathbar: one button per path segment. Local paths get a
// leading root button, URI-based locations render their scheme.

use std::cell::RefCell;
use std::rc::Rc;

type FileCallbacks = Rc<RefCell<Vec<Box<dyn Fn(&gio::File)>>>>;

use gtk4::prelude::*;

/// Breadcrumb widget. Emits `location_selected` through the registered
/// callback when a segment button is clicked.
#[derive(Clone)]
pub struct Pathbar {
    pub widget: gtk4::Box,
    callbacks: FileCallbacks,
}

impl Pathbar {
    /// Build a pathbar initialized to `location`.
    pub fn new(location: &gio::File) -> Self {
        let this = Self {
            widget: gtk4::Box::new(gtk4::Orientation::Horizontal, 0),
            callbacks: Rc::new(RefCell::new(Vec::new())),
        };
        this.widget.add_css_class("linked");
        this.set_location(location);
        this
    }

    /// Rebuild all segment buttons for `location`.
    pub fn set_location(&self, location: &gio::File) {
        while let Some(child) = self.widget.first_child() {
            self.widget.remove(&child);
        }

        // Collect ancestors root-first so buttons appear left-to-right.
        let mut chain: Vec<gio::File> = Vec::new();
        let mut cursor = Some(location.clone());
        while let Some(f) = cursor {
            chain.push(f.clone());
            cursor = f.parent();
        }
        chain.reverse();

        for (index, segment) in chain.iter().enumerate() {
            let label = if index == 0 && segment.path().is_none() {
                // URI roots ("trash:///", "network:///") show the scheme.
                segment.uri().trim_end_matches('/').to_string()
            } else {
                segment
                    .basename()
                    .map(|b| b.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "/".to_string())
            };

            let button = gtk4::Button::builder().label(&label).build();
            let file = segment.clone();
            let callbacks = self.callbacks.clone();
            button.connect_clicked(move |_| {
                for cb in callbacks.borrow().iter() {
                    cb(&file);
                }
            });
            self.widget.append(&button);
        }
    }

    /// Register a callback fired with the chosen directory.
    pub fn connect_location_selected<F: Fn(&gio::File) + 'static>(&self, f: F) {
        self.callbacks.borrow_mut().push(Box::new(f));
    }
}
