//! The scroll offset, its physics, and the bar that reports it.
//!
//! Generalised from the one scroll that exists today (the filesystem
//! widget's `scroll: f32`, its wheel handler, its clamp, its rounding to
//! whole rows and its hand-drawn overlay thumb). Everything here is
//! STATE and ARITHMETIC: nothing draws, so the tests need no window, and
//! the same state serves a host-side view and a plugin-side one.
//!
//! Behaviour is the theme's, not this file's: every number the physics
//! uses arrives in a [`ScrollPhysics`] and every number the bar uses
//! arrives in a [`ScrollbarLook`], both read from tokens ONCE per frame
//! by whoever owns the view (the `Look::read` pattern — a token read per
//! row would be a token read too many). [`ScrollPhysics::from_theme`]
//! and [`ScrollbarLook::from_theme`] do that reading on the host side;
//! a plugin fills the same structs from its own token cache across the
//! ABI.
//!
//! The master ships kinetics OFF (`scroll.fling_scale = 0`): a wheel
//! notch moves the offset and lands, exactly as it always has. A theme
//! that wants a flick turns it on and gets the glide, the settle and a
//! thumb that carries momentum — without a line of code changing.

use crate::theme::{self, TokenId};
use crate::view::surface::Surface;
use crate::view::virt;
use crate::Rect;
use std::sync::OnceLock;

fn tok(cell: &'static OnceLock<TokenId>, name: &'static str) -> TokenId {
    *cell.get_or_init(|| theme::id(name).unwrap_or(TokenId::MISSING))
}

/// Longest step the physics integrates in one go. A frame that took
/// longer than this did not happen — the window was unmapped, the
/// session was suspended — and integrating it would teleport the view.
/// A guard against the clock, not a look: no theme has an opinion on it.
const MAX_STEP: f32 = 0.1;

/// A glide is over once the whole remaining travel is under half a
/// device pixel: past that it can no longer move anything visible, and
/// running the exponential to zero would never end. Precision, not look.
const STOP_PX: f32 = 0.5;

/// Where a scroll container is allowed to come to rest.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Snap {
    /// Anywhere — a free offset, for content that is not a row list.
    None,
    /// On a row boundary of the given height: the filesystem widget's
    /// behaviour, and what a list without a clip has to do anyway.
    Row(f32),
}

/// The curve a settle runs on — the five words every `motion.*` effect's
/// `easing` enum takes, plus the enum's own linear fallback for a word
/// this build does not know.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Easing {
    Linear,
    EaseOut,
    EaseIn,
    EaseInOut,
    Sine,
    /// `t < duty ? floor : 1`. A step on a one-shot is a hard cut, which
    /// is a legitimate thing for a theme to ask for.
    Step { duty: f32, floor: f32 },
}

impl Easing {
    /// The eased 0..1 factor at linear progress `t01`.
    pub fn at(self, t01: f32) -> f32 {
        let t = t01.clamp(0.0, 1.0);
        match self {
            Easing::Linear => t,
            Easing::EaseOut => 1.0 - (1.0 - t) * (1.0 - t),
            Easing::EaseIn => t * t,
            Easing::EaseInOut => t * t * (3.0 - 2.0 * t),
            Easing::Sine => 0.5 - 0.5 * (std::f32::consts::PI * t).cos(),
            Easing::Step { duty, floor } => {
                if t >= duty {
                    1.0
                } else {
                    floor.clamp(0.0, 1.0)
                }
            }
        }
    }
}

/// Everything the physics reads from the theme.
///
/// Read it once per frame and pass it in; the view holds no tokens of
/// its own, which is what lets a plugin drive the identical physics from
/// values it fetched across the ABI.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollPhysics {
    /// `scroll.wheel_px` — one wheel notch, in pixels.
    pub wheel_px: f32,
    /// `scroll.fling_scale` — notches turned into velocity. `0` (the
    /// master's value) means the notch moves the offset directly and
    /// there is no kinetics at all. Velocity gained per notch is
    /// `wheel_px * fling_scale` px/s, so a theme that wants a real
    /// flick sets this well above 1.
    pub fling_scale: f32,
    /// `scroll.glide_halflife_ms` — how long the glide takes to lose
    /// half its speed. Read only when `fling_scale > 0`.
    pub glide_halflife_ms: f32,
    /// `motion.scroll_settle.duration_ms`, or 0 when that effect is
    /// disabled. Scaled by `motion_scale` where it is used.
    pub settle_ms: f32,
    /// `motion.scroll_settle.easing`.
    pub settle_easing: Easing,
    /// `motion.scale` — the global motion scale. Zero (reduced motion)
    /// freezes every glide and every settle: the offset jumps.
    pub motion_scale: f32,
}

impl ScrollPhysics {
    /// Read `scroll.*`, `motion.scroll_settle.*` and `motion.scale` from
    /// the active theme. Host side; a plugin builds the same struct from
    /// its own token ids.
    pub fn from_theme() -> Self {
        static WHEEL: OnceLock<TokenId> = OnceLock::new();
        static FLING: OnceLock<TokenId> = OnceLock::new();
        static HALFLIFE: OnceLock<TokenId> = OnceLock::new();
        static ENABLED: OnceLock<TokenId> = OnceLock::new();
        static DUR: OnceLock<TokenId> = OnceLock::new();
        static SCALE: OnceLock<TokenId> = OnceLock::new();
        let t = theme::resolved();
        let settle_ms = if t.flag(tok(&ENABLED, "motion.scroll_settle.enabled")) {
            t.px(tok(&DUR, "motion.scroll_settle.duration_ms"))
        } else {
            0.0
        };
        ScrollPhysics {
            wheel_px: t.px(tok(&WHEEL, "scroll.wheel_px")),
            fling_scale: t.px(tok(&FLING, "scroll.fling_scale")),
            glide_halflife_ms: t.px(tok(&HALFLIFE, "scroll.glide_halflife_ms")),
            settle_ms,
            settle_easing: settle_easing(),
            motion_scale: t.px(tok(&SCALE, "motion.scale")),
        }
    }
}

