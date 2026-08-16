//! Venetian-blind drop-down list: an anchor, and N SEPARATE elements
//! that slide out from under it.
//!
//! THERE IS NO BOX. The list used to occupy a surface level of its own
//! — `[elev.popover]`, one bed, one ring, the rows kept inside it by
//! `menu.pad` — and the owner looked at that and asked for the opposite:
//! the frame around the WHOLE is gone, and what is left is the anchor
//! plus a column of elements each of which is a complete object. A frame
//! around a group says "these belong to one body"; the owner's picture
//! says "these are N things you may pick", and N things are N frames.
//!
//! AN ELEMENT IS THE ANCHOR. The anchor is a button, so an element is
//! drawn by [`super::button::dress`] — the very code that draws the
//! anchor — and therefore wears the anchor's plate (`shape.button.fill`),
//! the anchor's corner (`button.corner`, `button.corner_style`), the
//! anchor's ring, and the anchor's DICTIONARY (the `button` class ladder:
//! idle, hover, selected, selected_hover). Not "the same tokens read
//! again here": the same call. What this file still owns is the LABEL,
//! because a row's label is set in the role its list binds
//! (`list.label_role` → `body`) and a cap is set in `button.role` — the
//! dress is shared, the type ladder is not.
//!
//! ONE GAP. `menu.anchor_gap` stands between the anchor and the first
//! element AND between every pair of elements. One number, not two: the
//! owner asked for every row of the list to be spaced like the first,
//! and a list that took its inner gap from `[list].gap` and its outer
//! one from somewhere else could not answer that with a single token.
//! `[list].gap` and `[list].rule` are the furniture of a list drawn as
//! one body and this one is not drawn as one body, so it reads neither.
//!
//! THE BLIND. At `p = 0` every element is stowed UNDER the anchor — one
//! stack, out of sight. At `p = 1` element `i` stands at
//! `anchor.bottom() + gap + i·(item_h + gap)`. The distance element `i`
//! travels is therefore `item_h + gap + i·(item_h + gap)`, which grows
//! LINEARLY with `i`: the last element goes furthest, and while the
//! stack is still stowed it is the one on top of it — which it is,
//! because the elements are drawn in index order and the painter's
//! algorithm puts the last one over its neighbours. Pull the cord and
//! the slat that was on top of the pile ends up at the bottom of the
//! blind. The order of the NAMES never changes: `DEFAULT` is the first
//! element at `p = 0` and the first element at `p = 1`. The blind is how
//! they arrive, not what they say.
//!
//! FROM UNDER, NOT OVER. The application draws the anchor and this
//! library draws the list AFTERWARDS, so without a clip the elements
//! would slide across the anchor's face on their way down. The list
//! pushes a clip whose top edge is `anchor.bottom()` — everything above
//! the anchor's bottom edge belongs to the anchor — and the elements
//! appear out of it. The clip is the draw list's, and the draw list's
//! clip is a RECTANGLE (`cmd_set_scissor`), so this is exact along a
//! straight bottom edge and only approximate where the anchor's own
//! bottom corners are cut: an emerging element is a full-width sliver at
//! a height where the anchor itself is already narrowing into its
//! rounding, so for the first pixels of the unfold the element's top
//! corners stand slightly proud of the anchor's silhouette. Clipping to
//! the rounding would need a shaped clip, which this draw list does not
//! have. Stated here rather than papered over.

use super::button::ButtonState;
use super::focus_ring;
use crate::focus::{Caps, FocusId};
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

