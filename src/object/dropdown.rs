//! Accordion drop-down list: unfolds from the bottom edge of its
//! anchor (a parallelogram button), exactly as wide as that edge.
//! Items are opaque. The caller hit-tests the returned rectangles.

use crate::font::FONT_UI;
use crate::theme::{self, bake::StateStyle, parse::State, Color, TokenId};
use crate::{Ctx, Rect};
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
    static FILL: OnceLock<TokenId> = OnceLock::new();
    static BORDER: OnceLock<TokenId> = OnceLock::new();
    static SKEW: OnceLock<TokenId> = OnceLock::new();
    static THRESHOLD: OnceLock<TokenId> = OnceLock::new();
    static TSIZE: OnceLock<TokenId> = OnceLock::new();
    static TMIN: OnceLock<TokenId> = OnceLock::new();
    static TRACKING: OnceLock<TokenId> = OnceLock::new();
    static LEADING: OnceLock<TokenId> = OnceLock::new();
    static CLASS: OnceLock<Option<u16>> = OnceLock::new();
    let t = theme::resolved();
    let class = *CLASS.get_or_init(|| theme::class_id("menu.item"));
    let px = (t.px(tok(&TSIZE, "type.body.size")) * ctx.ui_font_scale * ctx.panel_scale)
        .max(t.px(tok(&TMIN, "type.body.min_px")));
    let leading = t.px(tok(&LEADING, "type.body.leading"));
    let tracking = px * t.px(tok(&TRACKING, "type.body.tracking"));
    let border = t.px(tok(&BORDER, "menu.border"));
    // Same token as button::quad, so the list stays flush with the
    // anchor's slanted edge.
    let skew = t.px(tok(&SKEW, "button.skew"));
    // Below this height an unfolding row draws no text.
    let text_threshold = t.px(tok(&THRESHOLD, "menu.item_text_threshold"));
    let visible_h = p.clamp(0.0, 1.0) * item_h * names.len() as f32;
    let mut out = Vec::new();
    for (i, name) in names.iter().enumerate() {
        let top = item_h * i as f32;
        if top >= visible_h {
            break;
        }
        // The edge closest to the anchor coincides with the anchor's
        // bottom edge (shorter by the skew of the parallelogram).
        let h = (visible_h - top).min(item_h);
        let r = Rect::new(anchor.x, anchor.bottom() + top, anchor.w - skew, h);
        // Floating-point tolerance: the LAST item's height comes from a
        // subtraction and can be epsilon short of item_h.
        let full = h >= item_h - 0.5;
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
    out
}
