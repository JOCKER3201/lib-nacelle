//! The decoration plate — the CPU rasteriser of DECISION M10 / r1 §8.
//!
//! All STATIC decoration is baked once, on the CPU, into one screen-sized
//! RGBA image — the BACKDROP plate: z 0, under every panel, inside the
//! glass snapshot. Per frame it costs one image quad (6 verts); the bake
//! itself runs only when the theme or the surface size changes, never per
//! frame. The application registers the pixels through the renderer's
//! ordinary image path (`create_texture` / `update_texture`) and draws
//! them with `DrawList::image` before anything else.
//!
//! v1 bakes the three backdrop layers the console family uses, in the
//! z-order the master documents for `backdrop.plate.layers = []`
//! (traces, grid, starfield, vignette):
//!
//! * `decor.traces.*`  — PCB traces: a seeded random walk on a cell grid.
//!   The `seed` token pins the pattern; `0` derives it from the theme's
//!   name, so two themes differ without either authoring a number.
//! * `decor.grid.*`    — the measuring grid, minor and major lines.
//! * `decor.vignette.*`— the corner darkening. The master lets a theme
//!   put it on either plate; until the OVERLAY plate exists (v2) both
//!   words land here, on the backdrop — behind the panels instead of
//!   over them, stated rather than silent.
//!
//! NOT plates, deliberately:
//!
//! * `decor.scanlines.*` and `decor.noise.*` belong to the overlay plate
//!   (z 70, over the panels) and scanline drift is per-frame UV motion —
//!   r1 §8.2 moves both to tiny `REPEAT` tiles. v2, with the overlay.
//! * `decor.ribbons.*` are the ONLY animated decoration: real geometry
//!   every frame, drawn by the host inside its panel, never baked.
//! * `decor.starfield.*` is a backdrop layer (between grid and vignette
//!   in the stated order) but no v1 theme enables it. v2.
//!
//! Every colour and length below comes from a `decor.*` token; a token
//! the master does not declare degrades through the engine's per-kind
//! fallback (grey ink, zero, false) exactly like every other draw site —
//! there is no design constant in this file. With every layer off — which
//! is `default.theme`'s shipped state — [`bake_backdrop`] returns `None`
//! and the program draws no plate at all: the governing principle's raw
//! run grows no decoration.
//!
//! Measured cost (Ryzen 7 9800X3D, release): a 2560x1440 bake with
//! aurora's traces on lands in ~5 ms; all three layers stay inside the
//! tens-of-ms budget. The application runs it on a worker thread
//! besides, so even a slow bake never blocks a frame.

use super::bake::ResolvedTheme;
use super::color::Color;
use super::TokenId;

/// One baked plate: tightly packed straight-alpha RGBA, `w * h * 4`
/// bytes — exactly what `Gfx::update_texture` takes.
pub struct Plate {
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
    /// How long the bake took, for the log line and for the budget.
    pub bake_ms: f32,
}

/// Bake the backdrop plate for the CURRENT resolved theme at the given
/// surface size. `None` when nothing is enabled — the caller then draws
/// no quad and owns no texture. Reads the theme once at entry, so a
/// theme swap mid-bake cannot mix two designs.
pub fn bake_backdrop(w: u32, h: u32) -> Option<Plate> {
    let t = super::resolved();
    let p = gather(t);
    if p.is_empty() || w == 0 || h == 0 {
        return None;
    }
    Some(bake_params(&p, w, h))
}

// ------------------------------------------------------------ parameters

#[derive(Clone, Copy)]
enum Falloff {
    Cos2,
    Linear,
    Quad,
}

#[derive(Clone, Copy)]
struct TracesP {
    cell: f32,
    density: f32,
    width: f32,
    color: Color,
    alpha: f32,
    via_radius: f32,
    via_alpha: f32,
    seed: u64,
}

#[derive(Clone, Copy)]
struct GridP {
    spacing: f32,
    width: f32,
    alpha: f32,
    major_every: u32,
    major_alpha: f32,
    color: Color,
}

#[derive(Clone, Copy)]
struct VignetteP {
    strength: f32,
    radius: f32,
    color: Color,
    shape: Falloff,
}

#[derive(Default)]
struct Params {
    traces: Option<TracesP>,
    grid: Option<GridP>,
    vignette: Option<VignetteP>,
}

impl Params {
    fn is_empty(&self) -> bool {
        self.traces.is_none() && self.grid.is_none() && self.vignette.is_none()
    }
}

