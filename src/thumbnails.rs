// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// Freedesktop.org thumbnail handling for the icon grid: reuses
// thumbnails already cached by other applications (reported by GIO as
// thumbnail::path) and generates its own for local image files. New
// thumbnails land in $XDG_CACHE_HOME/thumbnails/large with the spec
// metadata so every application on the system shares one cache.
//
// Concurrency model: a small bounded pool of worker threads pulls
// jobs off a shared channel — decoding and file I/O never run on the
// UI thread, and no more than WORKERS decodes happen in parallel.
// Results travel back through a glib main-context channel, so there
// is exactly one wake-up per finished job and no per-item timers.
// A small in-memory texture cache makes re-bound cells (scrolling
// back) instant without touching disk again.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::sync::{mpsc, Arc, Mutex, OnceLock};

use gtk4::prelude::*;
use gtk4::{gdk, gio, glib};

use crate::file_object::FileObject;

mod generate;

/// Thumbnail edge length — the freedesktop "large" size, which covers
/// every zoom level the icon view offers.
const THUMB_SIZE: i32 = 256;

/// Parallel decode/generate jobs. Bounded deliberately: image decode
/// is CPU+IO heavy, and a thousand-entry picture folder must not
/// spawn a thousand threads.
const WORKERS: usize = 4;

/// Entries kept in the in-memory texture cache (FIFO). Re-binding a
/// recycled cell hits this instead of re-reading the PNG.
const TEXTURE_CACHE_MAX: usize = 512;

/// Monotonic job id — also the PENDING registry key.
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// The unit of work handed to the pool.
enum Work {
    /// Cached PNG exists — the worker still validates the embedded
    /// Thumb::MTime against the file before trusting the bytes
    /// (GIO reports thumbnail::path even for stale entries).
    Read {
        path: PathBuf,
        file: gio::File,
        ct: String,
    },
    /// No usable cached thumbnail: decode/scale/store a fresh one —
    /// the content type picks the generator (image, pdf, video).
    Fresh { file: gio::File, ct: String },
}

/// What the receiver needs to route a finished job back to its cell.
struct Pending {
    image: glib::WeakRef<gtk4::Image>,
    generation: u64,
    px: i32,
    /// Texture-cache key (the file URI).
    uri: String,
}

// Job ids → delivery info, the decoded-texture cache, and the
// per-image job generations — all main-thread only (images and
// textures are not Send). The generation map is keyed by widget
// pointer; entries for destroyed images linger harmlessly because
// generations only ever increment, so a recycled address can never
// produce a false match.
thread_local! {
    static PENDING: RefCell<HashMap<u64, Pending>> = RefCell::new(HashMap::new());
    static TEXTURES: RefCell<(HashMap<String, gdk::Texture>, VecDeque<String>)> =
        RefCell::new((HashMap::new(), VecDeque::new()));
    static GENERATIONS: RefCell<HashMap<usize, u64>> = RefCell::new(HashMap::new());
}

/// The pool's job channel, initialized on first use.
static JOBS: OnceLock<mpsc::Sender<(u64, Work)>> = OnceLock::new();

/// Spawn the worker pool. mpsc::Receiver is single-consumer, so it
/// is shared via Arc<Mutex<>> — each worker locks it only around
/// recv(), the standard small-pool pattern and contention-free at
/// this size. Results go back through glib::idle_add_once, which is
/// thread-safe and schedules straight onto the main context — no
/// per-item timers, one wake-up per finished job.
fn job_queue() -> &'static mpsc::Sender<(u64, Work)> {
    JOBS.get_or_init(|| {
        let (job_tx, job_rx) = mpsc::channel::<(u64, Work)>();
        let shared_rx = Arc::new(Mutex::new(job_rx));
        for _ in 0..WORKERS {
            let rx = shared_rx.clone();
            std::thread::spawn(move || loop {
                // Lock only around recv — job execution stays unlocked,
                // so WORKERS jobs genuinely run in parallel.
                let job = match rx.lock().map(|r| r.recv()) {
                    Ok(Ok(job)) => job,
                    // Queue poisoned or closed — the app is exiting.
                    _ => return,
                };
                let (id, work) = job;
                let bytes = match work {
                    Work::Read { path, file, ct } => generate::read_cached_or(&path, &file, &ct),
                    Work::Fresh { file, ct } => generate::exec_fresh(&file, &ct),
                };
                glib::idle_add_once(move || deliver(id, bytes));
            });
        }
        job_tx
    })
}

