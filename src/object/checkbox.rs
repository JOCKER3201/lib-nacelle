//! Checkbox object: an outlined box with a filled square when checked,
//! plus a label. The whole row is the click target.

use super::focus_ring;
use crate::draw::{Corner, CornerStyle};
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

/// The box's four corners and their tessellation. `checkbox.corner`
/// carries the radius alone; [corner]'s header rules the cut of a radius
/// with no `*_corner_style` sibling to `round`, and the checkbox has
/// none — so the shape is the theme's, not this file's. Zero is spelled
/// Square because a zero-radius arc is a square corner drawn the cheap
/// way.
///
/// The length goes through [`Corner::sized`], which is where §5.0's
/// `pill` is translated: `pill` bakes to a NEGATIVE number, so a box that
/// clamped the token at zero would answer a theme writing `pill` with the
/// square it wrote to avoid.
fn shape(t: &theme::ResolvedTheme, bx: Rect) -> ([Corner; 4], u8) {
    static CORNER: OnceLock<TokenId> = OnceLock::new();
    static SEGMENTS: OnceLock<TokenId> = OnceLock::new();
    let c = Corner::sized(CornerStyle::Round, t.px(tok(&CORNER, "checkbox.corner")), bx);
    let c = if c.size > 0.0 { c } else { Corner::SQUARE };
    ([c; 4], super::window::corner_segments(t, &SEGMENTS, c.size))
}

/// Draws the checked mark inside `m`, the box already inset by
/// `checkbox.tick_inset`, in the shape `checkbox.tick_shape` names.
///
/// The two stroked marks take their line weight from
/// `checkbox.tick_stroke`, which the master sends after `checkbox.border`
/// — so the shipped mark is drawn in the same hand as the box around it,
/// and a theme wanting a thin tick inside a heavy ring can now say so.
fn tick(ctx: &mut Ctx, m: Rect, color: Color) {
    static SHAPE: OnceLock<TokenId> = OnceLock::new();
    static IDX: OnceLock<(Option<u16>, Option<u16>)> = OnceLock::new();
    static STROKE: OnceLock<TokenId> = OnceLock::new();
    let t = theme::resolved();
    let id = tok(&SHAPE, "checkbox.tick_shape");
    let (check, cross) =
        *IDX.get_or_init(|| (theme::enum_index(id, "check"), theme::enum_index(id, "cross")));
    let cur = Some(t.enum_of(id));
    let w = t.px(tok(&STROKE, "checkbox.tick_stroke"));
    if cur == check {
        // The glyph's own proportions, as with menu.rs's chevron: where
        // the stroke turns is what makes a tick a tick.
        ctx.dl.polyline(
            &[
                [m.x, m.y + m.h * 0.55],
                [m.x + m.w * 0.38, m.bottom()],
                [m.right(), m.y],
            ],
            w,
            color,
            false,
        );
    } else if cur == cross {
        ctx.dl.polyline(&[[m.x, m.y], [m.right(), m.bottom()]], w, color, false);
        ctx.dl.polyline(&[[m.right(), m.y], [m.x, m.bottom()]], w, color, false);
    } else {
        // "square", plus anything the vocabulary does not name.
        ctx.dl.rect(m.x, m.y, m.w, m.h, color);
    }
}

/// Draws a checkbox row. The whole row is the hit target, which the
/// caller already has.
pub fn draw(ctx: &mut Ctx, row: Rect, label: &str, checked: bool, hover: bool) {
    static SIZE: OnceLock<TokenId> = OnceLock::new();
    static BORDER: OnceLock<TokenId> = OnceLock::new();
    static TICK: OnceLock<TokenId> = OnceLock::new();
    static TICK_INSET: OnceLock<TokenId> = OnceLock::new();
    static LABEL_GAP: OnceLock<TokenId> = OnceLock::new();
    static ROLE: OnceLock<TokenId> = OnceLock::new();
    static CLASS: OnceLock<Option<u16>> = OnceLock::new();
    let t = theme::resolved();
    // The box is its own length now, not a cut of the caller's row.
    let s = t.px(tok(&SIZE, "checkbox.size"));
    let bx = Rect::new(row.x, row.y + (row.h - s) / 2.0, s, s);
    let style = match *CLASS.get_or_init(|| theme::class_id("checkbox")) {
        Some(c) => t.class_state(c, if hover { State::Hover } else { State::Idle }),
        None => StateStyle::RAW,
    };
    let (corners, seg) = shape(t, bx);
    ctx.dl.ring(
        bx,
        &corners,
        seg,
        t.px(tok(&BORDER, "checkbox.border")),
        col(style.edge),
    );
    if checked {
        // checkbox.tick_inset bakes against checkbox.size, which `s` is.
        let m = t.px(tok(&TICK_INSET, "checkbox.tick_inset"));
        let mark = Rect::new(bx.x + m, bx.y + m, s - 2.0 * m, s - 2.0 * m);
        tick(ctx, mark, col(t.color(tok(&TICK, "component.checkbox.tick"))));
    }
    let role = ui::bound_role(&ROLE, "checkbox.role");
    // No `ui_font_scale`: the viewport carries the user's scale into u,
    // and the role's size is written in u — applying it here too squares it.
    let px = role.px(ctx, 1.0);
    let leading = role.leading();
    ctx.dl.text(
        ctx.fonts,
        FONT_UI,
        px,
        bx.right() + t.px(tok(&LABEL_GAP, "checkbox.label_gap")),
        row.y + (row.h - px * leading) / 2.0,
        label,
        col(style.text),
        role.tracking_px(px),
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
