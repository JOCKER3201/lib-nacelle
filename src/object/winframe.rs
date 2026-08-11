//! The frame the toolkit puts around a window it does NOT own.
//!
//! [`window::frame`](super::window) dresses the application's own
//! dialogs; this one is for somebody else's window — a client running
//! under a bare compositor today, and every window texture once the
//! compositor is the project's own. Both embedders share the one
//! object, which is the point: the chrome is decided here, once.
//!
//! The title bar reads, left to right: the options button, the title
//! centred on the window, and the minimize, maximize and close
//! buttons. The options button opens the window menu, anchored to the
//! icon's corner. The frame draws STATES, never transitions: how a
//! menu unfolds, how focus fades — that is animation, and animation
//! belongs to the compositor drawing the frame, not to the frame.
//!
//! The frame computes, draws and answers where a point landed. What a
//! hit MEANS — moving, closing, focusing, resizing the actual window —
//! is the embedder's job, exactly the way widgets return an [`Action`]
//! and the application decides. The content area is left untouched:
//! whoever owns the window's pixels puts them there. The one piece of
//! state a frame carries is whether its menu is open, which is why
//! each window gets a [`Frame`] value rather than a bare function
//! call.
//!
//! Every visual decision here — lengths, strokes, colours, alphas —
//! comes from the theme, with no fallback underneath: a missing token
//! degrades through the engine's per-kind default and is allowed to
//! look raw.
//!
//! [`Action`]: crate::widget::Action

use super::window::{corner_segments, corner_style, panel_edge_glow};
use crate::draw::{fit_tail, Corner};
use crate::font::FONT_UI;
use crate::theme::parse::State;
use crate::theme::{self, Color, TokenId};
use crate::{Ctx, Rect};
use std::sync::OnceLock;

fn tok(cell: &'static OnceLock<TokenId>, name: &'static str) -> TokenId {
    *cell.get_or_init(|| theme::id(name).unwrap_or(TokenId::MISSING))
}

/// A baked theme colour in the draw list's own colour type.
fn col(c: theme::ThemeColor) -> Color {
    Color { r: c.r, g: c.g, b: c.b, a: c.a }
}

/// The baked ladder for a named class, or the raw look when the master
/// declares no such class.
fn class_state(
    t: &theme::ResolvedTheme,
    cell: &'static OnceLock<Option<u16>>,
    name: &'static str,
    state: State,
) -> theme::bake::StateStyle {
    match *cell.get_or_init(|| theme::class_id(name)) {
        Some(c) => t.class_state(c, state),
        None => theme::bake::StateStyle::RAW,
    }
}

/// Frame measurements. The theme bakes them to device pixels for the
/// screen the engine was given, so every frame on a screen matches
/// every other regardless of how big its window is.
#[derive(Clone, Copy, Debug)]
pub struct Metrics {
    /// Title bar height, inside the border.
    pub title_h: f32,
    /// Border thickness.
    pub border: f32,
    /// Chamfer cut of the border corners.
    pub cut: f32,
    /// How far in from the outer edge a point still grabs a resize.
    pub grip: f32,
    /// The stretch of edge near a corner that counts as the corner.
    pub corner_zone: f32,
}

impl Metrics {
    /// All five lengths come from the theme, already baked for the real
    /// screen; the parameter remains only so embedders that sized frames
    /// per-screen keep compiling.
    pub fn new(_screen_h: f32) -> Self {
        static TITLE_H: OnceLock<TokenId> = OnceLock::new();
        static BORDER: OnceLock<TokenId> = OnceLock::new();
        static CUT: OnceLock<TokenId> = OnceLock::new();
        static GRIP: OnceLock<TokenId> = OnceLock::new();
        static ZONE: OnceLock<TokenId> = OnceLock::new();
        let t = theme::resolved();
        Metrics {
            title_h: t.px(tok(&TITLE_H, "winframe.title_h")).max(0.0),
            border: t.px(tok(&BORDER, "winframe.border")).max(0.0),
            // The corner LENGTH; `winframe.corner_mode` says how it is
            // cut, and the draw sites read that enum themselves.
            cut: t.px(tok(&CUT, "winframe.corner")).max(0.0),
            // `winframe.grip_min_px` is floored in by the engine (§3.2).
            grip: t.px(tok(&GRIP, "winframe.grip")).max(0.0),
            corner_zone: t.px(tok(&ZONE, "winframe.corner_zone")).max(0.0),
        }
    }
}