impl ScrollPhysics {
    /// The same numbers, read through a [`Surface`] — the one path that
    /// works on both sides of the plugin ABI, where a `TokenId` means
    /// nothing. Called once per frame, never per row.
    pub fn read(sf: &mut impl Surface) -> Self {
        let settle_ms = if sf.flag("motion.scroll_settle.enabled") {
            sf.px("motion.scroll_settle.duration_ms")
        } else {
            0.0
        };
        ScrollPhysics {
            wheel_px: sf.px("scroll.wheel_px"),
            fling_scale: sf.px("scroll.fling_scale"),
            glide_halflife_ms: sf.px("scroll.glide_halflife_ms"),
            settle_ms,
            settle_easing: settle_easing_on(sf),
            motion_scale: sf.px("motion.scale"),
        }
    }
}

/// [`settle_easing`] through a [`Surface`]: the word decides, exactly as
/// it does on the host, because a word is the one thing both sides of
/// the boundary can compare.
fn settle_easing_on(sf: &mut impl Surface) -> Easing {
    match sf.word("motion.scroll_settle.easing").as_str() {
        "ease_out" => Easing::EaseOut,
        "ease_in" => Easing::EaseIn,
        "ease_in_out" => Easing::EaseInOut,
        "sine" => Easing::Sine,
        "step" => Easing::Step {
            duty: sf.px("motion.scroll_settle.duty"),
            floor: sf.px("motion.scroll_settle.floor"),
        },
        _ => Easing::Linear,
    }
}

/// `motion.scroll_settle.easing`, resolved by word exactly as the board
/// ride's easing is (`deco::ride_ease`): the words are compared once at
/// init, the per-frame read is an index.
fn settle_easing() -> Easing {
    static EASING: OnceLock<TokenId> = OnceLock::new();
    static DUTY: OnceLock<TokenId> = OnceLock::new();
    static FLOOR: OnceLock<TokenId> = OnceLock::new();
    static WORDS: OnceLock<[Option<u16>; 5]> = OnceLock::new();
    let t = theme::resolved();
    let id = tok(&EASING, "motion.scroll_settle.easing");
    let w = WORDS.get_or_init(|| {
        ["ease_out", "ease_in", "ease_in_out", "sine", "step"]
            .map(|word| theme::enum_index(id, word))
    });
    let e = Some(t.enum_of(id));
    if e == w[0] {
        Easing::EaseOut
    } else if e == w[1] {
        Easing::EaseIn
    } else if e == w[2] {
        Easing::EaseInOut
    } else if e == w[3] {
        Easing::Sine
    } else if e == w[4] {
        Easing::Step {
            duty: t.px(tok(&DUTY, "motion.scroll_settle.duty")),
            floor: t.px(tok(&FLOOR, "motion.scroll_settle.floor")),
        }
    } else {
        Easing::Linear
    }
}

#[derive(Clone, Copy, Debug)]
struct Grab {
    /// Where inside the thumb the pointer took hold, so the thumb does
    /// not jump under the hand.
    inside: f32,
    /// The thumb's length when it was grabbed: the travel the pointer
    /// maps onto is the track minus this, and it must be the length that
    /// was actually DRAWN (`scrollbar.thumb_min` may have stretched it).
    thumb_h: f32,
}

#[derive(Clone, Copy, Debug)]
struct Settle {
    from: f32,
    to: f32,
    t0: f64,
    ms: f32,
}

/// One scrolled area: an offset in pixels, and what is being done to it.
#[derive(Clone, Debug)]
pub struct ScrollView {
    offset: f32,
    velocity: f32,
    last_t: f64,
    grab: Option<Grab>,
    last_move_t: f64,
    settle: Option<Settle>,
    /// The thumb was let go and the settle has not been started yet:
    /// [`ScrollView::release`] has no frame clock and no snap to start it
    /// with, so the next [`ScrollView::tick`] does.
    release_pending: bool,
}

impl Default for ScrollView {
    fn default() -> Self {
        Self::new()
    }
}

impl ScrollView {
    pub fn new() -> Self {
        ScrollView {
            offset: 0.0,
            velocity: 0.0,
            // NaN until the first tick, so no first frame ever integrates
            // the time since the epoch. `max` answers with the other
            // operand for NaN, which makes that first step exactly 0.
            last_t: f64::NAN,
            grab: None,
            // Never moved: an auto-hiding bar starts hidden.
            last_move_t: f64::NEG_INFINITY,
            settle: None,
            release_pending: false,
        }
    }

    /// Pixels from the top of the content.
    pub fn offset(&self) -> f32 {
        self.offset
    }

    /// Current glide speed in px/s — zero unless kinetics is on and a
    /// flick is in flight.
    pub fn velocity(&self) -> f32 {
        self.velocity
    }

    /// Is the thumb being dragged? (The thumb's class ladder wants to
    /// know: `scrollbar.thumb` has a `dragging` rung.)
    pub fn dragging(&self) -> bool {
        self.grab.is_some()
    }

