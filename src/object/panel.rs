//! The widget panel container — drawn by the HOST, for all twelve
//! widgets (u2 §4, spec §5.12).
//!
//! Until this existed there was no container, only four inventions of
//! one: the terminal's hand-drawn chamfer, the file browser's private
//! title arithmetic, the scripts' `title` element, and nothing at all.
//! Here the parts live once, outside in: fill/glass → edge ring → title
//! band (left text, right text, rule) → content box. The widget is then
//! handed the CONTENT BOX and draws content, never chrome.
//!
//! Every colour and metric is a theme token (`panel.*`, `elev.panel.*`,
//! the type role `panel.title.role` names, `component.panel.*`). There
//! is no fallback underneath any read: a missing token degrades through
//! the engine's per-kind default and is allowed to look raw.

use super::window::{corner_segments, corner_style, panel_edge_glow};
use crate::draw::Corner;
use crate::font::FONT_UI;
use crate::theme::{self, Color, TokenId};
use crate::ui;
use crate::view::surface::{CtxSurface, Surface};
use crate::widget::Chrome;
use crate::{Ctx, Rect};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};

fn tok(cell: &'static OnceLock<TokenId>, name: &'static str) -> TokenId {
    *cell.get_or_init(|| theme::id(name).unwrap_or(TokenId::MISSING))
}

/// A baked theme colour in the draw list's own colour type.
fn col(c: theme::ThemeColor) -> Color {
    Color { r: c.r, g: c.g, b: c.b, a: c.a }
}

/// A `same_as_parent` sentinel (baked negative) falls back to its parent
/// value; anything the theme actually stated is clamped to a length.
fn or_parent(v: f32, parent: f32) -> f32 {
    if v < 0.0 {
        parent
    } else {
        v
    }
}

/// The container's metrics, read fresh each call — they are what a mood,
/// a resize or a theme swap changes; the ids underneath are cached.
struct Metrics {
    border: f32,
    pad_x: f32,
    pad_y: f32,
    pad_y_min: f32,
    band_h: f32,
    band_h_min: f32,
    block_h: f32,
    min_content: f32,
}

fn metrics() -> Metrics {
    static BORDER: OnceLock<TokenId> = OnceLock::new();
    static PAD: OnceLock<TokenId> = OnceLock::new();
    static PAD_X: OnceLock<TokenId> = OnceLock::new();
    static PAD_Y: OnceLock<TokenId> = OnceLock::new();
    static PAD_Y_MIN: OnceLock<TokenId> = OnceLock::new();
    static BAND_H: OnceLock<TokenId> = OnceLock::new();
    static BAND_H_MIN: OnceLock<TokenId> = OnceLock::new();
    static BLOCK_H: OnceLock<TokenId> = OnceLock::new();
    static MIN_CONTENT: OnceLock<TokenId> = OnceLock::new();
    let t = theme::resolved();
    let pad = t.px(tok(&PAD, "panel.content_pad")).max(0.0);
    Metrics {
        border: t.px(tok(&BORDER, "panel.border")).max(0.0),
        pad_x: or_parent(t.px(tok(&PAD_X, "panel.content_pad_x")), pad).max(0.0),
        pad_y: or_parent(t.px(tok(&PAD_Y, "panel.content_pad_y")), pad).max(0.0),
        // Step 1 of the degradation ladder shrinks pad_y toward this.
        pad_y_min: t.px(tok(&PAD_Y_MIN, "space.1")).max(0.0),
        band_h: t.px(tok(&BAND_H, "panel.title.band_h")).max(0.0),
        band_h_min: t.px(tok(&BAND_H_MIN, "panel.title.band_h_min")).max(0.0),
        block_h: t.px(tok(&BLOCK_H, "panel.title.block_h")).max(0.0),
        min_content: t.px(tok(&MIN_CONTENT, "panel.min_content_h")).max(0.0),
    }
}

/// What the degradation ladder settled on for one panel.
struct Placement {
    /// The widget's content box.
    content: Rect,
    /// The title band, when one survives. `(rect, collapsed)`.
    band: Option<(Rect, bool)>,
    /// The highest ladder step taken (0 = the panel had room).
    step: u8,
}