/// An entry of the window menu.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum MenuItem {
    Move,
    Resize,
    Minimize,
    Maximize,
    Close,
}

const MENU: [(MenuItem, &str); 5] = [
    (MenuItem::Move, "MOVE"),
    (MenuItem::Resize, "RESIZE"),
    (MenuItem::Minimize, "MINIMIZE"),
    (MenuItem::Maximize, "MAXIMIZE"),
    (MenuItem::Close, "CLOSE"),
];

/// What a point in a frame means. The resize signs follow the screen:
/// -1 is the left or top edge, +1 the right or bottom, and a corner
/// carries both.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Part {
    /// Inside the client area: the window's own business.
    Content,
    /// The title bar — what the window is dragged by.
    Title,
    /// The options button, or the open menu's backdrop. Toggling on
    /// either is right: the second means clicking past the entries,
    /// which closes the menu.
    Menu,
    /// An entry of the open menu.
    MenuEntry(MenuItem),
    /// The minimize button.
    Minimize,
    /// The maximize button.
    Maximize,
    /// The close button.
    Close,
    /// A resize edge or corner: (horizontal, vertical) signs.
    Resize(i8, i8),
    /// Not in this frame at all.
    Outside,
}

/// The client area inside a frame.
pub fn content(outer: Rect, m: &Metrics) -> Rect {
    Rect::new(
        outer.x + m.border,
        outer.y + m.border + m.title_h,
        (outer.w - 2.0 * m.border).max(0.0),
        (outer.h - 2.0 * m.border - m.title_h).max(0.0),
    )
}

/// The frame a client area needs around it — the inverse of
/// [`content`], for an embedder that starts from the window's size.
pub fn outer_for(content: Rect, m: &Metrics) -> Rect {
    Rect::new(
        content.x - m.border,
        content.y - m.border - m.title_h,
        content.w + 2.0 * m.border,
        content.h + 2.0 * m.border + m.title_h,
    )
}

/// A title bar button: a small square, vertically centred. Slot 0 is
/// nearest the right edge, and the options button is its own place on
/// the left.
fn button_rect(outer: Rect, m: &Metrics, slot: usize) -> Rect {
    static SIZE: OnceLock<TokenId> = OnceLock::new();
    static PAD: OnceLock<TokenId> = OnceLock::new();
    static GAP: OnceLock<TokenId> = OnceLock::new();
    let t = theme::resolved();
    let s = t.px(tok(&SIZE, "winframe.button.size")).max(0.0);
    let pad = t.px(tok(&PAD, "winframe.button.pad")).max(0.0);
    let step = s + t.px(tok(&GAP, "winframe.button.gap")).max(0.0);
    Rect::new(
        outer.x + outer.w - m.border - pad - s - step * slot as f32,
        outer.y + m.border + (m.title_h - s) / 2.0,
        s,
        s,
    )
}

fn menu_button_rect(outer: Rect, m: &Metrics) -> Rect {
    static SIZE: OnceLock<TokenId> = OnceLock::new();
    static PAD: OnceLock<TokenId> = OnceLock::new();
    let t = theme::resolved();
    let s = t.px(tok(&SIZE, "winframe.button.size")).max(0.0);
    let pad = t.px(tok(&PAD, "winframe.button.pad")).max(0.0);
    Rect::new(
        outer.x + m.border + pad,
        outer.y + m.border + (m.title_h - s) / 2.0,
        s,
        s,
    )
}

