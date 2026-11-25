// Copyright (C) Jörg Schröder
//! valkyrie — filename effect configuration
//!
//! Parses ~/.config/ragnar-settings/font-effects.ini [names] —
//! written by the fonts page in ragnar-settings — and shares the
//! live config plus the animation clock between every FxLabel.
//! A file monitor reloads it so changes apply immediately.
//! Also hosts the pure drawing helpers (pulse scale, wave stops,
//! gradient axis) the widget's snapshot uses.

use gtk4::prelude::*;
use gtk4::{gdk, gio, glib, graphene, gsk};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Instant;

/// Effect ids stored under `mode=` in the ini.
#[derive(Clone, Copy, PartialEq)]
pub enum Mode {
    Pulse,
    Gradient,
}

/// Parsed contents of font-effects.ini [names].
#[derive(Clone)]
pub struct FxCfg {
    pub enabled: bool,
    pub mode: Mode,
    pub c1: gdk::RGBA,
    pub c2: gdk::RGBA,
    /// 0 l->r, 1 r->l, 2 t->b, 3 b->t
    pub dir: u8,
    pub strength: f32,
    pub speed: f32,
}

impl Default for FxCfg {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: Mode::Pulse,
            c1: gdk::RGBA::WHITE,
            c2: gdk::RGBA::new(0.3, 0.6, 1.0, 1.0),
            dir: 0,
            strength: 0.3,
            speed: 1.0,
        }
    }
}

fn ini_path() -> PathBuf {
    let mut p = glib::user_config_dir();
    p.push("ragnar-settings");
    p.push("font-effects.ini");
    p
}

impl FxCfg {
    fn load() -> Self {
        let mut c = Self::default();
        let Ok(text) = std::fs::read_to_string(ini_path()) else {
            return c;
        };
        let mut in_sec = false;
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with('[') {
                in_sec = line.trim_matches(|c| c == '[' || c == ']') == "names";
                continue;
            }
            if !in_sec {
                continue;
            }
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            let v = v.trim();
            match k.trim() {
                "enabled" => c.enabled = matches!(v, "true" | "yes" | "on" | "1"),
                "mode" => {
                    c.mode = if v == "gradient" {
                        Mode::Gradient
                    } else {
                        Mode::Pulse
                    }
                }
                "color1" => {
                    if let Ok(r) = gdk::RGBA::parse(v) {
                        c.c1 = r;
                    }
                }
                "color2" => {
                    if let Ok(r) = gdk::RGBA::parse(v) {
                        c.c2 = r;
                    }
                }
                "direction" => {
                    c.dir = match v {
                        "rtl" => 1,
                        "ttb" => 2,
                        "btt" => 3,
                        _ => 0,
                    }
                }
                "strength" => c.strength = v.parse().unwrap_or(0.3),
                "speed" => c.speed = v.parse().unwrap_or(1.0),
                _ => {}
            }
        }
        c
    }
}

/// Live config + animation clock shared by every FxLabel.
pub(crate) struct Shared {
    pub cfg: RefCell<FxCfg>,
    pub start: Instant,
}

pub(crate) fn shared() -> Rc<Shared> {
    thread_local! {
        static S: RefCell<Option<Rc<Shared>>> = const { RefCell::new(None) };
    }
    S.with(|s| {
        s.borrow_mut()
            .get_or_insert_with(|| {
                let sh = Rc::new(Shared {
                    cfg: RefCell::new(FxCfg::load()),
                    start: Instant::now(),
                });
                watch_ini(&sh);
                sh
            })
            .clone()
    })
}

/// Reload on every settings write — next draw picks it up.
fn watch_ini(s: &Rc<Shared>) {
    let file = gio::File::for_path(ini_path());
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir.path().unwrap_or_default());
    }
    let Ok(monitor) = file.monitor_file(gio::FileMonitorFlags::NONE, gio::Cancellable::NONE) else {
        return;
    };
    let sh = s.clone();
    monitor.connect_changed(move |_, _, _, _| {
        *sh.cfg.borrow_mut() = FxCfg::load();
    });
    std::mem::forget(monitor);
}

/// Multiply rgb by `amp`, clamped — pulse amplitude source.
pub(crate) fn scale_color(c: &gdk::RGBA, amp: f32) -> gdk::RGBA {
    gdk::RGBA::new(
        (c.red() * amp).clamp(0.0, 1.0),
        (c.green() * amp).clamp(0.0, 1.0),
        (c.blue() * amp).clamp(0.0, 1.0),
        c.alpha(),
    )
}

/// Start/end points of the sweep for the chosen direction.
pub(crate) fn gradient_axis(r: &graphene::Rect, dir: u8) -> (graphene::Point, graphene::Point) {
    match dir {
        1 => (
            graphene::Point::new(r.width(), 0.0),
            graphene::Point::new(0.0, 0.0),
        ),
        2 => (
            graphene::Point::new(0.0, 0.0),
            graphene::Point::new(0.0, r.height()),
        ),
        3 => (
            graphene::Point::new(0.0, r.height()),
            graphene::Point::new(0.0, 0.0),
        ),
        _ => (
            graphene::Point::new(0.0, 0.0),
            graphene::Point::new(r.width(), 0.0),
        ),
    }
}

/// Colour stops forming a band of c2 inside c1 that slides from
/// 0 to 1 with time `t` — exits one side, re-enters the other.
pub(crate) fn wave_stops(t: f32, cfg: &FxCfg) -> [gsk::ColorStop; 7] {
    let band = 0.35; // fraction of the sweep the c2 wave occupies
    let x = t.fract() * (1.0 + band) - band;
    let (a, b) = (&cfg.c1, &cfg.c2);
    [
        gsk::ColorStop::new(0.0, *a),
        gsk::ColorStop::new((x - band).clamp(0.0, 1.0), *a),
        gsk::ColorStop::new(x.clamp(0.0, 1.0), *b),
        gsk::ColorStop::new((x + band).clamp(0.0, 1.0), *b),
        gsk::ColorStop::new((x + 2.0 * band).clamp(0.0, 1.0), *a),
        gsk::ColorStop::new(1.0, *a),
        gsk::ColorStop::new(1.0, *a),
    ]
}
