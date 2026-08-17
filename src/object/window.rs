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

/// The rung a window frame is a surface of, dressed in the window's own
/// key names.
///
/// `[elev.panel]` is Elev 2, and its gloss is "the bordered panel body" —
/// which a window frame is, at the same elevation and out of the same
/// material. That was already true by hand: this file read
/// `component.panel.fill` for the body and `elev.panel.glass.*` for the
/// glass trio, key for key what the rung says, because the owner's scope
/// for a background is "windows and widgets" and one decision has to
/// serve both. What it had was a PRIVATE COPY of the rules, and
/// `elev.rs`'s header names this file as the copy that drifted — it drew
/// its body whatever the alpha where `panel.rs` guarded it, and it drew
/// its ring FLAT where the rung had grown a second colour.
///
/// So the frame states its five older key names once, here, and takes
/// everything else from the rung: the two-stop edge (`edge.mode`,
/// `edge.color2`, `edge.axis` — dead on this surface until now, which is
/// how a theme could write `edge.mode = gradient` and get a flat window
/// border), the glass pair, and every key the ladder grows after this
/// line.
///
/// ONE MODEL OF A WINDOW, and this is where it is kept: a window built
/// into the desktop and a window of an outside application are drawn by
/// this one function, so a rule stated here cannot reach one of them and
/// miss the other.
fn level() -> &'static super::elev::Level {
    static LEVEL: OnceLock<super::elev::Level> = OnceLock::new();
    LEVEL.get_or_init(|| {
        super::elev::Level::of("elev.panel").worn_as(
            "component.panel.fill",
            "panel.corner_mode",
            "panel.corner",
            "component.panel.border",
            "panel.border",
        )
    })
}

