//! One SURFACE LEVEL, drawn in one place.
//!
//! `[elev.backdrop]` … `[elev.fixture]` is the master's ladder of
//! surfaces (§5.12). Every rung states the same dictionary — a body
//! (`fill`), a shape (`corner`, `radius`), a ring (`edge.color`,
//! `edge.width`), and beyond them the glass pair, the two glows, the
//! drop shadow and the reflection. An object that assembles its rung out
//! of primitives at its own call site therefore owns a PRIVATE COPY of
//! those rules, and the copies drift: `panel.rs` reads the fill as a bed
//! and guards it on alpha, `window.rs` does not; whichever of them the
//! next level's author reads is the one that level will resemble.
//!
//! [`Level`] is the one reader. A consumer names a rung once — `"elev.
//! popover"` — and gets the whole dictionary, so when the glass ranks
//! and the shadow 9-slice land they land for every rung at once instead
//! of for whichever object was being edited that week.
//!
//! What is NOT here is any decision: no fallback colour, no minimum
//! radius, no "if the theme says nothing draw a hairline". A rung whose
//! `fill` is `none` and whose `edge.width` is `0` draws nothing, which
//! is the raw look the governing principle asks for.

use crate::draw::Corner;
use crate::theme::{self, Color, TokenId};
use crate::{Ctx, Rect};
use std::sync::OnceLock;

/// The engine's colour, in the draw list's clothes.
fn col(c: theme::ThemeColor) -> Color {
    Color { r: c.r, g: c.g, b: c.b, a: c.a }
}

/// The keys of one `[elev.*]` rung, resolved to ids once.
///
/// Ids and enum vocabularies are both stable for the life of the process
/// — [`theme::id`] says so of the first, and the second is interned out
/// of the master, which no user theme replaces — so a `Level` is built
/// inside a `OnceLock` at the call site exactly like the bare `TokenId`
/// statics everywhere else. What is NOT cached is any resolved VALUE:
/// the colour, the radius and the corner word are read from the live
/// [`theme::ResolvedTheme`] on every draw, so a theme swap moves them.
pub(crate) struct Level {
    fill: TokenId,
    corner: TokenId,
    radius: TokenId,
    edge_color: TokenId,
    edge_width: TokenId,
    glass_rank: TokenId,
    glass_tint: TokenId,
    glass_wash: TokenId,
    /// `corner`'s `(round, chamfer)` indices — see
    /// [`super::window::vocabulary`].
    words: (Option<u16>, Option<u16>),
}

impl Level {
    /// The rung named by `prefix`, e.g. `"elev.popover"`.
    ///
    /// A name and not a `TokenId` because a rung is a DICTIONARY: the
    /// caller states which surface it is, once, and the five keys under
    /// it are this module's business. A key the master does not declare
    /// degrades through [`TokenId::MISSING`], which is the engine's raw
    /// look and not a design.
    pub(crate) fn of(prefix: &str) -> Level {
        let id = |key: &str| theme::id(&format!("{prefix}.{key}")).unwrap_or(TokenId::MISSING);
        let corner = id("corner");
        Level {
            fill: id("fill"),
            corner,
            radius: id("radius"),
            edge_color: id("edge.color"),
            edge_width: id("edge.width"),
            glass_rank: id("glass.rank"),
            glass_tint: id("glass.tint"),
            glass_wash: id("glass.wash"),
            words: super::window::vocabulary(corner),
        }
    }

    /// The cut this rung makes on the box `r`, and the tessellation of
    /// its arcs.
    ///
    /// Through [`Corner::sized`] and not a clamp: §5.0's `pill` is a
    /// word about a box ("as round as this one can be") and bakes to a
    /// negative sentinel, so a floor at zero answers a master writing
    /// `pill` with the square it wrote to avoid.
    pub(crate) fn cut(&self, t: &theme::ResolvedTheme, r: Rect) -> ([Corner; 4], u8) {
        static SEGMENTS: OnceLock<TokenId> = OnceLock::new();
        let style = super::window::cut_of(t, self.corner, self.words);
        let c = Corner::sized(style, t.px(self.radius), r);
        ([c; 4], super::window::corner_segments(t, &SEGMENTS, c.size))
    }

    /// Material, ring, and family A's bloom over the ring.
    ///
    /// Answers the shape it drew, because a caller that has to fit
    /// content INSIDE the rung — a drop-down's rows, a panel's content
    /// box — would otherwise settle the same cut a second time and be
    /// free to settle it differently.
    pub(crate) fn draw(&self, ctx: &mut Ctx, r: Rect) -> ([Corner; 4], u8) {
        let t = theme::resolved();
        let (c, seg) = self.cut(t, r);
        // Glass INSTEAD of the fill, never on top of it — the master's own
        // contract at the `fill` key ("used INSTEAD of the glass pair while
        // rank = 0") and at the ladder's head: glass is TWO quads, the tint
        // that multiplies the blurred scene (it can only darken) and the
        // wash that lays over with alpha (the only knob that brightens).
        // This is the rank's FIRST reader: until 2026-08-16 the token was
        // declared on every rung and read by nobody, so a theme asking for
        // glass on a panel got a flat fill and no word about it.
        let rank = t.px(self.glass_rank).clamp(0.0, 3.0);
        if rank > 0.0 {
            ctx.dl.glass_fill(r, &c, seg, rank, col(t.color(self.glass_tint)));
            let wash = col(t.color(self.glass_wash));
            if wash.a > 0.0 {
                ctx.dl.ring_fill(r, &c, seg, wash);
            }
        } else {
            let fill = col(t.bed(self.fill));
            if fill.a > 0.0 {
                ctx.dl.ring_fill(r, &c, seg, fill);
            }
        }
        let edge = col(t.color(self.edge_color));
        let width = t.px(self.edge_width).max(0.0);
        if edge.a > 0.0 && width > 0.0 {
            ctx.dl.ring(r, &c, seg, width, edge);
            super::window::panel_edge_glow(ctx.dl, t, r, &c, seg, edge, ctx.t);
        }
        (c, seg)
    }
}
