//! Colour, in the four spaces this engine actually uses.
//!
//! * **sRGB, encoded** — what an author writes (`#3FE3AE`, `rgb(63 227 174)`)
//!   and what the GPU blends on the `FormatKind::Unorm` path.
//! * **linear light** — what the engine stores between parse and bake, and the
//!   space `mix()` and `over()` model physics in (§6).
//! * **OKLab / OKLCh** — perception. `shade`, `tint`, `lum*`, `sat`, `hue`,
//!   `ramp` and `ensure` all live here (§6).
//!
//! Two rules from the specification are load-bearing and are enforced by this
//! module rather than by its callers:
//!
//! 1. **Alpha is straight, never premultiplied** ([CONFLICT 20], §6.3). The blend
//!    state is `SRC_ALPHA / ONE_MINUS_SRC_ALPHA`, so a premultiplying draw-list
//!    builder would double-apply it. Nothing here multiplies rgb by a.
//! 2. **Every OKLCh -> sRGB conversion gamut-maps by chroma reduction** (§6.2),
//!    automatically, with 22 bisection steps at fixed L and hue. Per-channel
//!    clamping is forbidden: it collapsed two of pure-green's eight data series
//!    onto the same colour. [`Color::from_oklch`] is the only public entry point
//!    and it always maps; [`Color::from_oklch_unmapped`] exists solely for the
//!    extended-range (scRGB/PQ) path, where the clamp belongs to the output.
//!
//! ### Relationship to `theme::Color`
//!
//! `nacelle::theme::Color` IS this type: with the old engine deleted,
//! `pub use color::Color` replaced the legacy seven-field engine's colour and
//! no call site changed. The five methods the program was built on (`rgb8`,
//! `from_hex`, `alpha`, `dim`, `to_array`) keep their names and semantics.
//!
//! Not in this stage: `encode.rs`. The sRGB-encode / leave-linear decision keyed
//! on the live swapchain format (§6.3) is a swapchain-format dependency, so
//! [`Color::to_srgb`] is applied by `bake.rs` for the `Unorm` path today and
//! moves to `encode.rs` when that lands.

/// A colour: four `f32` channels, **straight** (non-premultiplied) alpha.
///
/// Which space `r`/`g`/`b` are in is a property of the *value*, not the type,
/// and the pipeline stage says which: parse decodes to linear, derivation works
/// in linear and OKLab, bake encodes to sRGB for the `Unorm` swapchain.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

/// OKLab: perceptual lightness plus two opponent axes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Oklab {
    pub l: f32,
    pub a: f32,
    pub b: f32,
    pub alpha: f32,
}

/// OKLCh: OKLab in polar form. `h` is degrees, `c` is chroma.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Oklch {
    pub l: f32,
    pub c: f32,
    pub h: f32,
    pub alpha: f32,
}

impl Color {
    pub const TRANSPARENT: Color = Color { r: 0.0, g: 0.0, b: 0.0, a: 0.0 };
    pub const BLACK: Color = Color { r: 0.0, g: 0.0, b: 0.0, a: 1.0 };
    pub const WHITE: Color = Color { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };
    /// The engine's per-kind raw ink (§governing principle): what a colour
    /// token answers when no theme anywhere declares it. Deliberately dull.
    pub const GREY: Color = Color { r: 0.5, g: 0.5, b: 0.5, a: 1.0 };

    // ---------------------------------------------------------------- ctors

    pub const fn new(r: f32, g: f32, b: f32, a: f32) -> Self {
        Color { r, g, b, a }
    }

    /// Eight-bit **sRGB-encoded** components, as an author writes them.
    /// Same semantics as the legacy `Color::rgb8`, which is why it keeps the name.
    pub fn rgb8(r: u8, g: u8, b: u8) -> Self {
        Color { r: r as f32 / 255.0, g: g as f32 / 255.0, b: b as f32 / 255.0, a: 1.0 }
    }

