//! Toaster (F2 §8.2): the transient notice at the top of the screen and
//! the queue behind it.
//!
//! This is the desktop's warning popup grown up. That popup held ONE
//! message: a second warning arriving a frame later replaced the first,
//! and the user never saw what was overwritten. The queue fixes that
//! without changing what a single toast looks like — `toast.max_visible`
//! ships at 1, so the master theme draws exactly the one box it always
//! drew, in exactly the same place, and a theme that wants a stack says
//! so.
//!
//! Everything visual comes from `[toast]` and `component.toast.*`, and
//! the frame itself is [`super::window::frame`] — the same call the
//! popup made, so the port is a move rather than a redrawing.
//!
//! Entry and exit animation is out of scope: it wants the property
//! animation engine a later phase brings, and `motion.toast_*` is the
//! place it will land. A toast appears and disappears, which is honest
//! rather than half-animated.

use crate::font::FONT_UI;
use crate::theme::{self, Color, TokenId};
use crate::ui::{self, Sev};
use crate::{Ctx, Rect};
use std::collections::VecDeque;
use std::sync::OnceLock;

fn tok(cell: &'static OnceLock<TokenId>, name: &'static str) -> TokenId {
    *cell.get_or_init(|| theme::id(name).unwrap_or(TokenId::MISSING))
}

/// The engine's colour, in the draw list's clothes.
fn col(c: theme::ThemeColor) -> Color {
    Color { r: c.r, g: c.g, b: c.b, a: c.a }
}

/// One notice.
#[derive(Clone)]
pub struct Toast {
    /// The severity this notice carries, if it carries one. It colours
    /// the title through `severity.<s>.text` — the master's own comment
    /// on `component.toast.title` says as much: the toast says WARNING
    /// and `[severity]` exists for exactly this.
    pub severity: Option<Sev>,
    /// The word at the top — `"WARNING"`, `"SAVED"`. The application's
    /// vocabulary, not the theme's.
    pub title: String,
    pub body: String,
    /// When the toast first became VISIBLE, in `Ctx::t` seconds; NaN
    /// until it has been drawn once.
    ///
    /// The clock starts at the first draw rather than at the push, so a
    /// toast that waited its turn in the queue still gets its full dwell
    /// — the whole point of queueing instead of overwriting. The menu's
    /// `opened_t` uses the same sentinel for the same reason.
    born: f64,
    /// This toast's own dwell in ms; None takes `toast.dwell_ms`.
    dwell_ms: Option<f32>,
}

impl Toast {
    pub fn new(title: &str, body: &str) -> Toast {
        Toast {
            severity: None,
            title: title.to_string(),
            body: body.to_string(),
            born: f64::NAN,
            dwell_ms: None,
        }
    }

    /// The warning the desktop has always shown: the word WARNING over
    /// the message, in `component.toast.title`.
    pub fn warning(body: String) -> Toast {
        Toast { severity: None, title: "WARNING".to_string(), body, born: f64::NAN, dwell_ms: None }
    }

    pub fn with_severity(mut self, s: Sev) -> Toast {
        self.severity = Some(s);
        self
    }

    /// Overrides `toast.dwell_ms` for this one notice.
    pub fn with_dwell_ms(mut self, ms: f32) -> Toast {
        self.dwell_ms = Some(ms);
        self
    }
}

/// The application's one toaster: what is on screen and what is waiting.
#[derive(Default)]
pub struct Toaster {
    queue: VecDeque<Toast>,
    /// The boxes the last [`Toaster::draw`] put on screen, in queue
    /// order. A click is answered against these rather than against a
    /// recomputed guess — the popup's own hit box was the minimum-width
    /// one, which missed the right end of every wider toast.
    shown: Vec<Rect>,
}

impl Toaster {
    pub fn new() -> Toaster {
        Toaster::default()
    }

    /// Queues a notice. FIFO, except that a notice identical to one
    /// already queued (same title AND body) only refreshes that one's
    /// dwell: an event repeating every frame must not build a wall of
    /// identical boxes, the same discipline `warn_once` keeps for the
    /// log.
    pub fn push(&mut self, t: Toast) {
        if let Some(dup) = self
            .queue
            .iter_mut()
            .find(|q| q.title == t.title && q.body == t.body)
        {
            // Restart the clock at the next draw: a repeat is a reason
            // to keep the notice up, not to show a second one.
            dup.born = f64::NAN;
            return;
        }
        self.queue.push_back(t);
    }

