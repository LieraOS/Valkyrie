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

use gtk4::gdk_pixbuf::Pixbuf;
use gtk4::prelude::*;
use gtk4::{gdk, gio, glib};

use crate::file_object::FileObject;

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
    /// Thumbnail already exists in the shared cache — just read it.
    Read(PathBuf),
    /// No cached thumbnail: decode, scale and store a new one (image).
    Generate(gio::File),
    /// PDF first page → PNG via pdftoppm (poppler-utils).
    Pdf(gio::File),
    /// Video frame → PNG via ffmpegthumbnailer (if installed).
    Video(gio::File),
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
                    Work::Read(path) => std::fs::read(path).ok(),
                    Work::Generate(file) => generate(&file),
                    Work::Pdf(file) => generate_pdf(&file),
                    Work::Video(file) => generate_video(&file),
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
    let uri = obj.file().uri().to_string();

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
    // A cached thumbnail (by us or any other application) wins.
    if let Some(path) = obj.thumbnail_path() {
        return Some(Work::Read(path));
    }
    // Self-generation is limited to local files — decoding is cheap
    // for images, and PDF/video use external thumbnailers.
    if !obj.file().is_native() {
        return None;
    }
    let ct = obj.content_type();
    if ct.starts_with("image/") {
        return Some(Work::Generate(obj.file()));
    }
    if ct == "application/pdf" {
        return Some(Work::Pdf(obj.file()));
    }
    if ct.starts_with("video/") {
        return Some(Work::Video(obj.file()));
    }
    None
}

/// Decode `file`, scale to the thumbnail size, encode as PNG with the
/// freedesktop metadata and store it in the shared cache.
fn generate(file: &gio::File) -> Option<Vec<u8>> {
    let path = file.path()?;
    let pixbuf = Pixbuf::from_file_at_scale(&path, THUMB_SIZE, THUMB_SIZE, true).ok()?;
    let uri = file.uri().to_string();
    let mtime = std::fs::metadata(&path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs().to_string())
        .unwrap_or_default();
    let bytes = pixbuf
        .save_to_bufferv(
            "png",
            &[
                ("tEXt::Thumb::URI", uri.as_str()),
                ("tEXt::Thumb::MTime", mtime.as_str()),
                ("tEXt::Software", "Valkyrie"),
            ],
        )
        .ok()?;
    cache_store(&uri, &bytes);
    Some(bytes)
}

/// Render the first page of a PDF to a thumbnail via pdftoppm
/// (poppler-utils). The PNG is scaled to THUMB_SIZE and cached.
fn generate_pdf(file: &gio::File) -> Option<Vec<u8>> {
    let path = file.path()?;
    let tmp = std::env::temp_dir().join(format!(
        "valkyrie-pdf-{}-{}.png",
        std::process::id(),
        path.file_name()?.to_string_lossy()
    ));
    // -png -singlefile -scale-to 256 → one PNG at THUMB_SIZE max edge.
    let out = std::process::Command::new("pdftoppm")
        .arg("-png")
        .arg("-singlefile")
        .arg("-scale-to")
        .arg(THUMB_SIZE.to_string())
        .arg(&path)
        .arg(&tmp)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let png_path = if tmp.exists() {
        tmp.clone()
    } else {
        // pdftoppm appends .png despite -singlefile on some versions.
        tmp.with_extension("png")
    };
    let _bytes = std::fs::read(&png_path).ok()?;
    let _ = std::fs::remove_file(&png_path);
    // Re-encode with freedesktop metadata via Pixbuf so the cache
    // entry matches the spec (Thumb::URI + MTime).
    let pixbuf = Pixbuf::from_file_at_scale(&png_path, THUMB_SIZE, THUMB_SIZE, true).ok()?;
    let uri = file.uri().to_string();
    let mtime = std::fs::metadata(&path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs().to_string())
        .unwrap_or_default();
    let bytes = pixbuf
        .save_to_bufferv(
            "png",
            &[
                ("tEXt::Thumb::URI", uri.as_str()),
                ("tEXt::Thumb::MTime", mtime.as_str()),
                ("tEXt::Software", "Valkyrie"),
            ],
        )
        .ok()?;
    cache_store(&uri, &bytes);
    Some(bytes)
}

/// Extract a video frame as a thumbnail via ffmpegthumbnailer.
/// Returns None silently when the tool is not installed — the icon
/// view keeps the generic video icon in that case.
fn generate_video(file: &gio::File) -> Option<Vec<u8>> {
    let path = file.path()?;
    let tmp = std::env::temp_dir().join(format!(
        "valkyrie-vid-{}-{}.png",
        std::process::id(),
        path.file_name()?.to_string_lossy()
    ));
    let out = std::process::Command::new("ffmpegthumbnailer")
        .arg("-i")
        .arg(&path)
        .arg("-o")
        .arg(&tmp)
        .arg("-s")
        .arg(THUMB_SIZE.to_string())
        .arg("-c")
        .arg("png")
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let bytes = std::fs::read(&tmp).ok()?;
    let _ = std::fs::remove_file(&tmp);
    // Re-encode with freedesktop metadata so the cache entry matches
    // the spec. Pixbuf::from_file_at_scale can't reuse the deleted
    // tmp, so decode the raw bytes we already hold.
    let loader = gtk4::gdk_pixbuf::PixbufLoader::new();
    loader.write(&bytes).ok()?;
    loader.close().ok()?;
    let pixbuf = loader.pixbuf()?;
    let uri = file.uri().to_string();
    let mtime = std::fs::metadata(&path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs().to_string())
        .unwrap_or_default();
    let bytes = pixbuf
        .save_to_bufferv(
            "png",
            &[
                ("tEXt::Thumb::URI", uri.as_str()),
                ("tEXt::Thumb::MTime", mtime.as_str()),
                ("tEXt::Software", "Valkyrie"),
            ],
        )
        .ok()?;
    cache_store(&uri, &bytes);
    Some(bytes)
}
/// Written via a temporary file + rename so a kill mid-write can never
/// leave a half-written PNG behind for other applications.
fn cache_store(uri: &str, bytes: &[u8]) {
    let Some(sum) = glib::compute_checksum_for_string(glib::ChecksumType::Md5, uri) else {
        return;
    };
    let dir = glib::user_cache_dir().join("thumbnails/large");
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let target = dir.join(format!("{sum}.png"));
    let tmp = dir.join(format!(".{sum}.tmp"));
    if std::fs::write(&tmp, bytes).is_ok() {
        let _ = std::fs::rename(&tmp, &target);
    }
}