    pub fn rgba8(r: u8, g: u8, b: u8, a: u8) -> Self {
        Color { a: a as f32 / 255.0, ..Self::rgb8(r, g, b) }
    }

    /// `#RGB` `#RGBA` `#RRGGBB` `#RRGGBBAA`, case-insensitive, short forms
    /// expanded by digit doubling (§3.2). The result is **sRGB-encoded**;
    /// the parser calls [`Color::to_linear`] on it.
    ///
    /// Rejects non-ASCII before slicing, so a six-*byte* two-character value
    /// cannot panic mid-character.
    pub fn from_hex(hex: &str) -> Option<Self> {
        let h = hex.trim().trim_start_matches('#');
        if !h.is_ascii() || !h.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let d = |i: usize| -> u8 { u8::from_str_radix(&h[i..i + 1], 16).unwrap() * 17 };
        let p = |i: usize| -> u8 { u8::from_str_radix(&h[i..i + 2], 16).unwrap() };
        match h.len() {
            3 => Some(Self::rgb8(d(0), d(1), d(2))),
            4 => Some(Self::rgba8(d(0), d(1), d(2), d(3))),
            6 => Some(Self::rgb8(p(0), p(2), p(4))),
            8 => Some(Self::rgba8(p(0), p(2), p(4), p(6))),
            _ => None,
        }
    }

    /// `#RRGGBB` of an sRGB-encoded colour — for diagnostics, which quote hex.
    pub fn to_hex(self) -> String {
        let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        format!("#{:02X}{:02X}{:02X}", q(self.r), q(self.g), q(self.b))
    }

    // ------------------------------------------------------- transfer curve

    /// sRGB-encoded -> linear light. Alpha is untouched: it is already linear.
    pub fn to_linear(self) -> Self {
        Color { r: srgb_to_linear(self.r), g: srgb_to_linear(self.g), b: srgb_to_linear(self.b), a: self.a }
    }

    /// Linear light -> sRGB-encoded. This is `encode.rs`'s `Unorm` path (§6.3),
    /// applied by `bake.rs` until that stage exists.
    pub fn to_srgb(self) -> Self {
        Color { r: linear_to_srgb(self.r), g: linear_to_srgb(self.g), b: linear_to_srgb(self.b), a: self.a }
    }

    // ------------------------------------------------------------- channels

    /// **Sets** alpha (§6 `alpha`). Kept at this name because the whole program
    /// already calls `Color::alpha`.
    pub fn alpha(self, a: f32) -> Self {
        Color { a: a.clamp(0.0, 1.0), ..self }
    }

    /// **Multiplies** alpha (§6 `fade`) — the honest name for GTK's `alpha()`.
    pub fn fade(self, f: f32) -> Self {
        Color { a: (self.a * f.max(0.0)).clamp(0.0, 1.0), ..self }
    }

    /// Per-channel multiply. **Cut from the derivation functions** (§6.1): in
    /// sRGB it makes red vanish while green survives. Retained only because
    /// the program still calls it; authors get `lum()`, which is the same
    /// intent done in OKLCh.
    pub fn dim(self, f: f32) -> Self {
        Color { r: self.r * f, g: self.g * f, b: self.b * f, a: self.a }
    }

    pub fn to_array(self) -> [f32; 4] {
        [self.r, self.g, self.b, self.a]
    }

    pub fn is_finite(self) -> bool {
        self.r.is_finite() && self.g.is_finite() && self.b.is_finite() && self.a.is_finite()
    }

    /// Clamp into the unit cube. Used only at the very end of the pipeline, and
    /// never as a substitute for gamut mapping (§6.2).
    pub fn clamped(self) -> Self {
        Color {
            r: self.r.clamp(0.0, 1.0),
            g: self.g.clamp(0.0, 1.0),
            b: self.b.clamp(0.0, 1.0),
            a: self.a.clamp(0.0, 1.0),
        }
    }

    // --------------------------------------------------------------- OKLab

