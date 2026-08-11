//! The shared drawing vocabulary widgets are built from.
//!
//! These are the shapes that kept being written out by hand in every
//! widget: a block centred in its panel, rows of label and value, a
//! framed meter, a matrix of lit cells. Having them here means a widget
//! is a short description of WHAT to show rather than a page of layout
//! arithmetic — and it is the vocabulary the Rhai script renderer
//! ([`crate::script`]) interprets its elements into.
//!
//! Every colour and metric below comes from the theme: this file is the
//! single place where the look of every board panel is decided, so a
//! literal here would be a literal everywhere.

use crate::font::FONT_UI;
use crate::theme::{self, Color, TokenId};
use crate::view::paint;
use crate::view::surface::{CtxSurface, Surface};
use crate::{Ctx, Rect};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

/// Token id resolved once by name; MISSING degrades through the engine's
/// per-kind fallback rather than panicking.
fn tok(cell: &'static OnceLock<TokenId>, name: &'static str) -> TokenId {
    *cell.get_or_init(|| theme::id(name).unwrap_or(TokenId::MISSING))
}

/// A colour token, delivered in the `Color` the draw calls take.
fn col(cell: &'static OnceLock<TokenId>, name: &'static str) -> Color {
    let c = theme::resolved().color(tok(cell, name));
    Color { r: c.r, g: c.g, b: c.b, a: c.a }
}

/// Said once, then quiet: the widgets run every frame, and the second
/// copy of a diagnostic is already noise.
pub(crate) fn warn_once(key: &str, msg: &str) {
    thread_local! {
        static SAID: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
    }
    SAID.with(|s| {
        if s.borrow_mut().insert(key.to_string()) {
            eprintln!("nacelle-desktop: {msg}");
        }
    });
}

/// The word an enum token currently resolves to, memoised per (token, index)
/// so a draw loop pays the engine lock once per distinct value, not per
/// frame. A theme switch that lands on a new word is a new index and a new
/// memo entry, so the cache never goes stale.
pub(crate) fn theme_word(token: TokenId) -> String {
    word_of(token)
}

fn word_of(token: TokenId) -> String {
    thread_local! {
        static WORDS: RefCell<HashMap<(usize, u16), String>> = RefCell::new(HashMap::new());
    }
    let i = theme::resolved().enum_of(token);
    WORDS.with(|w| {
        w.borrow_mut()
            .entry((token.index(), i))
            .or_insert_with(|| theme::enum_word_of(token).unwrap_or_default())
            .clone()
    })
}

// ---------------------------------------------------------------- severity
//
// §5.10's closed set, in the master's declaration order. A severity is an
// INDEX into this set — never a colour: the script (or plugin) judges the
// data, the theme decides what the judgement looks like.

/// The severity roles the master declares, in declaration order.
pub const SEVERITY_ROLES: [&str; 7] =
    ["ok", "info", "warning", "critical", "contained", "offline", "unknown"];

/// An index into [`SEVERITY_ROLES`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Sev(pub u16);

/// The severity for a name from the closed set — `None` for a word outside
/// it, so the CALLER decides the fallback ([`sev_fallback`], never `ok`).
pub fn sev_of(name: &str) -> Option<Sev> {
    SEVERITY_ROLES
        .iter()
        .position(|s| *s == name)
        .map(|i| Sev(i as u16))
}

/// What an unrecognised severity resolves to: `script.severity_fallback`,
/// which the master pins to `unknown` and §5.10 forbids ever being `ok`.
pub fn sev_fallback() -> Sev {
    static FB: OnceLock<TokenId> = OnceLock::new();
    let word = word_of(tok(&FB, "script.severity_fallback"));
    sev_of(&word).unwrap_or(Sev(6))
}

/// The `text` token id of each severity role, resolved once per role.
///
/// The other four members (`edge`, `fill`, `on`, `badge_style`) are read
/// by [`crate::view::paint`], which names them by string because it has
/// to work on the far side of the plugin ABI, where a `TokenId` means
/// nothing. Only the ink is asked for often enough on the host to be
/// worth a static.
fn sev_tok(s: Sev) -> TokenId {
    static TOKS: OnceLock<Vec<TokenId>> = OnceLock::new();
    let all = TOKS.get_or_init(|| {
        SEVERITY_ROLES
            .iter()
            .map(|n| theme::id(&format!("severity.{n}.text")).unwrap_or(TokenId::MISSING))
            .collect()
    });
    all[(s.0 as usize).min(all.len() - 1)]
}

/// The ink a severity writes in — the label, the value, the status word.
pub fn sev_text(s: Sev) -> Color {
    theme::resolved().color(sev_tok(s))
}

// ---------------------------------------------------------------- type roles
//
// A role is named by a STRING — scripts name their own (`display.clock`) and
// role-binding tokens resolve to one — so the ids cannot live in per-site
// statics. They are memoised by name instead; token ids are stable for the
// life of the process, so the map never goes stale.

/// The token ids behind one `type.*` role.
#[derive(Clone, Copy)]
pub struct Role {
    size: TokenId,
    tracking: TokenId,
    leading: TokenId,
    fg: TokenId,
    alpha: TokenId,
}

/// The role for a name. An unknown role warns once and falls back to `body`
/// (the rule of `script.text_role`): a typo must stay readable, not vanish.
pub fn role(name: &str) -> Role {
    thread_local! {
        static ROLES: RefCell<HashMap<String, Role>> = RefCell::new(HashMap::new());
    }
    fn lookup(name: &str) -> Option<Role> {
        Some(Role {
            size: theme::id(&format!("type.{name}.size"))?,
            tracking: theme::id(&format!("type.{name}.tracking")).unwrap_or(TokenId::MISSING),
            leading: theme::id(&format!("type.{name}.leading")).unwrap_or(TokenId::MISSING),
            fg: theme::id(&format!("type.{name}.fg")).unwrap_or(TokenId::MISSING),
            alpha: theme::id(&format!("type.{name}.alpha")).unwrap_or(TokenId::MISSING),
        })
    }
    ROLES.with(|r| {
        if let Some(role) = r.borrow().get(name) {
            return *role;
        }
        let resolved = lookup(name).unwrap_or_else(|| {
            warn_once(
                &format!("role:{name}"),
                &format!("unknown type role \"{name}\" — falling back to body"),
            );
            lookup("body").unwrap_or(Role {
                size: TokenId::MISSING,
                tracking: TokenId::MISSING,
                leading: TokenId::MISSING,
                fg: TokenId::MISSING,
                alpha: TokenId::MISSING,
            })
        });
        r.borrow_mut().insert(name.to_string(), resolved);
        resolved
    })
}

/// The role a `*_role` binding token resolves to. Read through [`word_of`],
/// so a theme switching the binding lands on the next frame.
pub fn bound_role(cell: &'static OnceLock<TokenId>, binding: &'static str) -> Role {
    let word = word_of(tok(cell, binding));
    if word.is_empty() {
        role("body")
    } else {
        role(&word)
    }
}

impl Role {
    /// The role's px for the panel being drawn, at the stack's shrink
    /// factor. The baked size carries the unit, density and user scale;
    /// `panel_scale` and `shrink` are runtime state, so they multiply here.
    pub fn px(&self, ctx: &Ctx, shrink: f32) -> f32 {
        static MIN: OnceLock<TokenId> = OnceLock::new();
        let t = theme::resolved();
        (t.px(self.size) * ctx.panel_scale * shrink).max(t.px(tok(&MIN, "type.min_px")))
    }

    /// Letter spacing in px for a run of this role at `px`. Tracking tokens
    /// are em — a fraction of the run's own size.
    pub fn tracking_px(&self, px: f32) -> f32 {
        px * theme::resolved().px(self.tracking)
    }

    /// Line height as a multiple of the resolved px.
    pub fn leading(&self) -> f32 {
        let l = theme::resolved().px(self.leading);
        if l > 0.0 {
            l
        } else {
            1.0
        }
    }

    /// The colour this role draws in: fg × its constant alpha.
    pub fn color(&self) -> Color {
        let t = theme::resolved();
        let c = t.color(self.fg);
        let a = t.px(self.alpha);
        Color {
            r: c.r,
            g: c.g,
            b: c.b,
            a: c.a * if a > 0.0 { a.min(1.0) } else { 1.0 },
        }
    }
}