/// Bump and return the image's job generation. Side effect: any
/// in-flight job for the previously bound file becomes stale.
fn next_generation(image: &gtk4::Image) -> u64 {
    GENERATIONS.with(|g| {
        let mut map = g.borrow_mut();
        let gen = map.entry(image.as_ptr() as usize).or_insert(0);
        *gen += 1;
        *gen
    })
}

/// Whether `gen` is still the image's current job generation.
fn is_current(image: &gtk4::Image, gen: u64) -> bool {
    GENERATIONS.with(|g| g.borrow().get(&(image.as_ptr() as usize)).copied()) == Some(gen)
}

/// Look up a decoded texture by URI (FIFO, capped).
fn texture_cached(uri: &str) -> Option<gdk::Texture> {
    TEXTURES.with(|t| t.borrow().0.get(uri).cloned())
}

/// Remember a decoded texture; evicts the oldest entry when full.
fn texture_store(uri: String, texture: gdk::Texture) {
    TEXTURES.with(|t| {
        let (map, order) = &mut *t.borrow_mut();
        map.insert(uri.clone(), texture);
        order.push_back(uri);
        while order.len() > TEXTURE_CACHE_MAX {
            if let Some(oldest) = order.pop_front() {
                map.remove(&oldest);
            }
        }
    });
}

/// Load `obj`'s thumbnail into `image` when available; entries without
/// a thumbnail keep the themed icon the bind already set. Cheap path:
/// every call bumps the image's generation, queueing only real work.
pub fn attach(obj: &FileObject, image: &gtk4::Image, px: i32) {
    // Policy gate FIRST — cached thumbnails (memory, freedesktop
    // cache, other apps) must respect the same limit as freshly
    // generated ones, otherwise "Up to/Never" would still show the
    // old preview.
    let ct = obj.content_type();
    let size = obj
        .info()
        .map(|i| i.attribute_uint64("standard::size"))
        .unwrap_or(0);
    if !crate::settings::thumbnails_allowed(&ct, size) {
        return;
    }
    let generation = next_generation(image);
    // The texture key carries the mtime: an in-place overwrite must
    // miss the cache, not repaint the old thumbnail.
    let mtime = obj
        .info()
        .and_then(|i| i.modification_date_time())
        .map(|d| d.to_unix())
        .unwrap_or(0);
    let uri = format!("{}@{}", obj.file().uri(), mtime);

    // In-memory hit — the common case when scrolling back or when a
    // neighbouring bind already decoded the same file.
    if let Some(texture) = texture_cached(&uri) {
        image.set_paintable(Some(&texture));
        image.set_size_request(px, px);
        return;
    }

    let Some(work) = plan(obj) else {
        return;
    };

    let id = NEXT_ID.fetch_add(1, AtomicOrdering::Relaxed);
    PENDING.with(|p| {
        p.borrow_mut().insert(
            id,
            Pending {
                image: image.downgrade(),
                generation,
                px,
                uri,
            },
        );
    });
    if job_queue().send((id, work)).is_err() {
        PENDING.with(|p| p.borrow_mut().remove(&id));
    }
}

/// Main-context delivery: decode the PNG, cache it, and paint it only
/// if the cell still waits for *this* job (generation check guards
/// against recycled rows showing a stale thumbnail).
fn deliver(id: u64, bytes: Option<Vec<u8>>) {
    let Some(p) = PENDING.with(|p| p.borrow_mut().remove(&id)) else {
        return;
    };
    let Some(bytes) = bytes else {
        return;
    };
    let bytes = glib::Bytes::from_owned(bytes);
    let Ok(texture) = gdk::Texture::from_bytes(&bytes) else {
        return;
    };
    texture_store(p.uri, texture.clone());
    if let Some(image) = p.image.upgrade() {
        if is_current(&image, p.generation) {
            image.set_paintable(Some(&texture));
            image.set_size_request(p.px, p.px);
        }
    }
}

/// Decide whether a thumbnail is worth loading or generating.
fn plan(obj: &FileObject) -> Option<Work> {
    if obj.is_dir() {
        return None;
    }
    // A cached thumbnail (by us or any other application) wins — but
    // only after the worker confirms its Thumb::MTime is still the
    // file's. The stale path regenerates inside the same job.
    if let Some(path) = obj.thumbnail_path() {
        return Some(Work::Read {
            path,
            file: obj.file(),
            ct: obj.content_type(),
        });
    }
    // Self-generation is limited to local files — decoding is cheap
    // for images, and PDF/video use external thumbnailers.
    if !obj.file().is_native() {
        return None;
    }
    Some(Work::Fresh {
        file: obj.file(),
        ct: obj.content_type(),
    })
}