    /// Linear-light sRGB -> OKLab (Ottosson's matrices).
    pub fn to_oklab(self) -> Oklab {
        let (r, g, b) = (self.r, self.g, self.b);
        let l = 0.412_221_47 * r + 0.536_332_54 * g + 0.051_445_995 * b;
        let m = 0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b;
        let s = 0.088_302_46 * r + 0.281_718_84 * g + 0.629_978_7 * b;
        let (l_, m_, s_) = (cbrt(l), cbrt(m), cbrt(s));
        Oklab {
            l: 0.210_454_26 * l_ + 0.793_617_8 * m_ - 0.004_072_047 * s_,
            a: 1.977_998_5 * l_ - 2.428_592_2 * m_ + 0.450_593_7 * s_,
            b: 0.025_904_037 * l_ + 0.782_771_77 * m_ - 0.808_675_77 * s_,
            alpha: self.a,
        }
    }

    /// OKLab -> linear-light sRGB, **without** gamut mapping. Callers that will
    /// display the result must go through [`Color::from_oklch`].
    pub fn from_oklab_unmapped(v: Oklab) -> Self {
        let l_ = v.l + 0.396_337_78 * v.a + 0.215_803_76 * v.b;
        let m_ = v.l - 0.105_561_346 * v.a - 0.063_854_17 * v.b;
        let s_ = v.l - 0.089_484_18 * v.a - 1.291_485_5 * v.b;
        let (l, m, s) = (l_ * l_ * l_, m_ * m_ * m_, s_ * s_ * s_);
        Color {
            r: 4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_94 * s,
            g: -1.268_438 * l + 2.609_757_4 * m - 0.341_319_38 * s,
            b: -0.004_196_086_3 * l - 0.703_418_6 * m + 1.707_614_7 * s,
            a: v.alpha,
        }
    }

    /// OKLab -> sRGB with the mandatory chroma-reduction gamut map (§6.2).
    pub fn from_oklab(v: Oklab) -> Self {
        Self::from_oklch(v.to_oklch())
    }

    pub fn to_oklch(self) -> Oklch {
        self.to_oklab().to_oklch()
    }