    /// Everything goes, on screen and queued.
    pub fn clear(&mut self) {
        self.queue.clear();
        self.shown.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    /// How many notices are on screen or waiting.
    pub fn len(&self) -> usize {
        self.queue.len()
    }

    /// Dismisses the toast the click landed on; true when one was hit.
    pub fn click(&mut self, x: f32, y: f32) -> bool {
        let hit = self.shown.iter().position(|r| r.contains(x, y));
        match hit {
            Some(i) if i < self.queue.len() => {
                self.queue.remove(i);
                self.shown.remove(i);
                true
            }
            _ => false,
        }
    }

    /// Retires whatever has outlived its dwell and starts the clock of
    /// whatever became visible — the arithmetic of the queue, with no
    /// drawing in it, so the ageing can be tested without a window.
    ///
    /// `max_visible` is how many stand on screen at once; only those
    /// age, which is what makes the queue a queue.
    fn age(&mut self, now: f64, dwell_ms: f32, max_visible: usize) {
        let mut i = 0;
        while i < self.queue.len().min(max_visible) {
            let t = &mut self.queue[i];
            if !t.born.is_finite() {
                t.born = now;
            }
            let dwell = t.dwell_ms.unwrap_or(dwell_ms);
            if ((now - t.born) * 1000.0) as f32 > dwell {
                // The one behind moves up and is born on this frame.
                self.queue.remove(i);
            } else {
                i += 1;
            }
        }
    }

    /// Draws the visible end of the queue, stacked downwards from
    /// `toast.top`.
    pub fn draw(&mut self, ctx: &mut Ctx) {
        static DWELL: OnceLock<TokenId> = OnceLock::new();
        static MIN_W: OnceLock<TokenId> = OnceLock::new();
        static MAX_W: OnceLock<TokenId> = OnceLock::new();
        static TH: OnceLock<TokenId> = OnceLock::new();
        static TOP: OnceLock<TokenId> = OnceLock::new();
        static PAD_X: OnceLock<TokenId> = OnceLock::new();
        static TITLE_GAP: OnceLock<TokenId> = OnceLock::new();
        static MSG_GAP: OnceLock<TokenId> = OnceLock::new();
        static TITLE_C: OnceLock<TokenId> = OnceLock::new();
        static TEXT_C: OnceLock<TokenId> = OnceLock::new();
        static MAX_VISIBLE: OnceLock<TokenId> = OnceLock::new();
        static GAP: OnceLock<TokenId> = OnceLock::new();
        static TITLE_ROLE: OnceLock<TokenId> = OnceLock::new();
        static BODY_ROLE: OnceLock<TokenId> = OnceLock::new();

        let t = theme::resolved();
        // A theme silencing every toast would be a broken application,
        // not a look: one is the floor.
        let max_visible = (t.px(tok(&MAX_VISIBLE, "toast.max_visible")) as i32).max(1) as usize;
        self.age(ctx.t, t.px(tok(&DWELL, "toast.dwell_ms")), max_visible);
        self.shown.clear();
        if self.queue.is_empty() {
            return;
        }

        // ---- metrics ----------------------------------------------------
        let title_role = ui::bound_role(&TITLE_ROLE, "toast.title.role");
        let body_role = ui::bound_role(&BODY_ROLE, "toast.body.role");
        let px = body_role.px(ctx, ctx.ui_font_scale);
        let title_px = title_role.px(ctx, ctx.ui_font_scale);
        let track = body_role.tracking_px(px);
        let title_track = title_role.tracking_px(title_px);
        let pad_x = t.px(tok(&PAD_X, "toast.pad_x"));
        let bh = t.px(tok(&TH, "toast.h"));
        let top = t.px(tok(&TOP, "toast.top"));
        let title_gap = t.px(tok(&TITLE_GAP, "toast.title_gap"));
        let msg_gap = t.px(tok(&MSG_GAP, "toast.msg_gap"));
        let title_ink = col(t.color(tok(&TITLE_C, "component.toast.title")));
        let body_ink = col(t.color(tok(&TEXT_C, "component.toast.text")));
        // Read only when the stack is on: at max_visible = 1 there is no
        // second box for a gap to sit between.
        let gap = if max_visible > 1 { t.px(tok(&GAP, "toast.gap")) } else { 0.0 };

        let n = self.queue.len().min(max_visible);
        for i in 0..n {
            let (title, body, sev) = {
                let toast = &self.queue[i];
                (toast.title.clone(), toast.body.clone(), toast.severity)
            };
            let text_w = ctx.fonts.measure(FONT_UI, px, &body, track);
            let bw = (text_w + 2.0 * pad_x)
                .max(ctx.w * t.px(tok(&MIN_W, "toast.min_w_frac")))
                .min(ctx.w * t.px(tok(&MAX_W, "toast.max_w_frac")));
            let bx = (ctx.w - bw) / 2.0;
            let by = top + i as f32 * (bh + gap);
            let r = Rect::new(bx, by, bw, bh);
            self.shown.push(r);

            super::window::frame(ctx, r);
            // A toast carrying a severity says so in the title's colour;
            // one that carries none keeps the theme's toast title.
            let ink = match sev {
                Some(s) => ui::sev_text(s),
                None => title_ink,
            };
            ctx.dl.text_center(
                ctx.fonts,
                FONT_UI,
                title_px,
                bx + bw / 2.0,
                by + title_gap,
                &title,
                ink,
                title_track,
            );
            ctx.dl.text_center(
                ctx.fonts,
                FONT_UI,
                px,
                bx + bw / 2.0,
                by + msg_gap,
                &body,
                body_ink,
                track,
            );
        }
    }
}

// ---------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const DWELL: f32 = 8000.0;