/// The height the container adds around a widget's content: what the
/// sizing pass must add to a `Sizing::Content` want before publishing it,
/// and divide back out of the box when computing the widget's scale
/// (u2 §4.2). Uses the resting metrics — the ladder exists for panels a
/// LAYOUT made short, and a panel sized from its content is not one.
pub fn chrome_extra(titled: bool) -> f32 {
    let m = metrics();
    2.0 * (m.border + m.pad_y) + if titled { m.block_h } else { 0.0 }
}

/// The ordered, stated, diagnosed ladder for panels too short for the
/// full container (u2 §4.2) — never a silent overlap:
///
/// 1. shrink the vertical content padding toward `space.1`;
/// 2. collapse the band to `panel.title.band_h_min`;
/// 3. drop the band (the widget's own inline title takes over);
/// 4. clamp the content box to `panel.min_content_h` and let the
///    widget's overflow policy decide.
fn place(r: Rect, titled: bool) -> Placement {
    let m = metrics();
    let inner_w = (r.w - 2.0 * (m.border + m.pad_x)).max(1.0);
    let cx = r.x + m.border + m.pad_x;
    let inner_h = (r.h - 2.0 * m.border).max(0.0);

    let content_h = |pad_y: f32, block: f32| inner_h - 2.0 * pad_y - block;
    let mut pad_y = m.pad_y;
    let mut block = if titled { m.block_h } else { 0.0 };
    let mut band = titled.then_some((m.band_h.min(block), false));
    let mut step = 0u8;

    // Step 1: give the padding back before touching the band.
    if content_h(pad_y, block) < m.min_content {
        step = 1;
        let need = m.min_content - content_h(pad_y, block);
        pad_y = (pad_y - need / 2.0).max(m.pad_y_min);
    }
    // Step 2: the collapsed band — smaller, but still a band.
    if titled && content_h(pad_y, block) < m.min_content {
        step = 2;
        block = m.band_h_min.min(m.band_h);
        band = Some((block, true));
    }
    // Step 3: no band; the widget's `title` element draws inline as it
    // does today, and nothing of the heading is lost.
    if titled && content_h(pad_y, block) < m.min_content {
        step = 3;
        block = 0.0;
        band = None;
    }
    // Step 4: the box will not fit even bare — clamp it and let the
    // widget's overflow policy (scale to its floor, then clip) act.
    let mut h = content_h(pad_y, block);
    if h < m.min_content {
        step = 4;
        h = m.min_content.min(inner_h.max(1.0));
    }

    let band = band.map(|(bh, collapsed)| {
        (
            Rect::new(cx, r.y + m.border + pad_y, inner_w, bh),
            collapsed,
        )
    });
    Placement {
        content: Rect::new(cx, r.y + m.border + pad_y + block, inner_w, h.max(1.0)),
        band,
        step,
    }
}

/// The content box the container leaves inside a panel rect — the same
/// arithmetic [`draw`] uses, without drawing. For code that must answer
/// geometry with no frame in flight.
pub fn content_box(r: Rect, titled: bool) -> Rect {
    place(r, titled).content
}

/// How many times each ladder step has been entered, for tests and
/// diagnostics. Index 0 = step 1.
pub fn degradation_counts() -> [u32; 4] {
    [0, 1, 2, 3].map(|i| COUNTS[i].load(Ordering::Relaxed))
}

static COUNTS: [AtomicU32; 4] =
    [AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0)];

/// Says once — per panel, per theme epoch — which ladder step a panel
/// landed on, and bumps that step's counter. Sixty identical lines a
/// second would bury everything else; a theme swap is allowed to say it
/// again, because the numbers it complains about have changed.
fn report_step(panel: usize, step: u8) {
    if step == 0 {
        return;
    }
    static SEEN: Mutex<Vec<(u32, usize, u8)>> = Mutex::new(Vec::new());
    let epoch = theme::epoch();
    let Ok(mut seen) = SEEN.lock() else { return };
    if seen.iter().any(|&(e, p, s)| e == epoch && p == panel && s == step) {
        return;
    }
    seen.retain(|&(e, _, _)| e == epoch);
    seen.push((epoch, panel, step));
    COUNTS[(step - 1) as usize].fetch_add(1, Ordering::Relaxed);
    eprintln!(
        "nacelle: panel {panel} is too short for its container — degradation step {step} \
         ({})",
        match step {
            1 => "vertical padding shrunk toward space.1",
            2 => "title band collapsed to panel.title.band_h_min",
            3 => "title band dropped; the widget's inline title stands in",
            _ => "content box clamped to panel.min_content_h",
        }
    );
}

