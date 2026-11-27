// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// Async directory enumeration driver. `request` serializes
// enumerations through State::enum_busy: a refresh asked for while
// one is running queues exactly one trailing pass, and results carry
// a generation so a superseded enumeration never clobbers newer
// state. The listing itself is applied as a minimal diff (diff.rs).

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{gio, glib};

use super::State;
use crate::view_model::ATTRIBUTES;

/// Page size per next_files round trip — large enough to keep the
/// GIO round trips few, small enough to stay incremental.
const PAGE: i32 = 256;

/// Start an enumeration unless one is already running — in that
/// case mark the directory dirty so a trailing pass runs right
/// after the current results land.
pub fn request(state: &Rc<RefCell<State>>) {
    let (location, generation) = {
        let mut s = state.borrow_mut();
        if s.enum_busy {
            s.refresh_dirty = true;
            return;
        }
        s.enum_busy = true;
        s.generation += 1;
        (s.location.clone(), s.generation)
    };
    let st = state.clone();
    glib::MainContext::ref_thread_default().spawn_local(async move {
        let result = collect(&location).await;
        let (run_again, notify) = {
            let mut s = st.borrow_mut();
            s.enum_busy = false;
            // `Ok` clears the banner (None); a real error on a still-
            // existing directory surfaces it (Some). NotFound stays
            // silent — the dir-watch handles true deletion.
            let mut notify: Option<Option<glib::Error>> = None;
            if s.generation == generation {
                match result {
                    Ok(infos) => {
                        super::diff::apply(&s.store, &location, infos);
                        notify = Some(None);
                    }
                    Err(e) => {
                        // Directory really gone → honest empty view; a
                        // transient error keeps the last good listing.
                        if !location.query_exists(gio::Cancellable::NONE) {
                            s.store.remove_all();
                        }
                        if !e.matches(gio::IOErrorEnum::NotFound) {
                            notify = Some(Some(e));
                        }
                    }
                }
            }
            (std::mem::take(&mut s.refresh_dirty), notify)
        };
        // Listeners run AFTER the RefCell is released — a callback
        // that reaches back into the model must not panic on the
        // still-held borrow.
        if let Some(err) = notify {
            for cb in &st.borrow().error_notify {
                cb(err.as_ref());
            }
        }
        if run_again {
            request(&st);
        }
    });
}

/// Pull every page of the directory listing into one Vec. Runs as a
/// GIO async chain — no worker thread needed, no main-context block.
async fn collect(location: &gio::File) -> Result<Vec<gio::FileInfo>, glib::Error> {
    let enumerator = location
        .enumerate_children_future(
            ATTRIBUTES,
            gio::FileQueryInfoFlags::NONE,
            glib::Priority::DEFAULT,
        )
        .await?;
    let mut infos = Vec::new();
    loop {
        let page = enumerator
            .next_files_future(PAGE, glib::Priority::DEFAULT)
            .await?;
        if page.is_empty() {
            break;
        }
        infos.extend(page);
    }
    enumerator.close_future(glib::Priority::DEFAULT).await.ok();
    Ok(infos)
}
