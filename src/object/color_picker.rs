//! Colour picker: a two-dimensional field of hue by saturation, a value
//! bar beside it, the chosen colour as a patch, that same colour written
//! out in one of six notations, and two grids of ready-made colours.
//!
//! WHY AN OBJECT AND NOT A PAGE OF SLIDERS. Until 2026-08-18 a colour in
//! the theme editor was three sliders — brightness, saturation, hue —
//! and thirteen colours were thirty-nine rows in which the one thing you
//! could not see was the colour. Three numbers are the SHAPE of the
//! value; they are not a way of ANSWERING the question "what colour is
//! this". The owner looked at a picker and asked for one, "dopasowane do
//! projektu": so the behaviour is the behaviour every picker has had
//! since the eighties, and the geometry, the colours, the corner
//! language and the grid of ready-made colours are this theme's, read
//! from `[picker]` like everything else in this toolkit.
//!
//! THE FIELD IS EXACT, AND THAT IS WHY IT IS TWO CALLS AND NOT A GRID OF
//! CELLS. HSV's own definition is affine in saturation:
//!
//! ```text
//! rgb(h, s, v) = v·(1 − s) + s·rgb(h, 1, v)
//! ```
//!
//! — a grey of value `v` mixed with the fully saturated hue by `s`. So
//! the field is a horizontal hue ramp drawn at the current value, with
//! ONE two-stop vertical overlay of that grey whose alpha runs 0 at the
//! top (fully saturated) to 1 at the bottom. The compositor's straight
//! alpha over encoded values reproduces the line above exactly, which is
//! what [`field_colour`] and its test assert. Dicing the field into
//! cells would have been the obvious way and would have banded, cost a
//! quad per cell, and put a number of cells in Rust that no theme could
//! have argued with.
//!
//! THE VALUE BAR IS EXACT FOR THE SAME REASON: `rgb(h, s, v) = v ·
//! rgb(h, s, 1)`, so it is two stops, the colour at full value and
//! black.
//!
//! HSV AND NOT OKLCh FOR THE FIELD, and the owner ruled on this on
//! 2026-08-16 about the sliders this replaces: brightness at 100 % must
//! be the FULL BRIGHTNESS OF THE HUE — red lands on #FF0000 — and never
//! white. OKLCh's lightness at 1.0 is white by definition, which reads
//! as a broken control. OKLCh is on the list of NOTATIONS, where it
//! belongs and where it is mandatory: the theme file writes `oklch(...)`,
//! so an author who cannot type one cannot move a value between the
//! editor and their own file.
//!
//! THE TRAP THIS FILE IS WRITTEN AROUND. The colour a picker holds is
//! **sRGB-ENCODED** — that is what a bake hands back, what hex spells and
//! what the field's arithmetic above is true of. OKLCh is defined over
//! **LINEAR LIGHT**. Every crossing therefore decodes on the way in
//! ([`Color::to_linear`]) and encodes on the way back ([`Color::to_srgb`]),
//! and neither step is optional. The one time this program mixed the two
//! it did not merely mis-report: the editor seeded itself from what it
//! had just written, so the accent's lightness climbed 0.8200 → 0.8904 →
//! 0.9413 → 0.9715 over successive visits with every slider at rest.
//! `the_notation_survives_twenty_round_trips` is that measurement turned
//! into a test.

use super::focus_ring;
use crate::corner::Cuts;
use crate::draw::Corner;
use crate::focus::{Caps, FocusId};
use crate::theme::color::Oklch;
use crate::theme::{self, Color, TokenId};
use crate::{ui, Ctx, Rect};
use std::sync::OnceLock;

fn tok(cell: &'static OnceLock<TokenId>, name: &'static str) -> TokenId {
    *cell.get_or_init(|| theme::id(name).unwrap_or(TokenId::MISSING))
}

/// The engine's colour, in the draw list's clothes.
fn col(c: theme::ThemeColor) -> Color {
    Color { r: c.r, g: c.g, b: c.b, a: c.a }
}

// ------------------------------------------------------------- notation

/// How the chosen colour is written out, and read back in.
///
/// SIX, AND EVERY ONE OF THEM EARNS ITS PLACE:
///
/// * [`Format::Argb`] — `#AARRGGBB`, the owner's default (2026-08-18).
///   Eight digits, alpha first, so **the alpha lives in the format** and
///   there is no separate opacity knob anywhere near this control.
/// * [`Format::Rgba`] — `#RRGGBBAA`, what CSS and every drawing program
///   spells. The same eight digits in a different order, which is
///   precisely why both are offered: a colour carried between two
///   programs that disagree about where alpha goes is the commonest way
///   to arrive at a transparent red instead of a dark one.
/// * [`Format::Oklch`] — `oklch(L, C, H / A)`, **mandatory**: it is what
///   a `.theme` file is full of, so it is the only notation in which a
///   value typed here and a value read out of the author's own file are
///   the same text.
/// * [`Format::Hsv`] — the field's own coordinates. The three numbers
///   under the field ARE where the two handles stand, so this is the
///   notation in which the control explains itself.
/// * [`Format::Hsl`] — what web tooling means by "hue, saturation,
///   lightness", and it is NOT [`Format::Hsv`]: at 100 % HSL is white
///   and HSV is the full hue. Offering one and calling it the other is
///   how a picker lies to half its users.
/// * [`Format::Dec`] — four plain numbers 0..255, which is what
///   screenshot tools, eyedroppers and image editors report.
///
/// NO CMYK: the owner withdrew it on 2026-08-18. It describes ink on
/// paper and there is no press at the end of this pipeline.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Format {
    Argb,
    Rgba,
    Oklch,
    Hsv,
    Hsl,
    Dec,
}

impl Format {
    /// The offer, in the order the control steps through it. ARGB stands
    /// first because it is the default and a cycler that starts anywhere
    /// else would make the default the hardest one to get back to.
    pub const ALL: [Format; 6] =
        [Format::Argb, Format::Rgba, Format::Oklch, Format::Hsv, Format::Hsl, Format::Dec];