/// Opaque window frame: a shaped background and its border, both from the
/// theme.
///
/// `panel.corner` is a length already baked to device pixels, so a theme that
/// wants square corners sets it to `0u` and one that wants a deep cut sets it
/// large; `panel.corner_mode` says HOW that length is cut — a tessellated arc
/// or a 45° chamfer — and `panel.border` is the stroke. All five are read
/// through [`level`], which is what makes them the SAME five a panel, a
/// menu and a tooltip are drawn from.
///
/// The box is claimed for the pointer ([`crate::pointer`]) before anything
/// is drawn into it: an OPAQUE frame is exactly the statement "what was
/// under this rectangle can no longer be seen", and a control that cannot
/// be seen is not the one the hand is on. Claimed here rather than by each
/// caller so that every window in every application gets it — including
/// the ones written after this line — and claimed FIRST so the window's
/// own contents, drawn into it afterwards, keep the pointer.
pub fn frame(ctx: &mut Ctx, r: Rect) {
    ctx.mouse.cover(r);
    level().draw(ctx, r);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::draw::{DrawCmd, DrawList};
    use crate::object::elev::tests::{same_picture, AT_REST};

    /// The box every proof below draws into. Any box would do — what is
    /// read off it is which COMMANDS a frame emits and in which colours,
    /// never where a particular vertex landed.
    fn box_() -> Rect {
        Rect::new(30.0, 18.0, 240.0, 150.0)
    }

    /// What this file drew before it joined the ladder: the body of the
    /// old `frame`, transcribed statement for statement — the glass
    /// branch, the fill under it, the ring, and the bloom over the ring.
    ///
    /// Its OWN transcript, and not the one `menu.rs` and `tooltip.rs`
    /// share ([`crate::object::elev::tests::the_private_copy`]), because
    /// a window's copy was never their copy. Theirs departed from the
    /// rung in TWO places — the body drawn whatever its alpha, the ring
    /// drawn on the width alone — and a window's departed in FOUR: it
    /// also stroked its ring whatever the EDGE's alpha, and it laid the
    /// edge bloom unconditionally where the rung asks for a visible edge
    /// first. Borrowing their transcript would have made this file's
    /// no-move proof a proof about a picture it never drew, and would
    /// have left the two extra departures — the two a theme that lights
    /// `glow.panel_edge` can see — untested.
    fn the_frames_private_copy(dl: &mut DrawList, t: &theme::ResolvedTheme, r: Rect, now: f64) {
        static SEG: OnceLock<TokenId> = OnceLock::new();
        let id = |n: &str| theme::id(n).unwrap_or(TokenId::MISSING);
        let fill = col(t.bed(id("component.panel.fill")));
        let line = col(t.color(id("component.panel.border")));
        let mode = id("panel.corner_mode");
        let corner = Corner::sized(cut_of(t, mode, vocabulary(mode)), t.px(id("panel.corner")), r);
        let width = t.px(id("panel.border")).max(0.0);
        let c = [corner; 4];
        let seg = corner_segments(t, &SEG, corner.size);
        let rank = t.px(id("elev.panel.glass.rank")).clamp(0.0, 3.0);
        if rank > 0.0 {
            dl.glass_fill(r, &c, seg, rank, col(t.color(id("elev.panel.glass.tint"))));
            let wash = col(t.color(id("elev.panel.glass.wash")));
            if wash.a > 0.0 {
                dl.ring_fill(r, &c, seg, wash);
            }
        } else {
            dl.ring_fill(r, &c, seg, fill);
        }
        dl.ring(r, &c, seg, width, line);
        panel_edge_glow(dl, t, r, &c, seg, line, now);
    }

    /// The no-move proof, in the words `menu.rs` and `tooltip.rs` already
    /// use: a window frame is a surface of Elev 2, and joining the ladder
    /// had to leave the picture exactly where it was under the master.
    /// Compared against [`the_frames_private_copy`], command for command
    /// and vertex for vertex.
    ///
    /// Under the master ALONE, which is half the claim and the weaker
    /// half: the master leaves `elev.panel.glass.rank` at 0 and the base
    /// `glow.panel_edge.enabled` at false, so two of the four things this
    /// file used to do are not reached at all.
    /// [`joining_the_ladder_moved_no_pixel_with_the_glass_and_the_glow_lit`]
    /// is where they are.
    #[test]
    fn joining_the_ladder_moved_no_pixel() {
        let t = theme::resolved();
        let mut was = DrawList::recording();
        the_frames_private_copy(&mut was, t, box_(), AT_REST);
        let mut now = DrawList::recording();
        level().draw_in(&mut now, t, box_(), box_(), AT_REST);
        same_picture(&was, &now);
    }

    /// The same proof where the master cannot make it.
    ///
    /// Two of the frame's four departures from the rung are invisible
    /// under a theme that ships the glass off and the bloom unlit, and
    /// `[mood.alert]` — which the engine ships and a host may select at
    /// any moment — lights the bloom. So the picture is taken again over
    /// a theme that raises the rung's glass rank AND turns
    /// `glow.panel_edge` on, and the two lists still have to agree: the
    /// old ring-then-bloom pair and the rung's guarded one draw the same
    /// thing whenever the edge is there to be drawn.
    ///
    /// Both commands are asserted present first, because two pictures
    /// that agree by both being empty prove nothing.
    #[test]
    fn joining_the_ladder_moved_no_pixel_with_the_glass_and_the_glow_lit() {
        let t = theme::bake_over_master(
            "[elev.panel]\n\
             glass.rank = 2\n\
             glass.wash = #40FFC0 / 0.5\n\
             [glow]\n\
             panel_edge.enabled = true\n\
             panel_edge.radius = 2.0u\n\
             panel_edge.alpha = 0.6\n",
        );
        let mut was = DrawList::recording();
        the_frames_private_copy(&mut was, &t, box_(), AT_REST);
        let has = |dl: &DrawList, what: fn(&DrawCmd) -> bool| dl.cmds().iter().any(what);
        assert!(
            has(&was, |c| matches!(c, DrawCmd::GlassFill { .. })),
            "the raised rank drew no glass, so this proves nothing: {:?}",
            was.cmds()
        );
        assert!(
            has(&was, |c| matches!(c, DrawCmd::GlowRing { .. })),
            "the lit bloom drew nothing, so this proves nothing: {:?}",
            was.cmds()
        );
        let mut now = DrawList::recording();
        level().draw_in(&mut now, &t, box_(), box_(), AT_REST);
        same_picture(&was, &now);
    }

    /// Z16 on the surface that shows it most: a window's ring is the one
    /// a user looks at all day, and until 2026-08-17 it was drawn by this
    /// file's own `dl.ring` call, which has one colour. A theme writing
    /// `edge.mode = gradient` beside a second colour got a flat border
    /// and no word about it — the complaint the audit records against the
    /// cockpit theme, which shipped exactly that pair.
    ///
    /// The overlay restates the two `enum:` lists because a
    /// re-declaration in the same stage replaces the token whole
    /// (`cascade.rs`'s `declare`) and an enum's baked value is an INDEX
    /// into the list it was declared with.
    #[test]
    fn a_gradient_edge_reaches_the_window_frame() {
        let t = theme::bake_over_master(
            "[elev.panel]\n\
             edge.mode = gradient    # · enum: solid | gradient ·\n\
             edge.color2 = #FF00FF / 1.0\n\
             edge.axis = y    # · enum: x | y | diag_down | diag_up ·\n",
        );
        let mut dl = DrawList::recording();
        level().draw_in(&mut dl, &t, box_(), box_(), AT_REST);
        let rings: Vec<_> = dl
            .cmds()
            .iter()
            .filter(|c| matches!(c, DrawCmd::Ring { .. } | DrawCmd::RingGrad { .. }))
            .cloned()
            .collect();
        assert_eq!(rings.len(), 1, "a window strokes its ring once: {rings:?}");
        match &rings[0] {
            DrawCmd::RingGrad { near, far, dir, .. } => {
                let want = t.color(theme::id("component.panel.border").unwrap());
                assert!((near.r - want.r).abs() < 1e-6, "near {near:?} is not the panel border");
                for (got, want) in [(far.r, 1.0), (far.g, 0.0), (far.b, 1.0)] {
                    assert!((got - want).abs() < 1e-6, "far {far:?} is not #FF00FF");
                }
                assert_eq!(*dir, [0.0, 1.0], "the theme said y, which is DOWN the screen");
            }
            other => panic!("the theme asked for a gradient window border and got {other}"),
        }
    }

    /// The frame's five keys are the window's OWN spellings and not the
    /// rung's: `component.panel.fill` is the seam both a window and a
    /// panel read (the master derives `[elev.panel] fill` from it), so a
    /// frame that started reading `elev.panel.fill` instead would sever
    /// the derivation the theme editor's background depends on. Read off
    /// the picture rather than off the source: an overlay moves the
    /// window's key, and the body has to follow it.
    #[test]
    fn the_window_keeps_reading_the_shared_seam_for_its_body() {
        let t = theme::bake_over_master("[component.panel]\nfill = #00FF00 / 1.0\n");
        let mut dl = DrawList::recording();
        level().draw_in(&mut dl, &t, box_(), box_(), AT_REST);
        let body = dl
            .cmds()
            .iter()
            .find_map(|c| match c {
                DrawCmd::RingFill { color, .. } => Some(*color),
                _ => None,
            })
            .expect("a window with an opaque body draws one");
        assert!(body.g > 0.99 && body.r < 0.01, "the body {body:?} is not component.panel.fill");
    }
}