/// The window menu at full size: anchored to the options button's
/// top-left corner, growing towards the window's opposite corner, and
/// never past the border.
fn menu_rect(outer: Rect, m: &Metrics) -> Rect {
    static ROW_H: OnceLock<TokenId> = OnceLock::new();
    static PAD: OnceLock<TokenId> = OnceLock::new();
    static MIN_W: OnceLock<TokenId> = OnceLock::new();
    let t = theme::resolved();
    let b = menu_button_rect(outer, m);
    let row = t.px(tok(&ROW_H, "menu.row_h")).max(0.0);
    let pad = t.px(tok(&PAD, "menu.pad")).max(0.0);
    let w = t
        .px(tok(&MIN_W, "menu.min_w"))
        .max(0.0)
        .min(outer.x + outer.w - m.border - b.x);
    let h = (pad * 2.0 + row * MENU.len() as f32)
        .min(outer.y + outer.h - m.border - b.y);
    Rect::new(b.x, b.y, w, h)
}

fn menu_row(menu: Rect, i: usize) -> Rect {
    static ROW_H: OnceLock<TokenId> = OnceLock::new();
    static PAD: OnceLock<TokenId> = OnceLock::new();
    let t = theme::resolved();
    let row = t.px(tok(&ROW_H, "menu.row_h")).max(0.0);
    let pad = t.px(tok(&PAD, "menu.pad")).max(0.0);
    Rect::new(menu.x, menu.y + pad + row * i as f32, menu.w, row)
}

/// One window's frame. Everything about it is recomputed from the
/// rectangle each frame; the value only remembers whether the menu is
/// open.
pub struct Frame {
    open: bool,
}

impl Default for Frame {
    fn default() -> Self {
        Self::new()
    }
}

impl Frame {
    pub fn new() -> Self {
        Frame { open: false }
    }

    pub fn menu_open(&self) -> bool {
        self.open
    }

    pub fn toggle_menu(&mut self) {
        self.open = !self.open;
    }

    pub fn close_menu(&mut self) {
        self.open = false;
    }

    /// Where a point lands in this frame. Resize wins over the title
    /// bar — the border is thin and a grab must not fall through it —
    /// and a stretch of edge near a corner counts as the corner,
    /// because a corner the size of the border would be unhittable.
    pub fn hit(&self, outer: Rect, m: &Metrics, x: f32, y: f32) -> Part {
        if !outer.contains(x, y) {
            return Part::Outside;
        }
        let (lx, rx) = (x - outer.x, outer.x + outer.w - x);
        let (ty, by) = (y - outer.y, outer.y + outer.h - y);
        let corner = m.corner_zone;
        let mut sx: i8 = if lx <= m.grip {
            -1
        } else if rx <= m.grip {
            1
        } else {
            0
        };
        let mut sy: i8 = if ty <= m.grip {
            -1
        } else if by <= m.grip {
            1
        } else {
            0
        };
        if sx != 0 && sy == 0 {
            sy = if ty <= corner {
                -1
            } else if by <= corner {
                1
            } else {
                0
            };
        } else if sy != 0 && sx == 0 {
            sx = if lx <= corner {
                -1
            } else if rx <= corner {
                1
            } else {
                0
            };
        }
        if (sx, sy) != (0, 0) {
            return Part::Resize(sx, sy);
        }
        // The open menu overlays the bar and the content alike.
        if self.open {
            let mr = menu_rect(outer, m);
            if mr.contains(x, y) {
                for (i, (item, _)) in MENU.iter().enumerate() {
                    if menu_row(mr, i).contains(x, y) {
                        return Part::MenuEntry(*item);
                    }
                }
                return Part::Menu;
            }
        }
        if y < outer.y + m.border + m.title_h {
            if menu_button_rect(outer, m).contains(x, y) {
                return Part::Menu;
            }
            for (slot, part) in
                [Part::Close, Part::Maximize, Part::Minimize].into_iter().enumerate()
            {
                if button_rect(outer, m, slot).contains(x, y) {
                    return part;
                }
            }
            return Part::Title;
        }
        Part::Content
    }

