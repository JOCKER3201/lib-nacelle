//! Resolved -> [`ResolvedTheme`]: geometry made final.
//!
//! ```text
//! u = clamp(screen_h * metric.unit_pct_h / 100, metric.unit_min_px,
//!           metric.unit_max_px) * metric.ui_scale
//! ```
//!
//! Every length becomes absolute px, strokes are rounded, ratios are multiplied
//! out, `%` bakes to a 0..1 fraction and the §5.0 sentinels fold to their `f32`.
//! This stage re-runs on load, on resize and on a density / ui-scale change —
//! **never per frame** (§2.2 step 4).
//!
//! ### The shape, and why it is not §7's
//!
//! §7 asks for a `ResolvedTheme` with one named field per token, fed by a
//! generated `tokens.rs`. Under "`default.theme` is the schema" (see
//! `cascade.rs`) there is no generated table to name those fields from, so the
//! struct is four parallel arrays indexed by [`TokenId`]:
//!
//! ```text
//! colors:  Box<[Color]>    scalars: Box<[f32]>
//! flags:   Box<[bool]>     enums:   Box<[u16]>
//! ```
//!
//! No strings, no `Vec`, no `HashMap`, no `Option` on a draw path; diagnostics
//! live in `ThemeDiagnostics` beside it. **Every promise §7.2 makes about the
//! per-frame budget survives**: a read is `theme.colors[id]`, one bounds-checked
//! slice index, the same cost as a struct field — no hash, no probe, no string,
//! no allocation while drawing. At ~2 190 tokens the four arrays come to ≈50 KB,
//! which is the figure §7.2 budgets for, and eight resolved siblings is ≈400 KB
//! against a process already holding a 4 MB glyph atlas.
//!
//! The arrays are addressed by the *global* token id rather than by a per-kind
//! slot so that the accessor needs nothing but the id: a widget holding
//! `static ID: OnceLock<TokenId>` reads without consulting the schema at all.
//!
//! Not in this stage: `encode.rs` (§6.3). The format-keyed decision — sRGB-encode
//! on `FormatKind::Unorm`, leave linear on `ScRgbLinear` — depends on the live
//! swapchain, so this module performs the `Unorm` encode unconditionally, which
//! is exactly today's behaviour, and the choice moves to `encode.rs` when the
//! swapchain format is available. `mask.rs` and `plate.rs` (M0/M10/M11) and
//! `abi.rs` (§7.4) are renderer-side and later still.

use super::cascade::{Schema, TokenId};
use super::color::Color;
use super::expr::{Kind, Unit, Value};
use super::parse::{Diagnostic, Span};
use super::resolve::Resolved;

// --------------------------------------------------------------- the struct

/// One rung of the state ladder, baked for one class: everything a control
/// needs to draw itself in one interaction state, resolved against the
/// class's own base colour. Colours are in the output encoding, lengths in
/// device px — the same contract as the flat arrays.
#[derive(Clone, Copy, Debug)]
pub struct StateStyle {
    pub fill: Color,
    pub edge: Color,
    pub text: Color,
    pub glyph: Color,
    pub edge_width: f32,
    pub glow_radius: f32,
    pub glow_alpha: f32,
    pub elevation: f32,
}

impl StateStyle {
    /// The per-kind raw look (§governing principle): grey ink, no fill, one
    /// hairline. What a control looks like when no theme says otherwise.
    pub const RAW: StateStyle = StateStyle {
        fill: Color::TRANSPARENT,
        edge: Color::GREY,
        text: Color::GREY,
        glyph: Color::GREY,
        edge_width: 1.0,
        glow_radius: 0.0,
        glow_alpha: 0.0,
        elevation: 0.0,
    };
}

/// The theme a frame is drawn from. Pure data, cheap to clone, safe to hand out
/// by reference for the life of an epoch.
#[derive(Clone)]
pub struct ResolvedTheme {
    colors: Box<[Color]>,
    scalars: Box<[f32]>,
    flags: Box<[bool]>,
    enums: Box<[u16]>,
    /// Increments whenever the host swaps the resolved theme (reload, mood,
    /// variant, resize, format change). A widget caching derived geometry
    /// invalidates on a change (§7.4).
    pub epoch: u32,
    /// `u` in device px, for the callers that scale something the theme does
    /// not name.
    pub unit_px: f32,
    pub density_space: f32,
    pub density_type: f32,
    /// The class x state matrix: `class_states[class * 7 + state]`. Classes
    /// are the `class.*` tokens in declaration order; states are
    /// [`super::parse::State`] in its own order. Empty when the master
    /// declares no `[class]` block, in which case every lookup answers RAW.
    class_states: Box<[StateStyle]>,
    class_count: u16,
}

