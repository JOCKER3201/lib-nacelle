//! Tooltip (F2 §8.1): the label that explains what the pointer is
//! resting on, after it has rested long enough to be asking.
//!
//! One manager per application, drawn LAST — the menu's rule, for the
//! menu's reason: the draw list is immediate and draw order is z-order,
//! so a tooltip drawn anywhere else would sit under whatever came after
//! it.
//!
//! There is no registry of hover-able rectangles. Whoever owns a rect
//! already knows where it is and whether the pointer is inside it, so it
//! files a [`Tooltips::request`] while it draws and the manager decides
//! — which target the pointer has actually settled on, whether the delay
//! has elapsed, and where the box fits on screen. Two requests in one
//! frame are answered by the LAST one: it was drawn later, so it is on
//! top, so it is the one under the pointer.
//!
//! Everything visual comes from `[tooltip]` and `component.tooltip.*`;
//! the module holds no literal of its own. There is no fade — the phase
//! that gives the toolkit a property-animation engine gives the tooltip
//! its `motion.tooltip_*`, and until then appearing is instantaneous,
//! which is honest rather than half-animated.

use crate::draw::Corner;
use crate::theme::{self, Color, TokenId};
use crate::{ui, Ctx, Rect};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::OnceLock;

fn tok(cell: &'static OnceLock<TokenId>, name: &'static str) -> TokenId {
    *cell.get_or_init(|| theme::id(name).unwrap_or(TokenId::MISSING))
}

/// The engine's colour, in the draw list's clothes.
fn col(c: theme::ThemeColor) -> Color {
    Color { r: c.r, g: c.g, b: c.b, a: c.a }
}

/// An id for a target named by a string — a table heading, a tab label.
///
/// Callers with a natural number (a row index, a widget handle) should
/// use it directly; this is for the ones whose only stable name is text.
pub fn key(name: &str) -> u64 {
    let mut h = DefaultHasher::new();
    name.hash(&mut h);
    h.finish()
}

/// An id for one cell of a view: which view drew it, which column it is,
/// and the row it belongs to (empty for a heading).
///
/// A table's cells cannot be named by their text — two rows may hold the
/// same word, and the text is exactly what changes when the model is
/// rewritten — so the identity is the PLACE, and the row's key is the
/// part of the place that survives a sort.
pub fn cell_key(view: u32, col: usize, row: &str) -> u64 {
    let mut h = DefaultHasher::new();
    view.hash(&mut h);
    col.hash(&mut h);
    row.hash(&mut h);
    h.finish()
}

/// What a requester asked for this frame.
struct Pending {
    id: u64,
    anchor: Rect,
    text: String,
    /// When the pointer was found inside `anchor` — the caller's clock,
    /// so a caller drawing at a different time than the manager still
    /// measures one delay.
    t: f64,
}

/// The target the pointer is currently resting on.
struct Armed {
    id: u64,
    /// When it was reached.
    since: f64,
    /// Whether the delay is being skipped because the pointer stepped
    /// here straight off another explained target (`tooltip.linger_ms`).
    instant: bool,
}

/// The application's one tooltip manager.
#[derive(Default)]
pub struct Tooltips {
    armed: Option<Armed>,
    pending: Option<Pending>,
    /// The last moment a tooltip was actually on screen. The grace
    /// window that lets the next neighbour skip the delay is measured
    /// from here, so walking along a row of controls explains each one
    /// without a pause between them.
    last_shown: Option<f64>,
    /// What the last [`Tooltips::draw`] put on screen, for a caller
    /// that needs to know whether the pointer is over a tooltip.
    rect: Option<Rect>,
    /// The text in that box. Kept beside the rectangle because the draw
    /// list holds glyph quads and not words: without it, nothing outside
    /// this module can say WHICH explanation reached the screen.
    shown: Option<String>,
}

impl Tooltips {
    pub fn new() -> Tooltips {
        Tooltips::default()
    }

    /// Files a request while drawing, when the pointer is inside
    /// `anchor`. Empty text is not a request — a cell with nothing more
    /// to say than what is already drawn says nothing.
    pub fn request(&mut self, id: u64, anchor: Rect, text: &str, t: f64) {
        if text.is_empty() {
            return;
        }
        self.pending = Some(Pending { id, anchor, text: text.to_string(), t });
    }

