// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// Re-establishes directory monitoring after the displayed folder
// itself was deleted and recreated in place. GtkDirectoryList's
// internal monitor tracks the inode it was created on — when a
// process removes the directory and creates a fresh one at the same
// path (build scripts cleaning `chroot/`/`binary/`, git checkouts,
// package operations), that monitor dies silently and the view goes
// permanently stale until a manual F5. Watching the *parent*
// directory survives the swap; when our child reappears the caller
// reloads so a monitor bound to the new inode takes over.

use gtk4::gio;
use gtk4::prelude::*;

/// Watch `location`'s parent for the directory reappearing after a
/// delete+recreate cycle; invoke `on_recreated` when it comes back.
/// The returned monitor must be kept alive — dropping it stops the
/// watch. Returns `None` when there is no parent (`file:///`) or the
/// backend cannot monitor (GVFS/network locations): reload-on-
/// recreate is best-effort, never a failure path.
pub fn watch_recreation(
    location: &gio::File,
    on_recreated: impl Fn() + 'static,
) -> Option<gio::FileMonitor> {
    let parent = location.parent()?;
    let monitor = parent
        .monitor_directory(gio::FileMonitorFlags::NONE, gio::Cancellable::NONE)
        .ok()?;
    let watched = location.clone();
    monitor.connect_changed(move |_, file, _, event| {
        // Created covers plain mkdir; MovedIn covers atomic swaps
        // where a new directory is renamed into place.
        if matches!(
            event,
            gio::FileMonitorEvent::Created | gio::FileMonitorEvent::MovedIn
        ) && file.equal(&watched)
        {
            on_recreated();
        }
    });
    Some(monitor)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;
    use std::time::{Duration, Instant};

    /// Deleting and recreating the watched directory must notify the
    /// callback — the scenario that silently kills the monitored
    /// DirectoryList and leaves the view stale until F5.
    #[test]
    fn recreate_notifies() {
        let base = std::env::temp_dir().join(format!("valkyrie-watch-{}", std::process::id()));
        let dir = base.join("watched");
        std::fs::create_dir_all(&dir).expect("create test dir");

        let fired = Rc::new(Cell::new(false));
        let flag = fired.clone();
        let target = gio::File::for_path(&dir);

        // Delete+recreate from another thread while the main context
        // pumps the file monitor events below.
        let dir_thread = dir.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(150));
            std::fs::remove_dir_all(&dir_thread).expect("remove watched dir");
            std::fs::create_dir_all(&dir_thread).expect("recreate watched dir");
        });

        // Own thread-default context — the shared default steals
        // wakeups between tests running in parallel threads. The
        // monitor is created inside so its sources attach here.
        let ctx = glib::MainContext::new();
        let monitor = Rc::new(RefCell::new(None));
        let _ = ctx.with_thread_default(|| {
            *monitor.borrow_mut() = watch_recreation(&target, move || flag.set(true));
            assert!(monitor.borrow().is_some(), "parent monitor must be created");
            let deadline = Instant::now() + Duration::from_secs(5);
            while !fired.get() && Instant::now() < deadline {
                ctx.iteration(false);
                std::thread::sleep(Duration::from_millis(10));
            }
        });

        drop(monitor);
        std::fs::remove_dir_all(&base).ok();
        assert!(fired.get(), "recreation of the watched dir must notify");
    }

    /// A directory without a parent (the filesystem root) yields no
    /// monitor rather than failing.
    #[test]
    fn root_has_no_parent_watch() {
        let root = gio::File::for_path("/");
        assert!(watch_recreation(&root, || {}).is_none());
    }
}