/// How a list is dressed for this frame — the two things about it that
/// are not its geometry or its contents.
///
/// A struct and not two more free functions: `focus` and `current` are
/// independent, so entry points would multiply as their product, and
/// the pair that draws a focusable list WITH a chosen row is exactly the
/// one the settings window needs. Written the way [`InputStyle`] is
/// written, for the same reason.
///
/// [`InputStyle`]: crate::object::text_input::InputStyle
#[derive(Clone, Copy, Debug, Default)]
pub struct AccordionStyle {
    /// The focus chain root, when the list joins the chain. Every
    /// element of a list AT REST registers as `base.item(i)` (an
    /// element's order is its content's order, so the index is legal),
    /// letting arrows walk the open list and Enter pick — the router
    /// compares the chain's focused id against the same derived ids.
    /// A blind still running registers nothing: its elements are moving,
    /// and a ring on a moving rect is the board-ride pitfall in
    /// miniature.
    pub focus: Option<FocusId>,
    /// The element that is ALREADY in force — the theme now applied, the
    /// layout now loaded — drawn on the button ladder's `selected` rung,
    /// which is the rung the anchor itself wears while its list is open.
    ///
    /// Not a fashion: with the anchor wearing the list's own name, a
    /// list that cannot mark its current element leaves the standing
    /// choice unstated everywhere in the window. `None` says the set has
    /// no member in force, which is not the same as "the first one".
    pub current: Option<usize>,
}