    /// OKLCh -> linear-light sRGB, **gamut-mapped**: L and hue are held exactly
    /// and chroma is bisected down (22 iterations) until every channel is inside
    /// [0,1]. §6.2, mandatory and automatic.
    pub fn from_oklch(v: Oklch) -> Self {
        let l = v.l.clamp(0.0, 1.0);
        let c0 = v.c.max(0.0);
        let at = |c: f32| Self::from_oklab_unmapped(Oklch { l, c, h: v.h, alpha: v.alpha }.to_oklab());
        let top = at(c0);
        if in_gamut(top) {
            return Color { a: v.alpha.clamp(0.0, 1.0), ..top.clamped() };
        }
        // C = 0 is always in gamut for L in [0,1], so the bisection is total.
        let (mut lo, mut hi) = (0.0f32, c0);
        for _ in 0..22 {
            let mid = 0.5 * (lo + hi);
            if in_gamut(at(mid)) {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        Color { a: v.alpha.clamp(0.0, 1.0), ..at(lo).clamped() }
    }

    /// The extended-range escape hatch (§6.2, last paragraph): where the
    /// downstream scRGB/PQ pipeline can carry out-of-[0,1] values, the clamp
    /// belongs to the output stage and not to the derivation.
    pub fn from_oklch_unmapped(v: Oklch) -> Self {
        Self::from_oklab_unmapped(v.to_oklab())
    }

    // ------------------------------------------------------------ contrast

    /// WCAG 2.x relative luminance. **Input must be linear light.**
    pub fn luminance(self) -> f32 {
        0.2126 * self.r + 0.7152 * self.g + 0.0722 * self.b
    }

    /// The blend the renderer really performs: `SRC_ALPHA / ONE_MINUS_SRC_ALPHA`,
    /// straight alpha, applied to the values in whatever encoding they are in.
    ///
    /// §2.2 calls this `composite_as_rendered` and is emphatic that enforcement
    /// must measure *this* and not the linear [`Color::over`], because the GPU
    /// composites in the swapchain's own encoding. It is an internal engine
    /// routine, **not** a fifteenth derivation function: it is not authorable and
    /// does not appear in `fn-name` (§6).
    pub fn composite_as_rendered(fg: Color, bg: Color) -> Color {
        let ia = 1.0 - fg.a;
        Color {
            r: fg.r * fg.a + bg.r * ia,
            g: fg.g * fg.a + bg.g * ia,
            b: fg.b * fg.a + bg.b * ia,
            a: fg.a + bg.a * ia,
        }
    }

    /// The **authoring** composite (§6 `over`): translucent `fg` onto opaque
    /// `bg`, in linear light, returning an opaque colour. This models physics;
    /// `composite_as_rendered` models the hardware. Two questions, two answers.
    pub fn over(fg: Color, bg: Color) -> Color {
        let a = fg.a + bg.a * (1.0 - fg.a);
        if a <= 0.0 {
            return Color::TRANSPARENT;
        }
        let f = |x: f32, y: f32| (x * fg.a + y * bg.a * (1.0 - fg.a)) / a;
        Color { r: f(fg.r, bg.r), g: f(fg.g, bg.g), b: f(fg.b, bg.b), a: 1.0 }
    }

    /// WCAG 2.x contrast ratio, 1.0 ..= 21.0. **Both inputs must be linear.**
    pub fn wcag_contrast(a: Color, b: Color) -> f32 {
        let (x, y) = (a.luminance(), b.luminance());
        let (hi, lo) = if x >= y { (x, y) } else { (y, x) };
        (hi + 0.05) / (lo + 0.05)
    }

    /// APCA Lc (SAPC/APCA-W3 0.1.9 G-4g), **advisory only** (§4.4 pass D 4,
    /// [CONFLICT 6]). Computed for every text pair, reported, never enforced.
    ///
    /// **Both inputs must be sRGB-encoded** — APCA's own transfer exponent is
    /// 2.4 applied to the encoded value, which is not the sRGB decode curve.
    /// Sign carries polarity: positive is dark-on-light, negative light-on-dark.
    pub fn apca_lc(text_srgb: Color, bg_srgb: Color) -> f32 {
        const TRC: f32 = 2.4;
        const BLK_THRS: f32 = 0.022;
        const BLK_CLMP: f32 = 1.414;
        const DELTA_Y_MIN: f32 = 0.0005;
        const LO_CLIP: f32 = 0.1;
        let y = |c: Color| {
            let f = |v: f32| v.clamp(0.0, 1.0).powf(TRC);
            let y = 0.212_672_9 * f(c.r) + 0.715_152_2 * f(c.g) + 0.072_175 * f(c.b);
            if y < BLK_THRS { y + (BLK_THRS - y).powf(BLK_CLMP) } else { y }
        };
        let (yt, yb) = (y(text_srgb), y(bg_srgb));
        if (yb - yt).abs() < DELTA_Y_MIN {
            return 0.0;
        }
        let sapc = if yb > yt {
            (yb.powf(0.56) - yt.powf(0.57)) * 1.14
        } else {
            (yb.powf(0.65) - yt.powf(0.62)) * 1.14
        };
        if sapc.abs() < LO_CLIP {
            0.0
        } else if sapc > 0.0 {
            (sapc - 0.027) * 100.0
        } else {
            (sapc + 0.027) * 100.0
        }
    }

    /// OKLab ΔE — plain Euclidean distance in OKLab, which is what §4.4's
    /// separation floors (0.09 .. 0.115) are calibrated against.
    pub fn delta_e_ok(a: Color, b: Color) -> f32 {
        let (x, y) = (a.to_oklab(), b.to_oklab());
        ((x.l - y.l).powi(2) + (x.a - y.a).powi(2) + (x.b - y.b).powi(2)).sqrt()
    }
}

impl Oklab {
    pub fn to_oklch(self) -> Oklch {
        let c = (self.a * self.a + self.b * self.b).sqrt();
        let h = if c < 1e-7 { 0.0 } else { self.b.atan2(self.a).to_degrees().rem_euclid(360.0) };
        Oklch { l: self.l, c, h, alpha: self.alpha }
    }
}

impl Oklch {
    pub fn to_oklab(self) -> Oklab {
        let r = self.h.to_radians();
        Oklab { l: self.l, a: self.c * r.cos(), b: self.c * r.sin(), alpha: self.alpha }
    }
}

// ------------------------------------------------------------------ helpers

fn in_gamut(c: Color) -> bool {
    const E: f32 = 1e-4;
    (-E..=1.0 + E).contains(&c.r) && (-E..=1.0 + E).contains(&c.g) && (-E..=1.0 + E).contains(&c.b)
}

fn cbrt(v: f32) -> f32 {
    if v < 0.0 { -(-v).powf(1.0 / 3.0) } else { v.powf(1.0 / 3.0) }
}

pub fn srgb_to_linear(v: f32) -> f32 {
    if v <= 0.040_45 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}

pub fn linear_to_srgb(v: f32) -> f32 {
    if v <= 0.003_130_8 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f32, b: f32, eps: f32) -> bool {
        (a - b).abs() <= eps
    }

    /// Within `n` 8-bit steps on every channel.
    fn near_hex(c: Color, hex: &str, n: i32) -> bool {
        let want = Color::from_hex(hex).unwrap();
        let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as i32;
        (q(c.r) - q(want.r)).abs() <= n
            && (q(c.g) - q(want.g)).abs() <= n
            && (q(c.b) - q(want.b)).abs() <= n
    }

    #[test]
    fn hex_forms_and_the_multibyte_trap() {
        assert_eq!(Color::from_hex("#3FE3AE"), Some(Color::rgb8(0x3F, 0xE3, 0xAE)));
        assert_eq!(Color::from_hex("3fe3ae"), Some(Color::rgb8(0x3F, 0xE3, 0xAE)));
        // digit doubling
        assert_eq!(Color::from_hex("#abc"), Color::from_hex("#aabbcc"));
        assert_eq!(Color::from_hex("#abcd"), Color::from_hex("#aabbccdd"));
        // straight alpha: #RRGGBBAA does not scale rgb
        let c = Color::from_hex("#3FE3AE80").unwrap();
        assert!(approx(c.r, 0x3F as f32 / 255.0, 1e-6));
        assert!(approx(c.a, 128.0 / 255.0, 1e-6));
        // six BYTES, two chars: must not panic on a mid-character slice
        assert!(Color::from_hex("#\u{20ac}\u{20ac}").is_none());
        assert!(Color::from_hex("#zzzzzz").is_none());
        assert!(Color::from_hex("#fffff").is_none());
    }

    #[test]
    fn transfer_curve_round_trips() {
        for i in 0..=255u32 {
            let v = i as f32 / 255.0;
            assert!(approx(linear_to_srgb(srgb_to_linear(v)), v, 1e-5), "at {v}");
        }
    }

    #[test]
    fn oklab_anchors() {
        let w = Color::WHITE.to_oklab();
        assert!(approx(w.l, 1.0, 1e-3), "white L = {}", w.l);
        assert!(approx(w.a, 0.0, 1e-3) && approx(w.b, 0.0, 1e-3));
        let k = Color::BLACK.to_oklab();
        assert!(approx(k.l, 0.0, 1e-4));
        // round trip through OKLCh for an in-gamut colour
        let src = Color::from_hex("#3FE3AE").unwrap().to_linear();
        let back = Color::from_oklch(src.to_oklch());
        assert!(Color::delta_e_ok(src, back) < 1e-3, "ΔE {}", Color::delta_e_ok(src, back));
    }

    #[test]
    fn gamut_map_reduces_chroma_and_never_clamps_per_channel() {
        // Wildly out-of-gamut: L held, hue held, chroma bisected down (§6.2).
        let want = Oklch { l: 0.75, c: 0.40, h: 150.0, alpha: 1.0 };
        let got = Color::from_oklch(want);
        let back = got.to_oklch();
        assert!(approx(back.l, want.l, 2e-3), "L moved: {} -> {}", want.l, back.l);
        let dh = (back.h - want.h).abs();
        assert!(dh < 1.0, "hue moved {dh} deg");
        assert!(back.c < want.c, "chroma not reduced: {} -> {}", want.c, back.c);
        assert!(back.c > 0.05, "chroma over-reduced to {}", back.c);
        // A per-channel clamp would have moved the hue; this is the exact
        // failure §6.2 forbids.
        let naive = Color::from_oklch_unmapped(want).clamped();
        assert!((naive.to_oklch().h - want.h).abs() > dh);
    }

    #[test]
    fn wcag_extremes_and_a_known_pair() {
        assert!(approx(Color::wcag_contrast(Color::WHITE, Color::BLACK), 21.0, 1e-3));
        assert!(approx(Color::wcag_contrast(Color::WHITE, Color::WHITE), 1.0, 1e-6));
        // the azure #29B6F6 chip: WCAG says dark text (§6 contrast_on).
        let chip = Color::from_hex("#29B6F6").unwrap().to_linear();
        let vs_black = Color::wcag_contrast(chip, Color::BLACK);
        let vs_white = Color::wcag_contrast(chip, Color::WHITE);
        assert!(vs_black > vs_white, "black {vs_black} white {vs_white}");
    }

    #[test]
    fn apca_polarity_and_direction() {
        let black = Color::BLACK;
        let white = Color::WHITE;
        // light text on dark bg is the reverse polarity: negative Lc.
        assert!(Color::apca_lc(white, black) < -90.0);
        // dark on light is positive.
        assert!(Color::apca_lc(black, white) > 90.0);
        // equal colours produce no signal at all.
        assert_eq!(Color::apca_lc(white, white), 0.0);
    }

    #[test]
    fn composite_as_rendered_differs_from_linear_over() {
        // §4.4: `#15201B / 0.82` over `#0B1310` is #131E1A as rendered and
        // #141F1A when composited in linear light. The two must not agree, or
        // enforcing on the wrong one would be harmless and the spec pointless.
        let fg_s = Color::from_hex("#15201B").unwrap().alpha(0.82);
        let bg_s = Color::from_hex("#0B1310").unwrap();
        let rendered = Color::composite_as_rendered(fg_s, bg_s);
        let authored = Color::over(fg_s.to_linear(), bg_s.to_linear()).to_srgb();
        assert_ne!(rendered.to_hex(), authored.to_hex());
        // The spec quotes #131E1A and #141F1A; both land within one 8-bit step
        // of that, the difference being how it rounded 0.82. What matters — and
        // what §4.4 is built on — is that the two answers are NOT the same.
        assert!(near_hex(rendered, "#131E1A", 1), "as-rendered {}", rendered.to_hex());
        assert!(near_hex(authored, "#141F1A", 1), "authored {}", authored.to_hex());
    }

    #[test]
    fn over_returns_opaque_and_transparent_is_absorbing() {
        let fg = Color::new(1.0, 0.0, 0.0, 0.0);
        let bg = Color::new(0.0, 0.0, 1.0, 1.0);
        let out = Color::over(fg, bg);
        assert_eq!(out.a, 1.0);
        assert!(approx(out.b, 1.0, 1e-6) && approx(out.r, 0.0, 1e-6));
    }

    #[test]
    fn alpha_sets_fade_multiplies() {
        let c = Color::WHITE.alpha(0.5);
        assert_eq!(c.alpha(0.25).a, 0.25);
        assert_eq!(c.fade(0.5).a, 0.25);
        assert_eq!(c.fade(4.0).a, 1.0); // clamped
        // and neither touches rgb — straight alpha, never premultiplied
        assert_eq!(c.fade(0.5).r, 1.0);
    }
}
