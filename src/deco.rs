//! The decoration engine (u3 L6): what a frame paints that is not a
//! widget — the clear under everything, the fixtures' frosted glass,
//! the board ride's clock and easing. WHERE things sit is the layout
//! engine's; WHAT the stage furniture looks like is decided here, and
//! every value is a theme token, per the governing principle. The
//! backdrop PLATE (traces, grid, vignette) is `theme::plate` — baked
//! pixels, not per-frame geometry.
//!
//! A board standing still paints NO ground of its own: the clear and
//! the plate already fill the screen behind it. A board turning
//! SIDEWAYS is a different thing — a WALL of a solid — and takes its
//! ground with it ([`board_ground`]) over the flat [`ride_void`] the
//! whole turn happens in; without that the walls are panes of glass
//! with the frame's own clear showing through them.

use crate::draw::{DrawList, ImageId};
use crate::theme::{self, Color, TokenId};
use std::sync::OnceLock;

fn tok(cell: &'static OnceLock<TokenId>, name: &'static str) -> TokenId {
    *cell.get_or_init(|| theme::id(name).unwrap_or(TokenId::MISSING))
}

/// A baked theme colour in the draw list's own colour type.
fn col(c: theme::ThemeColor) -> Color {
    Color { r: c.r, g: c.g, b: c.b, a: c.a }
}

/// The colour every frame clears to: `surface.void`, read as a BED so
/// a raw master clears near-black rather than mid-grey.
pub fn clear_color() -> Color {
    static VOID: OnceLock<TokenId> = OnceLock::new();
    col(theme::resolved().bed(tok(&VOID, "surface.void")))
}

/// The ground one board stands on, screen-sized, in the theme's own
/// order: `backdrop.solid` — what lies behind the board — then the
/// board's field `elev.board.fill`, then the baked backdrop plate, the
/// decoration whose traces, grid and stars live on that field (5.5).
/// Emitted by a board riding SIDEWAYS, before its panels, so the
/// caller's yaw and perspective carry ground and panels together and
/// the face turns as one solid wall. Two levels rather than one because
/// a family-B board paints NOTHING of its own (`elev.board.fill` at
/// alpha 0) and a wall of nothing is a pane of glass, not a wall: what
/// that theme puts behind its panes is the backdrop, and the backdrop
/// is what the wall carries. `plate` is the host's baked backdrop
/// texture, or `None` when the theme bakes no decoration at all.
pub fn board_ground(dl: &mut DrawList, w: f32, h: f32, plate: Option<ImageId>) {
    static SOLID: OnceLock<TokenId> = OnceLock::new();
    static FILL: OnceLock<TokenId> = OnceLock::new();
    let t = theme::resolved();
    for id in [tok(&SOLID, "backdrop.solid"), tok(&FILL, "elev.board.fill")] {
        let c = col(t.bed(id));
        if c.a > 0.0 {
            dl.rect(0.0, 0.0, w, h, c);
        }
    }
    if let Some(id) = plate {
        // White at 1.0 is the multiplicative identity: the plate's
        // pixels ARE the theme's baked colours.
        dl.image(0.0, 0.0, w, h, id, Color { r: 1.0, g: 1.0, b: 1.0, a: 1.0 });
    }
}

/// The flat colour the sideways ride happens in: painted once under the
/// whole cube, and the colour a wall settles toward as it turns away
/// from the viewer, so a wall edge-on melts into the space behind it
/// instead of into grey. Read as a BED — a raw master rides through
/// near-black rather than mid-grey.
pub fn ride_void() -> Color {
    static VOID: OnceLock<TokenId> = OnceLock::new();
    col(theme::resolved().bed(tok(&VOID, "motion.board_ride.void")))
}

/// A fixture's face: frosted glass over whatever sits beneath, plus
/// the theme's own wash. `wash_scale` is the USER's opacity setting —
/// a multiplier on the wash's alpha, nothing else (the BlurOpacity
/// slider's contract). The glass is sampled by screen position, so a
/// ride may carry the quad and the frost stays put.
pub fn fixture_glass(dl: &mut DrawList, w: f32, h: f32, wash_scale: f32) {
    static TINT: OnceLock<TokenId> = OnceLock::new();
    static WASH: OnceLock<TokenId> = OnceLock::new();
    let t = theme::resolved();
    dl.blur(0.0, 0.0, w, h, col(t.color(tok(&TINT, "elev.fixture.glass.tint"))));
    let wash = t.color(tok(&WASH, "elev.fixture.glass.wash"));
    let a = wash.a * wash_scale;
    if a > 0.0 {
        dl.rect(0.0, 0.0, w, h, col(wash).alpha(a));
    }
}

/// The board ride's clock: seconds for the full move, after the
/// theme's global motion scale. Zero — disabled, scale 0, or no
/// token — is a hard cut, which is exactly what reduced motion asks
/// for.
pub fn ride_secs() -> f32 {
    static ENABLED: OnceLock<TokenId> = OnceLock::new();
    static DUR: OnceLock<TokenId> = OnceLock::new();
    static SCALE: OnceLock<TokenId> = OnceLock::new();
    let t = theme::resolved();
    if !t.flag(tok(&ENABLED, "motion.board_ride.enabled")) {
        return 0.0;
    }
    t.px(tok(&DUR, "motion.board_ride.duration_ms")) * t.px(tok(&SCALE, "motion.scale"))
        / 1000.0
}

/// The ride's easing, picked by the motion token's word. `custom`'s
/// cubic-bezier control points wait on a shared motion resolver; until
/// it exists an unrecognised word runs linear, the enum's own
/// fallback.
pub fn ride_ease(t01: f32) -> f32 {
    static EASING: OnceLock<TokenId> = OnceLock::new();
    static DUTY: OnceLock<TokenId> = OnceLock::new();
    static FLOOR: OnceLock<TokenId> = OnceLock::new();
    static WORDS: OnceLock<[Option<u16>; 5]> = OnceLock::new();
    let t = theme::resolved();
    let id = tok(&EASING, "motion.board_ride.easing");
    let w = WORDS.get_or_init(|| {
        ["ease_out", "ease_in", "ease_in_out", "sine", "step"]
            .map(|word| theme::enum_index(id, word))
    });
    let e = Some(t.enum_of(id));
    if e == w[0] {
        1.0 - (1.0 - t01) * (1.0 - t01)
    } else if e == w[1] {
        t01 * t01
    } else if e == w[2] {
        t01 * t01 * (3.0 - 2.0 * t01)
    } else if e == w[3] {
        0.5 - 0.5 * (std::f32::consts::PI * t01).cos()
    } else if e == w[4] {
        if t01 >= t.px(tok(&DUTY, "motion.board_ride.duty")) {
            1.0
        } else {
            t.px(tok(&FLOOR, "motion.board_ride.floor"))
        }
    } else {
        t01
    }
}
