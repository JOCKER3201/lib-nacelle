//! Parallelogram button — the standard clickable object of the
//! interface (terminal-tab style: slanted sides, hover highlight,
//! flash on click, optional "selected" state).

use super::focus_ring;
use crate::focus::{Caps, FocusId};
use crate::font::FONT_UI;
use crate::draw::Corner;
use crate::theme::{self, bake::StateStyle, parse::State, Color, TokenId};
use crate::ui;
use crate::{Ctx, Rect};
use std::sync::OnceLock;

fn tok(cell: &'static OnceLock<TokenId>, name: &'static str) -> TokenId {
    *cell.get_or_init(|| theme::id(name).unwrap_or(TokenId::MISSING))
}

/// The engine's colour, in the draw list's clothes.
fn col(c: theme::ThemeColor) -> Color {
    Color { r: c.r, g: c.g, b: c.b, a: c.a }
}

#[derive(Clone, Copy, Default)]
pub struct ButtonState {
    pub hover: bool,
    pub flash: bool,
    pub selected: bool,
}

impl ButtonState {
    /// Which ladder slot this button occupies. A decaying click flash IS
    /// press; selection persists under the pointer as selected_hover.
    fn state(self) -> State {
        if self.flash {
            State::Press
        } else if self.hover && self.selected {
            State::SelectedHover
        } else if self.hover {
            State::Hover
        } else if self.selected {
            State::Selected
        } else {
            State::Idle
        }
    }
}

/// The outline points of a button rectangle. With `button.skew` at
/// zero — where the master leaves it, because a button now wears the
/// same corners as the frames around it — these are the rectangle's
/// own four points, and the SHAPE comes from [`corners`] instead. A
/// theme that wants the old parallelogram back only has to give the
/// token a width again.
pub fn quad(r: &Rect) -> [[f32; 2]; 4] {
    // The dropdown reads the same token, so the accordion stays flush
    // with its anchor's edge whichever shape that edge has.
    static SKEW: OnceLock<TokenId> = OnceLock::new();
    let skew = theme::resolved().px(tok(&SKEW, "button.skew"));
    [
        [r.x + skew, r.y],
        [r.right(), r.y],
        [r.right() - skew, r.bottom()],
        [r.x, r.bottom()],
    ]
}

/// A button's four corners: the same STYLE and the same RADIUS the
/// frames use, because a control that sits inside a rounded panel and
/// answers with a sharp corner reads as a different material. The
/// tokens are the button's own (`button.corner_style`,
/// `button.corner`), and the master points both at the panel's — so
/// the shape is one decision, made once, in the theme.
pub fn corners(t: &theme::ResolvedTheme) -> ([Corner; 4], u8) {
    static MODE: OnceLock<TokenId> = OnceLock::new();
    static IDX: OnceLock<(Option<u16>, Option<u16>)> = OnceLock::new();
    static RADIUS: OnceLock<TokenId> = OnceLock::new();
    static SEGMENTS: OnceLock<TokenId> = OnceLock::new();
    let cut = t.px(tok(&RADIUS, "button.corner")).max(0.0);
    let style = super::window::corner_style(t, tok(&MODE, "button.corner_style"), &IDX);
    (
        [Corner { style, size: cut }; 4],
        super::window::corner_segments(t, &SEGMENTS, cut),
    )
}

/// Draws an opaque parallelogram button with a centered label.
/// Nothing behind the button shows through it.
pub fn draw(ctx: &mut Ctx, r: Rect, label: &str, st: ButtonState) {
    static PLATE: OnceLock<TokenId> = OnceLock::new();
    static ROLE: OnceLock<TokenId> = OnceLock::new();
    static CLASS: OnceLock<Option<u16>> = OnceLock::new();
    let t = theme::resolved();
    let (corners, seg) = corners(t);
    // Opaque plate first, the ladder's state wash on top of it. Both
    // ride the same corners as the frames — one shape, drawn by the
    // same primitive a panel's ring uses.
    ctx.dl.ring_fill(r, &corners, seg, col(t.color(tok(&PLATE, "shape.button.fill"))));
    let style = match *CLASS.get_or_init(|| theme::class_id("button")) {
        Some(c) => t.class_state(c, st.state()),
        None => StateStyle::RAW,
    };
    ctx.dl.ring_fill(r, &corners, seg, col(style.fill));
    if style.edge_width > 0.0 {
        ctx.dl.ring(r, &corners, seg, style.edge_width, col(style.edge));
    }
    // The cap is set in the role `button.role` NAMES, not in the role that
    // happens to share the object's name: repointing the binding moves the
    // label's whole ladder at once, which is the only reason the binding
    // exists. Config scaling (UIFontSize=, panel container query) is
    // behaviour, not design, so it rides the role's own arithmetic.
    let role = ui::bound_role(&ROLE, "button.role");
    // The role's own px floor and ceiling are `Role::px`'s business now,
    // and one place is the point of them: this file used to spell the key
    // from the binding's word by hand, which every other consumer of every
    // other binding did not.
    let px = role.px(ctx, ctx.ui_font_scale);
    let leading = role.leading();
    ctx.dl.text_center(
        ctx.fonts,
        FONT_UI,
        px,
        r.cx(),
        r.y + (r.h - px * leading) / 2.0,
        label,
        col(style.text),
        role.tracking_px(px),
    );
}

/// [`draw`], joined to the world's focus chain: `id` is the caller's
/// stable path (`"settings.btn.reset"` — a path, never an index). A
/// button eats no keys — activation is the router's Enter/Space — so it
/// registers with no capabilities. Focus never touches the state ladder
/// (`ButtonState` grows no `focused` field); the ring overlay is the
/// only focus signal, drawn around the same slanted quad.
pub fn draw_focusable(ctx: &mut Ctx, r: Rect, label: &str, st: ButtonState, id: FocusId) {
    let f = ctx.focus.as_deref_mut().map(|fc| fc.register(id, r, Caps::NONE));
    draw(ctx, r, label, st);
    if f.map_or(false, |f| f.ring) {
        focus_ring::draw_quad(ctx, quad(&r));
    }
}
