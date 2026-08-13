//! Accordion drop-down list: unfolds from the bottom edge of its
//! anchor (a parallelogram button), exactly as wide as that edge.
//! Items are opaque. The caller hit-tests the returned rectangles.
//!
//! A row is an instance of the `list.item` class and is measured out of
//! the `[list]` dictionary — the same class and the same dictionary
//! [`crate::view::list`]'s rows wear, because a drop-down's rows and a
//! task list's rows are the same object seen twice. They used to be
//! dressed as `menu.item`, which put a full ring around EVERY row: nine
//! themes in an open list read as nine loose boxes with two hairlines
//! stacked between each neighbouring pair. What marks a row out now is a
//! PLATE under it, cut to `[list].corner_style` — the button's own
//! shape, so the anchor and the rows it opens answer in one shape
//! language — and drawn only on the rungs that actually mark a row:
//! a plate every row wears marks nothing.
//!
//! The open list is ONE OBJECT and it occupies a SURFACE LEVEL: Elev 5,
//! `[elev.popover]`, which the master glosses "menu, tooltip, dropdown,
//! context menu, drag ghost". It used to occupy none — every row painted
//! `component.menu.fill` under itself and nothing was drawn around the
//! whole, so an open list was a stack of rectangles rather than a box:
//! it had no ring at all, and its rows ran the full width of the anchor
//! while the anchor and the button above it both kept an inset. The one
//! ring it now draws comes from `elev.popover.edge.color`, which is
//! `@component.panel.border` — the SAME token `[elev.focused]` states,
//! so the list is framed like the window it opens in and not one new
//! token was needed to say so.
//!
//! The level is drawn through [`super::elev::Level`] and not out of
//! primitives, because a rung is a dictionary — glass, ring, both glows,
//! the drop shadow, the reflection — and an object that assembles its
//! own owns a private copy of every one of those rules.
//!
//! `menu.anchor_width` / `menu.min_w` still decide how wide the popover
//! opens and `menu.pad` is still the room inside it: the container is
//! menu furniture and stays that way. The context menu (`menu.rs`) keeps
//! `menu.item` for its rows, which is the point: two objects, two
//! classes.

use super::focus_ring;
use crate::draw::{Corner, CornerStyle};
use crate::focus::{Caps, FocusId};
use crate::theme::{self, bake::StateStyle, parse::State, Color, TokenId};
use crate::{ui, Ctx, Rect};
use std::sync::OnceLock;

fn tok(cell: &'static OnceLock<TokenId>, name: &'static str) -> TokenId {
    *cell.get_or_init(|| theme::id(name).unwrap_or(TokenId::MISSING))
}

/// The engine's colour, in the draw list's clothes.
fn col(c: theme::ThemeColor) -> Color {
    Color { r: c.r, g: c.g, b: c.b, a: c.a }
}

/// How far a cut reaches into the box along its corner's diagonal, in
/// the cut's own units.
///
/// The three styles are not comparable by `size`: a chamfer of `s` puts
/// its 45° face at `s/√2` from the corner point, a round corner of `s`
/// puts its arc at `s(√2 − 1)`, and a square corner removes nothing at
/// all. So a chamfer takes more material than a round of the same
/// length, and the number below is what says so — the only ordering
/// this file makes on shapes, and it is arithmetic rather than a
/// preference.
fn depth(c: Corner) -> f32 {
    let s = c.size.max(0.0);
    match c.style {
        CornerStyle::Square => 0.0,
        CornerStyle::Round => s * (std::f32::consts::SQRT_2 - 1.0),
        CornerStyle::Chamfer => s / std::f32::consts::SQRT_2,
    }
}