// ---------------------------------------------------------------- motion
//
// The blink a `runs` item may carry (§5.29): a 0..1 factor from
// `motion.<id>`, applied to the run's ALPHA — the glyph keeps its advance,
// which is what stops the clock jittering. Frozen fully visible under
// reduced motion (`motion.scale = 0`) or when the effect is disabled.

/// The 0..1 factor of the cyclic motion effect `motion.<id>` at time `t`.
pub fn blink_factor(id: &str, t: f64) -> f32 {
    thread_local! {
        static MOTION: RefCell<HashMap<String, (TokenId, TokenId, TokenId, TokenId)>> =
            RefCell::new(HashMap::new());
    }
    static SCALE: OnceLock<TokenId> = OnceLock::new();
    let (period, duty, floor, enabled) = MOTION.with(|m| {
        *m.borrow_mut().entry(id.to_string()).or_insert_with(|| {
            let g = |member: &str| {
                theme::id(&format!("motion.{id}.{member}")).unwrap_or(TokenId::MISSING)
            };
            if theme::id(&format!("motion.{id}.period_ms")).is_none() {
                warn_once(
                    &format!("blink:{id}"),
                    &format!("unknown motion effect \"{id}\" — the run stays visible"),
                );
            }
            (g("period_ms"), g("duty"), g("floor"), g("enabled"))
        })
    });
    let th = theme::resolved();
    let scale = th.px(tok(&SCALE, "motion.scale"));
    // Reduced motion and a disabled effect both FREEZE the run at fully
    // visible: a separator that never returns is a content change.
    if scale <= 0.0 || !th.flag(enabled) {
        return 1.0;
    }
    let p = th.px(period) * scale;
    if p <= 0.0 {
        return 1.0;
    }
    // The cyclic sources are step-eased (`t < duty ? 1 : floor`); the other
    // easings belong to one-shot transitions and no run carries those.
    let phase = ((t * 1000.0) % p as f64) / p as f64;
    if (phase as f32) < th.px(duty) {
        1.0
    } else {
        th.px(floor).clamp(0.0, 1.0)
    }
}

/// A type role's px for the panel being drawn. The baked size already
/// carries the unit, the density and the user's scale; the panel's
/// container-query factor is runtime state, so it multiplies here rather
/// than in the bake.
fn role_px(ctx: &Ctx, cell: &'static OnceLock<TokenId>, name: &'static str) -> f32 {
    static MIN: OnceLock<TokenId> = OnceLock::new();
    let t = theme::resolved();
    (t.px(tok(cell, name)) * ctx.panel_scale).max(t.px(tok(&MIN, "type.min_px")))
}

/// Top of a single line centred in a box of `box_h`. The line occupies
/// its role's leading; in optical mode the cap-height bias nudges it.
/// True optical centring wants the font's cap height, which the draw
/// list does not expose yet — the bias is the part that can draw today.
///
/// The arithmetic itself lives in [`paint::center_line_y`], where the
/// views on the far side of the plugin boundary reach it too; this is
/// the host's way in.
fn center_line_y(ctx: &mut Ctx, y: f32, box_h: f32, px: f32, leading: f32) -> f32 {
    paint::center_line_y(&mut CtxSurface::new(ctx), y, box_h, px, leading)
}

/// Top edge for a block of known natural height, centred vertically in
/// `r` and never pushed above it.
pub fn block_top(r: &Rect, natural: f32) -> f32 {
    r.y + ((r.h - natural) / 2.0).max(0.0)
}

/// Trims text with a trailing ellipsis so it fits `max_w`, measured at
/// the SAME letter tracking the caller draws with. `base::fit_end`
/// measures at a fixed legacy tracking; under a role whose tracking
/// differs, a string would trim against one width and draw at another —
/// which is how a content-measured table column came to ellipsise the
/// very cell it was sized from.
fn fit_end_tracked(ctx: &mut Ctx, px: f32, text: &str, max_w: f32, track: f32) -> String {
    paint::fit_end(&mut CtxSurface::new(ctx), px, text, max_w, track)
}

/// Breaks text into lines no wider than `max_w`, greedily by words —
/// the host's way into [`paint::wrap`], where the arithmetic lives so
/// that a view on the far side of the plugin boundary shares it.
///
/// The tooltip is its first caller; the text phase will be its second,
/// which is why it is public vocabulary rather than a private helper.
pub fn wrap_text(ctx: &mut Ctx, px: f32, text: &str, max_w: f32, track: f32) -> Vec<String> {
    paint::wrap(&mut CtxSurface::new(ctx), px, text, max_w, track)
}

/// How a `rows` block sizes its label column (u2 §3.1 #4).
#[derive(Clone, Copy, PartialEq)]
pub enum LabelWidth {
    /// Each value placed against its own label — today's ragged cloud,
    /// values right-aligned at the row's edge.
    Auto,
    /// Every label in the block measured once; all values start at one x,
    /// left-aligned — the images' tight label column (u2 §2.3).
    Max,
}

/// One `rows` line: a label, a value, and the script's judgement of the
/// value's severity — an index into the closed set, never a colour.
pub struct RowItem {
    pub label: String,
    pub value: String,
    pub sev: Option<Sev>,
}

/// How a `rows` block is arranged. Metrics that shrink with the stack
/// arrive pre-scaled from the caller ([`crate::script`]'s fit pass);
/// everything else is read from the theme here.
pub struct RowsStyle {
    pub label_role: Role,
    pub value_role: Role,
    pub columns: usize,
    pub label_width: LabelWidth,
    /// One line's height, already at the stack's shrink factor.
    pub row_h: f32,
    /// The stack's shrink factor, for the type sizes.
    pub shrink: f32,
}

/// Rows of `label` and `value`, flowed into `st.columns` grid columns
/// row-major, the whole block centred vertically. A line with fewer cells
/// than the grid spans the width it has (u2 §2.3's 2+1 case). Values are
/// trimmed to the space their label leaves. Returns the height used.
pub fn rows_label_value(ctx: &mut Ctx, r: Rect, rows: &[RowItem], st: &RowsStyle) -> f32 {
    if rows.is_empty() {
        return 0.0;
    }
    static LABEL_PAD: OnceLock<TokenId> = OnceLock::new();
    static COL_GAP: OnceLock<TokenId> = OnceLock::new();
    static LABEL_C: OnceLock<TokenId> = OnceLock::new();
    static VALUE_C: OnceLock<TokenId> = OnceLock::new();
    let t = theme::resolved();
    let cols = st.columns.max(1);
    let lines = rows.len().div_ceil(cols);
    let row_h = st.row_h.min(r.h / lines as f32);
    let lpx = st.label_role.px(ctx, st.shrink);
    let vpx = st.value_role.px(ctx, st.shrink);
    let ltrack = st.label_role.tracking_px(lpx);
    let vtrack = st.value_role.tracking_px(vpx);
    let pad = t.px(tok(&LABEL_PAD, "rhythm.label_pad")) * st.shrink;
    let gap = t.px(tok(&COL_GAP, "script.rows_col_gap")) * st.shrink;
    let label_c = col(&LABEL_C, "component.script.label");
    let value_c = col(&VALUE_C, "component.script.value");
    // The shared label column: the widest label per GRID column, so all
    // values in that column start at one x. A spanning cell aligns with
    // column 0 — its value keeps the left column's x.
    let mut label_w = vec![0.0f32; cols];
    if st.label_width == LabelWidth::Max {
        for (i, row) in rows.iter().enumerate() {
            let line = i / cols;
            let cells_on_line = (rows.len() - line * cols).min(cols);
            let j = if cells_on_line < cols { 0 } else { i % cols };
            let w = ctx.fonts.measure(FONT_UI, lpx, &row.label, ltrack);
            label_w[j] = label_w[j].max(w);
        }
    }
    let natural = row_h * lines as f32;
    let top = block_top(&r, natural);
    for (i, row) in rows.iter().enumerate() {
        let line = i / cols;
        let cells_on_line = (rows.len() - line * cols).min(cols);
        let j = i % cols;
        let cell_w = (r.w - gap * (cells_on_line as f32 - 1.0)) / cells_on_line as f32;
        let cx = r.x + (cell_w + gap) * j as f32;
        let y = top + row_h * line as f32;
        let lty = center_line_y(ctx, y, row_h, lpx, st.label_role.leading());
        let vty = center_line_y(ctx, y, row_h, vpx, st.value_role.leading());
        ctx.dl.text(ctx.fonts, FONT_UI, lpx, cx, lty, &row.label, label_c, ltrack);
        let vc = row.sev.map(sev_text).unwrap_or(value_c);
        match st.label_width {
            LabelWidth::Max => {
                let colw = if cells_on_line < cols { label_w[0] } else { label_w[j] };
                let vx = cx + colw + pad;
                let room = (cx + cell_w - vx).max(pad);
                let shown = fit_end_tracked(ctx, vpx, &row.value, room, vtrack);
                ctx.dl.text(ctx.fonts, FONT_UI, vpx, vx, vty, &shown, vc, vtrack);
            }
            LabelWidth::Auto => {
                let lw = ctx.fonts.measure(FONT_UI, lpx, &row.label, ltrack);
                let room = (cell_w - lw - pad).max(pad);
                let shown = fit_end_tracked(ctx, vpx, &row.value, room, vtrack);
                ctx.dl
                    .text_right(ctx.fonts, FONT_UI, vpx, cx + cell_w, vty, &shown, vc, vtrack);
            }
        }
    }
    natural
}