    /// [`Tooltips::request`] with the pointer test done here — the form
    /// almost every caller wants, since almost every caller has a `Ctx`
    /// in hand and a rect it has just drawn.
    pub fn hover(&mut self, ctx: &Ctx, id: u64, anchor: Rect, text: &str) {
        if anchor.contains(ctx.mouse.0, ctx.mouse.1) {
            self.request(id, anchor, text, ctx.t);
        }
    }

    /// Drops everything: no request stands, nothing is armed, and the
    /// next target pays the full delay. For the moments a tooltip must
    /// not survive — a menu opening over it, a window closing.
    pub fn clear(&mut self) {
        self.armed = None;
        self.pending = None;
        self.last_shown = None;
        self.rect = None;
        self.shown = None;
    }

    /// The box drawn by the last [`Tooltips::draw`], if any.
    pub fn rect(&self) -> Option<Rect> {
        self.rect
    }

    /// The text in that box — what the tooltip actually SAID.
    pub fn shown(&self) -> Option<&str> {
        self.shown.as_deref()
    }

    /// The frame's decision, as arithmetic: which request (if any) is
    /// due to be shown, given the two themed times in milliseconds.
    ///
    /// Separated from the drawing so the delay, the disarming and the
    /// grace window can be tested without a window, a font or a theme.
    fn step(&mut self, now: f64, delay_ms: f32, linger_ms: f32) -> Option<(Rect, String)> {
        let Some(p) = self.pending.take() else {
            // Nothing asked this frame: the pointer left every anchor.
            self.armed = None;
            return None;
        };
        let fresh = match &self.armed {
            Some(a) if a.id == p.id => false,
            _ => true,
        };
        if fresh {
            let instant = self
                .last_shown
                .is_some_and(|t0| (now - t0) * 1000.0 <= linger_ms as f64);
            self.armed = Some(Armed { id: p.id, since: p.t, instant });
        }
        let a = self.armed.as_ref()?;
        let due = a.instant || (now - a.since) * 1000.0 >= delay_ms as f64;
        if !due {
            return None;
        }
        self.last_shown = Some(now);
        Some((p.anchor, p.text))
    }