/// Draws the container for one panel and answers the content box.
///
/// `r` is the widget box — the panel rect already deflated by the USER's
/// GridPadding, which is a layout preference and not the theme's. The
/// container draws inside it: `elev.panel`'s material, `shape`d ring and
/// edge glow, then the title band from `chrome`, and the same rect this
/// returns must be the one `click` and `wheel` later receive (u2 §4.1).
pub fn draw(ctx: &mut Ctx, r: Rect, chrome: &Chrome, panel_idx: usize) -> Rect {
    static FILL: OnceLock<TokenId> = OnceLock::new();
    static EDGE: OnceLock<TokenId> = OnceLock::new();
    static EDGE_W: OnceLock<TokenId> = OnceLock::new();
    static CORNER_MODE: OnceLock<TokenId> = OnceLock::new();
    static CORNER_IDX: OnceLock<(Option<u16>, Option<u16>)> = OnceLock::new();
    static RADIUS: OnceLock<TokenId> = OnceLock::new();
    static SEGMENTS: OnceLock<TokenId> = OnceLock::new();
    let t = theme::resolved();

    // Material. `elev.panel.glass.rank` is 0 in every shipped theme, so
    // the body is the fill; the glass pair joins when the renderer's
    // blur ranks do (Appendix B R3/R6 — the container does not wait).
    let fill = col(t.bed(tok(&FILL, "elev.panel.fill")));
    let style = corner_style(t, tok(&CORNER_MODE, "elev.panel.corner"), &CORNER_IDX);
    // Through `Corner::sized`, not a clamp: `elev.panel.radius` is a
    // length a master may write as `pill`, and `pill` bakes negative
    // because it has no value until there is a box to be round on.
    let cut = Corner::sized(style, t.px(tok(&RADIUS, "elev.panel.radius")), r);
    let corners = [cut; 4];
    let seg = corner_segments(t, &SEGMENTS, cut.size);
    if fill.a > 0.0 {
        ctx.dl.ring_fill(r, &corners, seg, fill);
    }
    // The ring, and family A's bloom over it when the theme opts in.
    let edge = col(t.color(tok(&EDGE, "elev.panel.edge.color")));
    let edge_w = t.px(tok(&EDGE_W, "elev.panel.edge.width")).max(0.0);
    if edge.a > 0.0 && edge_w > 0.0 {
        ctx.dl.ring(r, &corners, seg, edge_w, edge);
        panel_edge_glow(ctx.dl, t, r, &corners, seg, edge);
    }

    let titled = chrome.title.is_some() || chrome.right.is_some();
    let placed = place(r, titled);
    report_step(panel_idx, placed.step);
    if let Some((band, collapsed)) = placed.band {
        draw_band(ctx, band, collapsed, r, chrome, panel_idx);
    }
    placed.content
}