/// A framed meter with a proportional fill: the outline shows the whole,
/// the fill shows `frac` of it (clamped, so bad data cannot overdraw).
/// Track and fill come from `component.bar.*` — read here, not passed:
/// a caller with a colour in hand is a caller doing the theme's job.
/// A severity is the script's judgement of the DATA (an index into the
/// closed set, not a colour) and tints the fill; `track = false` says the
/// value has no meaningful whole, so no outline claims one.
pub fn meter(ctx: &mut Ctx, r: Rect, frac: f32, sev: Option<Sev>, track: bool) {
    paint::meter(&mut CtxSurface::new(ctx), r, frac, sev, track);
}

/// A grid of cells of which the first `frac` are lit — the dot matrix
/// used for memory. The preferred pitch is `script.dots_cell` with its
/// `script.dots_cell_min_px` floor, read here; `shrink` is the stack's
/// shrink-to-fit factor — runtime state like `panel_scale`, never a look
/// decision. The grid is fitted to `r` and always keeps at least one cell.
pub fn dot_matrix(ctx: &mut Ctx, r: Rect, frac: f32, shrink: f32) {
    let frac = if frac.is_finite() {
        frac.clamp(0.0, 1.0)
    } else {
        0.0
    };
    static PITCH: OnceLock<TokenId> = OnceLock::new();
    static PITCH_MIN: OnceLock<TokenId> = OnceLock::new();
    static CELL: OnceLock<TokenId> = OnceLock::new();
    static CELL_MIN: OnceLock<TokenId> = OnceLock::new();
    static FILL_RATIO: OnceLock<TokenId> = OnceLock::new();
    static FILL_MIN: OnceLock<TokenId> = OnceLock::new();
    static GAP_MIN: OnceLock<TokenId> = OnceLock::new();
    static ON: OnceLock<TokenId> = OnceLock::new();
    static OFF: OnceLock<TokenId> = OnceLock::new();
    let t = theme::resolved();
    let cell = (t.px(tok(&PITCH, "script.dots_cell")) * shrink)
        .max(t.px(tok(&PITCH_MIN, "script.dots_cell_min_px")));
    let step = cell.max(t.px(tok(&CELL_MIN, "dotmatrix.cell_min_px"))).max(1.0);
    let cols = ((r.w / step).floor() as usize).max(1);
    let rows = ((r.h / step).floor() as usize).max(1);
    let total = cols * rows;
    let lit = (frac * total as f32).round() as usize;
    // fill_ratio is baked against the theme's own cell, so it is turned
    // back into a fraction and applied to the pitch actually in use;
    // gap_min_px is what stops adjacent lit rows fusing into bars.
    let cell_ref = t.px(tok(&CELL, "dotmatrix.cell"));
    let ratio = if cell_ref > 0.0 {
        t.px(tok(&FILL_RATIO, "dotmatrix.fill_ratio")) / cell_ref
    } else {
        0.0
    };
    let size = (step * ratio)
        .max(t.px(tok(&FILL_MIN, "dotmatrix.fill_min_px")))
        .min(step - t.px(tok(&GAP_MIN, "dotmatrix.gap_min_px")))
        .max(1.0);
    let on = col(&ON, "component.matrix.cell_on");
    let off = col(&OFF, "component.matrix.cell_off");
    for i in 0..total {
        let cx = r.x + (i % cols) as f32 * step;
        let cy = r.y + (i / cols) as f32 * step;
        ctx.dl.rect(cx, cy, size, size, if i < lit { on } else { off });
    }
}

/// What one gauge is drawn as (u2 §2.5). `bar` and `donut` exist in the
/// vocabulary but cannot yet carry the per-core number they owe (content
/// preservation), so the caller degrades them to `Row` with a warning.
#[derive(Clone, Copy, PartialEq)]
pub enum GaugeKind {
    /// label + thin track + value — image 1's resource row.
    Row,
    /// A framed box with the number inside at the far end — today's look.
    Cell,
}

/// Where a row-style gauge's label comes from. The label is arrangement
/// data from the script (a core is `C0` because the script says so).
pub enum GaugeLabels {
    None,
    /// `prefix` + the gauge's index: `C0`, `C1`, …
    Index(String),
    /// One label per value.
    Text(Vec<String>),
}

/// How the numeric readout is written. The number itself is the same value
/// the fill encodes — a second presentation, not new data.
#[derive(Clone, Copy, PartialEq)]
pub enum GaugeValueFmt {
    /// `{v:.0}%` — today's format.
    Percent,
    /// `{v:.0}` — a plain number.
    Raw,
}

/// How a `gauges` element is arranged. Everything visual is read from the
/// theme inside; this struct carries the script's arrangement choices and
/// the stack's runtime shrink factor.
pub struct GaugeStyle {
    pub cols: usize,
    pub kind: GaugeKind,
    pub labels: GaugeLabels,
    pub value_fmt: GaugeValueFmt,
    pub shrink: f32,
}

/// A grid of gauges, one per value, flowed into `st.cols` columns. `Cell`
/// is a framed meter with its value written inside, flipping to
/// `component.bar.text_on_fill` where the fill would swallow it; `Row` is
/// label + thin track + value, the images' instrument row, where the
/// number always fits and so is always drawn (u2 §2.5). The colours and
/// metrics are all read here.
pub fn gauge_grid(ctx: &mut Ctx, r: Rect, values: &[f32], st: &GaugeStyle) {
    if values.is_empty() {
        return;
    }
    if st.kind == GaugeKind::Row {
        return gauge_rows(ctx, r, values, st);
    }
    let cols = st.cols;
    static GAP: OnceLock<TokenId> = OnceLock::new();
    static CAP_SIZE: OnceLock<TokenId> = OnceLock::new();
    static CAP_LEAD: OnceLock<TokenId> = OnceLock::new();
    static CAP_TRACK: OnceLock<TokenId> = OnceLock::new();
    static MIN_H: OnceLock<TokenId> = OnceLock::new();
    static BORDER: OnceLock<TokenId> = OnceLock::new();
    static CLEARANCE: OnceLock<TokenId> = OnceLock::new();
    static INSET: OnceLock<TokenId> = OnceLock::new();
    static TEXT_C: OnceLock<TokenId> = OnceLock::new();
    static ON_FILL_C: OnceLock<TokenId> = OnceLock::new();
    let t = theme::resolved();
    let cols = cols.max(1);
    let rows = values.len().div_ceil(cols);
    let gap = t.px(tok(&GAP, "gauge.gap"));
    let gw = (r.w - gap * (cols as f32 - 1.0)) / cols as f32;
    let gh = ((r.h - gap * (rows as f32 - 1.0)) / rows as f32).max(1.0);
    let px = role_px(ctx, &CAP_SIZE, "type.caption.size");
    let leading = t.px(tok(&CAP_LEAD, "type.caption.leading"));
    let track = px * t.px(tok(&CAP_TRACK, "type.caption.tracking"));
    // min_h_for_label is baked from the caption's resting size, so it
    // follows the same container-query factor the drawn px does.
    let min_h = t.px(tok(&MIN_H, "gauge.min_h_for_label")) * ctx.panel_scale;
    let bw = t.px(tok(&BORDER, "gauge.border"));
    let clearance = t.px(tok(&CLEARANCE, "gauge.label_clearance"));
    let inset = t.px(tok(&INSET, "gauge.label_inset"));
    let text_c = col(&TEXT_C, "component.gauge.text");
    let on_fill_c = col(&ON_FILL_C, "component.bar.text_on_fill");
    for (i, v) in values.iter().enumerate() {
        let gx = r.x + (i % cols) as f32 * (gw + gap);
        let gy = r.y + (i / cols) as f32 * (gh + gap);
        let cell = Rect::new(gx, gy, gw, gh);
        meter(ctx, cell, v / 100.0, None, true);
        // The number only fits — and is only worth drawing — when the
        // gauge is tall enough for it.
        if gh < min_h {
            continue;
        }
        let text = gauge_value(*v, st.value_fmt);
        let tw = ctx.fonts.measure(FONT_UI, px, &text, track);
        let fill_w = (gw - 2.0 * bw) * (v / 100.0).clamp(0.0, 1.0);
        // The number sits at the far END of the gauge, where the fill
        // arrives last. On the near end — where it used to be — every
        // small reading had its own first digit painted over by the few
        // pixels of fill that were the whole point of the gauge.
        let swallowed = fill_w >= gw - 2.0 * bw - tw - clearance;
        let c = if swallowed { on_fill_c } else { text_c };
        let ty = center_line_y(ctx, gy, gh, px, leading);
        ctx.dl
            .text_right(ctx.fonts, FONT_UI, px, gx + gw - inset, ty, &text, c, track);
    }
}

