//! Slider object: a horizontal track with a filled part and a knob.
//! The caller draws its own label/value text and hit-tests the
//! returned track rectangle.

use super::focus_ring;
use crate::focus::{Caps, FocusId};
use crate::theme::{self, Color, TokenId};
use crate::{Ctx, Rect};
use std::sync::OnceLock;

fn tok(cell: &'static OnceLock<TokenId>, name: &'static str) -> TokenId {
    *cell.get_or_init(|| theme::id(name).unwrap_or(TokenId::MISSING))
}

/// The engine's colour, in the draw list's clothes.
fn col(c: theme::ThemeColor) -> Color {
    Color { r: c.r, g: c.g, b: c.b, a: c.a }
}

/// Draws the track with the knob at position `t` (0..1).
pub fn track(ctx: &mut Ctx, track: Rect, t: f32) {
    static TRACK_COLOR: OnceLock<TokenId> = OnceLock::new();
    static FILL_COLOR: OnceLock<TokenId> = OnceLock::new();
    static KNOB_COLOR: OnceLock<TokenId> = OnceLock::new();
    static TRACK_H: OnceLock<TokenId> = OnceLock::new();
    static FILL_H: OnceLock<TokenId> = OnceLock::new();
    static KNOB_W: OnceLock<TokenId> = OnceLock::new();
    static KNOB_H: OnceLock<TokenId> = OnceLock::new();
    let th = theme::resolved();
    let cy = track.y + track.h / 2.0;
    let track_h = th.px(tok(&TRACK_H, "slider.track_h"));
    ctx.dl.line(
        track.x,
        cy,
        track.right(),
        cy,
        track_h,
        col(th.color(tok(&TRACK_COLOR, "slider.track_color"))),
    );
    let t = t.clamp(0.0, 1.0);
    let knob_x = track.x + t * track.w;
    // same_as_parent bakes to a negative sentinel: the fill inherits the
    // track's thickness.
    let mut fill_h = th.px(tok(&FILL_H, "slider.fill_h"));
    if fill_h < 0.0 {
        fill_h = track_h;
    }
    ctx.dl.line(
        track.x,
        cy,
        knob_x,
        cy,
        fill_h,
        col(th.color(tok(&FILL_COLOR, "slider.fill_color"))),
    );
    // The knob is its own length now, not a cut of the row height.
    let kw = th.px(tok(&KNOB_W, "slider.knob_w"));
    let kh = th.px(tok(&KNOB_H, "slider.knob_h"));
    ctx.dl.rect(
        knob_x - kw / 2.0,
        cy - kh / 2.0,
        kw,
        kh,
        col(th.color(tok(&KNOB_COLOR, "slider.knob_color"))),
    );
}

/// [`track`], joined to the world's focus chain. A slider EATS the
/// arrows (`GREEDY_ARROWS`): while it owns focus, Left/Right adjust the
/// value instead of navigating — the router dispatches them to the
/// caller's value logic. Tab still leaves. The ring wraps the track
/// rect the caller already hit-tests.
pub fn track_focusable(ctx: &mut Ctx, r: Rect, t: f32, id: FocusId) {
    let f = ctx.focus.as_deref_mut().map(|fc| fc.register(id, r, Caps::GREEDY_ARROWS));
    track(ctx, r, t);
    if f.map_or(false, |f| f.ring) {
        focus_ring::draw(ctx, r);
    }
}