/// The cold-path token reads. Runs on a rebake only, so the by-name
/// lookups are fine here — this is exactly the "resolve at init, not in
/// the draw loop" split, with the bake standing in for init.
fn gather(t: &ResolvedTheme) -> Params {
    let id = |name: &str| super::id(name).unwrap_or(TokenId::MISSING);
    let mut out = Params::default();

    // The master switch, and the user's ceiling over the theme:
    // `performance.decor = none` means no plates at all.
    if !t.flag(id("decor.enabled")) {
        return out;
    }
    if let Some(perf) = super::id("performance.decor") {
        if super::enum_index(perf, "none") == Some(t.enum_of(perf)) {
            return out;
        }
    }

    if t.flag(id("decor.traces.enabled")) {
        let seed = t.px(id("decor.traces.seed"));
        out.traces = Some(TracesP {
            cell: t.px(id("decor.traces.cell")),
            density: t.px(id("decor.traces.density")),
            width: t.px(id("decor.traces.width")),
            color: t.color(id("decor.traces.color")),
            alpha: t.px(id("decor.traces.alpha")),
            via_radius: t.px(id("decor.traces.via_radius")),
            via_alpha: t.px(id("decor.traces.via_alpha")),
            // seed = 0 derives from the theme's name, as the token's own
            // comment specifies — two silent themes still differ.
            seed: if seed != 0.0 {
                seed as u64
            } else {
                fnv(super::diagnostics().localised_name("").as_bytes())
            },
        });
    }

    if t.flag(id("decor.grid.enabled")) {
        // The master declares no `decor.grid.color`; the read degrades
        // through the engine's kind fallback (RAW ink) rather than any
        // constant of this file's choosing.
        out.grid = Some(GridP {
            spacing: t.px(id("decor.grid.spacing")),
            width: t.px(id("decor.grid.width")),
            alpha: t.px(id("decor.grid.alpha")),
            major_every: t.px(id("decor.grid.major_every")).round().max(0.0) as u32,
            major_alpha: t.px(id("decor.grid.major_alpha")),
            color: t.color(id("decor.grid.color")),
        });
    }

    if t.flag(id("decor.vignette.enabled")) {
        let shape = super::id("decor.vignette.shape");
        let e = shape.map(|s| t.enum_of(s));
        let word = |w: &str| shape.and_then(|s| super::enum_index(s, w));
        out.vignette = Some(VignetteP {
            strength: t.px(id("decor.vignette.strength")),
            radius: t.px(id("decor.vignette.radius")),
            color: t.color(id("decor.vignette.color")),
            // Index 0 of an enum's word list is the master's own declared
            // word (`cos2`), so the unmatched arm IS the kind fallback.
            shape: if e.is_some() && e == word("linear") {
                Falloff::Linear
            } else if e.is_some() && e == word("quad") {
                Falloff::Quad
            } else {
                Falloff::Cos2
            },
        });
    }

    out
}