    /// When the offset last changed, for [`ScrollView::fade_alpha`].
    pub fn moved_at(&self) -> f64 {
        self.last_move_t
    }

    /// Back to the top, with nothing in flight — a model change, not an
    /// interaction: a new directory, a new filter, a new sort.
    pub fn reset(&mut self) {
        self.offset = 0.0;
        self.velocity = 0.0;
        self.settle = None;
        self.grab = None;
        self.release_pending = false;
    }

    /// Put the offset somewhere directly (restoring a position, keeping
    /// a selected row in view). Clamped and snapped by the next tick.
    pub fn set_offset(&mut self, px: f32) {
        self.offset = px;
        self.velocity = 0.0;
        self.settle = None;
    }

    /// A wheel notch. Positive `notches` scrolls toward the END of the
    /// content (the offset grows), whichever way the platform spells its
    /// deltas.
    ///
    /// With `fling_scale = 0` — the master — the notch moves the offset
    /// directly and the next tick lands it on its snap boundary, with no
    /// animation whatsoever: `motion.scroll_settle` is for the END of a
    /// glide and for a released thumb, never for a direct move. A wheel
    /// that took 220 ms to answer would not be today's behaviour.
    pub fn wheel(&mut self, notches: f32, p: &ScrollPhysics, t: f64) {
        self.settle = None;
        self.last_move_t = t;
        if p.fling_scale > 0.0 && p.motion_scale > 0.0 {
            self.velocity += notches * p.wheel_px * p.fling_scale;
        } else {
            self.offset += notches * p.wheel_px;
            self.velocity = 0.0;
        }
    }

    /// A click on the track beside the thumb: one viewport toward it.
    /// Direct, like the wheel — a page is a jump, not a flick.
    pub fn page(&mut self, toward_end: bool, viewport: f32, t: f64) {
        self.settle = None;
        self.last_move_t = t;
        self.velocity = 0.0;
        self.offset += if toward_end { viewport } else { -viewport };
    }

    /// The pointer took hold of the thumb. `thumb` is the rectangle that
    /// was DRAWN — [`scrollbar`] returns it.
    pub fn press_thumb(&mut self, y: f32, thumb: Rect) -> bool {
        if thumb.h <= 0.0 || y < thumb.y || y >= thumb.y + thumb.h {
            return false;
        }
        self.grab = Some(Grab { inside: y - thumb.y, thumb_h: thumb.h });
        self.velocity = 0.0;
        self.settle = None;
        self.release_pending = false;
        true
    }

    /// The pointer moved while holding the thumb: the offset follows it
    /// absolutely — the thumb goes where the hand is, which is the only
    /// behaviour that survives a dropped frame.
    pub fn drag(&mut self, y: f32, viewport: f32, content: f32, track: Rect) {
        let Some(g) = self.grab else { return };
        let travel = (track.h - g.thumb_h).max(0.0);
        let max = (content - viewport).max(0.0);
        self.offset = if travel > 0.0 {
            ((y - g.inside - track.y) / travel).clamp(0.0, 1.0) * max
        } else {
            0.0
        };
        self.velocity = 0.0;
    }

    /// The pointer let the thumb go. The next tick settles onto the
    /// nearest legal stop through `motion.scroll_settle`.
    pub fn release(&mut self) {
        if self.grab.take().is_some() {
            self.release_pending = true;
        }
    }

    /// One frame of physics: decay a glide, run a settle, clamp to the
    /// content and come to rest on a legal stop.
    ///
    /// `motion.scale = 0` (reduced motion) freezes all of it — no glide,
    /// no settle, the offset simply is where it was put.
    pub fn tick(&mut self, t: f64, viewport: f32, content: f32, snap: Snap, p: &ScrollPhysics) {
        let dt = ((t - self.last_t).max(0.0) as f32).min(MAX_STEP);
        self.last_t = t;
        let max = (content - viewport).max(0.0);
        let frozen = p.motion_scale <= 0.0;

        // A held thumb owns the offset: nothing glides under the hand
        // and nothing snaps against it.
        if self.grab.is_some() {
            self.offset = self.offset.clamp(0.0, max);
            self.last_move_t = t;
            return;
        }

        // (b) of the settle contract: the thumb was just let go.
        if self.release_pending {
            self.release_pending = false;
            self.begin_settle(t, max, snap, p, frozen);
        }

        // A glide in flight. Reduced motion, or a theme with no
        // half-life, has none: the flick is over before it starts.
        if self.velocity != 0.0 {
            if frozen || p.glide_halflife_ms <= 0.0 {
                self.velocity = 0.0;
            } else {
                // Exponential decay integrated exactly over the step, so
                // the travel does not depend on the frame rate:
                //   v(t) = v0 * 2^(-t/h),  s = v0 * (1 - 2^(-dt/h)) * h/ln2
                let ln2 = std::f32::consts::LN_2;
                let decay = (-(dt * 1000.0) / p.glide_halflife_ms * ln2).exp();
                self.offset += self.velocity * (1.0 - decay) * p.glide_halflife_ms / (1000.0 * ln2);
                self.velocity *= decay;
                self.last_move_t = t;
                let rest = self.velocity.abs() * p.glide_halflife_ms / (1000.0 * ln2);
                // An edge only stops a glide that is pushing INTO it: a
                // flick that starts at the very top is a flick, not a
                // view that has already arrived.
                let edge = (self.offset <= 0.0 && self.velocity < 0.0)
                    || (self.offset >= max && self.velocity > 0.0);
                if rest >= STOP_PX && !edge {
                    self.offset = self.offset.clamp(0.0, max);
                    return; // still flying
                }
                // (a) of the settle contract: the glide is spent, or it
                // hit an edge. Land it.
                self.velocity = 0.0;
                self.begin_settle(t, max, snap, p, frozen);
            }
        }

        // A settle in flight.
        if let Some(s) = self.settle {
            let done = s.ms <= 0.0;
            let t01 = if done {
                1.0
            } else {
                (((t - s.t0) * 1000.0 / s.ms as f64).clamp(0.0, 1.0)) as f32
            };
            // Clamped every frame, not just at the end: the model may
            // shrink under a settle that is already in flight, and a
            // target computed against the old content would overshoot.
            self.offset = (s.from + (s.to - s.from) * p.settle_easing.at(t01)).clamp(0.0, max);
            self.last_move_t = t;
            if t01 >= 1.0 {
                self.offset = s.to.clamp(0.0, max);
                self.settle = None;
            }
            return;
        }

        // At rest: on a legal stop, immediately.
        self.offset = stop(self.offset, max, snap);
    }

