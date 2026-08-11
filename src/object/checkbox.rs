//! Checkbox object: an outlined box with a filled square when checked,
//! plus a label. The whole row is the click target.

use super::focus_ring;
use crate::focus::{Caps, FocusId};
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

/// Draws a checkbox row. The whole row is the hit target, which the
/// caller already has.
pub fn draw(ctx: &mut Ctx, row: Rect, label: &str, checked: bool, hover: bool) {
    static SIZE: OnceLock<TokenId> = OnceLock::new();
    static BORDER: OnceLock<TokenId> = OnceLock::new();
    static TICK: OnceLock<TokenId> = OnceLock::new();
    static TICK_INSET: OnceLock<TokenId> = OnceLock::new();
    static LABEL_GAP: OnceLock<TokenId> = OnceLock::new();
    static TSIZE: OnceLock<TokenId> = OnceLock::new();
    static TMIN: OnceLock<TokenId> = OnceLock::new();
    static TRACKING: OnceLock<TokenId> = OnceLock::new();
    static LEADING: OnceLock<TokenId> = OnceLock::new();
    static CLASS: OnceLock<Option<u16>> = OnceLock::new();
    let t = theme::resolved();
    // The box is its own length now, not a cut of the caller's row.
    let s = t.px(tok(&SIZE, "checkbox.size"));
    let bx = Rect::new(row.x, row.y + (row.h - s) / 2.0, s, s);
    let style = match *CLASS.get_or_init(|| theme::class_id("checkbox")) {
        Some(c) => t.class_state(c, if hover { State::Hover } else { State::Idle }),
        None => StateStyle::RAW,
    };
    ctx.dl.rect_outline(
        bx.x,
        bx.y,
        bx.w,
        bx.h,
        t.px(tok(&BORDER, "checkbox.border")),
        col(style.edge),
    );
    if checked {
        // checkbox.tick_inset bakes against checkbox.size, which `s` is.
        let m = t.px(tok(&TICK_INSET, "checkbox.tick_inset"));
        ctx.dl.rect(
            bx.x + m,
            bx.y + m,
            s - 2.0 * m,
            s - 2.0 * m,
            col(t.color(tok(&TICK, "component.checkbox.tick"))),
        );
    }
    let px = (t.px(tok(&TSIZE, "type.body.size")) * ctx.ui_font_scale * ctx.panel_scale)
        .max(t.px(tok(&TMIN, "type.body.min_px")));
    let leading = t.px(tok(&LEADING, "type.body.leading"));
    ctx.dl.text(
        ctx.fonts,
        FONT_UI,
        px,
        bx.right() + t.px(tok(&LABEL_GAP, "checkbox.label_gap")),
        row.y + (row.h - px * leading) / 2.0,
        label,
        col(style.text),
        px * t.px(tok(&TRACKING, "type.body.tracking")),
    );
}

/// [`draw`], joined to the world's focus chain: the whole row registers
/// — it is already the click target, and the ring wraps the same rect
/// the pointer hits. A checkbox eats no keys (toggling is the router's
/// Space/Enter), and focus never feeds `hover` — the ring is the only
/// focus signal.
pub fn draw_focusable(
    ctx: &mut Ctx,
    row: Rect,
    label: &str,
    checked: bool,
    hover: bool,
    id: FocusId,
) {
    let f = ctx.focus.as_deref_mut().map(|fc| fc.register(id, row, Caps::NONE));
    draw(ctx, row, label, checked, hover);
    if f.map_or(false, |f| f.ring) {
        focus_ring::draw(ctx, row);
    }
}