    /// Draws the frame: the opaque band around the client area, the
    /// chamfered outline, the bar with its buttons, and the menu at
    /// wherever its unfolding stands. The client area itself is not
    /// touched. Focus is a swap of the edge role plus
    /// `focus.unfocused_dim` on the rest of the chrome, the way the
    /// current board is brighter in the BOARDS view.
    pub fn draw(&self, ctx: &mut Ctx, m: &Metrics, outer: Rect, title: &str, focused: bool) {
        static TITLEBAR_FILL: OnceLock<TokenId> = OnceLock::new();
        static BODY_FILL: OnceLock<TokenId> = OnceLock::new();
        static BORDER_FOCUS: OnceLock<TokenId> = OnceLock::new();
        static WINDOW_CLASS: OnceLock<Option<u16>> = OnceLock::new();
        static TITLEBAR_RULE: OnceLock<TokenId> = OnceLock::new();
        static RULE_W: OnceLock<TokenId> = OnceLock::new();
        static UNFOCUSED_DIM: OnceLock<TokenId> = OnceLock::new();
        static ICON_BUTTON: OnceLock<Option<u16>> = OnceLock::new();
        static BUTTON_BORDER: OnceLock<TokenId> = OnceLock::new();
        static WC_IDLE: OnceLock<TokenId> = OnceLock::new();
        static WC_HOVER: OnceLock<TokenId> = OnceLock::new();
        static WC_CLOSE: OnceLock<TokenId> = OnceLock::new();
        static ICON_STROKE: OnceLock<TokenId> = OnceLock::new();
        static ICON_INSET: OnceLock<TokenId> = OnceLock::new();
        static MENU_ROWS: OnceLock<TokenId> = OnceLock::new();
        static MENU_PITCH: OnceLock<TokenId> = OnceLock::new();
        static MINIMISE_Y: OnceLock<TokenId> = OnceLock::new();
        static MODE: OnceLock<TokenId> = OnceLock::new();
        static MODE_IDX: OnceLock<(Option<u16>, Option<u16>)> = OnceLock::new();
        static SEGMENTS: OnceLock<TokenId> = OnceLock::new();
        let t = theme::resolved();
        let c = content(outer, m);
        // The band: top (with the title bar), bottom, left, right —
        // each shaped by the corner cut, so no fill pokes past the cut
        // corners. The top band is a trapezoid down to the cut depth
        // and a rectangle below; the bottom band a trapezoid narrowing
        // toward the floor (the cut is always deeper than the border).
        // The bands keep the 45° silhouette in every corner mode: a
        // round arc of the same length lies OUTSIDE its chamfer chord,
        // so under `corner_mode = round` the trapezoid stays inside the
        // ring and only under-fills by the sagitta at the corner.
        // The title band and the frame body are two materials now;
        // `shape.window.fill` is declared `same_as_parent` and its
        // parent chain is not walkable from a colour read, so the body
        // reads the chain's documented target directly.
        let band = col(t.color(tok(&TITLEBAR_FILL, "component.titlebar.fill")));
        let body = col(t.color(tok(&BODY_FILL, "surface.panel")));
        ctx.dl.quad(
            [
                [outer.x, outer.y + m.cut],
                [outer.x + m.cut, outer.y],
                [outer.x + outer.w - m.cut, outer.y],
                [outer.x + outer.w, outer.y + m.cut],
            ],
            band,
        );
        ctx.dl.rect(
            outer.x,
            outer.y + m.cut,
            outer.w,
            m.border + m.title_h - m.cut,
            band,
        );
        let inset = (m.cut - m.border).max(0.0);
        ctx.dl.quad(
            [
                [outer.x + inset, c.y + c.h],
                [outer.x + outer.w - inset, c.y + c.h],
                [outer.x + outer.w - m.cut, outer.y + outer.h],
                [outer.x + m.cut, outer.y + outer.h],
            ],
            body,
        );
        ctx.dl.rect(outer.x, c.y, m.border, c.h, body);
        ctx.dl.rect(c.x + c.w, c.y, m.border, c.h, body);
        // Focus swaps the edge role (§5.21): the focused ring is
        // `border.focus`, the resting one the window class's idle edge.
        let line = if focused {
            col(t.color(tok(&BORDER_FOCUS, "border.focus")))
        } else {
            col(class_state(t, &WINDOW_CLASS, "window", State::Idle).edge)
        };
        let style = corner_style(t, tok(&MODE, "winframe.corner_mode"), &MODE_IDX);
        let corners = [Corner { style, size: m.cut }; 4];
        let seg = corner_segments(t, &SEGMENTS, m.cut);
        ctx.dl.ring(outer, &corners, seg, m.border, line);
        panel_edge_glow(ctx.dl, t, outer, &corners, seg, line);
        // The title bar's floor.
        ctx.dl.line(
            outer.x + m.border,
            c.y,
            outer.x + outer.w - m.border,
            c.y,
            t.px(tok(&RULE_W, "winframe.rule")).max(0.0),
            col(t.color(tok(&TITLEBAR_RULE, "component.titlebar.rule"))),
        );
        let dim = if focused {
            1.0
        } else {
            t.px(tok(&UNFOCUSED_DIM, "focus.unfocused_dim")).clamp(0.0, 1.0)
        };
        let bw = t.px(tok(&BUTTON_BORDER, "winframe.button.border")).max(0.0);
        let ink_idle = col(t.color(tok(&WC_IDLE, "component.window_control.idle")));
        let ink_hover = col(t.color(tok(&WC_HOVER, "component.window_control.hover")));
        let ink_close = col(t.color(tok(&WC_CLOSE, "component.window_control.close_hover")));
        // A control plate: the icon_button ladder's edge for the ring,
        // the window_control roles for the glyph — the close button is
        // the one destructive control and hovers in its own colour.
        let plate = |ctx: &mut Ctx, r: Rect, close: bool| -> Color {
            let hot = r.contains(ctx.mouse.0, ctx.mouse.1);
            let st = class_state(
                t,
                &ICON_BUTTON,
                "icon_button",
                if hot { State::Hover } else { State::Idle },
            );
            let ring = col(st.edge);
            ctx.dl.rect_outline(r.x, r.y, r.w, r.h, bw, ring.alpha(ring.a * dim));
            let ink = if hot {
                if close {
                    ink_close
                } else {
                    ink_hover
                }
            } else {
                ink_idle
            };
            ink.alpha(ink.a * dim)
        };
        let stroke = t.px(tok(&ICON_STROKE, "winframe.icon.stroke")).max(0.0);
        let g_inset = t.px(tok(&ICON_INSET, "winframe.icon.inset")).max(0.0);
        // The options button: stacked lines, the universal "there is
        // more here", centred by their pitch.
        let mb = menu_button_rect(outer, m);
        let ic = plate(ctx, mb, false);
        let rows = t.px(tok(&MENU_ROWS, "winframe.icon.menu_rows")).max(0.0) as usize;
        let pitch = t.px(tok(&MENU_PITCH, "winframe.icon.menu_pitch")).max(0.0);
        let first = mb.y + (mb.h - pitch * rows.saturating_sub(1) as f32) / 2.0;
        for i in 0..rows {
            let ly = first + pitch * i as f32;
            ctx.dl.line(mb.x + g_inset, ly, mb.x + mb.w - g_inset, ly, stroke, ic);
        }
        // Right, outermost first: close, maximize, minimize.
        let xr = button_rect(outer, m, 0);
        let ic = plate(ctx, xr, true);
        ctx.dl.line(
            xr.x + g_inset,
            xr.y + g_inset,
            xr.x + xr.w - g_inset,
            xr.y + xr.h - g_inset,
            stroke,
            ic,
        );
        ctx.dl.line(
            xr.x + xr.w - g_inset,
            xr.y + g_inset,
            xr.x + g_inset,
            xr.y + xr.h - g_inset,
            stroke,
            ic,
        );
        let mx = button_rect(outer, m, 1);
        let ic = plate(ctx, mx, false);
        ctx.dl.rect_outline(
            mx.x + g_inset,
            mx.y + g_inset,
            mx.w - 2.0 * g_inset,
            mx.h - 2.0 * g_inset,
            stroke,
            ic,
        );
        let mn = button_rect(outer, m, 2);
        let ic = plate(ctx, mn, false);
        let ly = mn.y + t.px(tok(&MINIMISE_Y, "winframe.icon.minimise_y")).max(0.0);
        ctx.dl.line(mn.x + g_inset, ly, mn.x + mn.w - g_inset, ly, stroke, ic);
        self.draw_title(ctx, m, outer, title, mb, mn, dim);
        // The menu, anchored to its icon's corner. Open or closed,
        // nothing between: the unfolding is the compositor's to
        // animate.
        if self.open {
            self.draw_menu(ctx, m, outer);
        }
    }

