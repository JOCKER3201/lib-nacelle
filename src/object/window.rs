//! Window objects: the dimmed backdrop and the window frame.
//!
//! The frame takes its geometry from the theme as well as its colour — how
//! thick a border is, how far its corners are cut — so two themes can differ
//! in shape and not only in hue. There is no fallback underneath any read:
//! a missing token degrades through the engine's per-kind default and is
//! allowed to look raw, which is what keeps every design decision in the
//! theme files.

use crate::draw::{ring_segments, Corner, CornerStyle, DrawList};
use crate::font::FontSystem;
use crate::theme::{self, Color, TokenId};
use crate::{Ctx, Rect};
use std::sync::OnceLock;

fn tok(cell: &'static OnceLock<TokenId>, name: &'static str) -> TokenId {
    *cell.get_or_init(|| theme::id(name).unwrap_or(TokenId::MISSING))
}

/// A baked theme colour in the draw list's own colour type.
fn col(c: theme::ThemeColor) -> Color {
    Color { r: c.r, g: c.g, b: c.b, a: c.a }
}

/// The [`CornerStyle`] a corner-mode enum token resolves to. Enum words
/// intern in load order with the master's own word at index 0, so an index
/// is only meaningful against the vocabulary (`theme::enum_index`), never
/// as a bare number. A missing token — or a word the vocabulary does not
/// name — degrades to Square, the raw look of an unstyled rect.
pub(crate) fn corner_style(
    t: &theme::ResolvedTheme,
    mode: TokenId,
    idx: &'static OnceLock<(Option<u16>, Option<u16>)>,
) -> CornerStyle {
    cut_of(t, mode, *idx.get_or_init(|| vocabulary(mode)))
}

/// The `(round, chamfer)` indices in one corner-mode token's vocabulary.
///
/// Taken apart from [`corner_style`] because a caller that reads a WHOLE
/// dictionary at once — [`super::elev::Level`], which memoises every key
/// of one `[elev.*]` level in a single struct — has nowhere to hang a
/// `static` per token and no reason to: the vocabulary is the master's,
/// so it is settled once with the ids beside it.
pub(crate) fn vocabulary(mode: TokenId) -> (Option<u16>, Option<u16>) {
    (theme::enum_index(mode, "round"), theme::enum_index(mode, "chamfer"))
}

/// [`corner_style`] with the vocabulary already in hand.
pub(crate) fn cut_of(
    t: &theme::ResolvedTheme,
    mode: TokenId,
    (round, chamfer): (Option<u16>, Option<u16>),
) -> CornerStyle {
    let cur = Some(t.enum_of(mode));
    if cur == round {
        CornerStyle::Round
    } else if cur == chamfer {
        CornerStyle::Chamfer
    } else {
        // "square", plus anything the vocabulary does not name.
        CornerStyle::Square
    }
}

/// The arc tessellation for a corner of size `size`: the theme's
/// `corner.segments` is the ceiling, `ring_segments` the quarter-pixel
/// chord-error rule under it (r1 §3.4).
pub(crate) fn corner_segments(
    t: &theme::ResolvedTheme,
    cell: &'static OnceLock<TokenId>,
    size: f32,
) -> u8 {
    ring_segments(size, 0.25, t.px(tok(cell, "corner.segments")) as u8)
}

/// The panel-edge glow — `[glow] panel_edge`, family A's signature bloom.
///
/// Every frame that strokes a panel-class ring calls this right after the
/// stroke: an additive soft-sprite ring at the theme's radius, tinted with
/// the edge's own resolved colour (the `element` rule — no variant theme
/// names a different tint) at `panel_edge.alpha`, scaled by the one global
/// knob `glow.alpha_scale`. Default ships it off; a theme opts in, and a
/// raw master draws nothing because a missing flag reads false.
pub(crate) fn panel_edge_glow(
    dl: &mut DrawList,
    t: &theme::ResolvedTheme,
    r: Rect,
    c: &[Corner; 4],
    segments: u8,
    edge: Color,
) {
    static ON: OnceLock<TokenId> = OnceLock::new();
    static RADIUS: OnceLock<TokenId> = OnceLock::new();
    static ALPHA: OnceLock<TokenId> = OnceLock::new();
    static SCALE: OnceLock<TokenId> = OnceLock::new();
    if !t.flag(tok(&ON, "glow.panel_edge.enabled")) {
        return;
    }
    let radius = t.px(tok(&RADIUS, "glow.panel_edge.radius")).max(0.0);
    let alpha = (t.px(tok(&ALPHA, "glow.panel_edge.alpha"))
        * t.px(tok(&SCALE, "glow.alpha_scale")))
    .clamp(0.0, 1.0);
    if radius <= 0.0 || alpha <= 0.0 {
        return;
    }
    dl.glow_ring(r, c, segments, radius, edge.alpha(alpha), FontSystem::mask_soft_uv());
}