impl ResolvedTheme {
    /// The baked ladder for one class in one state. `class` is an index from
    /// [`super::class_id`], resolved once by name at init exactly like a
    /// TokenId; out of range answers [`StateStyle::RAW`], never panics.
    #[inline]
    pub fn class_state(&self, class: u16, state: super::parse::State) -> StateStyle {
        let i = class as usize * 7 + state as usize;
        self.class_states.get(i).copied().unwrap_or(StateStyle::RAW)
    }

    pub fn class_count(&self) -> u16 {
        self.class_count
    }

    /// The engine's raw INK: what a colour that gets DRAWN answers when no
    /// theme declares it. Mid grey — visibly unstyled, still legible.
    pub const RAW_INK: Color = Color::GREY;
    /// The engine's raw BED: what a colour that gets FILLED answers when no
    /// theme declares it. Near-black, the way a browser's unstyled page is
    /// white: one grey for everything made the themeless program an
    /// unreadable slab, and legible-but-undesigned needs exactly two
    /// achromatic values, not one.
    pub const RAW_BED: Color = Color { r: 0.05, g: 0.05, b: 0.05, a: 1.0 };

    /// A colour used as ink — text, lines, glyphs, edges. Missing = RAW_INK.
    #[inline]
    pub fn color(&self, id: TokenId) -> Color {
        match self.colors.get(id.index()) {
            Some(c) => *c,
            None => Self::RAW_INK,
        }
    }

    /// A colour used as a bed — fills, backgrounds, the canvas. Missing =
    /// RAW_BED. The split lives in the ACCESSOR because only the call site
    /// knows whether its colour is painted on or painted under; both remain
    /// engine constants, not designs.
    #[inline]
    pub fn bed(&self, id: TokenId) -> Color {
        match self.colors.get(id.index()) {
            Some(c) => *c,
            None => Self::RAW_BED,
        }
    }

    /// A length, already in absolute device px — or a plain scalar, a duration
    /// in ms, an angle in degrees, a 0..1 fraction, or one of §5.0's negative
    /// sentinels. A consumer testing `if v < 0.0` handles all four sentinels.
    #[inline]
    pub fn px(&self, id: TokenId) -> f32 {
        match self.scalars.get(id.index()) {
            Some(v) => *v,
            None => 0.0,
        }
    }

    #[inline]
    pub fn flag(&self, id: TokenId) -> bool {
        match self.flags.get(id.index()) {
            Some(v) => *v,
            None => false,
        }
    }

    /// The index of the token's word in its declared enum list. Resolve the
    /// index you are comparing against once, with `Schema::enum_index`, and
    /// keep it in a `static` beside the `TokenId`.
    #[inline]
    pub fn enum_of(&self, id: TokenId) -> u16 {
        match self.enums.get(id.index()) {
            Some(v) => *v,
            None => 0,
        }
    }

    pub fn len(&self) -> usize {
        self.colors.len()
    }

    pub fn is_empty(&self) -> bool {
        self.colors.is_empty()
    }

    /// The bytes this theme occupies, for the §7.2 budget test.
    pub fn size_of(&self) -> usize {
        let n = self.colors.len();
        n * (std::mem::size_of::<Color>() + 4 + 1 + 2)
            + self.class_states.len() * std::mem::size_of::<StateStyle>()
            + std::mem::size_of::<Self>()
    }
}

// ------------------------------------------------------------------- inputs

/// What only the host knows: how tall the window is and what the user asked for.
#[derive(Clone, Copy, Debug)]
pub struct Viewport {
    /// Viewport **height** in device px. Height, not width: every existing
    /// metric in this codebase is height-derived and a console is a stack of
    /// rows (§5.3).
    pub screen_h: f32,
    /// The user's `UIFontSize=` / 100, multiplied in after the clamp.
    pub ui_scale: f32,
}