/// The cut a row wears where its corner sits ON the popover's inner
/// boundary: whichever of the two takes more material.
///
/// This is the clip, and it is geometric rather than a scissor because
/// the draw list's clip stack is a RECTANGLE (`cmd_set_scissor`) and a
/// rectangle cannot hold a round or a chamfered corner. Taking the
/// deeper of the two is exact whenever the two agree on style — which
/// is the shipped case, `@corner.mode` being the one root the whole
/// file cuts from — and never leaves the row outside the box otherwise.
///
/// A row whose own cut is already the deeper one keeps it, which is why
/// the master's list is untouched by this: the row's clothes are the
/// row's, and the box only overrules them where the row would cross it.
fn clipped(row: Corner, box_: Corner) -> Corner {
    if depth(box_) > depth(row) {
        box_
    } else {
        row
    }
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
    /// The focus chain root, when the list joins the chain. Every FULLY
    /// unfolded row registers as `base.item(i)` (a row's order is its
    /// content's order, so the index is legal), letting arrows walk the
    /// open list and Enter pick — the router compares the chain's
    /// focused id against the same derived ids. Mid-unfold rows never
    /// register: their rects are still moving, and a ring on a moving
    /// rect is the board-ride pitfall in miniature.
    pub focus: Option<FocusId>,
    /// The row that is ALREADY in force — the theme now applied, the
    /// layout now loaded — drawn on the ladder's `selected` rung.
    ///
    /// Not a fashion: with the anchor wearing the list's own name, a
    /// list that cannot mark its current row leaves the standing choice
    /// unstated everywhere in the window. `None` says the set has no
    /// member in force, which is not the same as "the first one".
    pub current: Option<usize>,
}