    /// The word on the button. Upper case like every other word this
    /// window puts on a plate; the CASE is the type role's business
    /// (`type.<role>.case`), and this is the word itself.
    pub fn word(self) -> &'static str {
        match self {
            Format::Argb => "ARGB",
            Format::Rgba => "RGBA",
            Format::Oklch => "OKLCH",
            Format::Hsv => "HSV",
            Format::Hsl => "HSL",
            Format::Dec => "DEC",
        }
    }

    /// The next notation round the ring.
    pub fn next(self) -> Format {
        let i = Format::ALL.iter().position(|f| *f == self).unwrap_or(0);
        Format::ALL[(i + 1) % Format::ALL.len()]
    }
}

/// HSV -> sRGB-encoded RGB. `h` in degrees, `s` and `v` in 0..1.
///
/// The saturation line at the head of this file is this function read
/// sideways, and the field's two draw calls stand on it.
pub fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (f32, f32, f32) {
    let h = h.rem_euclid(360.0) / 60.0;
    let s = s.clamp(0.0, 1.0);
    let v = v.clamp(0.0, 1.0);
    let c = v * s;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    (r + m, g + m, b + m)
}

/// sRGB-encoded RGB -> HSV. Hue is 0 on the grey axis, where hue does not
/// exist; the picker never asks this of a colour it is already holding,
/// exactly so that a drag onto the axis does not forget which hue it came
/// from ([`Picker::hsv`]).
pub fn rgb_to_hsv(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let h = if d == 0.0 {
        0.0
    } else if max == r {
        60.0 * (((g - b) / d).rem_euclid(6.0))
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    let s = if max == 0.0 { 0.0 } else { d / max };
    (h, s, max)
}

/// The colour the field shows at `(fx, fy)`, both 0..1 from its top-left,
/// for a bar standing at `v`.
///
/// The one statement of what the field MEANS. The drawing does not call
/// it — two gradient calls do — and that is the point: this is the
/// definition the drawing is tested against.
pub fn field_colour(fx: f32, fy: f32, v: f32) -> (f32, f32, f32) {
    hsv_to_rgb(fx.clamp(0.0, 1.0) * 360.0, 1.0 - fy.clamp(0.0, 1.0), v)
}

fn q8(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// The chosen colour as text.
///
/// ALPHA IS SPELLED THE WAY THE NOTATION SPELLS NUMBERS, which is one
/// rule and not six: the hex notations carry it as a BYTE because bytes
/// are what they are made of, `DEC` likewise, and the three functional
/// notations carry it as a FRACTION after a slash, because that is what
/// `oklch(... / a)` means in the theme language this program already
/// parses. A picker that wrote `0.50` in one place and `128` in another
/// for the same channel would be teaching two dialects.
pub fn write(c: Color, f: Format) -> String {
    let (r, g, b, a) = (q8(c.r), q8(c.g), q8(c.b), q8(c.a));
    match f {
        Format::Argb => format!("#{a:02X}{r:02X}{g:02X}{b:02X}"),
        Format::Rgba => format!("#{r:02X}{g:02X}{b:02X}{a:02X}"),
        // The theme's own spelling, called and not copied: one program,
        // one way of writing a colour into a file.
        Format::Oklch => theme::edit::oklch_literal(c.to_linear().to_oklch()),
        Format::Hsv => {
            let (h, s, v) = rgb_to_hsv(c.r, c.g, c.b);
            with_alpha(format!("hsv({:.0}, {:.0}, {:.0}", h, s * 100.0, v * 100.0), c.a)
        }
        Format::Hsl => {
            let (h, s, l) = rgb_to_hsl(c.r, c.g, c.b);
            with_alpha(format!("hsl({:.0}, {:.0}, {:.0}", h, s * 100.0, l * 100.0), c.a)
        }
        Format::Dec => format!("{r}, {g}, {b}, {a}"),
    }
}

fn with_alpha(mut head: String, a: f32) -> String {
    if a < 1.0 {
        head.push_str(&format!(" / {:.3}", a));
    }
    head.push(')');
    head
}

/// sRGB-encoded RGB -> HSL, the web's triple. Separate from
/// [`rgb_to_hsv`] because they are different quantities that share two
/// names — see [`Format::Hsl`].
pub fn rgb_to_hsl(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    let d = max - min;
    let s = if d == 0.0 { 0.0 } else { d / (1.0 - (2.0 * l - 1.0).abs()).max(1e-6) };
    let (h, _, _) = rgb_to_hsv(r, g, b);
    (h, s.clamp(0.0, 1.0), l)
}

/// HSL -> sRGB-encoded RGB, the inverse of [`rgb_to_hsl`].
pub fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (f32, f32, f32) {
    let s = s.clamp(0.0, 1.0);
    let l = l.clamp(0.0, 1.0);
    // HSL and HSV meet through v = l + s·min(l, 1−l): the same cone read
    // from its middle instead of its tip.
    let v = l + s * l.min(1.0 - l);
    let sv = if v <= 0.0 { 0.0 } else { 2.0 * (1.0 - l / v) };
    hsv_to_rgb(h, sv, v)
}

/// Text back to a colour, or `None` when the text is not that notation.
///
/// FORGIVING ABOUT PUNCTUATION, STRICT ABOUT MEANING. The name in front
/// (`oklch(`, `hsv(`), the `#`, the parentheses, the commas and the
/// spaces are all optional, because a person pasting a value from a file
/// or a screenshot should not have to tidy it first; what is NOT
/// optional is the count and the order of the numbers, because those are
/// the notation. Six hex digits with no alpha mean an OPAQUE colour —
/// the reading every tool in the world agrees on.
pub fn parse(text: &str, f: Format) -> Option<Color> {
    let t = text.trim();
    match f {
        Format::Argb | Format::Rgba => {
            let h: String = t
                .trim_start_matches('#')
                .chars()
                .filter(|c| !c.is_whitespace())
                .collect();
            if !h.is_ascii() || !h.bytes().all(|b| b.is_ascii_hexdigit()) {
                return None;
            }
            let p = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
            match (h.len(), f) {
                (6, _) => Some(Color::rgb8(p(0)?, p(2)?, p(4)?)),
                (8, Format::Argb) => Some(Color::rgba8(p(2)?, p(4)?, p(6)?, p(0)?)),
                (8, _) => Some(Color::rgba8(p(0)?, p(2)?, p(4)?, p(6)?)),
                _ => None,
            }
        }
        Format::Oklch => {
            let (n, a) = numbers(t, "oklch")?;
            if n.len() != 3 {
                return None;
            }
            // The decode-free direction: OKLCh IS linear light, so the
            // encode happens on the way OUT and nowhere else.
            Some(
                Color::from_oklch(Oklch { l: n[0], c: n[1], h: n[2], alpha: a.unwrap_or(1.0) })
                    .to_srgb(),
            )
        }
        Format::Hsv | Format::Hsl => {
            let (n, a) = numbers(t, if f == Format::Hsv { "hsv" } else { "hsl" })?;
            if n.len() != 3 {
                return None;
            }
            let (r, g, b) = if f == Format::Hsv {
                hsv_to_rgb(n[0], n[1] / 100.0, n[2] / 100.0)
            } else {
                hsl_to_rgb(n[0], n[1] / 100.0, n[2] / 100.0)
            };
            Some(Color { r, g, b, a: a.unwrap_or(1.0).clamp(0.0, 1.0) })
        }
        Format::Dec => {
            let (n, _) = numbers(t, "")?;
            let b8 = |v: f32| (v / 255.0).clamp(0.0, 1.0);
            match n.len() {
                3 => Some(Color { r: b8(n[0]), g: b8(n[1]), b: b8(n[2]), a: 1.0 }),
                4 => Some(Color { r: b8(n[0]), g: b8(n[1]), b: b8(n[2]), a: b8(n[3]) }),
                _ => None,
            }
        }
    }
}

/// The numbers of a functional notation, and the alpha behind the slash
/// if one was written. The leading name is accepted and ignored — it says
/// which notation, and the CALLER already knows which notation it asked
/// for; refusing `hsv(...)` typed into an HSL field would be refusing to
/// read three numbers that are right there.
fn numbers(t: &str, name: &str) -> Option<(Vec<f32>, Option<f32>)> {
    let body = match t.find('(') {
        Some(i) if !name.is_empty() => t[i + 1..].trim_end_matches(')'),
        _ => t.trim_end_matches(')'),
    };
    let (head, tail) = match body.split_once('/') {
        Some((h, a)) => (h, a.trim().parse::<f32>().ok()),
        None => (body, None),
    };
    let n: Vec<f32> = head
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|s| !s.is_empty())
        .map(|s| s.trim_end_matches('%').parse::<f32>())
        .collect::<Result<_, _>>()
        .ok()?;
    if n.is_empty() {
        return None;
    }
    Some((n, tail))
}

// --------------------------------------------------------------- the model

/// What the control holds between frames.
///
/// THE HUE IS KEPT, NOT DERIVED. A colour on the grey axis has no hue —
/// `rgb_to_hsv` answers 0 there, and it has to — so a picker that
/// recomputed its coordinates from the colour every frame would swing the
/// field's handle to red the moment a drag reached the bottom edge, and
/// leave it there when the drag came back. The field's two numbers and
/// the bar's one are therefore the state, and the COLOUR is what they
/// answer.
#[derive(Clone, Debug, PartialEq)]
pub struct Picker {
    /// Hue in degrees, saturation and value 0..1 — the field's own
    /// coordinates.
    hsv: [f32; 3],
    /// The alpha channel, which is part of the colour and not a knob
    /// beside it (the owner's decision of 2026-08-18).
    alpha: f32,
    /// Which notation the text side is written in.
    pub format: Format,
}

impl Picker {
    /// A picker opened on a colour.
    pub fn of(c: Color) -> Picker {
        let mut p = Picker { hsv: [0.0, 0.0, 0.0], alpha: 1.0, format: Format::Argb };
        p.set_colour(c);
        p
    }

    /// The chosen colour, sRGB-encoded, alpha included.
    pub fn colour(&self) -> Color {
        let (r, g, b) = hsv_to_rgb(self.hsv[0], self.hsv[1], self.hsv[2]);
        Color { r, g, b, a: self.alpha }
    }

    /// Moves the picker onto a colour. The hue is taken from the colour
    /// EXCEPT on the grey axis, where the colour has none to give and the
    /// handle keeps the hue it was already standing on.
    pub fn set_colour(&mut self, c: Color) {
        let (h, s, v) = rgb_to_hsv(c.r.clamp(0.0, 1.0), c.g.clamp(0.0, 1.0), c.b.clamp(0.0, 1.0));
        if s > 0.0 {
            self.hsv[0] = h;
        }
        self.hsv[1] = s;
        self.hsv[2] = v;
        self.alpha = c.a.clamp(0.0, 1.0);
    }

    /// The chosen colour in the space the theme file writes.
    ///
    /// The decode is part of the trip and never an optimisation to skip;
    /// the head of this file records what skipping it cost.
    pub fn oklch(&self) -> Oklch {
        self.colour().to_linear().to_oklch()
    }

    /// The way back in, with the same discipline.
    pub fn set_oklch(&mut self, v: Oklch) {
        self.set_colour(Color::from_oklch(v).to_srgb());
    }

    /// The field's handle, 0..1 from the field's top-left.
    pub fn field_at(&self) -> (f32, f32) {
        (self.hsv[0].rem_euclid(360.0) / 360.0, 1.0 - self.hsv[1])
    }

    /// The bar's handle, 0..1 from its top (bright) to its bottom.
    pub fn value_at(&self) -> f32 {
        1.0 - self.hsv[2]
    }

    /// A press or a drag inside the field.
    pub fn pick_field(&mut self, fx: f32, fy: f32) {
        self.hsv[0] = fx.clamp(0.0, 1.0) * 360.0;
        self.hsv[1] = 1.0 - fy.clamp(0.0, 1.0);
    }

    /// A press or a drag along the value bar.
    pub fn pick_value(&mut self, fy: f32) {
        self.hsv[2] = 1.0 - fy.clamp(0.0, 1.0);
    }

    /// The colour as text, in the notation in force.
    pub fn text(&self) -> String {
        write(self.colour(), self.format)
    }

    /// Text typed by a person. `false` means it was not read and NOTHING
    /// moved — a picker that fell back to black on a typo would destroy
    /// the value it was showing.
    pub fn set_text(&mut self, s: &str) -> bool {
        match parse(s, self.format) {
            Some(c) => {
                self.set_colour(c);
                true
            }
            None => false,
        }
    }

    /// Steps to the next notation. THE COLOUR DOES NOT MOVE: this changes
    /// how the value is spelled and nothing else, which is what
    /// `changing_the_notation_changes_the_spelling_and_not_the_colour`
    /// pins down.
    pub fn cycle_format(&mut self) {
        self.format = self.format.next();
    }
}

// -------------------------------------------------------------- geometry

/// Where every part of the control stands, in the caller's coordinates.
#[derive(Clone, Debug)]
pub struct Layout {
    /// Hue across, saturation down.
    pub field: Rect,
    /// Value, bright at the top.
    pub value: Rect,
    /// The chosen colour over the transparency checker.
    pub patch: Rect,
    /// The plate that names the notation and steps to the next.
    pub format: Rect,
    /// The colour written out.
    pub text: Rect,
    /// The theme's own ready-made colours.
    pub base: Vec<Rect>,
    /// The caller's own, and the cell that banks the current colour.
    pub custom: Vec<Rect>,
    pub add: Rect,
}

/// What one part of the control answers to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Part {
    Field,
    Value,
    Format,
    Text,
    Base(usize),
    Custom(usize),
    Add,
}