fn fnv(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

// ------------------------------------------------------------ the bake

fn bake_params(p: &Params, w: u32, h: u32) -> Plate {
    let t0 = std::time::Instant::now();
    let (wi, hi) = (w as usize, h as usize);
    let mut rgba = vec![0u8; wi * hi * 4];

    // Each single-colour layer rasterises into an R8 coverage first and
    // composites ONCE: overlapping stamps within a layer meet as max(),
    // so a trace crossing itself (or a grid crossing) cannot darken
    // beyond the alpha its token states. The buffer is reused.
    let mut cov = vec![0u8; wi * hi];

    if let Some(tr) = &p.traces {
        cov.fill(0);
        rasterise_traces(&mut cov, wi, hi, tr);
        composite_coverage(&mut rgba, &cov, tr.color);
    }
    if let Some(g) = &p.grid {
        cov.fill(0);
        rasterise_grid(&mut cov, wi, hi, g);
        composite_coverage(&mut rgba, &cov, g.color);
    }
    // (v2: decor.starfield sits here, between grid and vignette.)
    if let Some(v) = &p.vignette {
        rasterise_vignette(&mut rgba, wi, hi, v);
    }

    Plate {
        w,
        h,
        rgba,
        bake_ms: t0.elapsed().as_secs_f32() * 1000.0,
    }
}

/// Straight-alpha OVER of `color` scaled by the coverage, per pixel.
fn composite_coverage(rgba: &mut [u8], cov: &[u8], color: Color) {
    for (i, &c) in cov.iter().enumerate() {
        if c == 0 {
            continue;
        }
        let a = color.a * (c as f32 / 255.0);
        blend_px(rgba, i, color.r, color.g, color.b, a);
    }
}

/// src OVER dst in straight alpha, one pixel.
#[inline]
fn blend_px(rgba: &mut [u8], i: usize, r: f32, g: f32, b: f32, a: f32) {
    if a <= 0.0 {
        return;
    }
    let o = i * 4;
    let da = rgba[o + 3] as f32 / 255.0;
    let oa = a + da * (1.0 - a);
    if oa <= 0.0 {
        return;
    }
    let mix = |s: f32, d: u8| {
        let d = d as f32 / 255.0;
        ((s * a + d * da * (1.0 - a)) / oa * 255.0).round().clamp(0.0, 255.0) as u8
    };
    rgba[o] = mix(r, rgba[o]);
    rgba[o + 1] = mix(g, rgba[o + 1]);
    rgba[o + 2] = mix(b, rgba[o + 2]);
    rgba[o + 3] = (oa * 255.0).round().clamp(0.0, 255.0) as u8;
}

// ----------------------------------------------------------- coverage ops

#[inline]
fn stamp_max(cov: &mut [u8], w: usize, h: usize, x: i64, y: i64, v: u8) {
    if x >= 0 && y >= 0 && (x as usize) < w && (y as usize) < h {
        let i = y as usize * w + x as usize;
        if cov[i] < v {
            cov[i] = v;
        }
    }
}

fn fill_box(cov: &mut [u8], w: usize, h: usize, x0: i64, y0: i64, x1: i64, y1: i64, v: u8) {
    let x0 = x0.max(0);
    let y0 = y0.max(0);
    let x1 = x1.min(w as i64);
    let y1 = y1.min(h as i64);
    for y in y0..y1 {
        for x in x0..x1 {
            let i = y as usize * w + x as usize;
            if cov[i] < v {
                cov[i] = v;
            }
        }
    }
}

fn fill_disc(cov: &mut [u8], w: usize, h: usize, cx: f32, cy: f32, r: f32, v: u8) {
    if r <= 0.0 {
        return;
    }
    let r2 = r * r;
    let x0 = (cx - r).floor() as i64;
    let x1 = (cx + r).ceil() as i64 + 1;
    let y0 = (cy - r).floor() as i64;
    let y1 = (cy + r).ceil() as i64 + 1;
    for y in y0..y1 {
        for x in x0..x1 {
            let dx = x as f32 + 0.5 - cx;
            let dy = y as f32 + 0.5 - cy;
            if dx * dx + dy * dy <= r2 {
                stamp_max(cov, w, h, x, y, v);
            }
        }
    }
}

/// A hard-edged thick segment: square stamps along the line, like every
/// other silhouette in this pipeline — nothing here is antialiased.
fn stamp_segment(
    cov: &mut [u8],
    w: usize,
    h: usize,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    width: f32,
    v: u8,
) {
    let half = (width * 0.5).max(0.5); // raster floor, not a design value
    let dx = x1 - x0;
    let dy = y1 - y0;
    let len = (dx * dx + dy * dy).sqrt();
    if len < 0.5 {
        fill_box(
            cov, w, h,
            (x0 - half).round() as i64,
            (y0 - half).round() as i64,
            (x0 + half).round() as i64,
            (y0 + half).round() as i64,
            v,
        );
        return;
    }
    let steps = (len / half.min(1.0)).ceil() as usize;
    for s in 0..=steps {
        let t = s as f32 / steps as f32;
        let px = x0 + dx * t;
        let py = y0 + dy * t;
        let bx0 = (px - half).round() as i64;
        let by0 = (py - half).round() as i64;
        let bx1 = ((px + half).round() as i64).max(bx0 + 1);
        let by1 = ((py + half).round() as i64).max(by0 + 1);
        fill_box(cov, w, h, bx0, by0, bx1, by1, v);
    }
}

// -------------------------------------------------------------- layers

/// The eight PCB walk directions: the four axes and the four diagonals.
const DIRS: [(i64, i64); 8] = [
    (1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1), (0, -1), (1, -1),
];

/// splitmix64 — tiny, seedable, deterministic: the `seed` token IS the
/// pattern, and a re-bake at the same size reproduces it bit for bit.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E3779B97F4A7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
    fn frac(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }
}