    /// End of frame: shows the request whose pointer has rested long
    /// enough, near the pointer, flipped to stay on screen.
    pub fn draw(&mut self, ctx: &mut Ctx) {
        static DELAY: OnceLock<TokenId> = OnceLock::new();
        static LINGER: OnceLock<TokenId> = OnceLock::new();
        static H: OnceLock<TokenId> = OnceLock::new();
        static PAD_X: OnceLock<TokenId> = OnceLock::new();
        static PAD_Y: OnceLock<TokenId> = OnceLock::new();
        static CORNER: OnceLock<TokenId> = OnceLock::new();
        static CORNER_MODE: OnceLock<TokenId> = OnceLock::new();
        static CORNER_IDX: OnceLock<(Option<u16>, Option<u16>)> = OnceLock::new();
        static BORDER: OnceLock<TokenId> = OnceLock::new();
        static OFFSET: OnceLock<TokenId> = OnceLock::new();
        static MAX_W: OnceLock<TokenId> = OnceLock::new();
        static ROLE: OnceLock<TokenId> = OnceLock::new();
        static FILL: OnceLock<TokenId> = OnceLock::new();
        static EDGE: OnceLock<TokenId> = OnceLock::new();
        static INK: OnceLock<TokenId> = OnceLock::new();
        static SEGMENTS: OnceLock<TokenId> = OnceLock::new();

        let t = theme::resolved();
        let delay = t.px(tok(&DELAY, "tooltip.delay_ms"));
        let linger = t.px(tok(&LINGER, "tooltip.linger_ms"));
        self.rect = None;
        self.shown = None;
        let Some((anchor, text)) = self.step(ctx.t, delay, linger) else { return };

        // ---- metrics ----------------------------------------------------
        let pad_x = t.px(tok(&PAD_X, "tooltip.pad_x")).max(0.0);
        let pad_y = t.px(tok(&PAD_Y, "tooltip.pad_y")).max(0.0);
        let offset = t.px(tok(&OFFSET, "tooltip.offset")).max(0.0);
        let max_w = t.px(tok(&MAX_W, "tooltip.max_w")).max(0.0);
        let min_h = t.px(tok(&H, "tooltip.h")).max(0.0);
        let role = ui::bound_role(&ROLE, "tooltip.role");
        // No `ui_font_scale`: the viewport carries the user's scale into u,
        // and the role's size is written in u — applying it here too squares it.
        let px = role.px(ctx, 1.0);
        let track = role.tracking_px(px);
        let leading = role.leading();
        // `tooltip.role`'s own face: the box wraps, measures and draws in
        // one family, which it could not while the wrap read the role and
        // the measure wrote FONT_UI.
        let face = role.font();

        // ---- the lines --------------------------------------------------
        let lines = ui::wrap_text(ctx, face, px, &text, max_w, track);
        let mut text_w: f32 = 0.0;
        for l in &lines {
            text_w = text_w.max(ctx.fonts.measure(face, px, l, track));
        }
        let line_h = px * leading;
        let block_h = line_h * lines.len() as f32;
        let w = text_w + 2.0 * pad_x;
        // `tooltip.h` is the box's MINIMUM: one line is the height the
        // theme wrote, and every line after it grows the box.
        let h = (block_h + 2.0 * pad_y).max(min_h);

        // ---- place ------------------------------------------------------
        let (x, y) = place(ctx.mouse, anchor, (w, h), offset, (ctx.w, ctx.h));
        let r = Rect::new(x, y, w, h);
        self.rect = Some(r);
        self.shown = Some(text.clone());

        // ---- the box ----------------------------------------------------
        // A tooltip is the same floating chrome a menu is, so the master
        // points `tooltip.corner_mode` at the menu's rather than letting
        // two boxes that appear side by side answer differently.
        let style =
            super::window::corner_style(t, tok(&CORNER_MODE, "tooltip.corner_mode"), &CORNER_IDX);
        // `Corner::sized` rather than a clamp: §5.0's `pill` bakes to a
        // negative number, so a floor at zero would draw the square a
        // master writing `pill` wrote to avoid — and say nothing.
        let corner = Corner::sized(style, t.px(tok(&CORNER, "tooltip.corner")), r);
        let c = [corner; 4];
        let seg = super::window::corner_segments(t, &SEGMENTS, corner.size);
        ctx.dl.ring_fill(r, &c, seg, col(t.bed(tok(&FILL, "component.tooltip.fill"))));
        let bw = t.px(tok(&BORDER, "tooltip.border")).max(0.0);
        if bw > 0.0 {
            ctx.dl.ring(r, &c, seg, bw, col(t.color(tok(&EDGE, "component.tooltip.edge"))));
        }

        // ---- the text ---------------------------------------------------
        let ink = col(t.color(tok(&INK, "component.tooltip.text")));
        let mut ty = r.y + (h - block_h) / 2.0;
        for l in &lines {
            ctx.dl.text(ctx.fonts, face, px, r.x + pad_x, ty, l, ink, track);
            ty += line_h;
        }
    }
}

/// Where a tooltip of `size` lands for a pointer at `at` explaining
/// `anchor`: below and to the right of the pointer by `offset`, flipped
/// when there is no room, clamped to the window as a last resort.
///
/// Two departures from the menu's [`super::menu`] placement, both
/// because a tooltip explains something that must stay visible:
///
/// * the flip keeps the gap on the far side too, instead of putting the
///   box's far edge on the point — a tooltip under the cursor it is
///   explaining is a tooltip in the way;
/// * flipping UP goes above the ANCHOR, not above the pointer, so the
///   target the user is pointing at is not the thing that gets covered.
fn place(
    at: (f32, f32),
    anchor: Rect,
    size: (f32, f32),
    offset: f32,
    win: (f32, f32),
) -> (f32, f32) {
    let x = if at.0 + offset + size.0 <= win.0 {
        at.0 + offset
    } else if at.0 - offset - size.0 >= 0.0 {
        at.0 - offset - size.0
    } else {
        (win.0 - size.0).max(0.0)
    };
    let y = if at.1 + offset + size.1 <= win.1 {
        at.1 + offset
    } else if anchor.y - offset - size.1 >= 0.0 {
        anchor.y - offset - size.1
    } else {
        (win.1 - size.1).max(0.0)
    };
    (x, y)
}