fn gauge_value(v: f32, fmt: GaugeValueFmt) -> String {
    match fmt {
        GaugeValueFmt::Percent => format!("{v:.0}%"),
        GaugeValueFmt::Raw => format!("{v:.0}"),
    }
}

/// The `Row` gauge form: label + thin track + value per cell, flowed into
/// the same grid the cells use. The label and value columns are measured
/// once across the block so every track starts and ends at one x — the
/// images align, they do not centre (u2 §2.5).
fn gauge_rows(ctx: &mut Ctx, r: Rect, values: &[f32], st: &GaugeStyle) {
    static GAP: OnceLock<TokenId> = OnceLock::new();
    static CAP_SIZE: OnceLock<TokenId> = OnceLock::new();
    static CAP_LEAD: OnceLock<TokenId> = OnceLock::new();
    static CAP_TRACK: OnceLock<TokenId> = OnceLock::new();
    static LABEL_GAP: OnceLock<TokenId> = OnceLock::new();
    static VALUE_GAP: OnceLock<TokenId> = OnceLock::new();
    static BAR_H: OnceLock<TokenId> = OnceLock::new();
    static TEXT_C: OnceLock<TokenId> = OnceLock::new();
    let t = theme::resolved();
    let cols = st.cols.max(1);
    let rows = values.len().div_ceil(cols);
    let gap = t.px(tok(&GAP, "gauge.gap")) * st.shrink;
    let gw = (r.w - gap * (cols as f32 - 1.0)) / cols as f32;
    let gh = ((r.h - gap * (rows as f32 - 1.0)) / rows as f32).max(1.0);
    let px = role_px(ctx, &CAP_SIZE, "type.caption.size") * st.shrink;
    let leading = t.px(tok(&CAP_LEAD, "type.caption.leading"));
    let track = px * t.px(tok(&CAP_TRACK, "type.caption.tracking"));
    let lgap = t.px(tok(&LABEL_GAP, "meter.label_gap")) * st.shrink;
    let vgap = t.px(tok(&VALUE_GAP, "meter.value_gap")) * st.shrink;
    let bar_h = t.px(tok(&BAR_H, "script.meter_bar_h")) * st.shrink;
    let text_c = col(&TEXT_C, "component.gauge.text");
    let label_of = |i: usize| -> String {
        match &st.labels {
            GaugeLabels::None => String::new(),
            GaugeLabels::Index(prefix) => format!("{prefix}{i}"),
            GaugeLabels::Text(v) => v.get(i).cloned().unwrap_or_default(),
        }
    };
    // One label column and one value column for the whole block, so the
    // tracks line up between rows and between grid columns.
    let mut label_w = 0.0f32;
    let mut value_w = 0.0f32;
    for (i, v) in values.iter().enumerate() {
        label_w = label_w.max(ctx.fonts.measure(FONT_UI, px, &label_of(i), track));
        let val = gauge_value(*v, st.value_fmt);
        value_w = value_w.max(ctx.fonts.measure(FONT_UI, px, &val, track));
    }
    let label_col = if label_w > 0.0 { label_w + lgap } else { 0.0 };
    for (i, v) in values.iter().enumerate() {
        let gx = r.x + (i % cols) as f32 * (gw + gap);
        let gy = r.y + (i / cols) as f32 * (gh + gap);
        let ty = center_line_y(ctx, gy, gh, px, leading);
        let label = label_of(i);
        if !label.is_empty() {
            ctx.dl.text(ctx.fonts, FONT_UI, px, gx, ty, &label, text_c, track);
        }
        let bar = Rect::new(
            gx + label_col,
            gy + (gh - bar_h).max(0.0) / 2.0,
            (gw - label_col - value_w - vgap).max(1.0),
            bar_h.min(gh),
        );
        meter(ctx, bar, v / 100.0, None, true);
        // A row always has room for its number, so the number is always
        // drawn — item 4 of the cpu inventory stops being conditional.
        let val = gauge_value(*v, st.value_fmt);
        ctx.dl.text_right(ctx.fonts, FONT_UI, px, gx + gw, ty, &val, text_c, track);
    }
}

/// Horizontal alignment of a table column.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Align {
    Left,
    Right,
    Center,
}

/// How a table cell renders its string (u2 §3.1 #10). The string is the
/// content and is never changed by the kind — a bar or a badge is a second
/// reading of the same value, not a replacement.
#[derive(Clone, Copy, PartialEq)]
pub enum CellKind {
    Text,
    /// The numeric value also drawn as a hairline track filled to
    /// `value / of` — image 1's resource rows.
    Bar { of: f32 },
    /// The string drawn as a status pill carrying the row's severity.
    Badge,
}

/// Where a fixed column's width comes from (u2 §2.7). `Content` — the
/// widest actual cell, not the heading — is the default: measuring from
/// headings is what ellipsised every five-digit pid.
#[derive(Clone, Copy, PartialEq)]
pub enum ColWidth {
    Heading,
    Content,
}

/// One table column: its heading, and how its cells are laid and drawn.
pub struct Column {
    pub title: String,
    pub align: Align,
    pub kind: CellKind,
    pub width: ColWidth,
}

/// The script's arrangement choices for a `table`, plus the stack's
/// runtime shrink factor. Everything visual is read from the theme inside.
pub struct TableStyle {
    /// Index of the column that absorbs the leftover width.
    pub elastic: usize,
    /// Whether every Nth row is tinted (`script.table_zebra_every` is the
    /// N and `component.table.zebra` the tint; the script only says the
    /// striping makes sense for this data).
    pub zebra: bool,
    /// Index into each ROW at which the script placed a severity word for
    /// that row. The word is consumed as style, never drawn as a cell.
    pub severity_col: Option<usize>,
    pub shrink: f32,
}

/// The view riding on a table: what it remembers between frames, where
/// it records the rectangles it drew, and which of its interactions the
/// script turned on.
///
/// F2 §2.1 gives this struct three fields (`state`, `hits`, `id`); the
/// per-table OPTIONS live here too rather than in [`TableStyle`],
/// because `TableStyle` is the shape every existing caller builds by
/// hand and growing it would break them for a look that has not moved.
///
/// Every option is OFF in a table built this way with `Default`, and a
/// table drawn with all of them off draws what [`table`] draws, to the
/// pixel — the two share one implementation, which is the only way that
/// claim stays true.
pub struct TableView<'a> {
    pub state: &'a mut crate::view::table::TableState,
    pub hits: &'a mut crate::view::hits::Hits,
    /// Which view recorded a rectangle: one [`crate::view::hits::Hits`]
    /// may serve every view in a widget.
    pub id: u32,
    /// The model's rewrite counter (`Snapshot::generation`). The sort is
    /// cached against it; a caller with no generation of its own passes
    /// 0 and gets an order rebuilt only when the sort itself moves.
    pub generation: u64,
    /// Headings sort and answer the pointer.
    pub interactive: bool,
    /// Rows answer the pointer and one of them may be selected.
    pub select: bool,
    /// The column whose text identifies a row. `None`: the row's
    /// position in the model, which is all there is to go on.
    pub key_col: Option<usize>,
    /// Scroll the body instead of truncating it at the bottom edge.
    pub scroll: bool,
    /// A heading or a cell the ellipsis cut short explains itself when
    /// the pointer rests on it (F2 §8.1). Only what was TRIMMED asks:
    /// a tooltip repeating text already on screen is noise.
    pub tooltip: bool,
}