fn rasterise_traces(cov: &mut [u8], w: usize, h: usize, p: &TracesP) {
    let cell = p.cell.max(1.0);
    let cols = ((w as f32 / cell).floor() as i64).max(1);
    let rows = ((h as f32 / cell).floor() as i64).max(1);
    let line_v = (p.alpha.clamp(0.0, 1.0) * 255.0).round() as u8;
    let via_v = (p.via_alpha.clamp(0.0, 1.0) * 255.0).round() as u8;
    if line_v == 0 && via_v == 0 {
        return;
    }
    // `density` is the fraction of cells that carry a trace; the walk
    // runs until it has stepped through that many cells (or gives up —
    // the guard keeps a degenerate density from spinning).
    let budget = ((cols * rows) as f32 * p.density.clamp(0.0, 1.0)) as i64;
    let mut rng = Rng(p.seed);
    let centre = |c: i64, r: i64| (c as f32 * cell + cell * 0.5, r as f32 * cell + cell * 0.5);
    let mut covered: i64 = 0;
    let mut guard = cols * rows * 4;
    while covered < budget && guard > 0 {
        guard -= 1;
        let mut cx = rng.below(cols as u64) as i64;
        let mut cy = rng.below(rows as u64) as i64;
        let mut dir = rng.below(8) as usize;
        let len = 3 + rng.below(12) as i64;
        let (sx, sy) = centre(cx, cy);
        fill_disc(cov, w, h, sx, sy, p.via_radius, via_v);
        for _ in 0..len {
            // PCB bends: an occasional 45-degree turn, never a U-turn.
            let turn = rng.frac();
            if turn < 0.18 {
                dir = (dir + 1) % 8;
            } else if turn < 0.36 {
                dir = (dir + 7) % 8;
            }
            let (dx, dy) = DIRS[dir];
            let (nx, ny) = (cx + dx, cy + dy);
            if nx < 0 || ny < 0 || nx >= cols || ny >= rows {
                break;
            }
            let (ax, ay) = centre(cx, cy);
            let (bx, by) = centre(nx, ny);
            stamp_segment(cov, w, h, ax, ay, bx, by, p.width, line_v);
            cx = nx;
            cy = ny;
            covered += 1;
        }
        let (ex, ey) = centre(cx, cy);
        fill_disc(cov, w, h, ex, ey, p.via_radius, via_v);
    }
}

fn rasterise_grid(cov: &mut [u8], w: usize, h: usize, p: &GridP) {
    if p.spacing < 1.0 {
        return; // sub-pixel spacing would be a solid fill, not a grid
    }
    let minor_v = (p.alpha.clamp(0.0, 1.0) * 255.0).round() as u8;
    let major_v = (p.major_alpha.clamp(0.0, 1.0) * 255.0).round() as u8;
    let lw = p.width.max(0.0);
    if lw <= 0.0 || (minor_v == 0 && major_v == 0) {
        return;
    }
    let value = |k: i64| {
        if p.major_every >= 2 && k % p.major_every as i64 == 0 {
            major_v
        } else {
            minor_v
        }
    };
    let mut k: i64 = 0;
    loop {
        let x = (k as f32 * p.spacing).round() as i64;
        if x >= w as i64 {
            break;
        }
        fill_box(cov, w, h, x, 0, x + lw.round().max(1.0) as i64, h as i64, value(k));
        k += 1;
    }
    let mut k: i64 = 0;
    loop {
        let y = (k as f32 * p.spacing).round() as i64;
        if y >= h as i64 {
            break;
        }
        fill_box(cov, w, h, 0, y, w as i64, y + lw.round().max(1.0) as i64, value(k));
        k += 1;
    }
}

fn rasterise_vignette(rgba: &mut [u8], w: usize, h: usize, p: &VignetteP) {
    let strength = p.strength.clamp(0.0, 1.0);
    if strength <= 0.0 {
        return;
    }
    let cx = w as f32 * 0.5;
    let cy = h as f32 * 0.5;
    let half_diag = (cx * cx + cy * cy).sqrt().max(1.0);
    let r0 = p.radius.clamp(0.0, 1.0);
    let inner2 = (r0 * half_diag) * (r0 * half_diag);
    let denom = (1.0 - r0).max(1e-3);
    for y in 0..h {
        let dy = y as f32 + 0.5 - cy;
        let dy2 = dy * dy;
        for x in 0..w {
            let dx = x as f32 + 0.5 - cx;
            let d2 = dx * dx + dy2;
            if d2 <= inner2 {
                continue; // inside the untouched radius
            }
            let t = (((d2.sqrt() / half_diag) - r0) / denom).clamp(0.0, 1.0);
            let f = match p.shape {
                Falloff::Linear => t,
                Falloff::Quad => t * t,
                // The photographic falloff the master documents: 0 at the
                // radius, 1 at the corner, cosine-squared in between.
                Falloff::Cos2 => {
                    let c = (t * std::f32::consts::FRAC_PI_2).sin();
                    c * c
                }
            };
            blend_px(rgba, y * w + x, p.color.r, p.color.g, p.color.b, strength * f);
        }
    }
}

