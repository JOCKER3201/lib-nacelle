//! Parallelogram button — the standard clickable object of the
//! interface (terminal-tab style: slanted sides, hover highlight,
//! flash on click, optional "selected" state).

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

/// The parallelogram outline points of a button rectangle.
pub fn quad(r: &Rect) -> [[f32; 2]; 4] {
    // The dropdown reads the same token, so the accordion stays flush
    // with its anchor's slanted edge.
    static SKEW: OnceLock<TokenId> = OnceLock::new();
    let skew = theme::resolved().px(tok(&SKEW, "button.skew"));
    [
        [r.x + skew, r.y],
        [r.right(), r.y],
        [r.right() - skew, r.bottom()],
        [r.x, r.bottom()],
    ]
}

/// Draws an opaque parallelogram button with a centered label.
/// Nothing behind the button shows through it.
pub fn draw(ctx: &mut Ctx, r: Rect, label: &str, st: ButtonState) {
    static PLATE: OnceLock<TokenId> = OnceLock::new();
    static SIZE: OnceLock<TokenId> = OnceLock::new();
    static MIN_PX: OnceLock<TokenId> = OnceLock::new();
    static TRACKING: OnceLock<TokenId> = OnceLock::new();
    static LEADING: OnceLock<TokenId> = OnceLock::new();
    static CLASS: OnceLock<Option<u16>> = OnceLock::new();
    let t = theme::resolved();
    let q = quad(&r);
    // Opaque plate first, the ladder's state wash on top of it.
    ctx.dl.quad(q, col(t.color(tok(&PLATE, "shape.button.fill"))));
    let style = match *CLASS.get_or_init(|| theme::class_id("button")) {
        Some(c) => t.class_state(c, st.state()),
        None => StateStyle::RAW,
    };
    ctx.dl.quad(q, col(style.fill));
    ctx.dl.polyline(&q, style.edge_width, col(style.edge), true);
    // Config scaling (UIFontSize=, panel container query) is behaviour,
    // not design; the size itself is the button role's.
    let px = (t.px(tok(&SIZE, "type.button.size")) * ctx.ui_font_scale * ctx.panel_scale)
        .max(t.px(tok(&MIN_PX, "type.button.min_px")));
    let leading = t.px(tok(&LEADING, "type.button.leading"));
    ctx.dl.text_center(
        ctx.fonts,
        FONT_UI,
        px,
        r.cx(),
        r.y + (r.h - px * leading) / 2.0,
        label,
        col(style.text),
        px * t.px(tok(&TRACKING, "type.button.tracking")),
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