    /// The title, set in the `title.window` role: size, tracking, case
    /// and leading are the role's, the colour is the title bar's, and
    /// an overlong title gives way to the room the theme keeps clear.
    #[allow(clippy::too_many_arguments)]
    fn draw_title(
        &self,
        ctx: &mut Ctx,
        m: &Metrics,
        outer: Rect,
        title: &str,
        mb: Rect,
        mn: Rect,
        dim: f32,
    ) {
        static SIZE: OnceLock<TokenId> = OnceLock::new();
        static TRACKING: OnceLock<TokenId> = OnceLock::new();
        static LEADING: OnceLock<TokenId> = OnceLock::new();
        static CASE: OnceLock<TokenId> = OnceLock::new();
        static CASE_IDX: OnceLock<(Option<u16>, Option<u16>)> = OnceLock::new();
        static ALIGN: OnceLock<TokenId> = OnceLock::new();
        static ALIGN_LEFT: OnceLock<Option<u16>> = OnceLock::new();
        static ROOM_PAD: OnceLock<TokenId> = OnceLock::new();
        static TEXT: OnceLock<TokenId> = OnceLock::new();
        let t = theme::resolved();
        let px = t.px(tok(&SIZE, "type.title.window.size")).max(0.0);
        // Tracking is an em multiple of the resolved size (§5.16).
        let spacing = px * t.px(tok(&TRACKING, "type.title.window.tracking"));
        let leading = t.px(tok(&LEADING, "type.title.window.leading")).max(0.0);
        let room_pad = t.px(tok(&ROOM_PAD, "winframe.title.room_pad")).max(0.0);
        let ink = col(t.color(tok(&TEXT, "component.titlebar.text")));
        let ink = ink.alpha(ink.a * dim);
        let case = tok(&CASE, "type.title.window.case");
        let (lower, none) = *CASE_IDX.get_or_init(|| {
            (theme::enum_index(case, "lower"), theme::enum_index(case, "none"))
        });
        // `smallcaps` is approximated as upper until FontSystem can set it.
        let shown = if Some(t.enum_of(case)) == none {
            title.to_string()
        } else if Some(t.enum_of(case)) == lower {
            title.to_lowercase()
        } else {
            title.to_uppercase()
        };
        let y = outer.y + m.border + (m.title_h - px * leading) / 2.0;
        let align = tok(&ALIGN, "winframe.title.align");
        let left = *ALIGN_LEFT.get_or_init(|| theme::enum_index(align, "left"));
        if Some(t.enum_of(align)) == left {
            let x0 = mb.x + mb.w + room_pad;
            let room = mn.x - room_pad - x0;
            let shown = fit_tail(ctx.fonts, FONT_UI, px, &shown, spacing, room);
            ctx.dl.text(ctx.fonts, FONT_UI, px, x0, y, &shown, ink, spacing);
        } else {
            // Centred on the window; the room is symmetric so the
            // centre holds.
            let cx = outer.x + outer.w / 2.0;
            let room = 2.0 * (cx - (mb.x + mb.w)).min(mn.x - cx) - room_pad;
            let shown = fit_tail(ctx.fonts, FONT_UI, px, &shown, spacing, room);
            ctx.dl.text_center(ctx.fonts, FONT_UI, px, cx, y, &shown, ink, spacing);
        }
    }

