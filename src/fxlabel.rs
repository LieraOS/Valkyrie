// Copyright (C) Jörg Schröder
//! valkyrie — animated filename label
//!
//! A GtkWidget that renders text through an animated effect:
//! "pulse" (brightness breathing) and "gradient" (a two-colour
//! wave travelling across the glyphs). GtkLabel is a final class
//! and cannot be subclassed, so this is a real widget: it owns a
//! Pango layout, mirrors the Label props the caption cells use
//! (wrap, lines, ellipsize, justify, xalign) and snapshots either
//! the plain text (effect off — zero cost) or the animated
//! variant. Configuration lives in crate::fxcfg.

use crate::fxcfg::{gradient_axis, scale_color, shared, wave_stops, Mode};
use gtk4::prelude::*;
use gtk4::subclass::prelude::ObjectSubclassIsExt;
use gtk4::{graphene, gsk, pango};
use std::cell::{Cell, RefCell};

mod imp {
    use super::*;
    use gtk4::glib;
    use gtk4::subclass::prelude::*;

    /// Label-ish text state the widget mirrors.
    pub struct FxLabelImp {
        pub text: RefCell<String>,
        pub wrap: Cell<bool>,
        pub wrap_mode: Cell<pango::WrapMode>,
        pub single_line: Cell<bool>,
        pub lines: Cell<i32>,
        pub justify: Cell<gtk4::Justification>,
        pub ellipsize: Cell<pango::EllipsizeMode>,
        pub xalign: Cell<f32>,
        pub yalign: Cell<f32>,
        /// Approximate char-count cap for the natural width.
        pub max_chars: Cell<i32>,
        pub layout: RefCell<Option<pango::Layout>>,
        /// Last drawn state — a config flip must repaint once.
        pub was_enabled: Cell<bool>,
    }

