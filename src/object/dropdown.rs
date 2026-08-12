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
//! The BOX those rows sit in is menu furniture and stays that way. The
//! master's `component.menu.fill` is by its own words "the opaque bed of
//! a drop-down or window menu", and `menu.anchor_width` / `menu.min_w`
//! decide how wide the popover opens. What changed here is the rows'
//! clothes, not the container's — and the context menu (`menu.rs`) keeps
//! `menu.item`, which is the point: two objects, two classes.

use super::focus_ring;
use crate::draw::Corner;
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
    static FILL: OnceLock<TokenId> = OnceLock::new();
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
    // Same token as button::quad, so the list stays flush with the
    // anchor's slanted edge: the rows take the anchor's own inset, and
    // a theme that shears its buttons shears the list under them.
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
    let mut row_w = anchor.w - skew;
    if floored {
        row_w = row_w.max(t.px(tok(&MIN_W, "menu.min_w")));
    }
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
    // The sweep covers the rows and the gaps between them, but not a
    // gap under the last row: a list is as tall as its content.
    let visible_h = p.clamp(0.0, 1.0) * (pitch * names.len() as f32 - gap).max(0.0);
    let mut out = Vec::new();
    let mut ring: Option<Rect> = None;
    for (i, name) in names.iter().enumerate() {
        let top = pitch * i as f32;
        if top >= visible_h {
            break;
        }
        // The edge closest to the anchor coincides with the anchor's
        // bottom edge (shorter by the skew of the parallelogram).
        let h = (visible_h - top).min(item_h);
        let r = Rect::new(anchor.x, anchor.bottom() + top, row_w, h);
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
        // Opaque menu material first — the bed the whole popover stands
        // on, unbroken from row to row.
        ctx.dl
            .rect(r.x, r.y, r.w, r.h, col(t.color(tok(&FILL, "component.menu.fill"))));
        // Then the mark: a PLATE under the row, cut to the shape
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
            if full {
                ctx.dl.ring_fill(r, &corners, seg, col(ink.fill));
            } else {
                let c = Corner::sized(cut, radius, r);
                let s = super::window::corner_segments(t, &SEGMENTS, c.size);
                ctx.dl.ring_fill(r, &[c; 4], s, col(ink.fill));
            }
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