/// The numbers `[picker]` states, read once per call and passed around
/// rather than re-read: a layout and the drawing that follows it must not
/// be able to disagree because the theme was re-baked between them.
struct Metrics {
    gap: f32,
    field_h: f32,
    value_w: f32,
    field_w_frac: f32,
    patch_h: f32,
    row_h: f32,
    format_w: f32,
    swatch: f32,
    swatch_gap: f32,
    cols: usize,
    base_count: usize,
}

impl Metrics {
    fn read() -> Metrics {
        static GAP: OnceLock<TokenId> = OnceLock::new();
        static FIELD_H: OnceLock<TokenId> = OnceLock::new();
        static VALUE_W: OnceLock<TokenId> = OnceLock::new();
        static FRAC: OnceLock<TokenId> = OnceLock::new();
        static PATCH_H: OnceLock<TokenId> = OnceLock::new();
        static ROW_H: OnceLock<TokenId> = OnceLock::new();
        static FORMAT_W: OnceLock<TokenId> = OnceLock::new();
        static SWATCH: OnceLock<TokenId> = OnceLock::new();
        static SWATCH_GAP: OnceLock<TokenId> = OnceLock::new();
        static COLS: OnceLock<TokenId> = OnceLock::new();
        static BASE_N: OnceLock<TokenId> = OnceLock::new();
        let t = theme::resolved();
        Metrics {
            gap: t.px(tok(&GAP, "picker.gap")),
            field_h: t.px(tok(&FIELD_H, "picker.field_h")),
            value_w: t.px(tok(&VALUE_W, "picker.value_w")),
            field_w_frac: t.px(tok(&FRAC, "picker.field_w_frac")).clamp(0.1, 0.9),
            patch_h: t.px(tok(&PATCH_H, "picker.patch_h")),
            row_h: t.px(tok(&ROW_H, "picker.row_h")),
            format_w: t.px(tok(&FORMAT_W, "picker.format_w")),
            swatch: t.px(tok(&SWATCH, "picker.swatch")),
            swatch_gap: t.px(tok(&SWATCH_GAP, "picker.swatch_gap")),
            // Counts, floored at one: a grid nought cells wide is a
            // division by zero, and a theme is a file a person edits.
            cols: (t.px(tok(&COLS, "picker.swatch_cols")).round() as usize).max(1),
            base_count: (t.px(tok(&BASE_N, "picker.base_count")).round() as usize).min(BASE_MAX),
        }
    }
}