    /// Start the settle onto the nearest legal stop, or land there at
    /// once when there is nothing to animate.
    fn begin_settle(&mut self, t: f64, max: f32, snap: Snap, p: &ScrollPhysics, frozen: bool) {
        let to = stop(self.offset, max, snap);
        let ms = if frozen { 0.0 } else { p.settle_ms * p.motion_scale };
        if ms <= 0.0 || (to - self.offset).abs() < STOP_PX {
            self.offset = to;
            self.settle = None;
        } else {
            self.settle = Some(Settle { from: self.offset, to, t0: t, ms });
        }
    }

    /// How visible an auto-hiding bar is at `now`: full while the view
    /// is moving, fading to nothing over `scrollbar.fade_ms` afterwards.
    /// A hovered or dragged bar is the caller's business — it holds the
    /// bar at 1 regardless.
    pub fn fade_alpha(&self, now: f64, auto_hide: bool, fade_ms: f32) -> f32 {
        if !auto_hide {
            return 1.0;
        }
        if fade_ms <= 0.0 {
            return if now <= self.last_move_t { 1.0 } else { 0.0 };
        }
        let age = ((now - self.last_move_t) * 1000.0).max(0.0) as f32;
        (1.0 - age / fade_ms).clamp(0.0, 1.0)
    }
}

/// The nearest offset the view is allowed to rest at.
///
/// Rows are the legal stops under [`Snap::Row`] — except for the very
/// end of the content, which stays reachable even when it is not a row
/// boundary: a viewport that is not a whole number of rows would
/// otherwise put the last row out of reach forever.
fn stop(offset: f32, max: f32, snap: Snap) -> f32 {
    let o = offset.clamp(0.0, max);
    match snap {
        Snap::Row(h) if h > 0.0 => {
            let s = virt::snap_offset(o, h);
            if s > max {
                max
            } else {
                s
            }
        }
        _ => o,
    }
}

// ------------------------------------------------------------- the bar

/// `scrollbar.mode` — whether the bar takes room from the content.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ScrollbarMode {
    Overlay,
    Inset,
    None,
}

/// `scrollbar.edge`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ScrollbarEdge {
    Right,
    Left,
}

/// Everything the bar's geometry reads from the theme, taken once per
/// frame beside [`ScrollPhysics`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollbarLook {
    pub mode: ScrollbarMode,
    /// `scrollbar.w` — the resting width.
    pub w: f32,
    /// `scrollbar.w_hover` — the width under the pointer.
    pub w_hover: f32,
    /// `scrollbar.margin` — gap between bar and content edge.
    pub margin: f32,
    /// `scrollbar.thumb_min` — shortest thumb, so a long list still
    /// shows one.
    pub thumb_min: f32,
    pub edge: ScrollbarEdge,
    /// `scrollbar.auto_hide`.
    pub auto_hide: bool,
    /// `scrollbar.fade_ms`.
    pub fade_ms: f32,
}

impl ScrollbarLook {
    /// Read `scrollbar.*` from the active theme. Host side; a plugin
    /// fills the same struct from its own token cache.
    pub fn from_theme() -> Self {
        static MODE: OnceLock<TokenId> = OnceLock::new();
        static MODE_WORDS: OnceLock<[Option<u16>; 3]> = OnceLock::new();
        static EDGE: OnceLock<TokenId> = OnceLock::new();
        static EDGE_LEFT: OnceLock<Option<u16>> = OnceLock::new();
        static W: OnceLock<TokenId> = OnceLock::new();
        static W_HOVER: OnceLock<TokenId> = OnceLock::new();
        static MARGIN: OnceLock<TokenId> = OnceLock::new();
        static THUMB_MIN: OnceLock<TokenId> = OnceLock::new();
        static AUTO_HIDE: OnceLock<TokenId> = OnceLock::new();
        static FADE: OnceLock<TokenId> = OnceLock::new();
        let t = theme::resolved();
        let mode_id = tok(&MODE, "scrollbar.mode");
        let words = MODE_WORDS
            .get_or_init(|| ["overlay", "inset", "none"].map(|w| theme::enum_index(mode_id, w)));
        let m = Some(t.enum_of(mode_id));
        let mode = if m == words[1] {
            ScrollbarMode::Inset
        } else if m == words[2] {
            ScrollbarMode::None
        } else {
            // The enum's own fallback: a bar that costs no layout is the
            // safe answer for a word this build does not know.
            ScrollbarMode::Overlay
        };
        let edge_id = tok(&EDGE, "scrollbar.edge");
        let left = *EDGE_LEFT.get_or_init(|| theme::enum_index(edge_id, "left"));
        ScrollbarLook {
            mode,
            w: t.px(tok(&W, "scrollbar.w")),
            w_hover: t.px(tok(&W_HOVER, "scrollbar.w_hover")),
            margin: t.px(tok(&MARGIN, "scrollbar.margin")),
            thumb_min: t.px(tok(&THUMB_MIN, "scrollbar.thumb_min")),
            edge: if Some(t.enum_of(edge_id)) == left {
                ScrollbarEdge::Left
            } else {
                ScrollbarEdge::Right
            },
            auto_hide: t.flag(tok(&AUTO_HIDE, "scrollbar.auto_hide")),
            fade_ms: t.px(tok(&FADE, "scrollbar.fade_ms")),
        }
    }