impl Default for Viewport {
    fn default() -> Self {
        Viewport { screen_h: 1080.0, ui_scale: 1.0 }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct BakeInput {
    pub viewport: Viewport,
    pub epoch: u32,
    /// Whether a stage after `default` pinned `metric.density_space` /
    /// `metric.density_type`. §5.3's precedence rule cannot be derived from the
    /// cascade — an explicit float wins over the enum **for that axis only** —
    /// so the loader states it here.
    pub explicit_density: (bool, bool),
}

/// §5.3's density levels. `metric.density` is a *generator* of the two floats,
/// not their peer.
pub fn density_level(word: &str) -> Option<(f32, f32)> {
    Some(match word {
        "airy" => (1.30, 1.06),
        "comfortable" => (1.15, 1.00),
        "compact" => (1.00, 1.00),
        "dense" => (0.85, 0.96),
        "instrument" => (0.72, 0.90),
        _ => return None,
    })
}

/// The seven `metric.*` tokens the baker itself must read by name, and the only
/// names outside the `ids` hot set that appear in engine code. Each degrades to
/// the §5.3 default with a warning if `default.theme` does not declare it.
pub const METRIC_TOKENS: [&str; 7] = [
    "metric.unit_pct_h",
    "metric.unit_min_px",
    "metric.unit_max_px",
    "metric.ui_scale",
    "metric.density",
    "metric.density_space",
    "metric.density_type",
];

#[derive(Clone, Copy, Debug)]
pub struct Metrics {
    pub u: f32,
    pub density_space: f32,
    pub density_type: f32,
}

fn num_by_name(
    schema: &Schema,
    r: &Resolved,
    name: &str,
    default: f32,
    out: &mut Vec<Diagnostic>,
) -> f32 {
    match schema.id(name).and_then(|id| r.get(id)).and_then(Value::as_num) {
        Some(v) if v.is_finite() => v,
        _ => {
            out.push(Diagnostic::warn(
                Span::default(),
                format!("default.theme does not declare \"{name}\" — using {default}"),
            ));
            default
        }
    }
}

/// `u = clamp(screen_h * unit_pct_h / 100, min, max) * ui_scale`, plus the two
/// density multipliers with §5.3's precedence applied.
pub fn metrics(
    schema: &Schema,
    r: &Resolved,
    input: &BakeInput,
    out: &mut Vec<Diagnostic>,
) -> Metrics {
    let pct = num_by_name(schema, r, "metric.unit_pct_h", 0.5, out);
    let lo = num_by_name(schema, r, "metric.unit_min_px", 4.0, out);
    let hi = num_by_name(schema, r, "metric.unit_max_px", 10.0, out);
    let theme_scale = num_by_name(schema, r, "metric.ui_scale", 1.0, out);
    let (lo, hi) = if lo <= hi { (lo, hi) } else { (hi, lo) };
    let raw = input.viewport.screen_h.max(1.0) * pct / 100.0;
    let u = raw.clamp(lo, hi) * theme_scale * input.viewport.ui_scale.max(0.01);

    // 1. the enum supplies a value for each axis...
    let level = schema
        .id("metric.density")
        .and_then(|id| r.get(id))
        .and_then(|v| match v {
            Value::Word(w) => density_level(w),
            _ => None,
        })
        .unwrap_or((1.0, 1.0));
    // 2. ...and an explicit float replaces it, for that axis only.
    let space = if input.explicit_density.0 {
        num_by_name(schema, r, "metric.density_space", level.0, out)
    } else {
        level.0
    };
    let type_ = if input.explicit_density.1 {
        num_by_name(schema, r, "metric.density_type", level.1, out)
    } else {
        level.1
    };
    Metrics { u: u.max(0.01), density_space: space.max(0.01), density_type: type_.max(0.01) }
}

// -------------------------------------------------------------- the classes

/// The three scaling classes of §5.0 — a property *of the token*, decided here
/// from the naming law rather than at the call site.
///
/// These are name **patterns**, not a list of tokens: §5.0's naming law fixes
/// what each suffix means, so `default.theme` can add a thousand keys without
/// this function learning any of them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scaling {
    /// `stroke(x) = max(1, round(x * u))`, in physical pixels, never
    /// panel-scaled and never density-scaled.
    Stroke,
    /// Multiplied by `metric.density_space`.
    Space,
    /// Multiplied by `metric.density_type`.
    Type,
    /// `u` only.
    Plain,
}

pub fn scaling_of(name: &str) -> Scaling {
    let last = name.rsplit('.').next().unwrap_or(name);
    // Density never touches strokes, corners, borders, rules or any floor.
    if name.starts_with("stroke.")
        || last == "width"
        || last == "border"
        || last == "rule"
        || last == "stroke"
        || last.ends_with("_width")
        || last.ends_with("_border")
        || last.ends_with("_rule")
        || last.ends_with("_stroke")
    {
        return Scaling::Stroke;
    }
    if name.starts_with("corner.")
        || name.starts_with("metric.unit_")
        || name.ends_with("_min_px")
        || name.ends_with("_max_px")
    {
        return Scaling::Plain;
    }
    // `metric.density_type` multiplies every `type.<role>.size`.
    if name.starts_with("type.") && (last == "size" || last.starts_with("size_")) {
        return Scaling::Type;
    }
    // `metric.density_space` multiplies the two ladders and every pad, gap,
    // inset, row/band/block height, plus the rhythm distances.
    if name.starts_with("space.") || name.starts_with("size.") || name.starts_with("rhythm.") {
        return Scaling::Space;
    }
    for m in ["pad", "gap", "inset", "row_h", "band_h", "block_h"] {
        if last == m || last.starts_with(&format!("{m}_")) || last.ends_with(&format!("_{m}")) {
            return Scaling::Space;
        }
    }
    Scaling::Plain
}

// --------------------------------------------------------------------- bake

/// Resolved + a screen height -> a theme a frame can be drawn from.
pub fn bake(
    schema: &Schema,
    r: &Resolved,
    input: &BakeInput,
    out: &mut Vec<Diagnostic>,
) -> ResolvedTheme {
    let m = metrics(schema, r, input, out);
    let n = schema.len();
    let mut colors = vec![Color::BLACK; n];
    let mut scalars = vec![0.0f32; n];
    let mut flags = vec![false; n];
    let mut enums = vec![0u16; n];

    for i in 0..n {
        let id = TokenId(i as u16);
        let Some(v) = r.get(id) else { continue };
        match v {
            Value::Color(c) => {
                // The `encode.rs` Unorm path (§6.3): an authored #3FE3AE lands
                // on screen as #3FE3AE, matching today's behaviour exactly.
                // Straight alpha throughout — the blend state is
                // SRC_ALPHA / ONE_MINUS_SRC_ALPHA and premultiplying here would
                // double-apply it ([CONFLICT 20]).
                colors[i] = c.to_srgb().clamped();
            }
            Value::Bool(b) => flags[i] = *b,
            Value::Word(w) => match super::expr::sentinel(w) {
                // A sentinel folds to its `f32` and can never reach a vertex.
                Some(s) => scalars[i] = s,
                None => enums[i] = schema.enum_index(id, w).unwrap_or(0),
            },
            Value::Codepoint(c) => scalars[i] = *c as f32,
            Value::Num(x) => scalars[i] = *x,
            Value::Len(x, u) => scalars[i] = length_px(schema.name(id), *x, *u, &m),
            // An array only exists as a gathered view; its slots are tokens.
            Value::Array(_) => {}
            Value::Text(_) => {}
        }
        // Nothing NaN is ever baked: §5.0's only legitimate "no answer" is
        // `theme_scalar()` on an unknown id, which is not this path.
        if !scalars[i].is_finite() {
            scalars[i] = 0.0;
        }
        if !colors[i].is_finite() {
            colors[i] = Color::BLACK;
        }
    }

    // A floored length is TWO tokens (§3.2). Applying the floor here is what
    // makes `px(min_hit)` already correct at the call site, and it is a naming
    // rule rather than a list: X_min_px / X_max_px bound X.
    for i in 0..n {
        let name = schema.name(TokenId(i as u16));
        if schema.kind(TokenId(i as u16)) != Kind::Scalar || scalars[i] < 0.0 {
            continue;
        }
        if let Some(lo) = schema.id(&format!("{name}_min_px")).map(|c| scalars[c.index()]) {
            if lo > 0.0 && scalars[i] < lo {
                scalars[i] = lo;
            }
        }
        if let Some(hi) = schema.id(&format!("{name}_max_px")).map(|c| scalars[c.index()]) {
            if hi > 0.0 && scalars[i] > hi {
                scalars[i] = hi;
            }
        }
    }

    // The four palette scalars (§m3): surface.lift / surface.chroma move the
    // whole surface ladder, text.lift / text.chroma the whole text ladder, in
    // OKLCh. They exist because the theme language has no arithmetic by
    // design, so "the same ladder, lifted" cannot be written as expressions —
    // it is applied here, the way density already is. The two families are
    // walked by NAME PREFIX, which is the one place the engine knows a token
    // family by its name; the scalars themselves and the scrim are excluded
    // (a scrim is an overlay, not a bed, and lifting it would brighten every
    // modal backdrop for free).
    let ladder = |colors: &mut Vec<Color>, prefix: &str, lift: f32, chroma: f32| {
        if lift == 0.0 && chroma == 1.0 {
            return;
        }
        for i in 0..n {
            let id = TokenId(i as u16);
            let name = schema.name(id);
            if !name.starts_with(prefix) || name == "surface.scrim" {
                continue;
            }
            if schema.kind(id) != Kind::Color {
                continue;
            }
            let mut p = colors[i].to_linear().to_oklch();
            p.l = (p.l + lift).clamp(0.0, 1.0);
            p.c = (p.c * chroma).max(0.0);
            colors[i] = Color::from_oklch(p).to_srgb().clamped();
        }
    };
    let scalar_of = |name: &str, dflt: f32| {
        schema.id(name).map(|c| scalars[c.index()]).unwrap_or(dflt)
    };
    ladder(
        &mut colors,
        "surface.",
        scalar_of("surface.lift", 0.0).clamp(-0.09, 0.09),
        scalar_of("surface.chroma", 1.0).clamp(0.0, 4.0),
    );
    ladder(
        &mut colors,
        "text.",
        scalar_of("text.lift", 0.0).clamp(-0.10, 0.10),
        scalar_of("text.chroma", 1.0).clamp(0.0, 3.0),
    );

    // The class x state matrix, from the raw values the resolver produced
    // with `base` bound per class. Channel mapping is by NAME within the
    // ladder — "state.<state>.<channel>" — so the matrix survives the master
    // reordering its [state] section.
    let mut class_states =
        vec![StateStyle::RAW; r.class_ids.len() * super::parse::STATE_NAMES.len()];
    {
        let chan = |state: &str, c: &str| {
            let want = format!("state.{state}.{c}");
            r.state_ids
                .iter()
                .position(|&id| schema.name(id) == want)
        };
        for (ci, row) in r.class_states.iter().enumerate() {
            for (si, state) in super::parse::STATE_NAMES.iter().enumerate() {
                let mut st = StateStyle::RAW;
                let col = |c: &str| -> Option<Color> {
                    chan(state, c)
                        .and_then(|p| row.get(p))
                        .and_then(|v| v.as_color())
                        .map(|c| c.to_srgb().clamped())
                };
                let num = |c: &str| -> Option<f32> {
                    chan(state, c).and_then(|p| row.get(p)).and_then(|v| match v {
                        Value::Num(x) => Some(*x),
                        Value::Len(x, u) => {
                            Some(length_px(&format!("state.{state}.{c}"), *x, *u, &m))
                        }
                        _ => None,
                    })
                };
                if let Some(c) = col("fill") { st.fill = c; }
                if let Some(c) = col("edge") { st.edge = c; }
                if let Some(c) = col("text") { st.text = c; }
                if let Some(c) = col("glyph") { st.glyph = c; }
                if let Some(v) = num("edge_width") { st.edge_width = v; }
                if let Some(v) = num("glow_radius") { st.glow_radius = v; }
                if let Some(v) = num("glow_alpha") { st.glow_alpha = v; }
                if let Some(v) = num("elevation") { st.elevation = v; }
                class_states[ci * super::parse::STATE_NAMES.len() + si] = st;
            }
        }
    }

    ResolvedTheme {
        class_states: class_states.into_boxed_slice(),
        class_count: r.class_ids.len() as u16,
        colors: colors.into_boxed_slice(),
        scalars: scalars.into_boxed_slice(),
        flags: flags.into_boxed_slice(),
        enums: enums.into_boxed_slice(),
        epoch: input.epoch,
        unit_px: m.u,
        density_space: m.density_space,
        density_type: m.density_type,
    }
}

/// One length to absolute device px.
pub fn length_px(name: &str, v: f32, unit: Unit, m: &Metrics) -> f32 {
    match unit {
        Unit::U | Unit::Ux | Unit::Vh | Unit::Vw => match scaling_of(name) {
            // max(1, round(x * u_global)) — from the GLOBAL u, so a hairline is
            // one physical pixel whatever a panel's own scale is doing. Without
            // this the 1 px panel borders blur to 2 px grey during a resize.
            Scaling::Stroke => (v * m.u).round().max(1.0),
            Scaling::Space => v * m.u * m.density_space,
            Scaling::Type => v * m.u * m.density_type,
            Scaling::Plain => v * m.u,
        },
        // Device px, exactly as written: the 720p defence and the 4K defence.
        Unit::Px => v,
        // A fraction of the host rect on the token's axis, baked 0..1.
        Unit::Pct => v / 100.0,
        // A multiple of the owning type role's resolved px. The role is not
        // known here; `Ctx`'s per-panel type cache (§7.1) multiplies it out
        // when it bakes `[Type; ROLE_COUNT]`.
        Unit::Em => v,
        Unit::Deg | Unit::Ms | Unit::S | Unit::Hz => v,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::cascade::{cascade, Options, Stage};
    use crate::theme::parse::{parse, Sources};
    use crate::theme::resolve::{resolve, resolve_default};

    const DEFAULT: &str = "\
[metric]
unit_pct_h = 0.5
unit_min_px = 4px
unit_max_px = 10px
ui_scale = 1.0
density = compact
density_space = 1.00
density_type = 1.00
[palette]
black = #0A100E
white = #EAF6F1
accent = #3FE3AE
[space]
2 = 1u
6 = 4u
[size]
md = 5.2u
[stroke]
hair = 0.2u
bold = 0.7u
[corner]
md = 1.2u
pill = pill
[panel]
content_pad = 2.8u
content_pad_x = same_as_parent
border = 0.2u
[a11y]
min_hit = 4.8u
min_hit_min_px = 24px
[type]
body.size = 2.4u
[decor]
enabled = true
vignette.strength = 55%
[motion]
hover.duration = 120ms
[term]
ansi = [ #000000, #CD3131 ]
";

    struct Fixture {
        schema: Schema,
        out: Vec<Diagnostic>,
    }

    fn fixture() -> Fixture {
        let mut src = Sources::new();
        let mut out = Vec::new();
        let f = src.add("default.theme", DEFAULT);
        let d = parse(&mut src, f, None, &mut out);
        let mut schema = Schema::from_default(&d, &mut out);
        let r = resolve_default(&schema, &mut out);
        schema.adopt_kinds(&r.values);
        assert!(out.is_empty(), "{out:?}");
        Fixture { schema, out }
    }

    fn bake_at(fx: &mut Fixture, screen_h: f32) -> ResolvedTheme {
        let r = resolve_default(&fx.schema, &mut fx.out);
        let input = BakeInput {
            viewport: Viewport { screen_h, ui_scale: 1.0 },
            ..Default::default()
        };
        bake(&fx.schema, &r, &input, &mut fx.out)
    }

    fn px(fx: &Fixture, t: &ResolvedTheme, name: &str) -> f32 {
        t.px(fx.schema.id(name).unwrap_or_else(|| panic!("no {name}")))
    }

    #[test]
    fn u_follows_the_five_three_table_at_every_screen_height() {
        let mut fx = fixture();
        // 1280x720 -> raw 3.60 -> floored to 4.00 (the 720p defence)
        let a = bake_at(&mut fx, 720.0);
        assert!((a.unit_px - 4.0).abs() < 1e-4, "{}", a.unit_px);
        // 1920x1080 -> 5.40, the reference
        let b = bake_at(&mut fx, 1080.0);
        assert!((b.unit_px - 5.4).abs() < 1e-4, "{}", b.unit_px);
        // 2560x1440 -> 7.20
        let c = bake_at(&mut fx, 1440.0);
        assert!((c.unit_px - 7.2).abs() < 1e-4, "{}", c.unit_px);
        // 3840x2160 -> raw 10.80 -> ceiled to 10.00 (the 4K defence)
        let d = bake_at(&mut fx, 2160.0);
        assert!((d.unit_px - 10.0).abs() < 1e-4, "{}", d.unit_px);
    }

    #[test]
    fn two_screen_heights_give_two_whole_baked_themes() {
        let mut fx = fixture();
        let lo = bake_at(&mut fx, 720.0);
        let hi = bake_at(&mut fx, 1440.0);
        // 5.2u title bar: 20.8 px at 720p, 37.44 px at 1440p
        assert!((px(&fx, &lo, "size.md") - 20.8).abs() < 1e-3, "{}", px(&fx, &lo, "size.md"));
        assert!((px(&fx, &hi, "size.md") - 37.44).abs() < 1e-3, "{}", px(&fx, &hi, "size.md"));
        // and nothing about the two is shared
        assert_ne!(px(&fx, &lo, "panel.content_pad"), px(&fx, &hi, "panel.content_pad"));
        assert_eq!(lo.len(), hi.len());
    }

    #[test]
    fn strokes_round_to_whole_physical_pixels_and_never_vanish() {
        let mut fx = fixture();
        // stroke.hair = 0.2u: 1 px at 720p, 1 px at 1080p, 1 px at 1440p, 2 at 4K
        for (h, want) in [(720.0, 1.0), (1080.0, 1.0), (1440.0, 1.0), (2160.0, 2.0)] {
            let t = bake_at(&mut fx, h);
            assert_eq!(px(&fx, &t, "stroke.hair"), want, "at {h}");
        }
        // stroke.bold = 0.7u: 3 px at 720p, 4 at 1080p, 5 at 1440p, 7 at 4K
        for (h, want) in [(720.0, 3.0), (1080.0, 4.0), (1440.0, 5.0), (2160.0, 7.0)] {
            let t = bake_at(&mut fx, h);
            assert_eq!(px(&fx, &t, "stroke.bold"), want, "at {h}");
        }
        // a border named by the naming law is a stroke too
        let t = bake_at(&mut fx, 1080.0);
        assert_eq!(px(&fx, &t, "panel.border"), 1.0);
    }

    #[test]
    fn a_floored_length_takes_its_companions_bound() {
        let mut fx = fixture();
        // a11y.min_hit = 4.8u = 19.2 px at 720p, under its own 24 px floor
        let lo = bake_at(&mut fx, 720.0);
        assert_eq!(px(&fx, &lo, "a11y.min_hit"), 24.0);
        // at 1440p 4.8u = 34.56 px, comfortably over it
        let hi = bake_at(&mut fx, 1440.0);
        assert!((px(&fx, &hi, "a11y.min_hit") - 34.56).abs() < 1e-3);
        // and the companion itself is device px, untouched by u
        assert_eq!(px(&fx, &lo, "a11y.min_hit_min_px"), 24.0);
        assert_eq!(px(&fx, &hi, "a11y.min_hit_min_px"), 24.0);
    }

    #[test]
    fn percent_bakes_to_a_fraction_and_ms_stays_ms() {
        let mut fx = fixture();
        let t = bake_at(&mut fx, 1080.0);
        assert!((px(&fx, &t, "decor.vignette.strength") - 0.55).abs() < 1e-6);
        assert_eq!(px(&fx, &t, "motion.hover.duration"), 120.0);
    }

    #[test]
    fn sentinels_fold_to_their_f32_and_never_reach_a_colour() {
        let mut fx = fixture();
        let t = bake_at(&mut fx, 1080.0);
        assert_eq!(px(&fx, &t, "panel.content_pad_x"), -3.0); // same_as_parent
        assert_eq!(px(&fx, &t, "corner.pill"), -2.0); // pill
        for i in 0..t.len() {
            assert!(t.color(TokenId(i as u16)).a >= 0.0);
        }
    }

    #[test]
    fn density_scales_space_but_never_strokes_or_corners() {
        let mut fx = fixture();
        let r = resolve_default(&fx.schema, &mut fx.out);
        let plain = bake(
            &fx.schema,
            &r,
            &BakeInput { viewport: Viewport { screen_h: 1080.0, ui_scale: 1.0 }, ..Default::default() },
            &mut fx.out,
        );
        // now with an explicitly instrument density
        let mut src2 = Sources::new();
        let mut out2 = Vec::new();
        let g = src2.add("t.theme", "[metric]\ndensity = instrument\n");
        let doc = parse(&mut src2, g, None, &mut out2);
        let spec = cascade(&mut fx.schema, &[Stage::Document(&doc)], Options::default(), &mut out2);
        let r2 = resolve(&fx.schema, &spec, &mut out2);
        let dense = bake(
            &fx.schema,
            &r2,
            &BakeInput { viewport: Viewport { screen_h: 1080.0, ui_scale: 1.0 }, ..Default::default() },
            &mut out2,
        );
        assert!((dense.density_space - 0.72).abs() < 1e-6, "{}", dense.density_space);
        assert!(px(&fx, &dense, "space.6") < px(&fx, &plain, "space.6"));
        assert!((px(&fx, &dense, "space.6") - px(&fx, &plain, "space.6") * 0.72).abs() < 1e-3);
        // "A 0.72x hairline is not a line, and corners are the theme's identity
        // rather than its density."
        assert_eq!(px(&fx, &dense, "stroke.hair"), px(&fx, &plain, "stroke.hair"));
        assert_eq!(px(&fx, &dense, "corner.md"), px(&fx, &plain, "corner.md"));
        assert_eq!(px(&fx, &dense, "a11y.min_hit_min_px"), px(&fx, &plain, "a11y.min_hit_min_px"));
        // type follows its own axis
        assert!((dense.density_type - 0.90).abs() < 1e-6);
        assert!((px(&fx, &dense, "type.body.size") - px(&fx, &plain, "type.body.size") * 0.90).abs() < 1e-3);
    }

    #[test]
    fn an_explicit_float_wins_over_the_enum_for_that_axis_only() {
        let mut fx = fixture();
        let mut src2 = Sources::new();
        let mut out2 = Vec::new();
        let g = src2.add("t.theme", "[metric]\ndensity = airy\ndensity_space = 0.9\n");
        let doc = parse(&mut src2, g, None, &mut out2);
        let spec = cascade(&mut fx.schema, &[Stage::Document(&doc)], Options::default(), &mut out2);
        let r = resolve(&fx.schema, &spec, &mut out2);
        let t = bake(
            &fx.schema,
            &r,
            &BakeInput {
                viewport: Viewport { screen_h: 1080.0, ui_scale: 1.0 },
                explicit_density: (true, false),
                ..Default::default()
            },
            &mut out2,
        );
        // §5.3: "spacing 0.90, type 1.06 — airy type, compact-ish spacing"
        assert!((t.density_space - 0.90).abs() < 1e-6, "{}", t.density_space);
        assert!((t.density_type - 1.06).abs() < 1e-6, "{}", t.density_type);
    }

    #[test]
    fn ui_scale_multiplies_after_the_clamp() {
        let mut fx = fixture();
        let r = resolve_default(&fx.schema, &mut fx.out);
        let t = bake(
            &fx.schema,
            &r,
            &BakeInput { viewport: Viewport { screen_h: 720.0, ui_scale: 1.5 }, ..Default::default() },
            &mut fx.out,
        );
        // floored to 4.0, THEN scaled: 6.0, not clamp(5.4, 4, 10) = 5.4
        assert!((t.unit_px - 6.0).abs() < 1e-4, "{}", t.unit_px);
    }

    #[test]
    fn colours_are_srgb_encoded_with_straight_alpha() {
        let mut fx = fixture();
        let t = bake_at(&mut fx, 1080.0);
        let acc = t.color(fx.schema.id("palette.accent").unwrap());
        // an authored #3FE3AE lands on screen as #3FE3AE (the Unorm path)
        assert_eq!(acc.to_hex(), "#3FE3AE");
        assert_eq!(acc.a, 1.0);
    }

    #[test]
    fn enums_bake_to_an_index_into_the_tokens_own_word_list() {
        let mut fx = fixture();
        let mut src2 = Sources::new();
        let mut out2 = Vec::new();
        let g = src2.add("t.theme", "[metric]\ndensity = dense\n");
        let doc = parse(&mut src2, g, None, &mut out2);
        let spec = cascade(&mut fx.schema, &[Stage::Document(&doc)], Options::default(), &mut out2);
        let r = resolve(&fx.schema, &spec, &mut out2);
        let t = bake(&fx.schema, &r, &BakeInput::default(), &mut out2);
        let id = fx.schema.id("metric.density").unwrap();
        // "compact" was default's word, index 0; "dense" was interned at 1
        assert_eq!(fx.schema.enum_index(id, "compact"), Some(0));
        assert_eq!(t.enum_of(id), fx.schema.enum_index(id, "dense").unwrap());
        assert_eq!(fx.schema.enum_word(id, t.enum_of(id)), Some("dense"));
    }

    #[test]
    fn accessors_never_panic_on_a_missing_id() {
        let mut fx = fixture();
        let t = bake_at(&mut fx, 1080.0);
        // Grey, not black: the raw default must be legible on a dark bed.
        assert_eq!(t.color(TokenId::MISSING), Color::GREY);
        assert_eq!(t.px(TokenId::MISSING), 0.0);
        assert!(!t.flag(TokenId::MISSING));
        assert_eq!(t.enum_of(TokenId::MISSING), 0);
        assert_eq!(t.px(TokenId(9999)), 0.0);
    }

    #[test]
    fn the_scaling_classes_come_from_the_naming_law_not_a_token_list() {
        assert_eq!(scaling_of("stroke.hair"), Scaling::Stroke);
        assert_eq!(scaling_of("panel.border"), Scaling::Stroke);
        assert_eq!(scaling_of("focus.ring.width"), Scaling::Stroke);
        assert_eq!(scaling_of("chart.axis_stroke"), Scaling::Stroke);
        assert_eq!(scaling_of("corner.md"), Scaling::Plain);
        assert_eq!(scaling_of("a11y.min_hit_min_px"), Scaling::Plain);
        assert_eq!(scaling_of("space.6"), Scaling::Space);
        assert_eq!(scaling_of("menu.row_h"), Scaling::Space);
        assert_eq!(scaling_of("panel.content_pad"), Scaling::Space);
        assert_eq!(scaling_of("filetile.caption_gap"), Scaling::Space);
        assert_eq!(scaling_of("type.title.size"), Scaling::Type);
        assert_eq!(scaling_of("panel.title_h"), Scaling::Plain);
    }

    #[test]
    fn the_struct_stays_inside_the_seven_two_budget() {
        let mut fx = fixture();
        let t = bake_at(&mut fx, 1080.0);
        // The flat arrays cost ~23 bytes per token; the class x state matrix
        // is a FIXED cost (25 classes x 7 states x 48 B ~ 8.4 KB in the real
        // master), so the budget separates the two — a tiny fixture would
        // otherwise blame the matrix on its handful of tokens.
        let matrix = t.class_count() as usize * 7 * std::mem::size_of::<StateStyle>();
        let flat = t.size_of() - matrix - std::mem::size_of::<ResolvedTheme>();
        let per_token = flat as f32 / t.len().max(1) as f32;
        assert!(per_token < 26.0, "{per_token} bytes per token");
        let real_matrix = 25.0 * 7.0 * std::mem::size_of::<StateStyle>() as f32;
        assert!(
            2190.0 * per_token + real_matrix < 64.0 * 1024.0,
            "over the 64 KB const assertion"
        );
    }

    #[test]
    fn a_missing_metric_token_degrades_with_a_warning() {
        let mut src = Sources::new();
        let mut out = Vec::new();
        let f = src.add("default.theme", "[palette]\naccent = #3FE3AE\n");
        let d = parse(&mut src, f, None, &mut out);
        let schema = Schema::from_default(&d, &mut out);
        let r = resolve_default(&schema, &mut out);
        let t = bake(&schema, &r, &BakeInput::default(), &mut out);
        assert!(out.iter().any(|x| x.message.contains("metric.unit_pct_h")), "{out:?}");
        // and it still produced a theme with a sane u
        assert!((t.unit_px - 5.4).abs() < 1e-4, "{}", t.unit_px);
    }

    #[test]
    fn baking_is_deterministic() {
        let mut fx = fixture();
        let a = bake_at(&mut fx, 1080.0);
        let b = bake_at(&mut fx, 1080.0);
        for i in 0..a.len() {
            let id = TokenId(i as u16);
            assert_eq!(a.px(id), b.px(id));
            assert_eq!(a.color(id), b.color(id));
        }
    }
}