// --------------------------------------------------------------- tests

#[cfg(test)]
mod tests {
    use super::*;

    fn grey(a: f32) -> Color {
        Color { r: 0.5, g: 0.5, b: 0.5, a }
    }

    /// The seed IS the pattern: two bakes with the same parameters are
    /// identical bytes, and a different seed is a different field.
    #[test]
    fn the_trace_walk_is_reproducible_from_its_seed() {
        let p = |seed| Params {
            traces: Some(TracesP {
                cell: 12.0,
                density: 0.3,
                width: 1.0,
                color: grey(1.0),
                alpha: 0.4,
                via_radius: 1.5,
                via_alpha: 0.6,
                seed,
            }),
            ..Default::default()
        };
        let a = bake_params(&p(11172124), 320, 180);
        let b = bake_params(&p(11172124), 320, 180);
        let c = bake_params(&p(7), 320, 180);
        assert_eq!(a.rgba, b.rgba);
        assert_ne!(a.rgba, c.rgba, "two seeds baked one pattern");
        assert!(a.rgba.iter().any(|&v| v != 0), "the walk drew nothing");
    }

    /// Coverage compositing is max(), so a self-crossing trace can never
    /// exceed the alpha its token states.
    #[test]
    fn a_layer_never_exceeds_its_own_alpha() {
        let p = Params {
            traces: Some(TracesP {
                cell: 6.0,
                density: 1.0,
                width: 3.0,
                color: grey(1.0),
                alpha: 0.25,
                via_radius: 0.0,
                via_alpha: 0.0,
                seed: 3,
            }),
            ..Default::default()
        };
        let plate = bake_params(&p, 128, 128);
        let max_a = plate.rgba.chunks(4).map(|px| px[3]).max().unwrap();
        assert!(max_a <= (0.25f32 * 255.0).round() as u8 + 1, "alpha {max_a}");
    }

    /// The vignette is zero inside its radius and rises to `strength`
    /// at the corner, monotonically, for every declared falloff word.
    #[test]
    fn the_vignette_rises_from_radius_to_corner() {
        for shape in [Falloff::Cos2, Falloff::Linear, Falloff::Quad] {
            let p = Params {
                vignette: Some(VignetteP {
                    strength: 0.55,
                    radius: 0.5,
                    color: grey(1.0),
                    shape,
                }),
                ..Default::default()
            };
            let plate = bake_params(&p, 200, 200);
            let a = |x: usize, y: usize| plate.rgba[(y * 200 + x) * 4 + 3];
            assert_eq!(a(100, 100), 0, "centre must stay untouched");
            let corner = a(0, 0);
            let mid = a(25, 25);
            assert!(corner as f32 >= 0.5 * 255.0 * 0.9, "corner {corner}");
            assert!(corner >= mid, "not monotone: corner {corner} < mid {mid}");
        }
    }

    /// The governing principle's own check: the embedded master ships
    /// every decor layer OFF, so the raw run grows no decoration — a
    /// plate is a thing a theme turns on, never a default. Built from
    /// the embedded text directly, so no environment variable and no
    /// user overlay on the machine running the tests can vote.
    #[test]
    fn the_default_master_ships_every_decor_layer_off() {
        use super::super::{bake, cascade::Schema, parse, resolve, BakeInput};
        let mut out = Vec::new();
        let mut src = parse::Sources::new();
        let f = src.add("default.theme", super::super::DEFAULT_THEME);
        let doc = parse::parse(&mut src, f, None, &mut out);
        let mut schema = Schema::from_default(&doc, &mut out);
        let r = resolve::resolve_default(&schema, &mut out);
        schema.adopt_kinds(&r.values);
        let rr = resolve::resolve(&schema, &schema.base_spec(), &mut out);
        let t = bake::bake(&schema, &rr, &BakeInput::default(), &mut out);
        for name in [
            "decor.enabled",
            "decor.traces.enabled",
            "decor.grid.enabled",
            "decor.starfield.enabled",
            "decor.vignette.enabled",
            "decor.scanlines.enabled",
            "decor.noise.enabled",
            "decor.ribbons.enabled",
        ] {
            let id = schema.id(name).unwrap_or_else(|| panic!("{name} not declared"));
            assert!(!t.flag(id), "{name} must ship OFF in the master");
        }
    }
}