/// The ceiling on the ready-made grid, and the reason it is a ceiling and
/// not a length: the tokens are `picker.base.1 ..` and a numbered series
/// is a promise about numbering that only a reader can keep (`[glow]`'s
/// own warning). The reader stops here whatever `base_count` says, so a
/// theme cannot ask for a colour this build has no token id for.
const BASE_MAX: usize = 24;

/// How tall the control stands in a band `w` wide, offering `custom`
/// colours of the caller's own.
///
/// Asked BEFORE the row is laid out, and answered from the same numbers
/// AND the same count the layout uses — a height that disagreed with the
/// layout would leave the swatches drawn over the row below, and it would
/// start disagreeing on the day somebody banked a ninth colour.
pub fn height(w: f32, custom: usize) -> f32 {
    let m = Metrics::read();
    layout_with(&m, Rect::new(0.0, 0.0, w, 0.0), custom).1
}

/// Where everything stands inside `area`, for a picker offering `custom`
/// colours of its own.
pub fn layout(area: Rect, custom: usize) -> Layout {
    let m = Metrics::read();
    layout_with(&m, area, custom).0
}

fn layout_with(m: &Metrics, area: Rect, custom: usize) -> (Layout, f32) {
    let left_w = (area.w * m.field_w_frac).max(m.value_w + m.gap);
    let field_w = (left_w - m.gap - m.value_w).max(0.0);
    let field = Rect::new(area.x, area.y, field_w, m.field_h);
    let value = Rect::new(field.right() + m.gap, area.y, m.value_w, m.field_h);
    let rx = area.x + left_w + m.gap;
    let rw = (area.w - left_w - m.gap).max(0.0);
    let patch = Rect::new(rx, area.y, rw, m.patch_h);
    let mut y = patch.bottom() + m.gap;
    let format = Rect::new(rx, y, m.format_w.min(rw), m.row_h);
    let text = Rect::new(
        format.right() + m.gap,
        y,
        (rw - format.w - m.gap).max(0.0),
        m.row_h,
    );
    y += m.row_h + m.gap;
    // HOW MANY CELLS THE THEME ASKS FOR, AND HOW MANY THERE IS ROOM FOR.
    // `picker.swatch_cols` is the theme's wish and this is the band's
    // answer: a grid wider than the column it stands in would lay cells
    // past the window's own edge, where they would be drawn and pressed
    // over whatever is beside them. Which of the two wins is not a look
    // — it is arithmetic about a width nobody knew when the theme was
    // written — so the cells wrap sooner and none of them leaves the
    // band.
    let pitch = m.swatch + m.swatch_gap;
    let fits = ((rw + m.swatch_gap) / pitch.max(f32::MIN_POSITIVE)).floor();
    let cols = m.cols.min((fits.max(1.0)) as usize).max(1);
    let cell = |i: usize, y0: f32| {
        let (c, r) = (i % cols, i / cols);
        Rect::new(rx + c as f32 * pitch, y0 + r as f32 * pitch, m.swatch, m.swatch)
    };
    let base: Vec<Rect> = (0..m.base_count).map(|i| cell(i, y)).collect();
    let base_rows = m.base_count.div_ceil(cols).max(1);
    y += base_rows as f32 * (m.swatch + m.swatch_gap) - m.swatch_gap + m.gap;
    // The caller's own colours, and the cell that banks the current one
    // AFTER them: a grid that put the bank first would move every custom
    // colour one place along the moment a new one was added.
    let custom_rects: Vec<Rect> = (0..custom).map(|i| cell(i, y)).collect();
    let add = cell(custom, y);
    let right_h = add.bottom() - area.y;
    (
        Layout { field, value, patch, format, text, base, custom: custom_rects, add },
        m.field_h.max(right_h),
    )
}