/// Dims everything behind a modal window.
///
/// The theme owns both the tint and the strength: `modal.scrim_alpha` states
/// how far the desktop darkens, so three call sites cannot carry three
/// designs. The caller's historical alpha is ignored for that reason; the
/// parameter stays only so existing embedders keep compiling.
///
/// It also claims the whole screen for the pointer ([`crate::pointer`]),
/// which is what MODAL means said in the one place it is drawn: nothing
/// behind the scrim is under the hand, including the parts of the desktop
/// the window itself does not stand on.
pub fn backdrop(ctx: &mut Ctx, _alpha: f32) {
    static SCRIM: OnceLock<TokenId> = OnceLock::new();
    static STRENGTH: OnceLock<TokenId> = OnceLock::new();
    ctx.mouse.cover(Rect::new(0.0, 0.0, ctx.w, ctx.h));
    let t = theme::resolved();
    let scrim = col(t.bed(tok(&SCRIM, "component.modal.scrim")));
    let strength = t.px(tok(&STRENGTH, "modal.scrim_alpha")).clamp(0.0, 1.0);
    ctx.dl.rect(0.0, 0.0, ctx.w, ctx.h, scrim.alpha(strength));
}

/// Opaque window frame: a shaped background and its border, both from the
/// theme.
///
/// `panel.corner` is a length already baked to device pixels, so a theme that
/// wants square corners sets it to `0u` and one that wants a deep cut sets it
/// large; `panel.corner_mode` says HOW that length is cut — a tessellated arc
/// or a 45° chamfer — and `panel.border` is the stroke.
///
/// The box is claimed for the pointer ([`crate::pointer`]) before anything
/// is drawn into it: an OPAQUE frame is exactly the statement "what was
/// under this rectangle can no longer be seen", and a control that cannot
/// be seen is not the one the hand is on. Claimed here rather than by each
/// caller so that every window in every application gets it — including
/// the ones written after this line — and claimed FIRST so the window's
/// own contents, drawn into it afterwards, keep the pointer.
pub fn frame(ctx: &mut Ctx, r: Rect) {
    static FILL: OnceLock<TokenId> = OnceLock::new();
    static LINE: OnceLock<TokenId> = OnceLock::new();
    static CUT: OnceLock<TokenId> = OnceLock::new();
    static WIDTH: OnceLock<TokenId> = OnceLock::new();
    static MODE: OnceLock<TokenId> = OnceLock::new();
    static MODE_IDX: OnceLock<(Option<u16>, Option<u16>)> = OnceLock::new();
    static SEGMENTS: OnceLock<TokenId> = OnceLock::new();
    ctx.mouse.cover(r);
    let t = theme::resolved();
    let fill = col(t.bed(tok(&FILL, "component.panel.fill")));
    let line = col(t.color(tok(&LINE, "component.panel.border")));
    let style = corner_style(t, tok(&MODE, "panel.corner_mode"), &MODE_IDX);
    // Not every negative scalar is nothing: §5.0's `pill` is a WORD about
    // the box (`as round as this one can be`) and bakes negative too, so
    // a clamp at zero answers a master writing `pill` with the square it
    // wrote to avoid. `Corner::sized` is the one place that tells the two
    // apart, and it needs `r` — which is why the radius is read here and
    // not up in a metrics struct that has no box yet.
    let corner = Corner::sized(style, t.px(tok(&CUT, "panel.corner")), r);
    let width = t.px(tok(&WIDTH, "panel.border")).max(0.0);
    let c = [corner; 4];
    let seg = corner_segments(t, &SEGMENTS, corner.size);
    ctx.dl.ring_fill(r, &c, seg, fill);
    ctx.dl.ring(r, &c, seg, width, line);
    panel_edge_glow(ctx.dl, t, r, &c, seg, line);
}
