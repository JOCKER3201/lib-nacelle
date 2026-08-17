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

/// The four corners a `shape.*` preset asks for, starting from the one
/// the object already settled (f3 K6's acceptance condition).
///
/// **This is where `shape.<preset>.corners_tl/tr/br/bl` reach the
/// screen.** Sixteen presets have carried the four keys since the theme
/// engine was written; [`crate::view::paint::preset`] gave them a
/// reader, and a reader nobody calls changes no picture — the key was
/// still dead where it counts. A frame calls this, so a theme writing
/// `shape.panel.corners_tl = [ chamfer, 2u ]` now cuts one corner of
/// every panel and leaves the other three where they were.
///
/// The BASE stays the object's own — `panel.corner_mode` /
/// `panel.corner` for a frame — so this ADDS a say rather than moving
/// one, and nothing that already read a corner reads it anywhere else.
/// Each per-corner key is a PAIR whose slots inherit separately, so a
/// slot left at `same_as_parent` — which is every slot the master ships
/// but `button_alt`'s and `tab`'s — answers exactly the corner that
/// arrived, and the shipped picture is bit for bit what it was.
///
/// The frame is the ONE caller today, and deliberately: `shape.*` has
/// sixteen presets and the audit's §7.2 leaves it open whether every
/// object moves onto them or the master loses the other fifteen. That
/// decision is not this step's to take. What this step owed was a road
/// with traffic on it, and `shape.panel` is the preset K6's own
/// acceptance condition names.
///
/// A preset that declares no such keys keeps `base` on all four and says
/// so once: reading a token that is not there gives zero, and zero is a
/// square corner nobody asked for.
pub(crate) fn per_corner(
    t: &theme::ResolvedTheme,
    cell: &'static OnceLock<[TokenId; 8]>,
    preset: &'static str,
    base: Corner,
    r: Rect,
) -> [Corner; 4] {
    let ids = *cell.get_or_init(|| {
        let mut out = [TokenId::MISSING; 8];
        for (i, slot) in ["tl", "tr", "br", "bl"].iter().enumerate() {
            let key = format!("{preset}.corners_{slot}");
            for (j, part) in ["[0]", "[1]"].iter().enumerate() {
                out[2 * i + j] =
                    theme::id(&format!("{key}{part}")).unwrap_or(TokenId::MISSING);
            }
        }
        out
    });
    if ids.iter().any(|id| *id == TokenId::MISSING) {
        crate::ui::warn_once(
            &format!("per_corner:{preset}"),
            &format!("\"{preset}\" declares no corners_tl/tr/br/bl pair: its corners cannot be set one at a time"),
        );
        return [base; 4];
    }
    // The RULE — what a half-stated pair means — is
    // `view::paint::override_corner` and is not repeated here: the
    // surface layer reads the same four keys for anything drawing
    // through the ABI, and two answers to one question is the drift
    // every shared reader in this crate was pulled out to end. What is
    // local is only HOW the four readings are taken, which on this side
    // is a memoised token and a borrowed word rather than a string key
    // and an allocation.
    let mut out = [base; 4];
    for (i, corner) in out.iter_mut().enumerate() {
        let (word, len) = (ids[2 * i], ids[2 * i + 1]);
        let scalar = t.px(word);
        let stated = t.px(len);
        // Compared as a WORD, not as an enum index: a preset's style
        // slot has no `enum:` list in the master, so its word table
        // grows out of the values a theme actually loaded and an index
        // memoised against the master's own table would name someone
        // else's word after a swap.
        *corner = crate::ui::with_theme_word(word, |w| {
            crate::view::paint::override_corner(base, r, scalar, w, stated)
        });
    }
    out
}

/// The largest ARC on a ring, which is the only thing its tessellation
/// count has to answer for.
///
/// [`crate::draw::ring_points`] takes ONE count for all four corners, so
/// something has to reconcile four sizes into it, and the honest reducer
/// is the largest one that is actually curved: a square corner is a
/// point and a chamfer is a single straight cut, and neither is improved
/// by a finer arc. Reading the plain maximum instead would let a theme
/// that chamfers one corner deeply raise the segment count of the three
/// round ones it never mentioned — a change to a corner it did not name.
pub(crate) fn round_reach(c: &[Corner; 4]) -> f32 {
    c.iter()
        .filter(|k| k.style == CornerStyle::Round)
        .fold(0.0f32, |m, k| m.max(k.size))
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
///
/// `now` is the caller's clock (`Ctx.t`) and it drives ONE thing:
/// `motion.glow_pulse`, §5.22's breathing halo, whose `amplitude` key is
/// documented as "± swing applied to glow_alpha" and had no reader
/// anywhere. The swing is on the halo's ALPHA and nothing else — a
/// breathing RADIUS is a different sprite every frame, and the master's
/// own prohibition list has "anything that affects layout" for the same
/// reason. `glow_pulse` ships disabled and so does `glow.panel_edge`, so
/// the master's picture is what it was, and a theme has to ask twice
/// before this costs a token read.
pub(crate) fn panel_edge_glow(
    dl: &mut DrawList,
    t: &theme::ResolvedTheme,
    r: Rect,
    c: &[Corner; 4],
    segments: u8,
    edge: Color,
    now: f64,
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
    // The breath, applied last so the theme's own number is the one it
    // swings about. A frozen pulse — off, no amplitude, or reduced motion
    // — answers exactly 1.0, and `alpha * 1.0` is `alpha`.
    let alpha =
        (alpha * crate::motion::Effect::of("glow_pulse").cyclic_amplitude(now)).clamp(0.0, 1.0);
    if alpha <= 0.0 {
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
    // `shape.panel` gets the last word on each corner SEPARATELY, which
    // is the whole of what its four per-corner keys were written for.
    static PER: OnceLock<[TokenId; 8]> = OnceLock::new();
    let c = per_corner(t, &PER, "shape.panel", corner, r);
    // The tessellation is settled by the biggest ARC on the ring
    // (`round_reach`): one count serves all four corners, so reading it
    // off the base alone would under-tessellate a corner a theme made
    // rounder than the preset.
    let seg = corner_segments(t, &SEGMENTS, round_reach(&c));
    // The same glass trio the panel rung reads, BY DESIGN and not by
    // accident: the owner's scope for a background is "windows and
    // widgets", one decision for both, so a frame asks the same three
    // tokens `elev::Level` does instead of growing a private copy of them
    // the way it once did for the fill (the drift `elev.rs`'s header
    // already names). Glass replaces the fill; the ring and its glow stand
    // on top either way.
    static G_RANK: OnceLock<TokenId> = OnceLock::new();
    static G_TINT: OnceLock<TokenId> = OnceLock::new();
    static G_WASH: OnceLock<TokenId> = OnceLock::new();
    let rank = t.px(tok(&G_RANK, "elev.panel.glass.rank")).clamp(0.0, 3.0);
    if rank > 0.0 {
        ctx.dl.glass_fill(r, &c, seg, rank, col(t.color(tok(&G_TINT, "elev.panel.glass.tint"))));
        let wash = col(t.color(tok(&G_WASH, "elev.panel.glass.wash")));
        if wash.a > 0.0 {
            ctx.dl.ring_fill(r, &c, seg, wash);
        }
    } else {
        ctx.dl.ring_fill(r, &c, seg, fill);
    }
    ctx.dl.ring(r, &c, seg, width, line);
    panel_edge_glow(ctx.dl, t, r, &c, seg, line, ctx.t);
}