/// Every part of the control and where it stands, in ONE order.
///
/// The hit test, the focus chain and whatever the application hangs off
/// each part all read this, so a part that is drawn is a part that can be
/// reached — the fault that list exists to prevent is a control with a
/// rect and no place in the Tab order, which is invisible until somebody
/// tries to use the window without a mouse.
///
/// The order is the reading order: the two areas a hand lands in first,
/// then the two plates, then the cells. Nothing overlaps, so it is a
/// statement about reading and not about precedence.
pub fn parts(l: &Layout) -> Vec<(Part, Rect)> {
    let mut out = vec![
        (Part::Field, l.field),
        (Part::Value, l.value),
        (Part::Format, l.format),
        (Part::Text, l.text),
    ];
    out.extend(l.base.iter().enumerate().map(|(i, r)| (Part::Base(i), *r)));
    out.extend(l.custom.iter().enumerate().map(|(i, r)| (Part::Custom(i), *r)));
    out.push((Part::Add, l.add));
    out
}

/// Which part of the control a point is over.
pub fn hit(l: &Layout, x: f32, y: f32) -> Option<Part> {
    parts(l)
        .into_iter()
        .find(|(_, r)| x >= r.x && x < r.right() && y >= r.y && y < r.bottom())
        .map(|(p, _)| p)
}

/// The theme's ready-made colours, in the order the grid shows them.
///
/// They are TOKENS and not a table in Rust, which is the whole rule this
/// program is built on: the colours a picker offers first are a look, and
/// a look lives in the theme. The master points them at its own palette
/// and severity roles, so the grid of a theme is that theme's own
/// vocabulary rather than a wheel of primaries nobody chose.
pub fn base_colours() -> Vec<Color> {
    static IDS: OnceLock<Vec<Option<TokenId>>> = OnceLock::new();
    let m = Metrics::read();
    let ids = IDS.get_or_init(|| {
        (1..=BASE_MAX).map(|i| theme::id(&format!("picker.base.{i}"))).collect()
    });
    let t = theme::resolved();
    ids.iter().take(m.base_count).filter_map(|i| i.map(|i| col(t.color(i)))).collect()
}

// -------------------------------------------------------------- the drawing

fn shape(t: &theme::ResolvedTheme, r: Rect) -> ([Corner; 4], u8) {
    static CORNER: OnceLock<TokenId> = OnceLock::new();
    static CUT: OnceLock<TokenId> = OnceLock::new();
    static CUT_IDX: OnceLock<Cuts> = OnceLock::new();
    static SEGMENTS: OnceLock<TokenId> = OnceLock::new();
    let cut = crate::corner::style(t, tok(&CUT, "picker.corner_style"), &CUT_IDX);
    let c = Corner::sized(cut, t.px(tok(&CORNER, "picker.corner")), r);
    let c = if c.size > 0.0 { c } else { Corner::SQUARE };
    ([c; 4], super::window::corner_segments(t, &SEGMENTS, c.size))
}

/// The chequerboard a colour with alpha is shown against — otherwise a
/// transparent colour and a colour the same shade as the page are the
/// same picture, and the one control that owns alpha could not show it.
fn checker(ctx: &mut Ctx, r: Rect) {
    static SIZE: OnceLock<TokenId> = OnceLock::new();
    static A: OnceLock<TokenId> = OnceLock::new();
    static B: OnceLock<TokenId> = OnceLock::new();
    let t = theme::resolved();
    let s = t.px(tok(&SIZE, "picker.checker")).max(1.0);
    let (a, b) = (col(t.color(tok(&A, "component.picker.checker_a"))), col(t.color(tok(&B, "component.picker.checker_b"))));
    ctx.dl.push_clip(r.x, r.y, r.w, r.h);
    let (nx, ny) = ((r.w / s).ceil() as usize, (r.h / s).ceil() as usize);
    for iy in 0..ny {
        for ix in 0..nx {
            let c = if (ix + iy) % 2 == 0 { a } else { b };
            ctx.dl.rect(r.x + ix as f32 * s, r.y + iy as f32 * s, s, s, c);
        }
    }
    ctx.dl.pop_clip();
}

/// The frame every part of this control wears: one ring, one corner
/// language, both the theme's.
fn frame(ctx: &mut Ctx, r: Rect) {
    static BORDER: OnceLock<TokenId> = OnceLock::new();
    static EDGE: OnceLock<TokenId> = OnceLock::new();
    let t = theme::resolved();
    let (c, seg) = shape(t, r);
    ctx.dl.ring(
        r,
        &c,
        seg,
        t.px(tok(&BORDER, "picker.border")),
        col(t.color(tok(&EDGE, "component.picker.edge"))),
    );
}

/// The handle that marks a chosen point: a ring, because a filled mark
/// would hide the very colour it is pointing at.
fn handle(ctx: &mut Ctx, at: Rect) {
    static STROKE: OnceLock<TokenId> = OnceLock::new();
    static INK: OnceLock<TokenId> = OnceLock::new();
    let t = theme::resolved();
    let (c, seg) = shape(t, at);
    ctx.dl.ring(
        at,
        &c,
        seg,
        t.px(tok(&STROKE, "picker.handle_stroke")),
        col(t.color(tok(&INK, "component.picker.handle"))),
    );
}