// ---------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const DELAY: f32 = 600.0;
    const LINGER: f32 = 120.0;

    fn anchor() -> Rect {
        Rect::new(10.0, 10.0, 100.0, 20.0)
    }

    /// One frame: file a request (or not) and take the decision.
    fn frame(
        tips: &mut Tooltips,
        now: f64,
        req: Option<(u64, &str)>,
    ) -> Option<(Rect, String)> {
        if let Some((id, text)) = req {
            tips.request(id, anchor(), text, now);
        }
        tips.step(now, DELAY, LINGER)
    }

    // ---- placement ----

    #[test]
    fn placement_keeps_its_gap_on_whichever_side_it_lands() {
        let win = (500.0, 300.0);
        let a = Rect::new(0.0, 270.0, 100.0, 20.0);
        // Room everywhere: down and right of the pointer, by the offset.
        assert_eq!(place((10.0, 20.0), a, (100.0, 40.0), 5.0, win), (15.0, 25.0));
        // No room on the right: the box's RIGHT edge keeps the gap.
        assert_eq!(place((450.0, 20.0), a, (100.0, 40.0), 5.0, win), (345.0, 25.0));
        // No room below: above the ANCHOR (270), not above the pointer.
        assert_eq!(place((10.0, 280.0), a, (100.0, 40.0), 5.0, win), (15.0, 225.0));
        // A box wider than the window: clamped, never negative.
        assert_eq!(place((10.0, 20.0), a, (900.0, 40.0), 5.0, win), (0.0, 25.0));
        // No room anywhere on the vertical: clamped to the window.
        let tall = Rect::new(0.0, 10.0, 100.0, 20.0);
        assert_eq!(place((10.0, 280.0), tall, (100.0, 40.0), 5.0, win), (15.0, 260.0));
    }

    // ---- the delay ----

    #[test]
    fn nothing_shows_before_the_delay_has_elapsed() {
        let mut tips = Tooltips::new();
        assert!(frame(&mut tips, 0.0, Some((1, "CPU"))).is_none());
        assert!(frame(&mut tips, 0.3, Some((1, "CPU"))).is_none());
        // 600 ms exactly: due.
        let out = frame(&mut tips, 0.6, Some((1, "CPU"))).expect("due at the delay");
        assert_eq!(out.1, "CPU");
        // The anchor comes back untouched — the box is placed against it.
        assert_eq!((out.0.x, out.0.y, out.0.w, out.0.h), (10.0, 10.0, 100.0, 20.0));
    }

    #[test]
    fn leaving_the_anchor_disarms_and_the_next_rest_pays_again() {
        let mut tips = Tooltips::new();
        frame(&mut tips, 0.0, Some((1, "CPU")));
        // The pointer leaves: no request at all, for long enough that
        // the grace window has closed too.
        assert!(frame(&mut tips, 0.4, None).is_none());
        assert!(frame(&mut tips, 2.0, None).is_none());
        // Back on the same target: the clock starts again.
        assert!(frame(&mut tips, 2.1, Some((1, "CPU"))).is_none());
        assert!(frame(&mut tips, 2.6, Some((1, "CPU"))).is_none());
        assert!(frame(&mut tips, 2.71, Some((1, "CPU"))).is_some());
    }

    #[test]
    fn a_new_target_restarts_the_delay_when_nothing_was_shown() {
        let mut tips = Tooltips::new();
        frame(&mut tips, 0.0, Some((1, "CPU")));
        frame(&mut tips, 0.5, Some((1, "CPU")));
        // Straight onto a neighbour before the first ever appeared:
        // there is no grace to inherit, so the delay is paid in full.
        assert!(frame(&mut tips, 0.5, Some((2, "RAM"))).is_none());
        assert!(frame(&mut tips, 1.0, Some((2, "RAM"))).is_none());
        assert!(frame(&mut tips, 1.11, Some((2, "RAM"))).is_some());
    }

    // ---- the grace window ----

    #[test]
    fn a_neighbour_reached_within_the_grace_window_shows_at_once() {
        let mut tips = Tooltips::new();
        frame(&mut tips, 0.0, Some((1, "CPU")));
        assert!(frame(&mut tips, 0.6, Some((1, "CPU"))).is_some());
        // 100 ms later, onto the next control: no second wait.
        let out = frame(&mut tips, 0.7, Some((2, "RAM"))).expect("within linger");
        assert_eq!(out.1, "RAM");
    }

    #[test]
    fn past_the_grace_window_the_neighbour_waits_like_the_first() {
        let mut tips = Tooltips::new();
        frame(&mut tips, 0.0, Some((1, "CPU")));
        assert!(frame(&mut tips, 0.6, Some((1, "CPU"))).is_some());
        // Pointer wandered over nothing for a while, then a neighbour.
        assert!(frame(&mut tips, 0.8, None).is_none());
        assert!(frame(&mut tips, 0.9, Some((2, "RAM"))).is_none());
        assert!(frame(&mut tips, 1.51, Some((2, "RAM"))).is_some());
    }

    #[test]
    fn clear_forgets_the_grace_window_too() {
        let mut tips = Tooltips::new();
        frame(&mut tips, 0.0, Some((1, "CPU")));
        assert!(frame(&mut tips, 0.6, Some((1, "CPU"))).is_some());
        tips.clear();
        assert!(frame(&mut tips, 0.65, Some((2, "RAM"))).is_none());
    }

    // ---- requests ----

    #[test]
    fn empty_text_is_not_a_request() {
        let mut tips = Tooltips::new();
        assert!(frame(&mut tips, 0.0, Some((1, ""))).is_none());
        assert!(tips.armed.is_none());
    }

    #[test]
    fn the_last_request_of_a_frame_wins() {
        let mut tips = Tooltips::new();
        tips.request(1, anchor(), "UNDER", 0.0);
        tips.request(2, Rect::new(0.0, 0.0, 5.0, 5.0), "OVER", 0.0);
        assert!(tips.step(0.0, DELAY, LINGER).is_none());
        tips.request(2, Rect::new(0.0, 0.0, 5.0, 5.0), "OVER", 0.0);
        let out = tips.step(0.6, DELAY, LINGER).expect("due");
        assert_eq!(out.1, "OVER");
    }

    // ---- who the target IS ----

    #[test]
    fn a_cells_identity_is_its_place_and_not_the_words_in_it() {
        // Two rows of one column are two targets, and the pointer moving
        // between them pays the delay again (or the grace window, which
        // is the same decision made on the id).
        assert_ne!(cell_key(1, 0, "1471"), cell_key(1, 0, "1472"));
        // The same row after a sort moved it: one target, still. The
        // identity is the ROW's key, which is what survives the reorder.
        assert_eq!(cell_key(1, 0, "1471"), cell_key(1, 0, "1471"));
        // A heading (no row) is not the cell under it, one column is not
        // its neighbour, and two views drawing the same cell are two
        // things to explain.
        assert_ne!(cell_key(1, 0, ""), cell_key(1, 0, "1471"));
        assert_ne!(cell_key(1, 0, "1471"), cell_key(1, 1, "1471"));
        assert_ne!(cell_key(1, 0, "1471"), cell_key(2, 0, "1471"));
    }

    #[test]
    fn a_target_that_keeps_its_identity_says_its_new_words_without_waiting_again() {
        // The model is rewritten under a resting pointer — a table
        // refreshes every frame, and a trimmed cell files its request
        // again with whatever it now holds. The place did not move, so
        // the delay is not paid twice and the box says the NEW text.
        let mut tips = Tooltips::new();
        let cell = cell_key(1, 2, "1471");
        assert!(frame(&mut tips, 0.0, Some((cell, "12.4 MB"))).is_none());
        let out = frame(&mut tips, 0.6, Some((cell, "12.9 MB"))).expect("due at the delay");
        assert_eq!(out.1, "12.9 MB");
        // And the row under it, reached at once, is a different target
        // with its own words.
        let next = cell_key(1, 2, "1472");
        let out = frame(&mut tips, 0.65, Some((next, "907 kB"))).expect("within linger");
        assert_eq!(out.1, "907 kB");
    }

    #[test]
    fn hover_files_only_when_the_pointer_is_inside() {
        // The pointer test is `Rect::contains`; the ids are the caller's.
        assert!(anchor().contains(11.0, 11.0));
        assert!(!anchor().contains(9.0, 11.0));
        assert_ne!(key("CPU"), key("RAM"));
        assert_eq!(key("CPU"), key("CPU"));
    }
}