/// A table: one heading per column, then rows. The column marked
/// `elastic` absorbs the leftover width and is trimmed with an ellipsis;
/// everything else is measured from its content or its heading (u2 §2.7).
pub fn table(ctx: &mut Ctx, r: Rect, columns: &[Column], rows: &[Vec<String>], st: &TableStyle) {
    table_surface(&mut CtxSurface::new(ctx), r, columns, rows, st, None);
}

/// [`table`] with a view riding on it: an offset window instead of the
/// top-of-list truncation, a sorted and pointer-aware header, and the
/// selected row's `script.row` wash. With a default [`TableView`] it
/// draws exactly what [`table`] draws — same function, same branches.
pub fn table_view(
    ctx: &mut Ctx,
    r: Rect,
    columns: &[Column],
    rows: &[Vec<String>],
    st: &TableStyle,
    view: TableView,
) {
    table_surface(&mut CtxSurface::new(ctx), r, columns, rows, st, Some(view));
}

/// The table on any [`Surface`] — the ONE implementation [`table`] and
/// [`table_view`] both are, and the one a plugin reaches through
/// [`crate::view::surface::AbiSurface`].
///
/// The port from `Ctx` was mechanical on purpose: every
/// `t.px(tok(&CELL, "table.cell_pad"))` became `sf.px("table.cell_pad")`,
/// resolving the same token through the same engine, so the host's
/// pixels did not move. What it buys is that the interactive table
/// cannot exist twice — the fate `ui::fit_end_tracked` and the file
/// panel's `fit_name` already met.
pub fn table_surface<S: Surface>(
    sf: &mut S,
    r: Rect,
    columns: &[Column],
    rows: &[Vec<String>],
    st: &TableStyle,
    mut view: Option<TableView>,
) {
    if columns.is_empty() {
        return;
    }
    // Header and body each have a type role of their own, from the
    // `script.table_head_role` / `script.table_cell_role` bindings.
    let head_role = paint::bound_role(sf, "script.table_head_role", st.shrink);
    let cell_role = paint::bound_role(sf, "script.table_cell_role", st.shrink);
    let (head_px, head_track) = (head_role.px, head_role.track);
    let (cell_px, cell_track) = (cell_role.px, cell_role.track);
    // The severity column is style, not content: the display columns are
    // the row's cells with that entry removed — but only when the row
    // actually carries the extra entry, so a script that declares the
    // column and then forgets the word loses nothing visible.
    let sev_slot = |row: &Vec<String>| -> Option<usize> {
        match st.severity_col {
            Some(sc) if row.len() > columns.len() && sc < row.len() => Some(sc),
            _ => None,
        }
    };
    let cell_of = |slot: Option<usize>, i: usize| -> usize {
        match slot {
            Some(sc) if i >= sc => i + 1,
            _ => i,
        }
    };
    // The view, taken apart before anything is drawn: the display ORDER
    // is READ from the state while the hit list is WRITTEN to, and the
    // borrow checker is right to insist those are two different things.
    // Without a view every one of these is the "off" value, and every
    // branch below that tests one is not taken.
    let (
        mut state,
        mut hits,
        view_id,
        interactive,
        select,
        key_col,
        wants_scroll,
        generation,
        explain,
    ) = match view.take() {
        Some(v) => (
            Some(v.state),
            Some(v.hits),
            v.id,
            v.interactive,
            v.select,
            v.key_col,
            v.scroll,
            v.generation,
            v.tooltip,
        ),
        None => (None, None, 0, false, false, None, false, 0, false),
    };

    // The table spans its box: as many rows as fit after the header,
    // sharing the height exactly, starting at the top edge. Only when
    // the data runs out before the space does does it keep its natural
    // row height and leave the remainder empty — stretching four rows
    // over a tall panel would look like a fault rather than a table.
    let head_h = sf.px("table.head_h") * st.shrink;
    let natural_h = sf.px("table.row_h") * st.shrink;
    let fits = ((r.h - head_h).max(0.0) / natural_h.max(1.0)).floor() as usize;
    let shown = rows.len().min(fits);
    let fitted_h = if shown >= fits && shown > 0 {
        (r.h - head_h) / shown as f32
    } else {
        natural_h
    };
    // The header block: the headings sit on the top edge, `head_gap`
    // above the rule, `head_gap_below` under it. `head_h` is what the
    // FIT arithmetic reserves for the header and is a different number
    // — that is how this function has always measured, and changing it
    // would move every table by a pixel.
    let head_gap = sf.px("table.head_gap") * st.shrink;
    let head_gap_below = sf.px("table.head_gap_below") * st.shrink;
    let body_y = r.y + head_gap + head_gap_below;
    let body_h = (r.bottom() - body_y).max(0.0);
    // A scrolled body keeps its natural row height: stretching the rows
    // to divide the box exactly is what a table does when it shows
    // everything it has, and it is meaningless once there is an offset.
    let scrolling = wants_scroll && body_h > 0.0;
    let row_h = if scrolling { natural_h } else { fitted_h };
    // A surface that cannot clip must not paint half a row outside its
    // box, so it scrolls by whole rows instead — the file panel's
    // behaviour, and the honest degradation of an old host.
    let can_clip = sf.can_clip();

    // The display order. The sort is the RENDERER's (F2 §2.1): the
    // script hands over rows in its own order and this decides which
    // one is shown where — rebuilt only when the model was rewritten or
    // the sort moved, never per frame.
    if let Some(s) = state.as_deref_mut() {
        let sc = s.sort.map(|(c, _)| c).unwrap_or(0);
        s.refresh_order(generation, rows.len(), |i| {
            rows.get(i)
                .and_then(|row| row.get(cell_of(sev_slot(row), sc)))
                .cloned()
                .unwrap_or_default()
        });
    }

    // The window of rows the body shows. Without scrolling it is the
    // top of the list, truncated where the box ends — today's `shown`,
    // expressed as a window so the drawing loop has one shape.
    let mut window = crate::view::virt::RowWindow { first: 0, count: shown, y0: 0.0 };
    let mut scroll_geom = None;
    let mut bar_look = None;
    if let Some(s) = state.as_deref_mut() {
        s.extent = crate::view::table::Extent {
            scrollable: scrolling,
            viewport: body_h,
            content: crate::view::virt::content_h(row_h, rows.len()),
            bar: None,
        };
    }
    if scrolling {
        let phys = crate::view::scroll::ScrollPhysics::read(sf);
        let look = crate::view::scroll::ScrollbarLook::read(sf);
        let now = sf.now();
        let mouse = sf.mouse();
        if let Some(s) = state.as_deref_mut() {
            let content = crate::view::virt::content_h(row_h, rows.len());
            // A clipping surface leaves the offset free and a row may be
            // half visible; one that cannot snaps to whole rows.
            let snap = if can_clip {
                crate::view::Snap::None
            } else {
                crate::view::Snap::Row(row_h)
            };
            s.scroll.tick(now, body_h, content, snap, &phys);
            window = crate::view::virt::row_window(s.scroll.offset(), body_h, row_h, rows.len());
            let area = Rect::new(r.x, body_y, r.w, body_h);
            // The band the bar could occupy at its WIDEST, on whichever
            // edge the theme puts it: a bar that grows under the pointer
            // must not shrink out from under it and start flickering.
            let reach = look.w_hover.max(look.w) + look.margin;
            let band = match look.edge {
                crate::view::scroll::ScrollbarEdge::Left => {
                    Rect::new(area.x, area.y, reach, area.h)
                }
                crate::view::scroll::ScrollbarEdge::Right => {
                    Rect::new(area.right() - reach, area.y, reach, area.h)
                }
            };
            let hovered = band.contains(mouse.0, mouse.1);
            scroll_geom = crate::view::scroll::scrollbar(
                area,
                &look,
                s.scroll.offset(),
                body_h,
                content,
                hovered || s.scroll.dragging(),
            );
            s.extent.bar = scroll_geom.as_ref().map(|g| (g.track, g.thumb));
            bar_look = Some((look, hovered));
        }
    }

    // From here on the state is only READ, which is what lets the order
    // be borrowed for the whole of the drawing below.
    let order: &[usize] = match state.as_deref() {
        Some(s) if s.order().len() == rows.len() => s.order(),
        _ => &[],
    };
    // `order[d]` when there is one, `d` when there is not: an identity
    // permutation is not worth a vector per frame.
    let model_of = |d: usize| -> usize { order.get(d).copied().unwrap_or(d) };
    let sort = state.as_deref().and_then(|s| s.sort);
    let pressed_head = state.as_deref().and_then(|s| s.pressed_head());
    let overrides: &[Option<f32>] = state.as_deref().map(|s| &s.widths[..]).unwrap_or(&[]);
    let selected_key: Option<&str> = state.as_deref().and_then(|s| s.selected.as_deref());
    let dragging_thumb = state.as_deref().is_some_and(|s| s.scroll.dragging());
    let now = sf.now();
    let bar_alpha = match (state.as_deref(), &bar_look) {
        (Some(s), Some((look, hovered))) => {
            if *hovered || dragging_thumb {
                1.0
            } else {
                s.scroll.fade_alpha(now, look.auto_hide, look.fade_ms)
            }
        }
        _ => 1.0,
    };

    // Fixed columns are measured from their WIDEST CELL (u2 §2.7), not
    // from their heading — measuring from headings is what made `PID` as
    // narrow as the word and ellipsised every five-digit pid. `Heading`
    // keeps the old rule for a column that asks for it; the elastic one
    // absorbs whatever is left either way.
    let col_gap = sf.px("table.col_gap") * st.shrink;
    let cell_pad = sf.px("table.cell_pad") * st.shrink;
    let bar_w = sf.px("table.bar_w") * st.shrink;
    let tokens = crate::view::table::TableTokens {
        col_gap,
        cell_pad,
        bar_w,
        // Raw, not shrunk — the asymmetry this function has always had.
        elastic_min_w: sf.px("table.elastic_min_w"),
        col_min_w: sf.px("table.col_min_w"),
    };
    // The rows the measure looks at: what the body is about to show.
    // Without a window that is `take(shown.max(1))`, exactly as before.
    let measured_span = if scrolling {
        window.first..window.first + window.count
    } else {
        0..shown.max(1).min(rows.len().max(1))
    };
    let mut measured: Vec<crate::view::table::ColMeasure> = Vec::with_capacity(columns.len());
    for (i, c) in columns.iter().enumerate() {
        let head = sf.measure(head_px, &c.title, head_track);
        let mut content = head;
        if c.width == ColWidth::Content && i != st.elastic {
            for d in measured_span.clone() {
                let Some(row) = rows.get(model_of(d)) else { continue };
                let slot = sev_slot(row);
                if let Some(text) = row.get(cell_of(slot, i)) {
                    content = content.max(sf.measure(cell_px, text, cell_track));
                }
            }
        }
        measured.push(crate::view::table::ColMeasure {
            head,
            content,
            bar: matches!(c.kind, CellKind::Bar { .. }),
        });
    }
    let widths = crate::view::table::solve_widths(&measured, r.w, st.elastic, overrides, &tokens);

    // Every column's width reserved `col_gap + cell_pad` beyond its
    // content, so every cell draws inside the CONTENT SPAN — a
    // right-aligned column ends a full gap before its neighbour instead
    // of touching it (u2 §2.7's `1471  firefox`, not `1471firefox`).
    // The TRIM budget keeps the cell_pad as headroom: a content-measured
    // column's widest cell measures exactly its own column, and trimming
    // at exactly its own width is a coin-toss on float rounding.
    let span = |w: f32| (w - col_gap - cell_pad).max(1.0);
    let trim_w = |w: f32| (w - col_gap).max(1.0);

    // The heading row, its rule, then the body.
    {
        let head_c = sf.color("component.table.head");
        let glyph = sf.px("table.sort_glyph") * st.shrink;
        let glyph_gap = sf.px("table.sort_glyph_gap") * st.shrink;
        let grip = sf.px("table.resize_grip") * st.shrink;
        let mouse = sf.mouse();
        let band_h = head_gap.max(0.0);
        let mut x = r.x;
        for (i, (c, w)) in columns.iter().zip(widths.iter()).enumerate() {
            let band = Rect::new(x, r.y, *w, band_h);
            let sorted = sort.map(|(sc, _)| sc) == Some(i);
            // The class ladder answers only for a heading the pointer
            // can actually reach; a table without `interactive` draws
            // the resting heading it has always drawn.
            let mut text_c = head_c;
            if interactive {
                let hovered = band.contains(mouse.0, mouse.1);
                let rung = match (pressed_head == Some(i), hovered, sorted) {
                    (true, _, _) => Some(theme::parse::State::Press),
                    (_, true, true) => Some(theme::parse::State::SelectedHover),
                    (_, true, false) => Some(theme::parse::State::Hover),
                    (_, false, true) => Some(theme::parse::State::Selected),
                    _ => None,
                };
                if let Some(rung) = rung {
                    let style = sf.class_state("table.head", rung);
                    if style.fill.a > 0.0 {
                        sf.rect(band, style.fill);
                    }
                    if style.text.a > 0.0 {
                        text_c = style.text;
                    }
                }
                if let Some(h) = hits.as_deref_mut() {
                    h.push(band, crate::view::Hit::TableHead { id: view_id, col: i });
                    // The grip straddles the join, so both neighbours
                    // reach it; recorded AFTER the heading because the
                    // last rectangle drawn is the one that takes the
                    // press.
                    if grip > 0.0 && i + 1 < columns.len() {
                        h.push(
                            Rect::new(x + w - grip, r.y, grip * 2.0, band_h),
                            crate::view::Hit::TableDivider { id: view_id, col: i },
                        );
                    }
                }
            }
            // The sort marker takes its room out of the trim budget, so
            // a sorted heading is trimmed rather than overdrawn. It
            // reports the ORDER, so it is drawn whenever there is one —
            // a script that opened the table sorted says so even where
            // the user cannot re-sort it.
            let marker = if sorted { glyph + glyph_gap } else { 0.0 };
            let budget = (trim_w(*w) - marker).max(1.0);
            let cell_w = (span(*w) - marker).max(1.0);
            let text = paint::fit_end(sf, head_px, &c.title, budget, head_track);
            // A heading the ellipsis cut short finishes its sentence when
            // the pointer rests on it (F2 §8.1). Only a TRIMMED one asks:
            // a tooltip that repeats what is already legible is noise.
            if explain && band.contains(mouse.0, mouse.1) && text != c.title {
                sf.tooltip(crate::object::tooltip::cell_key(view_id, i, ""), band, &c.title);
            }
            paint::cell_text(sf, x, r.y, cell_w, c.align, head_px, &text, text_c, head_track);
            if marker > 0.0 {
                if let Some((_, dir)) = sort {
                    paint::sort_marker(sf, x + span(*w) - glyph, r.y, glyph, head_px, dir, text_c);
                }
            }
            x += w;
        }
    }
    let mut y = r.y + head_gap;
    let rule_w = sf.px("table.rule");
    let rule_c = sf.color("component.table.rule");
    sf.line(r.x, y, r.right(), y, rule_w, rule_c);
    y += head_gap_below;
    let row_c = sf.color("component.table.row");
    let zebra_c = sf.bed("component.table.zebra");
    let zebra_every = sf.px("script.table_zebra_every").max(0.0) as usize;
    let bar_h = sf.px("script.meter_bar_h") * st.shrink;
    let vgap = sf.px("meter.value_gap") * st.shrink;
    let mouse = sf.mouse();
    // A window that starts part-way down a row needs the body clipped,
    // or the first row paints over the rule above it.
    let clipped = scrolling && sf.clip(Rect::new(r.x, body_y, r.w, body_h));
    for d in window.first..window.first + window.count {
        let Some(row) = rows.get(model_of(d)) else { continue };
        let row_y = if scrolling { body_y + window.y_of(d, row_h) } else { y };
        let rect = Rect::new(r.x, row_y, r.w, row_h);
        // Zebra follows the DISPLAY position, not the loop counter, so
        // the stripes stay put while the body scrolls under them.
        if st.zebra && zebra_every > 0 && (d + 1) % zebra_every == 0 {
            sf.rect(rect, zebra_c);
        }
        let slot = sev_slot(row);
        // The row's identity: the key column's text, or its place in the
        // model when the script named none.
        let key = match key_col.and_then(|k| row.get(cell_of(slot, k))) {
            Some(k) => k.clone(),
            None => model_of(d).to_string(),
        };
        if select {
            let hovered = rect.contains(mouse.0, mouse.1)
                && mouse.1 >= body_y
                && mouse.1 < body_y + body_h;
            let chosen = selected_key == Some(key.as_str());
            let rung = match (chosen, hovered) {
                (true, true) => Some(theme::parse::State::SelectedHover),
                (true, false) => Some(theme::parse::State::Selected),
                (false, true) => Some(theme::parse::State::Hover),
                _ => None,
            };
            if let Some(rung) = rung {
                // `script.row` — the class the master already declares
                // for "a selectable row a script widget draws". No new
                // selection colour exists, or needs to.
                let style = sf.class_state("script.row", rung);
                if style.fill.a > 0.0 {
                    sf.rect(rect, style.fill);
                }
            }
        }
        // Recorded whatever `select` says: a row rectangle is also how
        // the wheel finds out WHICH view the pointer is over, and a
        // table that scrolls without selecting still has to answer that.
        if let Some(h) = hits.as_deref_mut() {
            h.push(rect, crate::view::Hit::Row { id: view_id, key: key.clone() });
        }
        let sev = match slot.and_then(|sc| row.get(sc)) {
            Some(w) => Some(match sev_of(w) {
                Some(s) => s,
                None => paint::sev_fallback(sf),
            }),
            None => None,
        };
        let color = match sev {
            Some(s) => paint::sev_text(sf, s),
            None => row_c,
        };
        let mut x = r.x;
        for (i, (c, w)) in columns.iter().zip(widths.iter()).enumerate() {
            let Some(text) = row.get(cell_of(slot, i)) else {
                x += w;
                continue;
            };
            match c.kind {
                CellKind::Text => {
                    let shown = paint::fit_end(sf, cell_px, text, trim_w(*w), cell_track);
                    // The elastic column is the one the ellipsis usually
                    // reaches, but any column can be cut short by a
                    // dragged width, so the test is what HAPPENED rather
                    // than which column it was.
                    // The pointer test comes first: only one cell in the
                    // table can be under it, and comparing the drawn
                    // text with the whole of it for every cell of every
                    // visible row to answer a question about one of them
                    // is work the body loop does not need.
                    if explain {
                        let cell = Rect::new(x, row_y, *w, row_h);
                        if cell.contains(mouse.0, mouse.1)
                            && mouse.1 >= body_y
                            && mouse.1 < body_y + body_h
                            && shown != *text
                        {
                            let id = crate::object::tooltip::cell_key(view_id, i, &key);
                            sf.tooltip(id, cell, text);
                        }
                    }
                    paint::cell_text(
                        sf, x, row_y, span(*w), c.align, cell_px, &shown, color, cell_track,
                    );
                }
                CellKind::Bar { of } => {
                    // The number is unchanged; the track behind it is a
                    // second reading of the same value (u2 §2.7).
                    let tw = sf.measure(cell_px, text, cell_track);
                    let avail = (span(*w) - tw - vgap).max(0.0).min(bar_w);
                    if avail > 1.0 && of > 0.0 {
                        let v = paint::leading_number(text).unwrap_or(0.0);
                        let bar = Rect::new(
                            x + span(*w) - tw - vgap - avail,
                            row_y + (row_h - bar_h).max(0.0) / 2.0,
                            avail,
                            bar_h.min(row_h),
                        );
                        paint::meter(sf, bar, v / of, sev, true);
                    }
                    sf.text(
                        cell_px,
                        x + span(*w),
                        row_y,
                        text,
                        color,
                        cell_track,
                        Align::Right,
                    );
                }
                CellKind::Badge => {
                    paint::badge(
                        sf,
                        Rect::new(x, row_y, span(*w), row_h),
                        text,
                        sev,
                        BadgeStyle::FromTheme,
                        c.align,
                        st.shrink,
                    );
                }
            }
            x += w;
        }
        if !scrolling {
            y += row_h;
        }
    }
    if clipped {
        sf.unclip();
    }
    // The bar last (u2 §2.10), over the rows it covers — which is why
    // its rectangles are recorded last too: the pointer points at what
    // it can see.
    if let (Some(geom), Some((_, hovered))) = (scroll_geom, bar_look) {
        paint::scrollbar(sf, &geom, bar_alpha, hovered, dragging_thumb);
        if let Some(h) = hits.as_deref_mut() {
            let mid = geom.thumb.y + geom.thumb.h / 2.0;
            h.push(
                Rect::new(geom.track.x, geom.track.y, geom.track.w, mid - geom.track.y),
                crate::view::Hit::Track { id: view_id, toward_end: false },
            );
            h.push(
                Rect::new(geom.track.x, mid, geom.track.w, geom.track.bottom() - mid),
                crate::view::Hit::Track { id: view_id, toward_end: true },
            );
            h.push(geom.thumb, crate::view::Hit::Thumb { id: view_id });
        }
    }
}