/// Draws the whole control. `custom` are the caller's own colours; the
/// picker keeps none of its own, because a swatch a person banked
/// outlives the frame and the control does not.
pub fn draw(ctx: &mut Ctx, l: &Layout, p: &Picker, custom: &[Color]) {
    static HUE_STOPS: OnceLock<TokenId> = OnceLock::new();
    static HANDLE_R: OnceLock<TokenId> = OnceLock::new();
    static ROLE: OnceLock<TokenId> = OnceLock::new();
    static TEXT_INK: OnceLock<TokenId> = OnceLock::new();
    let t = theme::resolved();
    let v = 1.0 - p.value_at();

    // ---- the field: one hue ramp at the bar's value, one grey overlay.
    // `picker.hue_stops` is how finely the circle is sampled, and it is
    // the theme's number because it is a trade between a smooth ramp and
    // the bands `rect_grad` cuts between stops.
    let n = (t.px(tok(&HUE_STOPS, "picker.hue_stops")).round() as usize).clamp(2, 64);
    let stops: Vec<(f32, Color)> = (0..=n)
        .map(|i| {
            let f = i as f32 / n as f32;
            let (r, g, b) = hsv_to_rgb(f * 360.0, 1.0, v);
            (f, Color { r, g, b, a: 1.0 })
        })
        .collect();
    ctx.dl.push_clip(l.field.x, l.field.y, l.field.w, l.field.h);
    ctx.dl.rect_grad(l.field, &stops, 0.0);
    let grey = Color { r: v, g: v, b: v, a: 0.0 };
    ctx.dl.rect_grad(
        l.field,
        &[(0.0, grey), (1.0, Color { a: 1.0, ..grey })],
        std::f32::consts::FRAC_PI_2,
    );
    ctx.dl.pop_clip();
    frame(ctx, l.field);

    // ---- the value bar: the chosen hue at full value, down to black.
    let (hr, hg, hb) = hsv_to_rgb(p.hsv[0], p.hsv[1], 1.0);
    ctx.dl.rect_grad(
        l.value,
        &[
            (0.0, Color { r: hr, g: hg, b: hb, a: 1.0 }),
            (1.0, Color::BLACK),
        ],
        std::f32::consts::FRAC_PI_2,
    );
    frame(ctx, l.value);

    // ---- the two handles.
    let hr_px = t.px(tok(&HANDLE_R, "picker.handle"));
    let (fx, fy) = p.field_at();
    let hx = l.field.x + fx * l.field.w;
    let hy = l.field.y + fy * l.field.h;
    handle(ctx, Rect::new(hx - hr_px, hy - hr_px, hr_px * 2.0, hr_px * 2.0));
    let vy = l.value.y + p.value_at() * l.value.h;
    handle(
        ctx,
        Rect::new(l.value.x, vy - hr_px, l.value.w, hr_px * 2.0),
    );

    // ---- the patch, over the chequerboard so alpha is visible.
    checker(ctx, l.patch);
    let (c, seg) = shape(t, l.patch);
    ctx.dl.ring_fill(l.patch, &c, seg, p.colour());
    frame(ctx, l.patch);

    // ---- the notation's name, and the value written in it.
    let role = ui::bound_role(&ROLE, "picker.role");
    let px = role.px(ctx, 1.0);
    let font = role.font();
    let track = role.tracking_px(px);
    let fig = role.figures(ctx.fonts, font, px);
    let ink = col(t.color(tok(&TEXT_INK, "component.picker.text")));
    let baseline = |r: &Rect| r.y + (r.h - px * role.leading()) / 2.0;
    // BOTH PLATES CLIP THEIR OWN INK. `oklch(0.8200, 0.1531, 166.22)` is
    // the longest thing this control ever writes and there is no width
    // this file may give it: what a notation spells is the notation's,
    // and how wide the plate is, is `picker.format_w` and whatever the
    // band leaves over. So the text is cut at the plate's edge rather
    // than running across the swatches beside it.
    for (r, s) in [(l.format, role.cased(p.format.word())), (l.text, p.text().into())] {
        frame(ctx, r);
        ctx.dl.push_clip(r.x, r.y, r.w, r.h);
        ctx.dl.text_fig(ctx.fonts, font, px, r.x, baseline(&r), &s, ink, track, &fig);
        ctx.dl.pop_clip();
    }

    // ---- the two grids.
    for (r, c) in l.base.iter().zip(base_colours()) {
        swatch(ctx, *r, c);
    }
    for (r, c) in l.custom.iter().zip(custom.iter()) {
        swatch(ctx, *r, *c);
    }
    // The bank cell wears the colour it would bank, so it is a preview
    // and a button at once.
    swatch(ctx, l.add, p.colour());
}

fn swatch(ctx: &mut Ctx, r: Rect, c: Color) {
    let t = theme::resolved();
    if c.a < 1.0 {
        checker(ctx, r);
    }
    let (sh, seg) = shape(t, r);
    ctx.dl.ring_fill(r, &sh, seg, c);
    frame(ctx, r);
}

/// [`draw`], joined to the world's focus chain.
///
/// EVERY PART REGISTERS, not just the field: a swatch the pointer can
/// press and the keyboard cannot reach is a control that exists for half
/// its users. The caller says what each part's identity is (`id_of`),
/// because an id is a PATH in the application's own tree and this
/// library has no idea where in that tree its picker is standing.
///
/// NO PART CLAIMS THE ARROWS. A field could take them — arrows nudging
/// the handle is what a picker on a desktop does — and until something
/// answers them, claiming them would mean four keys that do nothing on
/// the one control that has swallowed them from the chain. So the arrows
/// go on walking the chain, and moving the handle by keyboard is the
/// next stage's work; the swatches make the control usable without a
/// mouse in the meantime.
pub fn draw_focusable(
    ctx: &mut Ctx,
    l: &Layout,
    p: &Picker,
    custom: &[Color],
    id_of: impl Fn(Part) -> FocusId,
) {
    let rings: Vec<(Rect, bool)> = parts(l)
        .into_iter()
        .map(|(part, r)| {
            let f = ctx
                .focus
                .as_deref_mut()
                .map(|fc| fc.register(id_of(part), r, Caps::NONE));
            (r, f.map_or(false, |f| f.ring))
        })
        .collect();
    draw(ctx, l, p, custom);
    // The rings go on TOP of the whole control, not each beside its own
    // part: a ring drawn before the patch beside it would be painted over
    // by it.
    for (r, on) in rings {
        focus_ring::draw_faded(ctx, r, on);
    }
}

