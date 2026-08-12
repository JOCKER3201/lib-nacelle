//! Accordion drop-down list: unfolds from the bottom edge of its
//! anchor (a parallelogram button), exactly as wide as that edge.
//! Items are opaque. The caller hit-tests the returned rectangles.

use super::focus_ring;
use crate::focus::{Caps, FocusId};
use crate::font::FONT_UI;
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

/// Draws the list with unfold progress `p` (0..1, eased by the caller
/// or pass 1.0 for fully open). Returns the drawn item rectangles in
/// order — partially unfolded items are included but marked not-full.
pub fn accordion(
    ctx: &mut Ctx,
    anchor: Rect,
    item_h: f32,
    names: &[String],
    p: f32,
) -> Vec<(Rect, bool)> {
    accordion_impl(ctx, anchor, item_h, names, p, None)
}

/// [`accordion`], joined to the world's focus chain: every FULLY
/// unfolded row registers as `base.item(i)` (a row's order is its
/// content's order, so the index is legal), letting arrows walk the
/// open list and Enter pick — the router compares the chain's focused
/// id against the same derived ids. Mid-unfold rows never register:
/// their rects are still moving, and a ring on a moving rect is the
/// board-ride pitfall in miniature.
pub fn accordion_focusable(
    ctx: &mut Ctx,
    anchor: Rect,
    item_h: f32,
    names: &[String],
    p: f32,
    base: FocusId,
) -> Vec<(Rect, bool)> {
    accordion_impl(ctx, anchor, item_h, names, p, Some(base))
}

fn accordion_impl(
    ctx: &mut Ctx,
    anchor: Rect,
    item_h: f32,
    names: &[String],
    p: f32,
    base: Option<FocusId>,
) -> Vec<(Rect, bool)> {
    static FILL: OnceLock<TokenId> = OnceLock::new();
    static BORDER: OnceLock<TokenId> = OnceLock::new();
    static SKEW: OnceLock<TokenId> = OnceLock::new();
    static THRESHOLD: OnceLock<TokenId> = OnceLock::new();
    static ROLE: OnceLock<TokenId> = OnceLock::new();
    static ANCHOR_W: OnceLock<TokenId> = OnceLock::new();
    static ANCHOR_W_IDX: OnceLock<Option<u16>> = OnceLock::new();
    static MIN_W: OnceLock<TokenId> = OnceLock::new();
    static CLASS: OnceLock<Option<u16>> = OnceLock::new();
    let t = theme::resolved();
    let class = *CLASS.get_or_init(|| theme::class_id("menu.item"));
    // The same object as menu.rs's rows, down to the class — so the same
    // binding decides how they are set.
    let role = ui::bound_role(&ROLE, "menu.item.role");
    let px = role.px(ctx, ctx.ui_font_scale);
    let leading = role.leading();
    let tracking = role.tracking_px(px);
    let border = t.px(tok(&BORDER, "menu.border"));
    // Same token as button::quad, so the list stays flush with the
    // anchor's slanted edge.
    let skew = t.px(tok(&SKEW, "button.skew"));
    // Below this height an unfolding row draws no text.
    let text_threshold = t.px(tok(&THRESHOLD, "menu.item_text_threshold"));
    // `menu.anchor_width` says whether the anchor's edge is the whole
    // story: under `min_w` the list still starts at that edge, but
    // `menu.min_w` is a floor under it, so a narrow anchor no longer
    // makes an unreadable list.
    let aw = tok(&ANCHOR_W, "menu.anchor_width");
    let floored = *ANCHOR_W_IDX.get_or_init(|| theme::enum_index(aw, "min_w")) == Some(t.enum_of(aw));
    let mut row_w = anchor.w - skew;
    if floored {
        row_w = row_w.max(t.px(tok(&MIN_W, "menu.min_w")));
    }
    let visible_h = p.clamp(0.0, 1.0) * item_h * names.len() as f32;
    let mut out = Vec::new();
    let mut ring: Option<Rect> = None;
    for (i, name) in names.iter().enumerate() {
        let top = item_h * i as f32;
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
            if let (Some(base), Some(fc)) = (base, ctx.focus.as_deref_mut()) {
                if fc.register(base.item(i), r, Caps::NONE).ring {
                    ring = Some(r);
                }
            }
        }
        let hover = full && r.contains(ctx.mouse.0, ctx.mouse.1);
        let style = match class {
            Some(c) => t.class_state(c, if hover { State::Hover } else { State::Idle }),
            None => StateStyle::RAW,
        };
        // Opaque menu material first, the ladder's state wash on top.
        ctx.dl
            .rect(r.x, r.y, r.w, r.h, col(t.color(tok(&FILL, "component.menu.fill"))));
        ctx.dl.rect(r.x, r.y, r.w, r.h, col(style.fill));
        ctx.dl
            .rect_outline(r.x, r.y, r.w, r.h, border, col(style.edge));
        if h >= text_threshold {
            ctx.dl.text_center(
                ctx.fonts,
                FONT_UI,
                px,
                r.cx(),
                r.y + (h - px * leading) / 2.0,
                name,
                col(style.text),
                tracking,
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