/// One cell of a `columns` strip: a small label, a larger value, and the
/// script's judgement of the value (u2 §2.2's POWER severity).
pub struct ColumnCell {
    pub label: String,
    pub value: String,
    pub sev: Option<Sev>,
}

/// A `columns` strip's arrangement. Roles come through the caller from the
/// `script.columns_*_role` bindings (or the script's own naming); align of
/// `None` defers to the theme's `columns.align`.
pub struct ColumnsStyle {
    pub label_role: Role,
    pub value_role: Role,
    pub align: Option<Align>,
    /// Hairline dividers between cells — arrangement furniture the script
    /// opts into; `columns.divider` and its colour decide the look.
    pub dividers: bool,
    pub shrink: f32,
}

/// Columns of a small label above a larger value — the shape used for
/// at-a-glance readouts. Cells are sized by their CONTENT, the leftover
/// shared evenly: the images' strips are runs of values separated by
/// dividers (u2 §2.2, image 7's pipes), not equal thirds — equal thirds
/// is how a long date ends in an ellipsis while `AC` hoards a third of
/// the strip. The images divide and align; whether this strip does
/// either is `st`.
pub fn columns(ctx: &mut Ctx, r: Rect, cells: &[ColumnCell], st: &ColumnsStyle) {
    if cells.is_empty() {
        return;
    }
    static BLOCK: OnceLock<TokenId> = OnceLock::new();
    static LABEL_GAP: OnceLock<TokenId> = OnceLock::new();
    static GUTTER: OnceLock<TokenId> = OnceLock::new();
    static ALIGN: OnceLock<TokenId> = OnceLock::new();
    static DIVIDER: OnceLock<TokenId> = OnceLock::new();
    static DIVIDER_INSET: OnceLock<TokenId> = OnceLock::new();
    static LABEL_C: OnceLock<TokenId> = OnceLock::new();
    static VALUE_C: OnceLock<TokenId> = OnceLock::new();
    static DIVIDER_C: OnceLock<TokenId> = OnceLock::new();
    let t = theme::resolved();
    let natural = (t.px(tok(&BLOCK, "script.columns_block")) * st.shrink).min(r.h);
    let lp = st.label_role.px(ctx, st.shrink);
    let vp = st.value_role.px(ctx, st.shrink);
    let ltrack = st.label_role.tracking_px(lp);
    let vtrack = st.value_role.tracking_px(vp);
    // The baseline step from label to value is the shape itself, so it
    // has a token of its own rather than riding on the label's size.
    let vgap = t.px(tok(&LABEL_GAP, "columns.label_gap")) * st.shrink;
    let gutter = t.px(tok(&GUTTER, "rhythm.value_gutter")) * st.shrink;
    let label_c = col(&LABEL_C, "component.columns.label");
    let value_c = col(&VALUE_C, "component.columns.value");
    let align = st.align.unwrap_or_else(|| {
        match word_of(tok(&ALIGN, "columns.align")).as_str() {
            "left" => Align::Left,
            "right" => Align::Right,
            _ => Align::Center,
        }
    });
    // Each cell's natural width, then the leftover shared evenly. An
    // overfull strip shrinks every cell evenly instead, and `fit_end`
    // trims inside the cell as before.
    let widths: Vec<f32> = {
        let nat: Vec<f32> = cells
            .iter()
            .map(|cell| {
                let lw = ctx.fonts.measure(FONT_UI, lp, &cell.label, ltrack);
                let vw = ctx.fonts.measure(FONT_UI, vp, &cell.value, vtrack);
                lw.max(vw) + 2.0 * gutter
            })
            .collect();
        let extra = (r.w - nat.iter().sum::<f32>()) / cells.len() as f32;
        nat.into_iter().map(|w| (w + extra).max(1.0)).collect()
    };
    let y = block_top(&r, natural);
    let mut x0 = r.x;
    for (cell, cw) in cells.iter().zip(widths.iter().copied()) {
        let vc = cell.sev.map(sev_text).unwrap_or(value_c);
        let shown = fit_end_tracked(ctx, vp, &cell.value, cw - 2.0 * gutter, vtrack);
        match align {
            Align::Center => {
                let cx = x0 + cw / 2.0;
                ctx.dl
                    .text_center(ctx.fonts, FONT_UI, lp, cx, y, &cell.label, label_c, ltrack);
                ctx.dl
                    .text_center(ctx.fonts, FONT_UI, vp, cx, y + vgap, &shown, vc, vtrack);
            }
            Align::Left => {
                let cx = x0 + gutter;
                ctx.dl.text(ctx.fonts, FONT_UI, lp, cx, y, &cell.label, label_c, ltrack);
                ctx.dl.text(ctx.fonts, FONT_UI, vp, cx, y + vgap, &shown, vc, vtrack);
            }
            Align::Right => {
                let cx = x0 + cw - gutter;
                ctx.dl
                    .text_right(ctx.fonts, FONT_UI, lp, cx, y, &cell.label, label_c, ltrack);
                ctx.dl
                    .text_right(ctx.fonts, FONT_UI, vp, cx, y + vgap, &shown, vc, vtrack);
            }
        }
        x0 += cw;
    }
    if st.dividers {
        let stroke = t.px(tok(&DIVIDER, "columns.divider"));
        if stroke > 0.0 {
            let inset = t.px(tok(&DIVIDER_INSET, "columns.divider_inset")) * st.shrink;
            let c = col(&DIVIDER_C, "component.columns.divider");
            // On the boundary between two cells, wherever content sizing
            // put it.
            let mut x = r.x;
            for w in widths.iter().take(cells.len() - 1) {
                x += w;
                ctx.dl
                    .line(x, r.y + inset, x, r.bottom() - inset, stroke, c);
            }
        }
    }
}