    fn t(body: &str) -> Toast {
        Toast::warning(body.to_string())
    }

    #[test]
    fn a_queued_toast_does_not_age_until_it_is_visible() {
        let mut ts = Toaster::new();
        ts.push(t("first"));
        ts.push(t("second"));
        ts.age(0.0, DWELL, 1);
        assert_eq!(ts.len(), 2);
        // Nine seconds later the first is long gone and the second has
        // only just started: it was never on screen before now.
        ts.age(9.0, DWELL, 1);
        assert_eq!(ts.len(), 1);
        assert_eq!(ts.queue[0].body, "second");
        assert_eq!(ts.queue[0].born, 9.0);
        ts.age(16.9, DWELL, 1);
        assert_eq!(ts.len(), 1);
        ts.age(17.1, DWELL, 1);
        assert!(ts.is_empty());
    }

    #[test]
    fn the_stack_ages_every_visible_toast_at_once() {
        let mut ts = Toaster::new();
        ts.push(t("a"));
        ts.push(t("b"));
        ts.push(t("c"));
        ts.age(0.0, DWELL, 3);
        assert_eq!(ts.len(), 3);
        ts.age(8.1, DWELL, 3);
        assert!(ts.is_empty());
    }

    #[test]
    fn an_identical_notice_refreshes_the_dwell_instead_of_stacking() {
        let mut ts = Toaster::new();
        ts.push(t("disk is full"));
        ts.age(0.0, DWELL, 1);
        ts.push(t("disk is full"));
        assert_eq!(ts.len(), 1);
        // The clock restarted, so the notice outlives its original dwell.
        ts.age(7.0, DWELL, 1);
        assert_eq!(ts.len(), 1);
        assert_eq!(ts.queue[0].born, 7.0);
        ts.age(14.9, DWELL, 1);
        assert_eq!(ts.len(), 1);
        ts.age(15.1, DWELL, 1);
        assert!(ts.is_empty());
    }

    #[test]
    fn a_different_body_is_a_different_notice() {
        let mut ts = Toaster::new();
        ts.push(t("one"));
        ts.push(t("two"));
        ts.push(Toast::new("SAVED", "one"));
        assert_eq!(ts.len(), 3);
    }

    #[test]
    fn a_toast_may_carry_its_own_dwell() {
        let mut ts = Toaster::new();
        ts.push(t("slow"));
        ts.push(t("quick").with_dwell_ms(500.0));
        ts.age(0.0, DWELL, 2);
        ts.age(0.6, DWELL, 2);
        assert_eq!(ts.len(), 1);
        assert_eq!(ts.queue[0].body, "slow");
    }

    #[test]
    fn a_click_dismisses_the_box_it_landed_on_and_nothing_else() {
        let mut ts = Toaster::new();
        ts.push(t("a"));
        ts.push(t("b"));
        ts.shown = vec![Rect::new(0.0, 0.0, 100.0, 20.0), Rect::new(0.0, 30.0, 100.0, 20.0)];
        assert!(!ts.click(200.0, 5.0));
        assert_eq!(ts.len(), 2);
        assert!(ts.click(50.0, 35.0));
        assert_eq!(ts.len(), 1);
        assert_eq!(ts.queue[0].body, "a");
    }

    #[test]
    fn a_click_on_nothing_drawn_hits_nothing() {
        let mut ts = Toaster::new();
        ts.push(t("a"));
        assert!(!ts.click(50.0, 5.0));
        assert_eq!(ts.len(), 1);
    }
}