/// Draws the blind at unfold progress `p` (0..1, eased by the caller or
/// pass 1.0 for fully open). Returns the element rectangles in order —
/// AS DRAWN, which for an element still half under the anchor is the
/// half that is out. The caller hit-tests these, so an element is
/// clickable where it can be seen and nowhere else; the `bool` says
/// whether the whole of it is out.
///
/// [`AccordionStyle`] carries the rest: whether the elements join the
/// focus chain, and which of them is the one already in force. A list
/// that wants neither passes `&AccordionStyle::default()`.
pub fn accordion(
    ctx: &mut Ctx,
    anchor: Rect,
    item_h: f32,
    names: &[String],
    p: f32,
    style: &AccordionStyle,
) -> Vec<(Rect, bool)> {
    static GAP: OnceLock<TokenId> = OnceLock::new();
    static SKEW: OnceLock<TokenId> = OnceLock::new();
    static THRESHOLD: OnceLock<TokenId> = OnceLock::new();
    static ROLE: OnceLock<TokenId> = OnceLock::new();
    static ANCHOR_W: OnceLock<TokenId> = OnceLock::new();
    static ANCHOR_W_IDX: OnceLock<Option<u16>> = OnceLock::new();
    static MIN_W: OnceLock<TokenId> = OnceLock::new();
    let t = theme::resolved();
    let role = ui::bound_role(&ROLE, "list.label_role");
    // No `ui_font_scale`: the viewport carries the user's scale into u,
    // and the role's size is written in u — applying it here too squares it.
    let px = role.px(ctx, 1.0);
    // The role's own face, asked of the role. A slot named here would
    // pin every theme's list to the interface family whatever
    // `type.<role>.face` says, which is a design decision taken in Rust.
    let font = role.font();
    let leading = role.leading();
    let tracking = role.tracking_px(px);
    // …and the role's figure box with it, so a list of versions or
    // addresses steps its digits the way the boxed label beside it does.
    // Read ONCE, outside the loop — the box costs a theme read and ten
    // glyph lookups.
    let fig = role.figures(ctx.fonts, font, px);
    // The BOTTOM edge is what the list hangs from, and `button.skew` is
    // what shortens it: under a theme that shears its buttons the
    // anchor's underside is `skew` narrower than its box, so the
    // elements are too. The master leaves the token at zero — a button
    // now wears the same corners as the frames around it and
    // [`super::button::dress`] fills a rectangle, so the shear survives
    // only in [`super::button::quad`], which the focus ring is drawn on.
    // Reading it here keeps the two in step for the theme that brings
    // the parallelogram back.
    let skew = t.px(tok(&SKEW, "button.skew"));
    // Below this SHARE of an element's full height an element that is
    // still coming out draws no label. A fraction and not a length,
    // because a blind's element height is the one its anchor hands it
    // and not `@list.row_h`.
    let text_threshold = item_h * t.px(tok(&THRESHOLD, "list.unfold_text_threshold"));
    // `menu.anchor_width` says whether the anchor's edge is the whole
    // story: under `min_w` the elements still start at that edge, but
    // `menu.min_w` is a floor under their width, so a narrow anchor no
    // longer makes an unreadable list.
    let aw = tok(&ANCHOR_W, "menu.anchor_width");
    let floored = *ANCHOR_W_IDX.get_or_init(|| theme::enum_index(aw, "min_w")) == Some(t.enum_of(aw));
    let mut row_w = anchor.w - skew;
    if floored {
        row_w = row_w.max(t.px(tok(&MIN_W, "menu.min_w")));
    }
    // THE gap — the one below the anchor and the one between any two
    // elements are the same number, read once.
    let gap = t.px(tok(&GAP, "menu.anchor_gap")).max(0.0);
    let p = p.clamp(0.0, 1.0);
    let mut out = Vec::new();
    if names.is_empty() || p <= 0.0 || item_h <= 0.0 || row_w <= 0.0 {
        // A closed blind is not a stack of zero-height elements, it is
        // nothing drawn at all — and a list of nothing is nothing either.
        return out;
    }
    // Where the anchor ends and the world below it begins. Everything
    // above this line belongs to the anchor.
    let horizon = anchor.bottom();
    // Stowed: the whole stack tucked under the anchor, every element at
    // the same place, none of it showing.
    let stowed = horizon - item_h;
    let pitch = item_h + gap;
    ctx.dl.push_clip(0.0, horizon, ctx.w, (ctx.h - horizon).max(0.0));
    // A blind that has stopped moving is a list; a blind still moving is
    // an animation. Only the first joins the focus chain.
    let at_rest = p >= 1.0;
    let mut ring: Option<Rect> = None;
    for (i, name) in names.iter().enumerate() {
        // Element `i`'s travel: `item_h` to clear the anchor, the gap
        // below it, and one pitch for every element that stands above
        // it. Linear in `i`, so the last one goes furthest.
        let y = stowed + p * (item_h + gap + pitch * i as f32);
        let slat = Rect::new(anchor.x, y, row_w, item_h);
        // What of it is out from under the anchor. The scissor's own
        // arithmetic, repeated here because the rect handed back has to
        // BE the rect that was drawn: a caller aiming at where an
        // element will eventually be would be aiming at the anchor.
        let top = y.max(horizon);
        let seen = (y + item_h - top).max(0.0);
        let shown = Rect::new(slat.x, top, slat.w, seen);
        let full = seen >= item_h - 0.5;
        if at_rest && full {
            if let (Some(base), Some(fc)) = (style.focus, ctx.focus.as_deref_mut()) {
                if fc.register(base.item(i), shown, Caps::NONE).ring {
                    ring = Some(shown);
                }
            }
        }
        // The pointer is over what it can SEE, which is the same rect
        // the caller was handed — and only if nothing already drawn this
        // frame stands over it.
        // The claim comes FIRST, the question second — the order is the
        // fix, not a preference. `Pointer::begin` reveals the pointer only
        // once as many covers have been recorded as the depth at which it
        // was claimed LAST frame, and last frame this element's own cover
        // was what claimed it. Asking before covering left the count one
        // short, so every element of every unfolded list was occluded by
        // ITSELF and hover never fired anywhere in the toolkit. `cover`'s
        // own doc states the intended shape: claim the box, then draw the
        // controls into it.
        ctx.mouse.cover(shown);
        let hover = ctx.mouse.over(shown);
        // And this element is itself something to stand over. The box that
        // used to claim this ground is gone, so each element claims its
        // own: an element of an open list covers whatever the list was
        // opened on top of.

        // The anchor's own dress, drawn by the anchor's own code. The
        // element in force wears `selected` — the rung the anchor wears
        // while its list is open — and keeps it under the pointer as
        // `selected_hover`, so a hovered current element does not lose
        // its mark exactly while the user decides whether to replace it.
        let ink = super::button::dress(
            ctx,
            slat,
            ButtonState { hover, flash: false, selected: style.current == Some(i) },
        );
        if seen >= text_threshold {
            ctx.dl.text_center_fig(
                ctx.fonts,
                font,
                px,
                slat.cx(),
                slat.y + (item_h - px * leading) / 2.0,
                name,
                col(ink.text),
                tracking,
                &fig,
            );
        }
        out.push((shown, full));
    }
    ctx.dl.pop_clip();
    // The focus ring is drawn OUTSIDE the clip: it is an overlay around
    // an element at rest, it reaches past that element's own edges by
    // whatever `[focus]` states, and a ring cut off at the horizon would
    // report the first element as a different object from the rest.
    if let Some(r) = ring {
        focus_ring::draw(ctx, r);
    }
    out
}