// ---------------------------------------------------------------- runs

/// One styled run of a `runs` line (u2 §3.1 #3): its text, its role, the
/// script's severity judgement, and the id of the `motion.*` effect that
/// drives its alpha — never its glyph, so the advance holds and the line
/// cannot jitter (I13).
pub struct Run {
    pub text: String,
    pub role: Role,
    pub sev: Option<Sev>,
    pub blink: Option<String>,
    /// Drawn flush to the line's RIGHT edge, after every start run — u2
    /// §2.5's right-aligned temperature. An arrangement flag, not a look.
    pub end: bool,
}

/// One line of styled runs, aligned as a unit. Sizes may differ between
/// runs; their em boxes are bottom-aligned, which is the closest thing to
/// a shared baseline the draw list can do until the cap-height primitive
/// lands (F021). Runs marked `end` form a trailing cluster on the line's
/// right edge; the rest align as one unit in the room that cluster leaves
/// (u2 §2.5's LOAD line). Returns the width drawn.
pub fn runs(ctx: &mut Ctx, r: Rect, items: &[Run], align: Align, shrink: f32) -> f32 {
    if items.is_empty() {
        return 0.0;
    }
    static VALUE_C: OnceLock<TokenId> = OnceLock::new();
    let sized: Vec<(f32, f32, f32)> = items
        .iter()
        .map(|run| {
            let px = run.role.px(ctx, shrink);
            let track = run.role.tracking_px(px);
            let w = ctx.fonts.measure(FONT_UI, px, &run.text, track);
            (px, track, w)
        })
        .collect();
    let start_w: f32 =
        items.iter().zip(&sized).filter(|(run, _)| !run.end).map(|(_, (_, _, w))| *w).sum();
    let end_w: f32 =
        items.iter().zip(&sized).filter(|(run, _)| run.end).map(|(_, (_, _, w))| *w).sum();
    let max_px = sized.iter().map(|(px, _, _)| *px).fold(0.0, f32::max);
    // The start cluster aligns in the room the end cluster leaves.
    let room = r.w - end_w;
    let mut x = match align {
        Align::Left => r.x,
        Align::Center => r.x + (room - start_w) / 2.0,
        Align::Right => r.x + room - start_w,
    };
    let mut ex = r.right() - end_w;
    let fallback = col(&VALUE_C, "component.script.value");
    for (run, (px, track, w)) in items.iter().zip(sized.iter()) {
        let mut c = run.sev.map(sev_text).unwrap_or_else(|| {
            let rc = run.role.color();
            // A role with no ink of its own (empty theme) still shows.
            if rc.a > 0.0 { rc } else { fallback }
        });
        if let Some(id) = &run.blink {
            c.a *= blink_factor(id, ctx.t);
        }
        // Bottom-aligned em boxes stand in for the shared baseline.
        let y = r.y + (max_px - px);
        let cursor = if run.end { &mut ex } else { &mut x };
        ctx.dl.text(ctx.fonts, FONT_UI, *px, *cursor, y, &run.text, c, *track);
        *cursor += w;
    }
    start_w + end_w
}