#[cfg(test)]
mod tests {
    //! The model's four promises, and one of them is a scar.
    //!
    //! Nothing here needs a window: the notations, the coordinates and
    //! the round trips are arithmetic. The layout tests read the theme,
    //! which every test in this crate may do — the master is compiled in.

    use super::*;

    fn approx(a: f32, b: f32, eps: f32, what: &str) {
        assert!((a - b).abs() <= eps, "{what}: {a} vs {b} (eps {eps})");
    }

    #[test]
    fn a_colour_typed_in_hex_comes_back_the_same_colour() {
        // ARGB: alpha FIRST, which is the whole reason this notation is
        // the default — the alpha is inside the value.
        let c = parse("#80112233", Format::Argb).expect("eight digits are a colour");
        assert_eq!(write(c, Format::Argb), "#80112233");
        assert_eq!((q8(c.r), q8(c.g), q8(c.b), q8(c.a)), (0x11, 0x22, 0x33, 0x80));
        // The same digits read as RGBA are a DIFFERENT colour, and that
        // is the confusion both notations exist to make visible.
        let d = parse("#80112233", Format::Rgba).expect("eight digits are a colour");
        assert_eq!((q8(d.r), q8(d.g), q8(d.b), q8(d.a)), (0x80, 0x11, 0x22, 0x33));
        assert_eq!(write(d, Format::Rgba), "#80112233");
        // Six digits are opaque in both, and come back as eight with the
        // alpha where that notation keeps it — which is the whole of the
        // difference between them.
        for (f, want) in [(Format::Argb, "#FF3FE3AE"), (Format::Rgba, "#3FE3AEFF")] {
            let s = parse("#3FE3AE", f).expect("six digits are a colour");
            assert_eq!(q8(s.a), 255);
            assert_eq!(write(s, f), want);
        }
        // Through the control itself: what the picker shows is what a
        // person typed.
        let mut p = Picker::of(Color::BLACK);
        assert!(p.set_text("#80112233"));
        assert_eq!(p.text(), "#80112233");
    }