    /// The same numbers, read through a [`Surface`]. The two enum
    /// tokens are compared by WORD here, where the host compares
    /// indices: an index is meaningless across the ABI, and both spell
    /// the same question.
    pub fn read(sf: &mut impl Surface) -> Self {
        let mode = match sf.word("scrollbar.mode").as_str() {
            "inset" => ScrollbarMode::Inset,
            "none" => ScrollbarMode::None,
            // The enum's own fallback: a bar that costs no layout is the
            // safe answer for a word this build does not know.
            _ => ScrollbarMode::Overlay,
        };
        let edge = if sf.word("scrollbar.edge") == "left" {
            ScrollbarEdge::Left
        } else {
            ScrollbarEdge::Right
        };
        ScrollbarLook {
            mode,
            w: sf.px("scrollbar.w"),
            w_hover: sf.px("scrollbar.w_hover"),
            margin: sf.px("scrollbar.margin"),
            thumb_min: sf.px("scrollbar.thumb_min"),
            edge,
            auto_hide: sf.flag("scrollbar.auto_hide"),
            fade_ms: sf.px("scrollbar.fade_ms"),
        }
    }
}

/// Where the bar's groove and thumb go.
#[derive(Clone, Copy, Debug)]
pub struct ScrollbarGeom {
    /// The full length the thumb travels in.
    pub track: Rect,
    /// The thumb as it is to be drawn — and as
    /// [`ScrollView::press_thumb`] wants to be told about it.
    pub thumb: Rect,
}

/// The bar for `area`, or `None` when there is none to draw: mode
/// `none`, a content that fits, or a width of zero.
///
/// `hovered` picks `scrollbar.w_hover` over `scrollbar.w` — the pointer
/// being over the bar is the caller's finding, since only it knows what
/// else is under the pointer.
///
/// **The returned geometry describes the inside of `area` and nothing
/// else.** `inset` mode narrows the content box a widget lays its own
/// rows out in — the same kind of inside-the-widget geometry as padding
/// — and it must never be handed upward: panel geometry and the flex
/// engine are not the theme's to move.
pub fn scrollbar(
    area: Rect,
    look: &ScrollbarLook,
    offset: f32,
    viewport: f32,
    content: f32,
    hovered: bool,
) -> Option<ScrollbarGeom> {
    if look.mode == ScrollbarMode::None || content <= viewport || area.h <= 0.0 {
        return None;
    }
    let w = if hovered { look.w_hover } else { look.w };
    if w <= 0.0 || !w.is_finite() {
        return None;
    }
    let x = match look.edge {
        ScrollbarEdge::Right => area.right() - look.margin - w,
        ScrollbarEdge::Left => area.x + look.margin,
    };
    let track = Rect::new(x, area.y, w, area.h);
    // The thumb is as long a share of the track as the viewport is of
    // the content — the proportion the filesystem's overlay thumb has
    // always drawn — held to `thumb_min` so it stays grabbable.
    let frac = if content > 0.0 { (viewport / content).clamp(0.0, 1.0) } else { 1.0 };
    let th = (track.h * frac).max(look.thumb_min).min(track.h);
    let max = (content - viewport).max(0.0);
    let p = if max > 0.0 { (offset / max).clamp(0.0, 1.0) } else { 0.0 };
    let thumb = Rect::new(x, track.y + (track.h - th) * p, w, th);
    Some(ScrollbarGeom { track, thumb })
}

/// How much width an `inset` bar takes from the content box. Zero in
/// every other mode.
///
/// Same warning as [`scrollbar`]: this narrows the widget's OWN content
/// box, and goes no further up than that.
pub fn inset_w(look: &ScrollbarLook) -> f32 {
    if look.mode == ScrollbarMode::Inset {
        (look.w + 2.0 * look.margin).max(0.0)
    } else {
        0.0
    }
}