// ---------------------------------------------------------------- badge

/// How a badge is filled. `FromTheme` asks `badge.style_from_severity`
/// and the severity's own `badge_style`; the script may insist on solid
/// or hollow, which is arrangement, not colour. `hatched` and
/// `hollow_dashed` degrade to hollow until the renderer can draw them.
#[derive(Clone, Copy, PartialEq)]
pub enum BadgeStyle {
    FromTheme,
    Solid,
    Hollow,
}

/// The CRITICAL / CONTAINED pill of images 1, 3 and 4 (u2 §3.1 #11): a
/// filled, ringed capsule around a short text, its four colours from the
/// severity at draw time. The pill's corner honours `badge.corner` as far
/// as the renderer can: a positive radius cuts a chamfer, `pill` (R5)
/// degrades to square — family A's look either way. Returns the pill
/// width.
pub fn badge(
    ctx: &mut Ctx,
    r: Rect,
    text: &str,
    sev: Option<Sev>,
    style: BadgeStyle,
    align: Align,
    shrink: f32,
) -> f32 {
    paint::badge(&mut CtxSurface::new(ctx), r, text, sev, style, align, shrink)
}

// ---------------------------------------------------------------- rule

/// A horizontal hairline as a stack element in its own right (u2 §3.1
/// #12) — until now the only rule in the vocabulary was welded to `title`.
/// Drawn across the middle of `r`; the stroke does not shrink with the
/// stack, a hairline being a hairline at every scale.
pub fn rule(ctx: &mut Ctx, r: Rect) {
    static W: OnceLock<TokenId> = OnceLock::new();
    static C: OnceLock<TokenId> = OnceLock::new();
    let stroke = theme::resolved().px(tok(&W, "script.rule_width"));
    if stroke <= 0.0 {
        return;
    }
    let y = r.y + r.h / 2.0;
    ctx.dl
        .line(r.x, y, r.right(), y, stroke, col(&C, "component.script.rule"));
}

// ---------------------------------------------------------------- group

/// A `group`'s caption line with its optional rule (u2 §3.1 #13): a
/// section label in `script.group_label_role`, and — when
/// `script.group_rule` says so — a hairline along the bottom edge of the
/// header's box. The nested elements are the caller's to draw below.
pub fn group_header(ctx: &mut Ctx, r: Rect, label: &str, shrink: f32) {
    static ROLE: OnceLock<TokenId> = OnceLock::new();
    static RULE_W: OnceLock<TokenId> = OnceLock::new();
    static LABEL_C: OnceLock<TokenId> = OnceLock::new();
    static RULE_C: OnceLock<TokenId> = OnceLock::new();
    let t = theme::resolved();
    let role = bound_role(&ROLE, "script.group_label_role");
    let px = role.px(ctx, shrink);
    let track = role.tracking_px(px);
    let ty = center_line_y(ctx, r.y, r.h, px, role.leading());
    ctx.dl.text(
        ctx.fonts, FONT_UI, px, r.x, ty, label,
        col(&LABEL_C, "component.script.label"), track,
    );
    let stroke = t.px(tok(&RULE_W, "script.group_rule"));
    if stroke > 0.0 {
        let y = r.bottom() - stroke / 2.0;
        ctx.dl
            .line(r.x, y, r.right(), y, stroke, col(&RULE_C, "component.script.rule"));
    }
}