/// The title band: left text, right text trimmed from the LEFT to the
/// room the title leaves (a path keeps its tail), and the hairline rule
/// on the band's floor.
fn draw_band(
    ctx: &mut Ctx,
    band: Rect,
    collapsed: bool,
    panel: Rect,
    chrome: &Chrome,
    panel_idx: usize,
) {
    static ROLE: OnceLock<TokenId> = OnceLock::new();
    static INSET_X: OnceLock<TokenId> = OnceLock::new();
    static GAP: OnceLock<TokenId> = OnceLock::new();
    static LEFT_C: OnceLock<TokenId> = OnceLock::new();
    static RIGHT_C: OnceLock<TokenId> = OnceLock::new();
    static RULE_W: OnceLock<TokenId> = OnceLock::new();
    static RULE_INSET: OnceLock<TokenId> = OnceLock::new();
    static RULE_C: OnceLock<TokenId> = OnceLock::new();
    let t = theme::resolved();

    // The role `panel.title.role` NAMES — size, tracking, case, leading and
    // the role's own alpha all move together when a theme repoints the
    // binding, which is what the binding is for. The container-query factor
    // is runtime state, so it multiplies here, and a collapsed band caps the
    // size so the text never overruns the band it was shrunk to keep.
    let role = ui::bound_role(&ROLE, "panel.title.role");
    // Two keys of the role that `Role` does not carry. They hang off the
    // role's NAME, which the binding states in a word, so they are spelled
    // from that word rather than pinned to `title.panel` here. The case is
    // compared as a WORD: each role declares its own enum list, so an index
    // memoised across roles would name a different transform in each. The
    // px floor is NOT among them any more — `Role::px` applies the role's
    // own, so a floor spelled again here would be a second answer.
    let (case, alpha) = {
        let mut sf = CtxSurface::new(ctx);
        let word = sf.word("panel.title.role");
        (
            sf.word(&format!("type.{word}.case")),
            sf.px(&format!("type.{word}.alpha")).clamp(0.0, 1.0),
        )
    };
    let mut px = role.px(ctx, 1.0);
    let leading = role.leading();
    // A leading of zero is a broken role, not a short band: dividing by it
    // is what must not happen, and the role draws whatever it draws.
    if collapsed && leading > 0.0 && px * leading > band.h {
        px = (band.h / leading).max(1.0);
    }
    let spacing = role.tracking_px(px);

    // `smallcaps` is approximated as upper until FontSystem can set true
    // small caps (§5.16 owes it the face work).
    let cased = |s: &str| match case.as_str() {
        "none" => s.to_string(),
        "lower" => s.to_lowercase(),
        _ => s.to_uppercase(),
    };

    let inset = t.px(tok(&INSET_X, "panel.title.inset_x")).max(0.0);
    let gap = t.px(tok(&GAP, "panel.title.gap")).max(0.0);
    let y = band.y + (band.h - px * leading) / 2.0;

    let left = chrome.title.as_deref().map(cased).unwrap_or_default();
    let left_c = col(t.color(tok(&LEFT_C, "component.panel.title")));
    let left_c = left_c.alpha(left_c.a * alpha);
    if !left.is_empty() {
        ctx.dl
            .text(ctx.fonts, FONT_UI, px, band.x + inset, y, &left, left_c, spacing);
    }

    if let Some(right) = chrome.right.as_deref() {
        let right = cased(right);
        let used = if left.is_empty() {
            0.0
        } else {
            ctx.fonts.measure(FONT_UI, px, &left, spacing) + gap
        };
        let room = (band.w - 2.0 * inset - used).max(0.0);
        let shown = fit_lead(ctx, px, &right, spacing, room);
        if !shown.is_empty() {
            // The one text in the chrome that is routinely cut: a cwd
            // keeps its tail and loses its root, and the root is exactly
            // what the user cannot reconstruct (F2 §8.1). The anchor is
            // the trimmed text's own box, so resting on the TITLE — a
            // different word, drawn whole — says nothing. The identity
            // is the panel's place plus the path, so two browsers open
            // on one directory are still two things to explain.
            let tw = ctx.fonts.measure(FONT_UI, px, &shown, spacing);
            crate::view::paint::explain_trim(
                &mut CtxSurface::new(ctx),
                crate::object::tooltip::cell_key(0, panel_idx, &right),
                Rect::new(band.right() - inset - tw, band.y, tw, band.h),
                &shown,
                &right,
            );
            let right_c = col(t.color(tok(&RIGHT_C, "panel.title_right_color")));
            let right_c = right_c.alpha(right_c.a * alpha);
            ctx.dl.text_right(
                ctx.fonts,
                FONT_UI,
                px,
                band.right() - inset,
                y,
                &shown,
                right_c,
                spacing,
            );
        }
    }

    // The rule on the band's floor. `panel.title.rule` is a stroke or
    // none; a stroke that bakes to nothing draws nothing.
    let rule_w = t.px(tok(&RULE_W, "panel.title.rule")).max(0.0);
    if rule_w > 0.0 {
        static BORDER: OnceLock<TokenId> = OnceLock::new();
        let b = t.px(tok(&BORDER, "panel.border")).max(0.0);
        let rin = t.px(tok(&RULE_INSET, "panel.title.rule_inset")).max(0.0);
        let ry = band.bottom();
        ctx.dl.line(
            panel.x + b + rin,
            ry,
            panel.right() - b - rin,
            ry,
            rule_w,
            col(t.color(tok(&RULE_C, "component.panel.header_underline"))),
        );
    }
}