    #[test]
    fn changing_the_notation_changes_the_spelling_and_not_the_colour() {
        let mut p = Picker::of(Color::rgba8(0x3F, 0xE3, 0xAE, 0xCC));
        let before = p.colour();
        let mut seen = Vec::new();
        for _ in 0..Format::ALL.len() {
            seen.push(p.text());
            p.cycle_format();
            // The colour is bit-for-bit what it was: a notation is a way
            // of writing, not a way of rounding.
            assert_eq!(p.colour(), before, "the notation moved the colour");
        }
        // A full ring comes back to where it started.
        assert_eq!(p.format, Format::Argb);
        // Six notations, six different strings: a format that spelled the
        // same as its neighbour would be a choice that does nothing.
        let mut sorted = seen.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), Format::ALL.len());
        // AND EVERY STRING SAYS WHICH NOTATION IT IS. Three of the six are
        // functional, and a functional notation that announced itself as
        // another one would be a value nobody could paste anywhere: the
        // numbers of `hsl` written under the word `hsv` are a different
        // colour to every reader in the world, this file's forgiving
        // parser included.
        for (f, s) in Format::ALL.iter().zip(seen.iter()) {
            if let Format::Oklch | Format::Hsv | Format::Hsl = f {
                assert!(
                    s.starts_with(&f.word().to_lowercase()),
                    "{f:?} must announce itself: {s}"
                );
            }
        }
        // And every one of them READS BACK as the colour it wrote, to
        // within the notation's own resolution.
        for f in Format::ALL {
            let s = write(before, f);
            let back = parse(&s, f).unwrap_or_else(|| panic!("{f:?} cannot read {s}"));
            for (a, b, ch) in [
                (back.r, before.r, 'r'),
                (back.g, before.g, 'g'),
                (back.b, before.b, 'b'),
                (back.a, before.a, 'a'),
            ] {
                approx(a, b, 0.02, &format!("{f:?} channel {ch}"));
            }
        }
    }

    #[test]
    fn the_alpha_of_an_argb_value_reaches_the_theme() {
        // The picker is handed a half-transparent colour as eight hex
        // digits and asked for what the FILE would receive.
        let mut p = Picker::of(Color::WHITE);
        assert!(p.set_text("#803FE3AE"));
        let lit = theme::edit::oklch_literal(p.oklch());
        assert!(
            lit.contains(" / 0.502"),
            "the alpha must cross into the theme's own spelling: {lit}"
        );
        // And it is the ALPHA that crossed, not a darkened colour: the
        // opaque twin writes no slash at all.
        let mut q = Picker::of(Color::WHITE);
        assert!(q.set_text("#FF3FE3AE"));
        let opaque = theme::edit::oklch_literal(q.oklch());
        assert!(!opaque.contains('/'), "an opaque colour writes no alpha: {opaque}");
        approx(p.oklch().l, q.oklch().l, 1e-4, "alpha must not move lightness");
    }

    #[test]
    fn the_notation_survives_twenty_round_trips() {
        //! THE SCAR. `.gap-program/obalone-naprawy.md` and the head of
        //! this file record what happened when a crossing to OKLCh
        //! skipped the decode: the editor seeded itself from what it had
        //! written, so the accent's lightness climbed 0.8200 -> 0.8904 ->
        //! 0.9413 -> 0.9715 with nobody touching a control. Twenty trips
        //! is far past where that was already obvious.
        let mut p = Picker::of(Color::rgb8(0x3F, 0xE3, 0xAE));
        let first = p.oklch();
        for i in 0..20 {
            let there = p.oklch();
            p.set_oklch(there);
            let back = p.oklch();
            approx(back.l, first.l, 2e-3, &format!("lightness after trip {i}"));
            approx(back.c, first.c, 2e-3, &format!("chroma after trip {i}"));
            approx(back.h, first.h, 0.5, &format!("hue after trip {i}"));
        }
        // The same walk through the TEXT, which is the road a person
        // takes: write it out, read it back, twenty times.
        let mut q = Picker::of(Color::rgb8(0x3F, 0xE3, 0xAE));
        q.format = Format::Oklch;
        for i in 0..20 {
            let s = q.text();
            assert!(q.set_text(&s), "trip {i} wrote a value it cannot read: {s}");
        }
        approx(q.oklch().l, first.l, 3e-3, "lightness after twenty written trips");
    }

    #[test]
    fn the_field_is_what_the_two_gradients_draw() {
        // The saturation line the drawing stands on: a grey of value v
        // laid over the full hue by alpha 1-s IS hsv(h, s, v).
        for &v in &[0.25f32, 0.6, 1.0] {
            for &s in &[0.0f32, 0.35, 1.0] {
                for &h in &[0.0f32, 95.0, 210.0, 359.0] {
                    let (r, g, b) = hsv_to_rgb(h, s, v);
                    let (fr, fg, fb) = field_colour(h / 360.0, 1.0 - s, v);
                    approx(fr, r, 1e-5, "field red");
                    approx(fg, g, 1e-5, "field green");
                    approx(fb, b, 1e-5, "field blue");
                    // What the compositor computes: base·s + grey·(1−s).
                    let (br, _, _) = hsv_to_rgb(h, 1.0, v);
                    approx(br * s + v * (1.0 - s), r, 1e-5, "overlay red");
                }
            }
        }
    }

    #[test]
    fn a_drag_onto_the_grey_axis_keeps_the_hue_it_came_from() {
        let mut p = Picker::of(Color::rgb8(0x3F, 0xE3, 0xAE));
        let (fx, _) = p.field_at();
        p.pick_field(fx, 1.0); // saturation to nothing
        assert_eq!(p.colour().r, p.colour().b, "the grey axis is grey");
        let (fx2, _) = p.field_at();
        approx(fx2, fx, 1e-6, "the hue handle stayed where the hand left it");
        // AND IT SURVIVES A RE-SEED, which is the road this actually
        // happens on: the editor reads the theme back into its controls
        // on every visit, and a grey read back in has no hue to give.
        let grey = p.colour();
        p.set_colour(grey);
        let (fx3, _) = p.field_at();
        approx(fx3, fx, 1e-6, "a re-seed off a grey kept the hue");
        // And coming back off the axis returns the same hue.
        p.pick_field(fx, 0.0);
        approx(p.oklch().h, Picker::of(Color::rgb8(0x00, 0xFF, 0xB0)).oklch().h, 40.0, "hue");
    }

    #[test]
    fn the_layout_reserves_exactly_the_height_it_reports() {
        let area = Rect::new(30.0, 40.0, 520.0, 0.0);
        // Past a full row of banked colours the grid grows a row, which
        // is the case a height that ignored the count would get wrong.
        for custom in [0usize, 1, 7, 8, 17] {
            let l = layout(area, custom);
            let h = height(area.w, custom);
            let low = l
                .base
                .iter()
                .chain(l.custom.iter())
                .chain([l.field, l.value, l.patch, l.format, l.text, l.add].iter())
                .fold(area.y, |acc, r| acc.max(r.bottom()));
            approx(h, low - area.y, 0.51, "the reported height covers every part");
            // NOTHING IS LAID OUTSIDE THE BAND IT WAS GIVEN, and the
            // swatches are the ones that would: their count comes from
            // the theme and the room comes from the window, so a narrow
            // band has to wrap them sooner rather than run them off the
            // edge. Asked at a width that cannot hold the theme's eight.
            for area in [area, Rect::new(30.0, 40.0, 260.0, 0.0)] {
                let l = layout(area, custom);
                assert!(l.field.x >= area.x && l.value.right() <= area.x + area.w);
                assert!(l.text.right() <= area.x + area.w + 0.51);
                for (part, r) in parts(&l) {
                    assert!(
                        r.right() <= area.x + area.w + 0.51 && r.x >= area.x - 0.51,
                        "{part:?} runs past the band at width {}",
                        area.w
                    );
                }
                approx(height(area.w, custom), {
                    let low = parts(&l).iter().fold(area.y, |a, (_, r)| a.max(r.bottom()));
                    low - area.y
                }, 0.51, "the reported height covers every part");
            }
        }
    }

    #[test]
    fn every_part_of_the_control_answers_for_itself() {
        let l = layout(Rect::new(0.0, 0.0, 520.0, 0.0), 3);
        let mid = |r: Rect| (r.x + r.w / 2.0, r.y + r.h / 2.0);
        let (x, y) = mid(l.field);
        assert_eq!(hit(&l, x, y), Some(Part::Field));
        let (x, y) = mid(l.value);
        assert_eq!(hit(&l, x, y), Some(Part::Value));
        let (x, y) = mid(l.format);
        assert_eq!(hit(&l, x, y), Some(Part::Format));
        let (x, y) = mid(l.text);
        assert_eq!(hit(&l, x, y), Some(Part::Text));
        let (x, y) = mid(l.base[0]);
        assert_eq!(hit(&l, x, y), Some(Part::Base(0)));
        let (x, y) = mid(l.custom[2]);
        assert_eq!(hit(&l, x, y), Some(Part::Custom(2)));
        let (x, y) = mid(l.add);
        assert_eq!(hit(&l, x, y), Some(Part::Add));
        // A point in none of them is nobody's.
        assert_eq!(hit(&l, -5.0, -5.0), None);
    }

    #[test]
    fn the_ready_made_colours_come_from_the_theme() {
        let base = base_colours();
        assert!(!base.is_empty(), "the master declares a grid");
        // They are the THEME's: the first cell is the accent the rest of
        // the interface is derived from, so a theme's picker opens on the
        // theme's own vocabulary.
        let accent = theme::id("palette.accent")
            .map(|i| theme::resolved().color(i))
            .expect("the accent is a token");
        approx(base[0].r, accent.r, 1e-5, "the first cell is the accent");
        approx(base[0].g, accent.g, 1e-5, "the first cell is the accent");
        approx(base[0].b, accent.b, 1e-5, "the first cell is the accent");
    }
}