    /// The open menu: its own material and ring, rows from the
    /// `menu.item` ladder.
    fn draw_menu(&self, ctx: &mut Ctx, m: &Metrics, outer: Rect) {
        static FILL: OnceLock<TokenId> = OnceLock::new();
        static RING: OnceLock<TokenId> = OnceLock::new();
        static RING_W: OnceLock<TokenId> = OnceLock::new();
        static CORNER: OnceLock<TokenId> = OnceLock::new();
        static ITEM_SIZE: OnceLock<TokenId> = OnceLock::new();
        static ITEM_TRACKING: OnceLock<TokenId> = OnceLock::new();
        static ITEM_LEADING: OnceLock<TokenId> = OnceLock::new();
        static ITEM_INSET: OnceLock<TokenId> = OnceLock::new();
        static MENU_ITEM: OnceLock<Option<u16>> = OnceLock::new();
        static MODE: OnceLock<TokenId> = OnceLock::new();
        static MODE_IDX: OnceLock<(Option<u16>, Option<u16>)> = OnceLock::new();
        static SEGMENTS: OnceLock<TokenId> = OnceLock::new();
        let t = theme::resolved();
        let mr = menu_rect(outer, m);
        let cut = t.px(tok(&CORNER, "menu.corner")).max(0.0);
        // `menu.corner` is declared as a ratio of `winframe.corner`, so
        // the menu's cut follows the frame's mode as well as its length:
        // the window's chrome speaks one corner language.
        let style = corner_style(t, tok(&MODE, "winframe.corner_mode"), &MODE_IDX);
        let corners = [Corner { style, size: cut }; 4];
        let seg = corner_segments(t, &SEGMENTS, cut);
        ctx.dl.ring_fill(mr, &corners, seg, col(t.color(tok(&FILL, "component.menu.fill"))));
        ctx.dl.ring(
            mr,
            &corners,
            seg,
            t.px(tok(&RING_W, "menu.border")).max(0.0),
            col(t.color(tok(&RING, "component.menu.border"))),
        );
        // Rows are set in the body role (`menu.item.role`).
        let ipx = t.px(tok(&ITEM_SIZE, "type.body.size")).max(0.0);
        let spacing = ipx * t.px(tok(&ITEM_TRACKING, "type.body.tracking"));
        let leading = t.px(tok(&ITEM_LEADING, "type.body.leading")).max(0.0);
        let inset = t.px(tok(&ITEM_INSET, "menu.item_inset")).max(0.0);
        for (i, (_, label)) in MENU.iter().enumerate() {
            let row = menu_row(mr, i);
            let hot = row.contains(ctx.mouse.0, ctx.mouse.1);
            let st = class_state(
                t,
                &MENU_ITEM,
                "menu.item",
                if hot { State::Hover } else { State::Idle },
            );
            if hot {
                ctx.dl.rect(row.x, row.y, row.w, row.h, col(st.fill));
            }
            ctx.dl.text(
                ctx.fonts,
                FONT_UI,
                ipx,
                row.x + inset,
                row.y + (row.h - ipx * leading) / 2.0,
                label,
                col(st.text),
                spacing,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Hand-built metrics: the tests exercise the frame's geometry and
    /// hit logic, not the theme's numbers.
    fn m() -> Metrics {
        Metrics {
            title_h: 26.0,
            border: 1.8,
            cut: 11.0,
            grip: 6.0,
            corner_zone: 26.0,
        }
    }

    #[test]
    fn content_and_outer_are_inverses() {
        let m = m();
        let outer = Rect::new(100.0, 100.0, 400.0, 300.0);
        let c = content(outer, &m);
        let back = outer_for(c, &m);
        for (a, b) in [
            (back.x, outer.x),
            (back.y, outer.y),
            (back.w, outer.w),
            (back.h, outer.h),
        ] {
            assert!((a - b).abs() < 0.001);
        }
    }

    #[test]
    fn every_part_answers_where_it_is() {
        let m = m();
        // title_h 26, border 1.8, grip 6, corner zone 26.
        let outer = Rect::new(100.0, 100.0, 400.0, 300.0);
        let f = Frame::new();
        assert_eq!(f.hit(outer, &m, 50.0, 50.0), Part::Outside);
        assert_eq!(f.hit(outer, &m, 300.0, 250.0), Part::Content);
        // Edges and their signs.
        assert_eq!(f.hit(outer, &m, 300.0, 102.0), Part::Resize(0, -1));
        assert_eq!(f.hit(outer, &m, 300.0, 398.0), Part::Resize(0, 1));
        assert_eq!(f.hit(outer, &m, 102.0, 250.0), Part::Resize(-1, 0));
        assert_eq!(f.hit(outer, &m, 498.0, 250.0), Part::Resize(1, 0));
        // A corner, and an edge close enough to one to count as it.
        assert_eq!(f.hit(outer, &m, 102.0, 102.0), Part::Resize(-1, -1));
        assert_eq!(f.hit(outer, &m, 102.0, 120.0), Part::Resize(-1, -1));
        assert_eq!(f.hit(outer, &m, 480.0, 398.0), Part::Resize(1, 1));
        // The bar between the buttons, and each button on it.
        assert_eq!(f.hit(outer, &m, 300.0, 115.0), Part::Title);
        for (r, part) in [
            (menu_button_rect(outer, &m), Part::Menu),
            (button_rect(outer, &m, 0), Part::Close),
            (button_rect(outer, &m, 1), Part::Maximize),
            (button_rect(outer, &m, 2), Part::Minimize),
        ] {
            assert_eq!(f.hit(outer, &m, r.x + r.w / 2.0, r.y + r.h / 2.0), part);
        }
        // The title bar hands the client area everything below it.
        let c = content(outer, &m);
        assert_eq!(f.hit(outer, &m, 300.0, c.y + 1.0), Part::Content);
    }

    #[test]
    fn the_menu_overlays_only_while_open() {
        let m = m();
        let outer = Rect::new(100.0, 100.0, 400.0, 300.0);
        let mut f = Frame::new();
        let mr = menu_rect(outer, &m);
        // The second row: below the title bar, so the closed answer is
        // unambiguously the client area.
        let second = menu_row(mr, 1);
        let (px, py) = (second.x + second.w / 2.0, second.y + second.h / 2.0);
        // Closed: the point is whatever sits under the folded menu.
        assert_eq!(f.hit(outer, &m, px, py), Part::Content);
        f.toggle_menu();
        assert!(f.menu_open());
        assert_eq!(f.hit(outer, &m, px, py), Part::MenuEntry(MenuItem::Resize));
        // Between the last entry and the menu's edge: the backdrop.
        let last = menu_row(mr, MENU.len() - 1);
        assert_eq!(
            f.hit(outer, &m, px, last.y + last.h + 1.0),
            Part::Menu
        );
        f.close_menu();
        assert!(!f.menu_open());
        assert_eq!(f.hit(outer, &m, px, py), Part::Content);
    }
}