    impl Default for FxLabelImp {
        fn default() -> Self {
            Self {
                text: RefCell::new(String::new()),
                wrap: Cell::new(false),
                wrap_mode: Cell::new(pango::WrapMode::Word),
                single_line: Cell::new(false),
                lines: Cell::new(0),
                justify: Cell::new(gtk4::Justification::Left),
                ellipsize: Cell::new(pango::EllipsizeMode::None),
                xalign: Cell::new(0.5),
                yalign: Cell::new(0.5),
                max_chars: Cell::new(0),
                layout: RefCell::new(None),
                was_enabled: Cell::new(false),
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for FxLabelImp {
        const NAME: &'static str = "ValkyrieFxLabel";
        type Type = super::FxLabel;
        type ParentType = gtk4::Widget;

        fn class_init(klass: &mut Self::Class) {
            // CSS: `fxlabel` styles it like a plain label.
            klass.set_css_name("fxlabel");
        }
    }

    impl ObjectImpl for FxLabelImp {
        fn constructed(&self) {
            self.parent_constructed();
            // Repaint while enabled, and once on the off-transition.
            self.obj().add_tick_callback(|w, _| {
                let enabled = super::shared().cfg.borrow().enabled;
                if enabled {
                    w.queue_draw();
                } else if let Ok(l) = w.clone().downcast::<super::FxLabel>() {
                    if l.imp().was_enabled.replace(false) {
                        w.queue_draw();
                    }
                }
                glib::ControlFlow::Continue
            });
        }
    }

    impl WidgetImpl for FxLabelImp {
        fn measure(&self, orientation: gtk4::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            let obj = self.obj();
            let l = self.layout_for();
            if orientation == gtk4::Orientation::Horizontal {
                let (mut w, _) = l.pixel_size();
                let mc = self.max_chars.get();
                if mc > 0 {
                    let ctx = obj.pango_context();
                    let met = ctx.metrics(None::<&pango::FontDescription>, None);
                    let cw = met.approximate_char_width() / pango::SCALE;
                    w = w.min(mc * cw.max(1));
                }
                (0, w, -1, -1)
            } else {
                let lay = l.clone();
                if for_size > 0 && self.wrap.get() {
                    lay.set_width(for_size * pango::SCALE);
                }
                let (_, h) = lay.pixel_size();
                (0, h, -1, -1)
            }
        }

        fn snapshot(&self, snapshot: &gtk4::Snapshot) {
            let cfg = shared().cfg.borrow().clone();
            self.was_enabled.set(cfg.enabled);
            let layout = self.layout_for();
            let (w, h) = (self.obj().width() as f32, self.obj().height() as f32);
            if w <= 0.0 || h <= 0.0 {
                return;
            }
            // Layout honours the allocated width; xalign shifts the
            // rendered glyphs inside a wider allocation.
            if self.wrap.get() {
                layout.set_width((w * pango::SCALE as f32) as i32);
            }
            let (lw, lh) = {
                let s = layout.pixel_size();
                (s.0 as f32, s.1 as f32)
            };
            let x = ((w - lw) * self.xalign.get()).max(0.0);
            let y = ((h - lh) * self.yalign.get()).max(0.0);
            let rect = graphene::Rect::new(x, y, w - x, h - y);
            #[allow(deprecated)]
            let fg = self.obj().style_context().color();
            if !cfg.enabled {
                let cr = snapshot.append_cairo(&rect);
                cr.set_source_rgba(
                    fg.red() as f64,
                    fg.green() as f64,
                    fg.blue() as f64,
                    fg.alpha() as f64,
                );
                pangocairo::functions::show_layout(&cr, &layout);
                return;
            }
            let t = shared().start.elapsed().as_secs_f32() * cfg.speed;
            match cfg.mode {
                Mode::Pulse => {
                    // Whole text breathes around c1's brightness.
                    let k = 0.5 + 0.5 * (t * 2.0 * std::f32::consts::PI * 0.25).sin();
                    let amp = 1.0 - cfg.strength + cfg.strength * 2.0 * k;
                    let c = scale_color(&cfg.c1, amp);
                    let cr = snapshot.append_cairo(&rect);
                    cr.set_source_rgba(
                        c.red() as f64,
                        c.green() as f64,
                        c.blue() as f64,
                        c.alpha() as f64,
                    );
                    pangocairo::functions::show_layout(&cr, &layout);
                }
                Mode::Gradient => {
                    // Mask = glyph alpha, source = travelling wave.
                    snapshot.push_mask(gsk::MaskMode::Alpha);
                    let cr = snapshot.append_cairo(&rect);
                    cr.set_source_rgba(
                        fg.red() as f64,
                        fg.green() as f64,
                        fg.blue() as f64,
                        fg.alpha() as f64,
                    );
                    pangocairo::functions::show_layout(&cr, &layout);
                    snapshot.pop();
                    let (p0, p1) = gradient_axis(&rect, cfg.dir);
                    let stops = wave_stops(t, &cfg);
                    snapshot.append_linear_gradient(&rect, &p0, &p1, &stops);
                    snapshot.pop();
                }
            }
        }
    }

    impl FxLabelImp {
        /// Pango layout with the mirrored label props applied.
        fn layout_for(&self) -> pango::Layout {
            let obj = self.obj();
            let l = obj.create_pango_layout(Some(&self.text.borrow()));
            l.set_wrap(self.wrap_mode.get());
            l.set_ellipsize(self.ellipsize.get());
            l.set_alignment(match self.justify.get() {
                gtk4::Justification::Right => pango::Alignment::Right,
                gtk4::Justification::Center => pango::Alignment::Center,
                gtk4::Justification::Fill => pango::Alignment::Left,
                _ => pango::Alignment::Left,
            });
            let n = if self.single_line.get() {
                1
            } else {
                self.lines.get()
            };
            if n > 0 {
                l.set_height(-n * pango::SCALE);
            }
            self.layout.replace(Some(l.clone()));
            l
        }
    }
}

glib::wrapper! {
    /// Text widget that renders pulsing or under a travelling
    /// colour wave — plain text while the effect is off.
    pub struct FxLabel(ObjectSubclass<imp::FxLabelImp>)
        @extends gtk4::Widget;
}

impl FxLabel {
    pub fn new() -> Self {
        shared(); // arm config + file monitor
        glib::Object::builder().build()
    }

    pub fn set_label(&self, s: &str) {
        *self.imp().text.borrow_mut() = s.to_string();
        self.queue_resize();
    }

    pub fn label(&self) -> String {
        self.imp().text.borrow().clone()
    }

    /// Whether the current layout truncates the text (Label API).
    pub fn is_ellipsized(&self) -> bool {
        self.imp()
            .layout
            .borrow()
            .as_ref()
            .is_some_and(|l| l.is_ellipsized())
    }

    pub fn set_wrap(&self, on: bool) {
        self.imp().wrap.set(on);
    }

    pub fn set_wrap_mode(&self, m: pango::WrapMode) {
        self.imp().wrap_mode.set(m);
        self.imp().wrap.set(true);
    }

    pub fn wrap_mode(&self) -> pango::WrapMode {
        self.imp().wrap_mode.get()
    }

    pub fn set_lines(&self, n: i32) {
        self.imp().lines.set(n);
    }

    pub fn set_single_line_mode(&self, on: bool) {
        self.imp().single_line.set(on);
    }

    pub fn set_justify(&self, j: gtk4::Justification) {
        self.imp().justify.set(j);
    }

    pub fn justify(&self) -> gtk4::Justification {
        self.imp().justify.get()
    }

    pub fn set_ellipsize(&self, e: pango::EllipsizeMode) {
        self.imp().ellipsize.set(e);
    }

    pub fn set_xalign(&self, x: f32) {
        self.imp().xalign.set(x);
    }

    pub fn xalign(&self) -> f32 {
        self.imp().xalign.get()
    }

    /// Vertical glyph alignment inside the allocation (Label API).
    pub fn set_yalign(&self, y: f32) {
        self.imp().yalign.set(y);
    }

    /// Width cap in approximate characters (Label-compatible).
    pub fn set_max_width_chars(&self, n: i32) {
        self.imp().max_chars.set(n);
        self.queue_resize();
    }
}