/// Shortens `text` from the LEFT with a leading ellipsis until it fits —
/// the file browser's cwd trim, now in the one place that draws the band
/// (u2 §4.3): the tail of a path is the part worth keeping.
fn fit_lead(ctx: &mut Ctx, px: f32, text: &str, spacing: f32, max_w: f32) -> String {
    if max_w <= 0.0 {
        return String::new();
    }
    if ctx.fonts.measure(FONT_UI, px, text, spacing) <= max_w {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let mut start = 1;
    while start < chars.len() {
        let cand: String =
            std::iter::once('\u{2026}').chain(chars[start..].iter().copied()).collect();
        if ctx.fonts.measure(FONT_UI, px, &cand, spacing) <= max_w {
            return cand;
        }
        start += 1;
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tall panel keeps the full container: band, padding, and a
    /// content box strictly inside the widget box.
    #[test]
    fn a_tall_panel_gets_band_and_padding() {
        let r = Rect::new(10.0, 10.0, 300.0, 200.0);
        let p = place(r, true);
        assert_eq!(p.step, 0);
        let (band, collapsed) = p.band.expect("a tall titled panel keeps its band");
        assert!(!collapsed);
        assert!(band.y >= r.y);
        assert!(p.content.y >= band.bottom());
        assert!(p.content.bottom() <= r.bottom() + 0.01);
        assert!(p.content.x > r.x && p.content.right() < r.right());
        // An untitled panel gets the same box without the band.
        let q = place(r, false);
        assert!(q.band.is_none());
        assert!(q.content.h > p.content.h);
    }

    /// The ladder: shrinking panels lose padding first, then collapse
    /// the band, then drop it — stated, ordered, never a silent overlap.
    #[test]
    fn short_panels_degrade_in_ladder_order() {
        let m = metrics();
        // Comfortable: content_pad + band + a roomy content box.
        let tall = 2.0 * (m.border + m.pad_y) + m.block_h + m.min_content * 4.0;
        assert_eq!(place(Rect::new(0.0, 0.0, 300.0, tall), true).step, 0);
        // sysinfo's real height at 1080p: 48.6 px panel, 32.6 widget box.
        // The full container cannot fit; the ladder must answer, and the
        // content box must never fall under min_content while the panel
        // itself can hold it.
        let p = place(Rect::new(0.0, 0.0, 300.0, 32.6), true);
        assert!(p.step >= 1, "a short panel must take a ladder step");
        assert!(p.content.h + 0.01 >= m.min_content.min(32.6 - 2.0 * m.border));
        // Short enough that the band cannot survive at all.
        let q = place(Rect::new(0.0, 0.0, 300.0, m.min_content + 1.0), true);
        assert!(q.band.is_none(), "step 3 drops the band");
        // The band, while it survives, sits above the content.
        let mid = 2.0 * m.border + m.pad_y_min * 2.0 + m.band_h_min + m.min_content + 1.0;
        let s = place(Rect::new(0.0, 0.0, 300.0, mid), true);
        if let Some((band, _)) = s.band {
            assert!(band.bottom() <= s.content.y + 0.01);
        }
    }

    /// `chrome_extra` and `place` are the same arithmetic: a panel given
    /// exactly `content + chrome_extra` hands the content back whole.
    #[test]
    fn chrome_extra_round_trips_through_place() {
        let m = metrics();
        let want = m.min_content * 3.0;
        for titled in [false, true] {
            let r = Rect::new(0.0, 0.0, 300.0, want + chrome_extra(titled));
            let p = place(r, titled);
            assert_eq!(p.step, 0, "titled={titled}");
            assert!(
                (p.content.h - want).abs() < 0.01,
                "titled={titled}: got {} want {want}",
                p.content.h
            );
        }
    }
}