// ---------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const ROW: f32 = 25.0;

    /// The master's physics, written out: kinetics off, settle on.
    fn master() -> ScrollPhysics {
        ScrollPhysics {
            wheel_px: ROW * 3.0,
            fling_scale: 0.0,
            glide_halflife_ms: 160.0,
            settle_ms: 220.0,
            settle_easing: Easing::EaseOut,
            motion_scale: 1.0,
        }
    }

    fn kinetic() -> ScrollPhysics {
        ScrollPhysics { fling_scale: 4.0, ..master() }
    }

    // 40 rows of 25 px seen through 250 px: content 1000, max offset 750.
    const CONTENT: f32 = ROW * 40.0;
    const VIEWPORT: f32 = 250.0;

    #[test]
    fn a_wheel_notch_lands_at_once_when_kinetics_is_off() {
        let p = master();
        let mut v = ScrollView::new();
        v.wheel(1.0, &p, 1.0);
        v.tick(1.0, VIEWPORT, CONTENT, Snap::Row(ROW), &p);
        // Three rows, on the row boundary, in the same frame — no
        // 220 ms slide, which is the whole point of the settle contract.
        assert_eq!(v.offset(), ROW * 3.0);
        assert_eq!(v.velocity(), 0.0);
        // And it stays there for as long as nothing else happens.
        v.tick(2.0, VIEWPORT, CONTENT, Snap::Row(ROW), &p);
        assert_eq!(v.offset(), ROW * 3.0);
    }

    #[test]
    fn the_offset_clamps_to_the_content_at_both_ends() {
        let p = master();
        let mut v = ScrollView::new();
        v.wheel(-3.0, &p, 1.0);
        v.tick(1.0, VIEWPORT, CONTENT, Snap::Row(ROW), &p);
        assert_eq!(v.offset(), 0.0, "the top does not move past itself");
        for i in 0..40 {
            v.wheel(1.0, &p, 2.0 + i as f64);
            v.tick(2.0 + i as f64, VIEWPORT, CONTENT, Snap::Row(ROW), &p);
        }
        assert_eq!(v.offset(), CONTENT - VIEWPORT, "the end is the last stop");
    }

    #[test]
    fn a_partial_viewport_still_reaches_the_end_of_the_content() {
        // 250.5 px of viewport: the end is not on a row boundary, and a
        // strict snap would leave the last row unreachable.
        let p = master();
        let mut v = ScrollView::new();
        v.set_offset(10_000.0);
        v.tick(1.0, 250.5, CONTENT, Snap::Row(ROW), &p);
        assert_eq!(v.offset(), CONTENT - 250.5);
    }

    #[test]
    fn a_row_snap_lands_on_the_filesystem_s_row() {
        let p = master();
        for i in 0..30 {
            let raw = i as f32 * 11.0;
            let mut v = ScrollView::new();
            v.set_offset(raw);
            v.tick(1.0, VIEWPORT, CONTENT, Snap::Row(ROW), &p);
            let want = virt::snap_row(raw, ROW);
            assert_eq!(
                v.offset(),
                want as f32 * ROW,
                "offset {raw} should sit on the row round(offset/row_h) picks"
            );
            assert_eq!(virt::snap_row(v.offset(), ROW), want);
        }
    }

    #[test]
    fn without_a_snap_the_offset_is_left_where_it_was_put() {
        let p = master();
        let mut v = ScrollView::new();
        v.set_offset(137.0);
        v.tick(1.0, VIEWPORT, CONTENT, Snap::None, &p);
        assert_eq!(v.offset(), 137.0);
    }

    #[test]
    fn a_flick_decays_and_comes_to_rest_on_a_row() {
        let p = kinetic();
        let mut v = ScrollView::new();
        v.wheel(1.0, &p, 0.0);
        assert!(v.velocity() > 0.0, "kinetics turns the notch into speed");
        let mut prev_v = v.velocity();
        let mut prev_off = v.offset();
        let mut t = 0.0;
        for _ in 0..600 {
            t += 1.0 / 60.0;
            v.tick(t, VIEWPORT, CONTENT, Snap::Row(ROW), &p);
            assert!(v.velocity().abs() <= prev_v, "speed never grows on its own");
            assert!(v.offset() >= prev_off - 0.001, "a forward flick never goes back");
            assert!(v.offset() >= 0.0 && v.offset() <= CONTENT - VIEWPORT);
            prev_v = v.velocity().abs();
            prev_off = v.offset();
        }
        assert_eq!(v.velocity(), 0.0, "the glide ends");
        assert_eq!(v.offset(), virt::snap_offset(v.offset(), ROW), "and lands on a row");
        assert!(v.offset() > 0.0, "it did travel");
    }

    #[test]
    fn the_glide_does_not_depend_on_the_frame_rate() {
        let p = kinetic();
        let (mut fast, mut slow) = (ScrollView::new(), ScrollView::new());
        // Both clocks start at 0, then both are flicked and integrated
        // over the same tenth of a second, at two frame rates.
        fast.tick(0.0, VIEWPORT, CONTENT, Snap::None, &p);
        slow.tick(0.0, VIEWPORT, CONTENT, Snap::None, &p);
        fast.wheel(1.0, &p, 0.0);
        slow.wheel(1.0, &p, 0.0);
        for i in 1..=12 {
            fast.tick(i as f64 / 120.0, VIEWPORT, CONTENT, Snap::None, &p);
        }
        for i in 1..=6 {
            slow.tick(i as f64 / 60.0, VIEWPORT, CONTENT, Snap::None, &p);
        }
        assert!(
            (fast.offset() - slow.offset()).abs() < 0.5,
            "120 fps and 60 fps travel the same distance: {} vs {}",
            fast.offset(),
            slow.offset()
        );
    }

    #[test]
    fn reduced_motion_freezes_every_animation() {
        let p = ScrollPhysics { motion_scale: 0.0, ..kinetic() };
        let mut v = ScrollView::new();
        v.wheel(1.0, &p, 0.0);
        assert_eq!(v.velocity(), 0.0, "no flick under reduced motion");
        v.tick(0.001, VIEWPORT, CONTENT, Snap::Row(ROW), &p);
        assert_eq!(v.offset(), ROW * 3.0, "the notch simply arrives");
        // A released thumb lands in the same frame instead of sliding.
        let g = scrollbar(
            Rect::new(0.0, 0.0, 200.0, VIEWPORT),
            &look(),
            v.offset(),
            VIEWPORT,
            CONTENT,
            false,
        )
        .unwrap();
        assert!(v.press_thumb(g.thumb.y + 1.0, g.thumb));
        v.drag(g.thumb.y + 40.0, VIEWPORT, CONTENT, g.track);
        v.release();
        let landed = stop(v.offset(), CONTENT - VIEWPORT, Snap::Row(ROW));
        v.tick(0.002, VIEWPORT, CONTENT, Snap::Row(ROW), &p);
        assert_eq!(v.offset(), landed);
    }

    #[test]
    fn a_released_thumb_settles_over_the_motion_effect_s_duration() {
        let p = master();
        let mut v = ScrollView::new();
        let area = Rect::new(0.0, 0.0, 200.0, VIEWPORT);
        let l = look();
        let g = scrollbar(area, &l, 0.0, VIEWPORT, CONTENT, false).unwrap();
        assert!(v.press_thumb(g.thumb.y + 2.0, g.thumb));
        assert!(v.dragging());
        // Halfway down the track is halfway down the content.
        let travel = g.track.h - g.thumb.h;
        v.drag(g.track.y + 2.0 + travel * 0.5, VIEWPORT, CONTENT, g.track);
        assert!(
            (v.offset() - (CONTENT - VIEWPORT) * 0.5).abs() < 0.01,
            "the thumb maps the track onto the content"
        );
        // Let go three pixels further on, deliberately between rows.
        v.drag(g.track.y + 2.0 + travel * 0.5 + 3.0, VIEWPORT, CONTENT, g.track);
        let dropped = v.offset();
        assert_ne!(virt::snap_offset(dropped, ROW), dropped);
        v.release();
        assert!(!v.dragging());
        // The settle starts where the thumb was let go, is still on its
        // way a frame later, and is finished after the effect's
        // duration.
        let target = stop(dropped, CONTENT - VIEWPORT, Snap::Row(ROW));
        v.tick(1.0 / 60.0, VIEWPORT, CONTENT, Snap::Row(ROW), &p);
        assert_eq!(v.offset(), dropped, "no jump at the moment of release");
        v.tick(2.0 / 60.0, VIEWPORT, CONTENT, Snap::Row(ROW), &p);
        assert_ne!(v.offset(), dropped, "it has set off");
        assert_ne!(v.offset(), target, "and it slides rather than jumping");
        v.tick(0.5, VIEWPORT, CONTENT, Snap::Row(ROW), &p);
        assert_eq!(v.offset(), target);
        assert_eq!(virt::snap_offset(v.offset(), ROW), v.offset());
    }

    #[test]
    fn a_held_thumb_is_not_argued_with() {
        let p = master();
        let mut v = ScrollView::new();
        let l = look();
        let g = scrollbar(Rect::new(0.0, 0.0, 200.0, VIEWPORT), &l, 0.0, VIEWPORT, CONTENT, false)
            .unwrap();
        assert!(v.press_thumb(g.thumb.y, g.thumb));
        v.drag(g.thumb.y + 7.0, VIEWPORT, CONTENT, g.track);
        let held = v.offset();
        assert_ne!(virt::snap_offset(held, ROW), held, "deliberately between rows");
        v.tick(1.0, VIEWPORT, CONTENT, Snap::Row(ROW), &p);
        assert_eq!(v.offset(), held, "no snap under the hand");
    }

    #[test]
    fn a_model_that_shrinks_under_a_settle_does_not_overshoot() {
        // A directory refreshes to a tenth of its rows while the settle
        // that a released thumb started is still running.
        let p = master();
        let mut v = ScrollView::new();
        let l = look();
        let g = scrollbar(Rect::new(0.0, 0.0, 200.0, VIEWPORT), &l, 0.0, VIEWPORT, CONTENT, false)
            .unwrap();
        assert!(v.press_thumb(g.thumb.y, g.thumb));
        v.drag(g.track.y + g.track.h, VIEWPORT, CONTENT, g.track);
        assert_eq!(v.offset(), CONTENT - VIEWPORT);
        v.release();
        let smaller = ROW * 10.0;
        for t in [1.0 / 60.0, 0.1, 1.0] {
            v.tick(t, VIEWPORT, smaller, Snap::Row(ROW), &p);
            assert!(v.offset() <= smaller - VIEWPORT, "offset {} left the content", v.offset());
            assert!(v.offset() >= 0.0);
        }
    }

    #[test]
    fn a_page_moves_one_viewport() {
        let p = master();
        let mut v = ScrollView::new();
        v.page(true, VIEWPORT, 1.0);
        v.tick(1.0, VIEWPORT, CONTENT, Snap::Row(ROW), &p);
        assert_eq!(v.offset(), VIEWPORT);
        v.page(false, VIEWPORT, 2.0);
        v.tick(2.0, VIEWPORT, CONTENT, Snap::Row(ROW), &p);
        assert_eq!(v.offset(), 0.0);
    }

    #[test]
    fn a_long_stall_does_not_teleport_the_view() {
        let p = kinetic();
        let mut v = ScrollView::new();
        v.tick(0.0, VIEWPORT, CONTENT, Snap::None, &p);
        v.wheel(1.0, &p, 0.0);
        let travelled_in_one_step = {
            let mut probe = v.clone();
            probe.tick(MAX_STEP as f64, VIEWPORT, CONTENT, Snap::None, &p);
            probe.offset()
        };
        // The window was away for a minute: the step is capped, so the
        // frame it comes back on moves no further than a real one.
        v.tick(60.0, VIEWPORT, CONTENT, Snap::None, &p);
        assert!((v.offset() - travelled_in_one_step).abs() < 0.001);
    }

    #[test]
    fn an_auto_hiding_bar_fades_after_the_last_move() {
        let p = master();
        let mut v = ScrollView::new();
        assert_eq!(v.fade_alpha(0.0, true, 260.0), 0.0, "hidden until something moves");
        assert_eq!(v.fade_alpha(0.0, false, 260.0), 1.0, "auto_hide off keeps it up");
        v.wheel(1.0, &p, 10.0);
        assert_eq!(v.fade_alpha(10.0, true, 260.0), 1.0);
        assert!((v.fade_alpha(10.13, true, 260.0) - 0.5).abs() < 0.01, "half faded halfway");
        assert_eq!(v.fade_alpha(10.5, true, 260.0), 0.0);
    }

    // --------------------------------------------------------- the bar

    fn look() -> ScrollbarLook {
        ScrollbarLook {
            mode: ScrollbarMode::Overlay,
            w: 6.0,
            w_hover: 10.0,
            margin: 3.0,
            thumb_min: 30.0,
            edge: ScrollbarEdge::Right,
            auto_hide: true,
            fade_ms: 260.0,
        }
    }

    #[test]
    fn the_thumb_is_the_viewport_s_share_of_the_content() {
        let area = Rect::new(10.0, 20.0, 200.0, VIEWPORT);
        let l = look();
        let g = scrollbar(area, &l, 0.0, VIEWPORT, CONTENT, false).unwrap();
        assert_eq!((g.track.x, g.track.y, g.track.w, g.track.h), (201.0, 20.0, 6.0, VIEWPORT));
        assert_eq!(g.thumb.h, VIEWPORT * (VIEWPORT / CONTENT));
        assert_eq!(g.thumb.y, area.y, "at the top of the track at offset 0");
        let end = scrollbar(area, &l, CONTENT - VIEWPORT, VIEWPORT, CONTENT, false).unwrap();
        assert_eq!(end.thumb.y + end.thumb.h, area.y + area.h, "and at the bottom at the end");
    }

    #[test]
    fn a_very_long_list_still_shows_a_grabbable_thumb() {
        let area = Rect::new(0.0, 0.0, 200.0, VIEWPORT);
        let l = look();
        let g = scrollbar(area, &l, 0.0, VIEWPORT, ROW * 4000.0, false).unwrap();
        assert_eq!(g.thumb.h, l.thumb_min);
    }

    #[test]
    fn the_bar_is_absent_when_there_is_nothing_to_scroll() {
        let area = Rect::new(0.0, 0.0, 200.0, VIEWPORT);
        let l = look();
        assert!(scrollbar(area, &l, 0.0, VIEWPORT, VIEWPORT, false).is_none());
        assert!(scrollbar(area, &l, 0.0, VIEWPORT, 10.0, false).is_none());
        let off = ScrollbarLook { mode: ScrollbarMode::None, ..l };
        assert!(scrollbar(area, &off, 0.0, VIEWPORT, CONTENT, false).is_none());
        assert_eq!(inset_w(&off), 0.0);
    }

    #[test]
    fn hover_widens_the_bar_and_the_edge_moves_it() {
        let area = Rect::new(0.0, 0.0, 200.0, VIEWPORT);
        let l = look();
        let hot = scrollbar(area, &l, 0.0, VIEWPORT, CONTENT, true).unwrap();
        assert_eq!(hot.track.w, l.w_hover);
        assert_eq!(hot.track.x, area.right() - l.margin - l.w_hover);
        let left = ScrollbarLook { edge: ScrollbarEdge::Left, ..l };
        let g = scrollbar(area, &left, 0.0, VIEWPORT, CONTENT, false).unwrap();
        assert_eq!(g.track.x, area.x + left.margin);
    }

    #[test]
    fn only_an_inset_bar_takes_room_from_the_content() {
        let l = look();
        assert_eq!(inset_w(&l), 0.0, "an overlay costs the content nothing");
        let inset = ScrollbarLook { mode: ScrollbarMode::Inset, ..l };
        assert_eq!(inset_w(&inset), l.w + 2.0 * l.margin);
    }

    // ------------------------------------------------------- the theme

    #[test]
    fn the_master_ships_kinetics_off() {
        // The gate the whole feature hangs on: with the default theme
        // loaded, a wheel notch is a direct move and nothing glides —
        // which is what makes today's pixels today's pixels.
        let p = ScrollPhysics::from_theme();
        assert_eq!(p.fling_scale, 0.0);
        assert!(p.wheel_px > 0.0, "but a notch does move something");
        assert!(p.glide_halflife_ms > 0.0, "and a theme that turns it on has a half-life");
        assert_eq!(p.settle_easing, Easing::EaseOut);
        let l = ScrollbarLook::from_theme();
        assert_eq!(l.mode, ScrollbarMode::Overlay);
        assert_eq!(l.edge, ScrollbarEdge::Right);
        assert!(l.auto_hide);
        assert!(l.fade_ms > 0.0);
        assert!(l.w > 0.0 && l.w_hover > l.w && l.thumb_min > 0.0);
    }

    #[test]
    fn easing_words_all_run_from_zero_to_one() {
        for e in [
            Easing::Linear,
            Easing::EaseOut,
            Easing::EaseIn,
            Easing::EaseInOut,
            Easing::Sine,
            Easing::Step { duty: 0.5, floor: 0.0 },
        ] {
            assert_eq!(e.at(0.0), 0.0);
            assert_eq!(e.at(1.0), 1.0);
            assert!(e.at(-3.0) >= 0.0 && e.at(9.0) <= 1.0, "progress is clamped");
        }
    }
}