/// Draws the list with unfold progress `p` (0..1, eased by the caller
/// or pass 1.0 for fully open). Returns the drawn item rectangles in
/// order — partially unfolded items are included but marked not-full.
///
/// The popover BOX is drawn first and everything else inside it. It is
/// `p` of its finished self in height — box and contents unfold as one
/// object, so there is never a full-size frame standing around a list
/// that is half open. `menu.pad` is the room it keeps, on all four
/// sides, which is what puts the rows inside the ring instead of
/// running past it.
///
/// [`AccordionStyle`] carries the rest: whether the rows join the focus
/// chain, and which of them is the one already in force. A list that
/// wants neither passes `&AccordionStyle::default()`.
pub fn accordion(
    ctx: &mut Ctx,
    anchor: Rect,
    item_h: f32,
    names: &[String],
    p: f32,
    style: &AccordionStyle,
) -> Vec<(Rect, bool)> {
    static LEVEL: OnceLock<super::elev::Level> = OnceLock::new();
    static PAD: OnceLock<TokenId> = OnceLock::new();
    static SKEW: OnceLock<TokenId> = OnceLock::new();
    static THRESHOLD: OnceLock<TokenId> = OnceLock::new();
    static ROLE: OnceLock<TokenId> = OnceLock::new();
    static ANCHOR_W: OnceLock<TokenId> = OnceLock::new();
    static ANCHOR_W_IDX: OnceLock<Option<u16>> = OnceLock::new();
    static MIN_W: OnceLock<TokenId> = OnceLock::new();
    static CLASS: OnceLock<Option<u16>> = OnceLock::new();
    static GAP: OnceLock<TokenId> = OnceLock::new();
    static RULE: OnceLock<TokenId> = OnceLock::new();
    static RULE_EVERY: OnceLock<TokenId> = OnceLock::new();
    static RULE_COLOR: OnceLock<TokenId> = OnceLock::new();
    static CORNER: OnceLock<TokenId> = OnceLock::new();
    static CORNER_STYLE: OnceLock<TokenId> = OnceLock::new();
    static CORNER_IDX: OnceLock<(Option<u16>, Option<u16>)> = OnceLock::new();
    static SEGMENTS: OnceLock<TokenId> = OnceLock::new();
    let t = theme::resolved();
    // `list.item`, not `menu.item`: a drop-down row is a LIST row. The
    // two classes differ in the master's own ladder (5.27) — a list row
    // can be dragged and takes no focus ring, a menu row takes a ring
    // and cannot be dragged — so drawing both off one class was drawing
    // two objects in one outfit.
    let class = *CLASS.get_or_init(|| theme::class_id("list.item"));
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
    // …and the role's figure box with it. `type.<role>.tabular` reached
    // every other object of this batch and stopped at this one, because
    // `text_center` is `text_center_fig` with the box left out: a list
    // of versions or addresses set beside a boxed label elsewhere in the
    // same window stepped its digits differently. Read ONCE, outside the
    // row loop — the box costs a theme read and ten glyph lookups.
    let fig = role.figures(ctx.fonts, font, px);
    // Same token as button::quad, so the BOX stays flush with the
    // anchor's slanted edge, and a theme that shears its buttons shears
    // the popover under them. The rows take their inset from the box.
    let skew = t.px(tok(&SKEW, "button.skew"));
    // Below this SHARE of a row's full height an unfolding row draws no
    // text. A fraction and not a length, because an accordion's row
    // height is the one its anchor hands it and not `@list.row_h`.
    let text_threshold = item_h * t.px(tok(&THRESHOLD, "list.unfold_text_threshold"));
    // `menu.anchor_width` says whether the anchor's edge is the whole
    // story: under `min_w` the list still starts at that edge, but
    // `menu.min_w` is a floor under it, so a narrow anchor no longer
    // makes an unreadable list. Container furniture, so still `[menu]`.
    let aw = tok(&ANCHOR_W, "menu.anchor_width");
    let floored = *ANCHOR_W_IDX.get_or_init(|| theme::enum_index(aw, "min_w")) == Some(t.enum_of(aw));
    let mut box_w = anchor.w - skew;
    if floored {
        box_w = box_w.max(t.px(tok(&MIN_W, "menu.min_w")));
    }
    // The room the box keeps inside its ring — `[menu].pad`, "padding
    // inside the menu box", which is what the context menu already
    // insets its rows by. THIS is the inset the owner's report was
    // about: the anchor and the editor button above the list both kept
    // one and the rows kept none, so the rows ran wider than everything
    // they belonged under.
    let pad = t.px(tok(&PAD, "menu.pad")).max(0.0);
    let row_w = (box_w - 2.0 * pad).max(0.0);
    // `[list].gap` is what stands between two rows and `[list].rule` is
    // what is drawn there. The master says `@space.0` and `none`: rows
    // touch and nothing is drawn between them, which is the whole of
    // "one list, not a stack of boxes".
    let gap = t.px(tok(&GAP, "list.gap")).max(0.0);
    let rule_w = t.px(tok(&RULE, "list.rule")).max(0.0);
    let rule_every = t.px(tok(&RULE_EVERY, "list.rule_every")).max(0.0) as usize;
    // The plate's cut. Settled once for a row at its FULL height, which
    // is what every row of a finished list is; §5.0's `pill` is not a
    // radius until there is a box, and this is that box.
    let cut = super::window::corner_style(t, tok(&CORNER_STYLE, "list.corner_style"), &CORNER_IDX);
    let radius = t.px(tok(&CORNER, "list.corner"));
    let corner = Corner::sized(cut, radius, Rect::new(0.0, 0.0, row_w, item_h));
    let corners = [corner; 4];
    let seg = super::window::corner_segments(t, &SEGMENTS, corner.size);
    let pitch = item_h + gap;
    // The content the box holds when it is finished: the rows and the
    // gaps between them, but not a gap under the last row — a list is as
    // tall as its content.
    let content_h = (pitch * names.len() as f32 - gap).max(0.0);
    // …and the box that holds it, plus the room it keeps above and
    // below. The unfold scales THE WHOLE BOX: at `p` the box is `p` of
    // its finished height, so the ring grows with the list instead of
    // standing at full size around a list that is half out. Everything
    // vertical below is a consequence of this one line.
    let p = p.clamp(0.0, 1.0);
    let box_h = p * (content_h + 2.0 * pad);
    let mut out = Vec::new();
    if box_h <= 0.0 || names.is_empty() {
        // A closed list is not a box of zero height, it is no box — and a
        // list of nothing is not a frame around nothing. Before the box
        // existed the row loop simply never ran, so an empty list drew
        // nothing by accident; now it has to be said, because the box is
        // drawn before the rows are counted.
        return out;
    }
    // The surface level, drawn before anything stands on it, and its cut
    // handed back so the rows are fitted to the shape that was actually
    // drawn rather than to a second reading of the same tokens.
    let popover = Rect::new(anchor.x, anchor.bottom(), box_w, box_h);
    let (box_corners, _) =
        LEVEL.get_or_init(|| super::elev::Level::of("elev.popover")).draw(ctx, popover);
    // The boundary the rows are held inside: the box's own, moved in by
    // the pad. `Corner::inset` is what keeps a moved boundary parallel
    // to the one it came from — a round corner offsets to a concentric
    // arc, a chamfer's 45° face shrinks by `(2 − √2)·pad` — so the
    // inner shape is the outer shape and not an approximation of it.
    let inner: [Corner; 4] = [
        box_corners[0].inset(pad),
        box_corners[1].inset(pad),
        box_corners[2].inset(pad),
        box_corners[3].inset(pad),
    ];
    let inner_y = anchor.bottom() + pad;
    let visible_h = (box_h - 2.0 * pad).max(0.0);
    let mut ring: Option<Rect> = None;
    for (i, name) in names.iter().enumerate() {
        let top = pitch * i as f32;
        if top >= visible_h {
            break;
        }
        // Inside the box on every side: the pad from its left edge, the
        // pad from the edge closest to the anchor, and as wide as the
        // room between its two flanks.
        let h = (visible_h - top).min(item_h);
        let r = Rect::new(anchor.x + pad, inner_y + top, row_w, h);
        // Floating-point tolerance: the LAST item's height comes from a
        // subtraction and can be epsilon short of item_h.
        let full = h >= item_h - 0.5;
        if full {
            if let (Some(base), Some(fc)) = (style.focus, ctx.focus.as_deref_mut()) {
                if fc.register(base.item(i), r, Caps::NONE).ring {
                    ring = Some(r);
                }
            }
        }
        let hover = full && r.contains(ctx.mouse.0, ctx.mouse.1);
        // The one in force keeps saying so under the pointer, which is
        // what `selected_hover` is for: a hovered current row that fell
        // back to plain `hover` would lose the mark exactly while the
        // user is deciding whether to replace it.
        //
        // `None` is the resting row, and it is a case and not a rung:
        // at rest a row IS the list's bed, so nothing is laid over it.
        let mark = match (style.current == Some(i), hover) {
            (true, true) => Some(State::SelectedHover),
            (true, false) => Some(State::Selected),
            (false, true) => Some(State::Hover),
            (false, false) => None,
        };
        let ink = match class {
            Some(c) => t.class_state(c, mark.unwrap_or(State::Idle)),
            None => StateStyle::RAW,
        };
        // No bed under the row. The bed is the BOX's, drawn once above:
        // a rectangle per row was the stack of rectangles the owner
        // reported, and the reason the list could not have a ring — a
        // ring needs a whole, and there was none.
        //
        // The mark is a PLATE under the row, cut to the shape
        // `[list].corner_style` names, and only for a row the ladder
        // has something to say about. It replaces the ring this object
        // used to stroke around every row from `menu.item`'s
        // `edge_width` — a ring is a box, and nine boxes in a column
        // are not a list.
        if mark.is_some() && ink.fill.a > 0.0 {
            // A row still unfolding is SHORTER than the row the cut was
            // measured on, and a cut is a statement about the box it is
            // made on: `pill` on a 30 px row is a radius of 15, and 15
            // on the 13.8 px that row passes through mid-unfold is not a
            // capsule but a radius wider than the shape can hold. The
            // full-height cut settled above is reused for the rows that
            // ARE at full height, which is every row of a list at rest.
            let (mut c, s) = if full {
                (corners, seg)
            } else {
                let c = Corner::sized(cut, radius, r);
                ([c; 4], super::window::corner_segments(t, &SEGMENTS, c.size))
            };
            // …and then held inside the box, at whichever of its four
            // corners the row is actually sitting on one. The top row
            // shares the box's top two, the row the unfold has reached
            // shares its bottom two, and a row in the middle shares
            // none — so a list of one row is clipped at all four.
            if top <= 0.0 {
                c[0] = clipped(c[0], inner[0]);
                c[1] = clipped(c[1], inner[1]);
            }
            if r.bottom() >= inner_y + visible_h - 0.5 {
                c[2] = clipped(c[2], inner[2]);
                c[3] = clipped(c[3], inner[3]);
            }
            ctx.dl.ring_fill(r, &c, s, col(ink.fill));
        }
        if h >= text_threshold {
            ctx.dl.text_center_fig(
                ctx.fonts,
                font,
                px,
                r.cx(),
                r.y + (h - px * leading) / 2.0,
                name,
                col(ink.text),
                tracking,
                &fig,
            );
        }
        // `[list].rule` where the master draws none, on the same terms
        // `view::list` states it: a hairline under every `rule_every`th
        // row, in the seam the gap leaves. Off in the master twice over
        // — `rule = none` and `rule_every = 0` — so a theme has to ask
        // for it in words before a single line appears.
        if full && rule_w > 0.0 && rule_every > 0 && (i + 1) % rule_every == 0 {
            let ry = r.y + item_h + gap / 2.0;
            ctx.dl.line(
                r.x,
                ry,
                r.right(),
                ry,
                rule_w,
                col(t.color(tok(&RULE_COLOR, "component.script.rule"))),
            );
        }
        out.push((r, full));
    }
    // The ring is an overlay: drawn after every row, so the next row's
    // opaque bed cannot cover the band below the focused one.
    if let Some(r) = ring {
        focus_ring::draw(ctx, r);
    }
    out
}
