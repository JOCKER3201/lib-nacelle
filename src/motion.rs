//! The shared motion resolver — ONE answer to "where is this animation
//! now", for every effect in `motion.*`'s closed catalogue (§5.22).
//!
//! Before this module existed the answer was copied three times: the
//! menu's unfold (`object/menu.rs`), the board ride (`deco.rs`) and the
//! scroll settle (`view/scroll.rs`) each carried their own easing table,
//! and the first two carried it over a cache of ENUM INDICES — the exact
//! pattern `scroll.rs` documents as broken: an index only names a word
//! against the schema it was interned in, so a theme swap froze both
//! curves at whatever the first theme meant. The one correct copy read
//! the WORD every time. This module is that copy, promoted: the header
//! of `deco.rs` promised "a shared motion resolver" and this is it.
//!
//! Three types, one contract each:
//!
//! * [`Easing`] — the closed set of curves a `motion.*.easing` word
//!   names, including `custom`'s cubic-bezier, which had no reader in
//!   Rust at all until here;
//! * [`Effect`] — the eight keys of one `motion.<id>` entry, memoised
//!   once per id, answering [`Effect::one_shot`] for transitions and
//!   [`Effect::cyclic`] for sources;
//! * [`Crossfade`] — a 0..1-ish property that runs a one-shot toward a
//!   target and can be retargeted mid-flight without jumping: the
//!   carrier the state fades (`hover`, `press`, `select`, `focus`,
//!   `disable`) will ride in the stones after this one.
//!
//! TIME IS A PARAMETER. Every entry point takes `now` (and `started`)
//! in seconds on the caller's clock — `Ctx.t` on the host, `elapsed`
//! across the plugin ABI. Nothing here calls `Instant::now()`, which is
//! what makes every consumer testable against a clock the test winds by
//! hand.
//!
//! THE FREEZE RULES, stated once:
//!
//! * a ONE-SHOT under `motion.scale <= 0`, a disabled effect, a zero
//!   duration or an id the master does not declare answers **1.0** —
//!   "already at the end state", never "never arrives". §5.22 spells it
//!   out for reduced motion: 0 is a JUMP to the end state, not a run in
//!   0 ms (the freeze-at-visible rule, first written beside the menu's
//!   unfold);
//! * a CYCLIC source under the same conditions freezes at **1.0** —
//!   fully visible: a caret that never returns is a usability failure
//!   and a separator that never returns is a content change;
//! * `sine` is legal on CYCLIC effects only (§5.22's own table). A
//!   one-shot whose theme writes `sine` runs `linear` and says so once —
//!   the two hand-rolled resolvers accepted it silently, which was a
//!   theme defect waiting to be authored.
//!
//! Out of scope, left here so the next stone finds it: an [`Effect`]
//! read through `view::Surface` for the plugin side (the
//! `ScrollPhysics::read` pattern), and `host.t_motion` for scripts
//! (§5.22: `clock.rhai` reads `host.t` raw and bypasses reduced motion).

use crate::theme::parse::State;
use crate::theme::{self, Color, TokenId};
use crate::ui::{warn_once, with_theme_word};
use crate::view::surface::StateInk;
use crate::Rect;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::OnceLock;

fn tok(cell: &'static OnceLock<TokenId>, name: &'static str) -> TokenId {
    *cell.get_or_init(|| theme::id(name).unwrap_or(TokenId::MISSING))
}

/// `motion.scale` — the global multiplier on every duration and period.
/// Zero (reduced motion) is the freeze, handled by the callers above.
fn motion_scale() -> f32 {
    static SCALE: OnceLock<TokenId> = OnceLock::new();
    theme::resolved().px(tok(&SCALE, "motion.scale"))
}

// ------------------------------------------------------------- the curves

/// The curve a `motion.*.easing` word names — §5.22's closed set, plus
/// the enum's own linear fallback for a word this build does not know.
///
/// Moved here from `view/scroll.rs`, which re-exports it so the type's
/// path in [`ScrollPhysics`] survives; [`Easing::Custom`] is new — the
/// `easing_p` cubic-bezier had no reader anywhere in Rust before this.
///
/// [`ScrollPhysics`]: crate::view::scroll::ScrollPhysics
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
    /// `easing_p`'s cubic-bezier `(x1, y1, x2, y2)`, solved with four
    /// Newton iterations (§5.22 names the count). Deliberately the
    /// awkward one, so it is not the default.
    Custom([f32; 4]),
}

impl Easing {
    /// The eased 0..1 factor at linear progress `t01`. `Custom` may
    /// legitimately answer outside 0..1 mid-run — an overshoot is the
    /// whole reason it exists — but its ends are anchored at 0 and 1.
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
            Easing::Custom(p) => bezier(p, t),
        }
    }
}

/// CSS's `cubic-bezier(x1, y1, x2, y2)` at progress `t`: find the curve
/// parameter `u` where the x-polynomial equals `t` (four Newton
/// iterations from `u = t`, per §5.22), then answer the y-polynomial
/// there.
///
/// The x control points are clamped into 0..1 — outside it the
/// x-polynomial stops being monotone and "the u where x(u) = t" stops
/// being one place. The y points are NOT clamped: overshoot is the
/// feature. Degenerate points (`x1 = x2 = 0` flattens the derivative
/// near 0) stall Newton rather than diverging — the step is skipped when
/// the derivative is too small to divide by — and a non-finite answer
/// degrades to linear, so no theme can put NaN into a colour's alpha.
fn bezier(p: [f32; 4], t: f32) -> f32 {
    if t <= 0.0 {
        return 0.0;
    }
    if t >= 1.0 {
        return 1.0;
    }
    let (x1, y1, x2, y2) = (p[0].clamp(0.0, 1.0), p[1], p[2].clamp(0.0, 1.0), p[3]);
    let x = |u: f32| 3.0 * (1.0 - u) * (1.0 - u) * u * x1 + 3.0 * (1.0 - u) * u * u * x2 + u * u * u;
    let dx = |u: f32| {
        3.0 * (1.0 - u) * (1.0 - u) * x1 + 6.0 * (1.0 - u) * u * (x2 - x1) + 3.0 * u * u * (1.0 - x2)
    };
    let mut u = t;
    for _ in 0..4 {
        let d = dx(u);
        if d.abs() < 1e-4 {
            break;
        }
        u = (u - (x(u) - t) / d).clamp(0.0, 1.0);
    }
    let y = 3.0 * (1.0 - u) * (1.0 - u) * u * y1 + 3.0 * (1.0 - u) * u * u * y2 + u * u * u;
    if y.is_finite() {
        y
    } else {
        t
    }
}

/// The curve a word names. ONE table, reached from every consumer: the
/// comparison is by WORD and it is made every time it is asked, because
/// enum indices cannot be cached across a theme swap (an index only
/// names a word against the schema it was interned in — the bug the
/// menu's and the board ride's private resolvers both carried).
///
/// `duty`/`floor` feed `step` and `bezier_p` feeds `custom`; the other
/// words ignore them.
pub fn easing_of(word: &str, duty: f32, floor: f32, bezier_p: [f32; 4]) -> Easing {
    match word {
        "ease_out" => Easing::EaseOut,
        "ease_in" => Easing::EaseIn,
        "ease_in_out" => Easing::EaseInOut,
        "sine" => Easing::Sine,
        "step" => Easing::Step { duty, floor },
        "custom" => Easing::Custom(bezier_p),
        _ => Easing::Linear,
    }
}

/// [`easing_of`] for a ONE-SHOT context: `sine` is cyclic-only (§5.22's
/// table), so a one-shot that asks for it runs linear and says so once.
/// `key` names the effect in the warning and keys the once.
pub fn one_shot_easing_of(word: &str, duty: f32, floor: f32, bezier_p: [f32; 4], key: &str) -> Easing {
    let e = easing_of(word, duty, floor, bezier_p);
    if e == Easing::Sine {
        warn_once(
            &format!("motion-sine:{key}"),
            &format!("motion.{key}.easing = sine — sine is cyclic-only (5.22); the one-shot runs linear"),
        );
        Easing::Linear
    } else {
        e
    }
}

// ------------------------------------------------------------- the effect

/// One `motion.<id>` entry: the eight token ids, memoised once per id
/// (the `blink_factor` pattern — a HashMap keyed by the effect's name,
/// filled on first ask). Ids are stable for the life of the process; the
/// VALUES behind them, the easing word included, are read per call, so a
/// theme swap moves every effect at once.
#[derive(Clone, Copy, Debug)]
pub struct Effect {
    duration_ms: TokenId,
    period_ms: TokenId,
    amplitude: TokenId,
    floor: TokenId,
    duty: TokenId,
    easing: TokenId,
    easing_p: [TokenId; 4],
    enabled: TokenId,
}

impl Effect {
    /// The effect named `motion.<id>`. An id outside the closed
    /// catalogue is reported once and every ask FREEZES AT VISIBLE
    /// (prohibition 6: an unknown effect is reported and ignored) —
    /// which falls out of the token fallbacks: a MISSING `enabled`
    /// reads `false`, and a disabled effect answers 1.0.
    ///
    /// A HIT ALLOCATES NOTHING. The map is keyed by `String` because it
    /// has to own the names it interned, but `HashMap<String, _>` looks
    /// up by `&str` — so the lookup comes first, alone, and the
    /// `to_string` happens once per id in the life of the process rather
    /// than once per ask. (`entry(id.to_string())` allocated on every
    /// call, including the ones a running fade makes per frame per
    /// control, which is the profile this memo exists to avoid.)
    pub fn of(id: &str) -> Effect {
        thread_local! {
            static EFFECTS: RefCell<HashMap<String, Effect>> = RefCell::new(HashMap::new());
        }
        if let Some(hit) = EFFECTS.with(|m| m.borrow().get(id).copied()) {
            return hit;
        }
        if theme::id(&format!("motion.{id}.duration_ms")).is_none() {
            warn_once(
                &format!("motion:{id}"),
                &format!("unknown motion effect \"{id}\" — it freezes at fully visible"),
            );
        }
        let g = |k: &str| theme::id(&format!("motion.{id}.{k}")).unwrap_or(TokenId::MISSING);
        let built = Effect {
            duration_ms: g("duration_ms"),
            period_ms: g("period_ms"),
            amplitude: g("amplitude"),
            floor: g("floor"),
            duty: g("duty"),
            easing: g("easing"),
            easing_p: [
                g("easing_p[0]"),
                g("easing_p[1]"),
                g("easing_p[2]"),
                g("easing_p[3]"),
            ],
            enabled: g("enabled"),
        };
        EFFECTS.with(|m| m.borrow_mut().insert(id.to_string(), built));
        built
    }

    /// The four `easing_p` control points, read from the live theme.
    fn bezier_points(&self) -> [f32; 4] {
        let t = theme::resolved();
        self.easing_p.map(|id| t.px(id))
    }

    /// The curve this effect's ONE-SHOT runs on, chosen by the live
    /// theme's WORD — never a cached index. `sine` is policed here.
    pub fn one_shot_easing(&self) -> Easing {
        let t = theme::resolved();
        // Borrowed, not cloned: this runs on every frame of every fade,
        // and a clone here is an allocation per control per frame.
        let e = with_theme_word(self.easing, |word| {
            easing_of(word, t.px(self.duty), t.px(self.floor), self.bezier_points())
        });
        if e == Easing::Sine {
            warn_once(
                &format!("motion-sine:#{}", self.easing.index()),
                "a one-shot motion effect asks for sine — sine is cyclic-only (5.22); it runs linear",
            );
            Easing::Linear
        } else {
            e
        }
    }

    /// The eased 0..1 progress of a one-shot begun at `started`, asked
    /// at `now` (both in seconds, the caller's clock — `Ctx.t`).
    ///
    /// Reduced motion (`motion.scale <= 0`), a disabled effect and a
    /// zero duration all answer **1.0 immediately**: already at the end
    /// state, never "never arrives" — the freeze-at-visible rule.
    pub fn one_shot(&self, started: f64, now: f64) -> f32 {
        let t = theme::resolved();
        let scale = motion_scale();
        if scale <= 0.0 || !t.flag(self.enabled) {
            return 1.0;
        }
        let dur = (t.px(self.duration_ms) * scale) as f64;
        if dur <= 0.0 {
            return 1.0;
        }
        let t01 = (((now - started) * 1000.0 / dur).clamp(0.0, 1.0)) as f32;
        self.one_shot_easing().at(t01)
    }

    /// The one-shot's full duration in SECONDS after the global scale,
    /// or 0.0 when the effect is off or frozen — for a host that
    /// integrates its own progress (the board ride's clock), where 0 is
    /// a hard cut.
    pub fn one_shot_secs(&self) -> f32 {
        let t = theme::resolved();
        if !t.flag(self.enabled) {
            return 0.0;
        }
        (t.px(self.duration_ms) * motion_scale() / 1000.0).max(0.0)
    }

    /// The one-shot curve applied to an EXTERNALLY-run linear progress
    /// `t01` — for a caller whose clock is not a start time (the board
    /// ride integrates a gesture as well as a timer).
    pub fn ease(&self, t01: f32) -> f32 {
        self.one_shot_easing().at(t01)
    }

    /// The 0..1 factor of a CYCLIC source at `now` — `blink_factor`'s
    /// semantics, generalised: frozen at **1.0** (fully visible) under
    /// reduced motion, a disabled effect or a zero period; otherwise the
    /// phase runs over `period_ms * motion.scale` and the curve is the
    /// step `phase < duty ? 1 : floor` — or a true sine for the one word
    /// that is legal ONLY here.
    ///
    /// A caller with a phase of its own (the caret restarts on every
    /// edit) passes `now - origin`; the modulo is safe on both sides of
    /// zero.
    pub fn cyclic(&self, now: f64) -> f32 {
        let t = theme::resolved();
        let scale = motion_scale();
        if scale <= 0.0 || !t.flag(self.enabled) {
            return 1.0;
        }
        let p = t.px(self.period_ms) * scale;
        if p <= 0.0 {
            return 1.0;
        }
        let phase = ((now * 1000.0).rem_euclid(p as f64) / p as f64) as f32;
        if with_theme_word(self.easing, |w| w == "sine") {
            0.5 - 0.5 * (std::f32::consts::TAU * phase).cos()
        } else if phase < t.px(self.duty) {
            1.0
        } else {
            t.px(self.floor).clamp(0.0, 1.0)
        }
    }

    /// `amplitude` — the ± swing of a cyclic source around its mean
    /// (`glow_pulse`). No consumer yet; the glow stone reads it here so
    /// the key does not grow a second resolver.
    pub fn amplitude(&self) -> f32 {
        theme::resolved().px(self.amplitude)
    }
}

// ---------------------------------------------------------- the crossfade

/// A property that FADES between values: the carrier for the state
/// transitions (`hover`, `press`, `select`, `focus`, `disable`) the
/// next stones wire up — today's consumers keep their own progress, but
/// the type lives here first so they all arrive on one shape.
///
/// The contract is retarget-without-jumping: however far along the
/// current fade is, [`Crossfade::retarget`] freezes that VALUE as the
/// new starting point and runs the effect toward the new target from
/// `now` — the pointer leaving mid-hover fades back from wherever the
/// fade had reached, not from 1.
///
/// Under reduced motion [`Effect::one_shot`] answers 1.0, so a sample IS
/// the target, immediately — a retarget during the freeze can never
/// strand the property half-way.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Crossfade {
    from: f32,
    target: f32,
    since: f64,
}

impl Crossfade {
    /// At rest at `value`: sampling answers `value` at any time — the
    /// start time is the infinite past, which every one-shot clamps to
    /// "finished".
    pub fn new(value: f32) -> Crossfade {
        Crossfade { from: value, target: value, since: f64::NEG_INFINITY }
    }

    /// Where the fade is heading (and where it rests once it arrives).
    pub fn target(&self) -> f32 {
        self.target
    }

    /// The property's value at `now`, fading under `effect`'s duration
    /// and curve. An overshooting `custom` curve legitimately passes the
    /// target on the way — that is what it was authored to do.
    pub fn sample(&self, effect: &Effect, now: f64) -> f32 {
        self.from + (self.target - self.from) * effect.one_shot(self.since, now)
    }

    /// Aim at a new target from wherever the fade stands at `now`. A
    /// retarget to the CURRENT target is a no-op — the fade in flight
    /// keeps flying, it is not restarted.
    pub fn retarget(&mut self, effect: &Effect, target: f32, now: f64) {
        if target == self.target {
            return;
        }
        self.from = self.sample(effect, now);
        self.target = target;
        self.since = now;
    }
}

// ------------------------------------------------------- the state fades
//
// Where the transition state LIVES, decided once, here.
//
// A control does not carry it. Almost every control in this toolkit is
// drawn by a free function handed a rectangle and a bool — `button::draw`,
// `checkbox::draw`, a list row inside `view::list` — and giving each of
// them somewhere to keep a fade would mean giving every CALLER somewhere
// to keep the control. That is the change this design exists to avoid:
// the settings window alone would grow a field per switch.
//
// So the fades live in ONE registry beside the resolver, keyed by the two
// things a caller already has: the interaction CLASS it is drawing on and
// the RECTANGLE it is drawing in — plus the SURFACE, which is ambient
// rather than passed, because the same content drawn once per screen is
// two controls and the call site has no idea which screen it is on
// ([`set_surface`]). Nothing is passed down and nothing is stored up; a
// call site gains one argument it was already holding.
//
// Three rules make that key honest:
//
// * **A key seen for the first time is born AT its rung**, settled, with
//   no fade owed. So a control that moved — a scrolling row, a dragged
//   thumb — simply appears in its state, exactly as it does today, rather
//   than fading in from wherever the pixel under it used to be.
// * **A transition needs a clock that moved.** Two asks at one `now` are
//   one frame asked twice, not a state change over time, and the second
//   jumps. This is what keeps a draw that is repeated within a frame — and
//   every test that draws at a fixed `t` — bit for bit what it was.
// * **An entry not asked about is dropped.** A control that left the
//   screen stops being re-seen, and the sweep below reclaims it; the map
//   is bounded by what is on screen, not by what has ever been on screen.
//
// AND NOTHING HERE STARTS A CLOCK. At rest a track is the surface's
// thread-local read, the viewport's atomic load, one hash lookup and a
// handful of compares — no token resolved, no allocation, no redraw
// asked for, nothing that takes the theme engine's lock. The host learns
// that it owes another frame by asking [`pending`], which answers false
// the moment the last fade lands. The desktop's 100 % CPU fault was a
// per-frame reload that nothing asked for; a fade that costs nothing
// until something changes cannot repeat it — and neither may the sweep
// that keeps its map, which is why the one below is rate-limited rather
// than run at every ask.

/// A control's place between the rungs of its ladder: a weight per rung,
/// summing to 1.
///
/// Usually one rung has all of it — that is what "settled" means, and it
/// is the answer at rest. During a fade the weight moves from whatever
/// mixture the control was showing to the rung it is heading for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mix {
    w: [f32; State::ALL.len()],
    to: State,
    settled: bool,
}

impl Mix {
    /// The mixture that is one rung and nothing else.
    fn pure(to: State) -> Mix {
        let mut w = [0.0; State::ALL.len()];
        w[to as usize] = 1.0;
        Mix { w, to, settled: true }
    }

    /// Whether the control is standing still on [`Mix::target`]. A settled
    /// mix is the signal to skip the blend entirely and read the one rung,
    /// which is what makes a resting frame identical to today's.
    pub fn is_settled(&self) -> bool {
        self.settled
    }

    /// The rung the control is heading for — and standing on, once
    /// [`Mix::is_settled`] answers true.
    pub fn target(&self) -> State {
        self.to
    }

    /// How much of `rung` is showing, 0..1.
    pub fn weight(&self, rung: State) -> f32 {
        self.w[rung as usize]
    }
}

/// One tracked control: the mixture it was showing when its last fade
/// began, the rung that fade is heading for, and the clock either side.
#[derive(Clone, Copy)]
struct Track {
    /// The mixture frozen at the moment the current fade started. This is
    /// what makes an interrupted fade continue from where it stood
    /// instead of jumping: the fade is always "from this mixture to that
    /// rung", and a mixture can hold an unfinished fade of its own.
    base: [f32; State::ALL.len()],
    to: State,
    /// When the current fade began, and when it will be over. Both
    /// `NEG_INFINITY` while the track is settled, which is what lets the
    /// resting path answer without reading a single token.
    since: f64,
    ends: f64,
    effect: &'static str,
    /// The last `now` this key was asked about — the sweep's evidence
    /// that the control is still on screen, and the clock-moved test.
    seen: f64,
}

impl Track {
    fn born(to: State, now: f64) -> Track {
        let mut base = [0.0; State::ALL.len()];
        base[to as usize] = 1.0;
        Track {
            base,
            to,
            since: f64::NEG_INFINITY,
            ends: f64::NEG_INFINITY,
            effect: "hover",
            seen: now,
        }
    }

    /// Snap to `to` with no fade — a first sighting, or a change asked for
    /// at an instant the clock has not left.
    fn jump(&mut self, to: State) {
        self.base = [0.0; State::ALL.len()];
        self.base[to as usize] = 1.0;
        self.to = to;
        self.since = f64::NEG_INFINITY;
        self.ends = f64::NEG_INFINITY;
    }

    /// How far the current fade has run at `now`, 1.0 once it is over.
    ///
    /// The end time is remembered so a settled track costs no token read
    /// at all; a theme swap that LENGTHENS a running effect is answered by
    /// the old end, which lands one fade early and never one fade late.
    fn progress(&self, now: f64) -> f32 {
        if self.ends <= now {
            return 1.0;
        }
        Effect::of(self.effect).one_shot(self.since, now)
    }

    fn mix_at(&self, p: f32) -> Mix {
        if p >= 1.0 {
            return Mix::pure(self.to);
        }
        let mut w = self.base;
        for v in w.iter_mut() {
            *v *= 1.0 - p;
        }
        w[self.to as usize] += p;
        Mix { w, to: self.to, settled: false }
    }

    /// The rung carrying most of the current mixture — the one the
    /// transition is fairly described as coming FROM, which is what picks
    /// the effect.
    fn dominant(&self) -> State {
        let mut best = self.to;
        let mut best_w = f32::MIN;
        for s in State::ALL {
            if self.base[s as usize] > best_w {
                best_w = self.base[s as usize];
                best = s;
            }
        }
        best
    }
}

/// Which `motion.<id>` entry a move between two rungs runs under.
///
/// The catalogue is closed and it names the STATES, not the pairs, so the
/// pair has to choose: the rung that CHANGED is the one that names the
/// effect, and when two changed at once the slower, more consequential
/// change wins. Disabled is the outermost — a control leaving the world
/// is a bigger event than the pointer arriving — then press (and its
/// sustained form, dragging), then selection, and hover last, which is
/// both the commonest and the quickest.
fn effect_for(from: State, to: State) -> &'static str {
    let pressed = |s: State| matches!(s, State::Press | State::Dragging);
    let chosen = |s: State| matches!(s, State::Selected | State::SelectedHover);
    if from == State::Disabled || to == State::Disabled {
        "disable"
    } else if pressed(from) != pressed(to) {
        "press"
    } else if chosen(from) != chosen(to) {
        "select"
    } else {
        "hover"
    }
}

/// The identity of one drawn control: the SURFACE it is drawn on, its
/// class, and the box it occupies, rounded to whole pixels.
///
/// A hash rather than a `String`, because this is asked once per control
/// per frame and a key that allocates is a key that shows up in a
/// profile. Two controls of one class cannot share a box, and a control
/// that MOVED is deliberately a different key — see the born-settled rule
/// above.
///
/// # WHY THE SURFACE IS PART OF IT
///
/// The desktop draws THE SAME CONTENT ONCE PER SCREEN. Class and box
/// alone therefore name one entry for what are two controls: on a
/// two-monitor desk the same button stands in the same rectangle on both
/// screens, and only one of them has the pointer. Frame after frame the
/// registry would be told `Hover` and then `Idle` about one track, so the
/// fade would turn round on every frame and never settle — the same
/// shape of fault as the control panel's hit boxes in August 2026, where
/// one slot served two screens.
///
/// The two words are kept SEPARATE rather than folded into one. Mixing
/// them saves eight bytes and buys a question that can only be answered
/// with a collision argument — and "two screens occasionally share one
/// fade" is a bug nobody would ever manage to report.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Key {
    surface: u64,
    viewport: u64,
    class: u64,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}

thread_local! {
    /// The surface the current frame is being drawn onto, as the HOST
    /// names it. Zero — "the only surface there is" — until a host says
    /// otherwise, which is what every single-window embedder and every
    /// test wants.
    static SURFACE: Cell<u64> = const { Cell::new(0) };
}

/// Names the surface the frames that FOLLOW are drawn onto, and answers
/// what it was, so a caller can put the old one back.
///
/// # What to pass, and why this is the host's word to give
///
/// The identity has to be STABLE between two frames of one screen and
/// DIFFERENT between two screens. Nothing inside the toolkit can supply
/// both halves:
///
/// * a control's rectangle is in the SCREEN's own coordinates, so two
///   monitors give the same numbers for the same control;
/// * `Ctx.w`/`Ctx.h` are the window's size — equal for two identical
///   monitors, and equal again for one monitor before and after a
///   sibling resizes;
/// * [`theme::epoch`] names which BAKE is published, and on a
///   mixed-height desktop it alternates every frame by design — the
///   reason `content_epoch` had to be split off it after the 100 % CPU
///   fault. It is a cache key, never a screen's name.
///
/// [`theme::viewport_key`] is the one thing the toolkit can read that is
/// right about the case the fault was found in — two screens of unequal
/// height are two viewports — and it is folded in below without anyone
/// asking. But two IDENTICAL monitors bake identically, so it cannot
/// separate them, and identical monitors are the ordinary desk. That
/// last step needs a word only the host has.
///
/// Pass anything stable per screen and distinct between screens: a hash
/// of the connector name (`DP-1`, `eDP-1` — nacelle-desktop's
/// `Screen::connector`, which is documented there as A SCREEN'S
/// IDENTITY, surviving unplugging and reordering), or, failing a
/// connector, the screen's index in the host's own list.
///
/// **NO CALLER IN THIS REPOSITORY.** It is called by the host, once, at
/// the top of the frame — in nacelle-desktop that is `draw_screen` in
/// `src/main.rs`, beside its existing `theme::set_viewport(h, …)`, which
/// is already the one place in that program certain to be looking at one
/// named screen. Stone 3 wires it.
pub fn set_surface(id: u64) -> u64 {
    SURFACE.with(|c| c.replace(id))
}

/// Which surface [`set_surface`] last named on this thread.
pub fn surface() -> u64 {
    SURFACE.with(|c| c.get())
}

fn key_of(class: &str, r: Rect) -> Key {
    // FNV-1a: a few bytes of a short name, no allocation, no dependency.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in class.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    Key {
        // The host's word for the screen, and the engine's word for the
        // viewport: EITHER one differing is a different control. The
        // viewport half costs one atomic load — no lock, no resolve —
        // and it is what makes a mixed-height desktop right before any
        // host has been taught to call [`set_surface`]. It also means a
        // resize re-keys every track, which is the born-settled rule
        // doing exactly what it is for: a screen whose every rectangle
        // just moved has no fades worth carrying over.
        surface: SURFACE.with(|c| c.get()),
        viewport: theme::viewport_key(),
        class: h,
        x: r.x.round() as i32,
        y: r.y.round() as i32,
        w: r.w.round() as i32,
        h: r.h.round() as i32,
    }
}

/// How long an unseen entry is kept before the sweep reclaims it. Long
/// enough that a control hidden for a frame or two keeps its fade, short
/// enough that a scrolled-away list does not sit in memory.
const KEEP_SECS: f64 = 0.5;
/// The smallest map the size guard will act on. Below this the clock is
/// the only thing that sweeps: a handful of entries is not worth walking
/// for, however fast they churn.
const SWEEP_FLOOR: usize = 2048;

/// Everything the fades keep between frames: the tracks, the scalar
/// gates, when the map was last swept, and when the last fade in flight
/// will land.
struct Fades {
    tracks: HashMap<Key, Track>,
    gates: HashMap<Key, Gate>,
    swept: f64,
    /// The size at which the next sweep is forced whatever the clock
    /// says. Raised to twice what SURVIVED the last sweep, so a map that
    /// is legitimately large is walked on the clock like any other and
    /// never twice for the same crowd. See [`Fades::sweep`].
    sweep_at: usize,
    /// The latest `ends` of anything started — what [`pending`] answers
    /// against. It is an upper bound, never a promise that something is
    /// still moving, so a host that redraws one frame too many is the
    /// worst it can cause.
    until: f64,
}

impl Fades {
    fn new() -> Fades {
        Fades {
            tracks: HashMap::new(),
            gates: HashMap::new(),
            swept: f64::NEG_INFINITY,
            sweep_at: SWEEP_FLOOR,
            until: f64::NEG_INFINITY,
        }
    }
}

thread_local! {
    static FADES: RefCell<Fades> = RefCell::new(Fades::new());
}

impl Fades {
    /// Drops what is no longer on screen. Called on EVERY ask, so what it
    /// costs at rest is what the registry costs at rest: two compares.
    ///
    /// # The size guard is a HIGH-WATER MARK, not a ceiling
    ///
    /// It was `len() >= 2048`, and that is a trap rather than a guard: a
    /// map over the mark that is entirely FRESH — every entry drawn this
    /// very frame, which is precisely what a screenful of 2 048 controls
    /// looks like — retains everything, keeps its length, and so trips
    /// the same test on the next ask, and the next. The result is a full
    /// walk of the map per control per frame: quadratic work in the one
    /// case the guard was written for, on a program with a history of
    /// spending 100 % of a core on a per-frame job nobody asked for.
    ///
    /// So a sweep now RAISES the mark to twice what survived it. Between
    /// two size-triggered sweeps the map must therefore double, which
    /// makes the walk amortised O(1) per insertion however a host
    /// misbehaves; and the mark comes back down the moment a sweep
    /// actually reclaims something. The clock keeps its own trigger — the
    /// real one, since a control that leaves the screen stops being seen
    /// rather than stops existing — and a host whose `now` never moves
    /// still cannot be reclaimed FROM (every entry looks fresh forever),
    /// which no sweep policy can fix and which the doubling at least
    /// stops charging by the frame.
    fn sweep(&mut self, now: f64) {
        let by_clock = now - self.swept >= KEEP_SECS;
        if !by_clock && self.tracks.len() + self.gates.len() < self.sweep_at {
            return;
        }
        self.tracks.retain(|_, t| now - t.seen < KEEP_SECS);
        self.gates.retain(|_, g| now - g.seen < KEEP_SECS);
        self.swept = now;
        let live = self.tracks.len() + self.gates.len();
        self.sweep_at = live.saturating_mul(2).max(SWEEP_FLOOR);
    }
}

/// Where the control drawn as `class` in `r` stands on its ladder at
/// `now`, having been asked for rung `to`.
///
/// The one entry point to the registry, and the one place a transition
/// begins. Callers that need the INK rather than the weights want
/// [`state_ink`], which is written in terms of this.
pub fn state_mix(class: &str, r: Rect, to: State, now: f64) -> Mix {
    FADES.with(|cell| {
        let mut f = cell.borrow_mut();
        f.sweep(now);
        let key = key_of(class, r);
        let fades = &mut *f;
        // A first sighting is born AT its rung — settled, no fade owed —
        // and then falls through the same arithmetic as every other ask.
        let track = fades.tracks.entry(key).or_insert_with(|| Track::born(to, now));
        if track.to != to {
            if now > track.seen {
                // A real frame boundary: freeze what is showing and fade
                // from THERE. `p` is 0 at this instant, so the mixture
                // answered below is the one the last frame drew — that is
                // the no-jump contract, and it holds however often the
                // target turns round mid-flight.
                let p = track.progress(now);
                track.base = track.mix_at(p).w;
                let effect = effect_for(track.dominant(), to);
                track.to = to;
                track.since = now;
                track.effect = effect;
                track.ends = now + Effect::of(effect).one_shot_secs() as f64;
            } else {
                track.jump(to);
            }
        }
        track.seen = now;
        let p = track.progress(now);
        let mix = track.mix_at(p);
        let ends = track.ends;
        if p >= 1.0 {
            // Arrived: collapse, so every later frame takes the resting
            // path and reads no tokens at all.
            track.jump(to);
        }
        if ends > now {
            fades.until = fades.until.max(ends);
        }
        mix
    })
}

/// The ink a control is drawn in at `now`: its ladder's rung, or the
/// blend of the rungs it stands between while a fade runs.
///
/// `rung` is the CALLER's answer to "what does this class look like in
/// that state", which is the whole reason this takes a closure rather
/// than reading the class itself. A list row at rest draws no plate at
/// all — the master's `idle.fill` is not transparent, and reading it
/// would put a wash under every resting row — so `view::list` hands in a
/// clear Idle and keeps the pixels it has always drawn. A button hands in
/// the ladder unchanged. Both fade between exactly what they would
/// otherwise have snapped between, which is the promise: **only the time
/// of arrival is animated, never the colours arrived at.**
///
/// A settled mix reads ONE rung and returns it untouched — so at rest,
/// under `motion.scale = 0`, and under a disabled effect, the ink is bit
/// for bit the ink of the build without any of this.
pub fn state_ink(
    class: &str,
    r: Rect,
    to: State,
    now: f64,
    mut rung: impl FnMut(State) -> StateInk,
) -> StateInk {
    let mix = state_mix(class, r, to, now);
    if mix.is_settled() {
        return rung(to);
    }
    // A mixture that is still ONE rung — the instant a fade sets off, and
    // the instant one is turned round — answers that rung's ink UNTOUCHED.
    // The blend below is arithmetic, and arithmetic on a colour the theme
    // wrote (`r * a / a` is not `r`) is a colour the theme did not write.
    if let Some(s) = State::ALL.into_iter().find(|s| mix.weight(*s) >= 1.0) {
        if s == to {
            return rung(to);
        }
        let mut out = rung(s);
        out.elevation = rung(to).elevation;
        return out;
    }
    let mut acc = StateInk::CLEAR;
    // Colours are accumulated PREMULTIPLIED and divided out at the end.
    // A straight-alpha average of a transparent rung and a solid one
    // drags the solid one toward the transparent one's RGB — which is
    // black, and a hover that fades in through a bruise is worse than no
    // fade at all.
    let mut prem = [[0.0f32; 4]; 4];
    for s in State::ALL {
        let w = mix.weight(s);
        if w <= 0.0 && s != to {
            continue;
        }
        let ink = rung(s);
        if s == to {
            // A rank is an INDEX into `elev.*`, not a length: half of
            // rank 1 and half of rank 2 is not rank 1.5, it is nothing.
            // The material the control is heading for is the material it
            // is made of the instant the move begins.
            acc.elevation = ink.elevation;
        }
        if w <= 0.0 {
            continue;
        }
        for (i, c) in [ink.fill, ink.edge, ink.text, ink.glyph].into_iter().enumerate() {
            prem[i][0] += c.r * c.a * w;
            prem[i][1] += c.g * c.a * w;
            prem[i][2] += c.b * c.a * w;
            prem[i][3] += c.a * w;
        }
        acc.edge_width += ink.edge_width * w;
        acc.glow_radius += ink.glow_radius * w;
        acc.glow_alpha += ink.glow_alpha * w;
    }
    let out = prem.map(unpremultiply);
    acc.fill = out[0];
    acc.edge = out[1];
    acc.text = out[2];
    acc.glyph = out[3];
    acc
}

/// `a` at 0, `b` at 1, mixed the way [`state_ink`] mixes a rung — for a
/// pair of colours that are NOT two rungs of one class.
///
/// The window controls are why it is public: their glyph wears
/// `component.window_control.idle` and `.hover` (and `.close_hover`,
/// which is the whole reason the pair is not a ladder), so the plate's
/// ring fades on the class and the glyph inside it has to fade on the
/// same clock or the two come apart mid-hover.
///
/// The ENDS ARE EXACT. A theme's colour must arrive as the theme wrote
/// it, not as `a + (b - a) * 1.0` happened to round.
pub fn mix_color(a: Color, b: Color, t: f32) -> Color {
    if t <= 0.0 {
        return a;
    }
    if t >= 1.0 {
        return b;
    }
    unpremultiply([
        a.r * a.a * (1.0 - t) + b.r * b.a * t,
        a.g * a.a * (1.0 - t) + b.g * b.a * t,
        a.b * a.a * (1.0 - t) + b.b * b.a * t,
        a.a * (1.0 - t) + b.a * t,
    ])
}

/// A premultiplied accumulator back to the straight-alpha colour the draw
/// list takes. Nothing showing is nothing — a transparent colour has no
/// hue to preserve.
///
/// The alpha is CLAMPED, and only the alpha: an `easing = custom` whose
/// control points overshoot is doing what it was authored to do, and an
/// overshoot is meaningful for a position but not for a coverage. The
/// hue is left alone, because it is the ratio the division already
/// normalised.
fn unpremultiply(c: [f32; 4]) -> Color {
    if c[3] <= 0.0 {
        return Color::TRANSPARENT;
    }
    Color { r: c[0] / c[3], g: c[1] / c[3], b: c[2] / c[3], a: c[3].min(1.0) }
}

// ------------------------------------------------------------- the gates

/// One tracked 0..1 property that is not a ladder at all — the focus
/// ring's presence being the first of them.
struct Gate {
    fade: Crossfade,
    effect: &'static str,
    ends: f64,
    seen: f64,
}

/// A 0..1 property that fades in and out under `motion.<effect>` — the
/// carrier for the signals that are NOT rungs of the state ladder.
///
/// Focus is the reason it exists. §5.21 is explicit that focus is not a
/// ladder rung: the ring is an overlay around the control, drawn or not
/// drawn. "Or not drawn" is exactly a 0..1 property, and `motion.focus`
/// has been sitting in the closed catalogue with no reader since it was
/// written. `name` and `r` identify the gate the way a class and a box
/// identify a track, and the three rules at the top of this section hold
/// here too — born at its value, a jump when the clock has not moved, and
/// swept when it stops being asked about.
///
/// [`Crossfade`] is what runs it, which is what that type was built for.
pub fn gate(name: &str, r: Rect, on: bool, effect: &'static str, now: f64) -> f32 {
    let target = if on { 1.0 } else { 0.0 };
    FADES.with(|cell| {
        let mut f = cell.borrow_mut();
        f.sweep(now);
        let key = key_of(name, r);
        let fades = &mut *f;
        let g = fades.gates.entry(key).or_insert_with(|| Gate {
            fade: Crossfade::new(target),
            effect,
            ends: f64::NEG_INFINITY,
            seen: now,
        });
        if g.fade.target() != target {
            if now > g.seen {
                let e = Effect::of(effect);
                g.fade.retarget(&e, target, now);
                g.effect = effect;
                g.ends = now + e.one_shot_secs() as f64;
            } else {
                g.fade = Crossfade::new(target);
                g.ends = f64::NEG_INFINITY;
            }
        }
        g.seen = now;
        let ends = g.ends;
        let v = if ends <= now {
            // Landed: no token read, and the value is the target exactly.
            g.fade = Crossfade::new(target);
            g.ends = f64::NEG_INFINITY;
            target
        } else {
            g.fade.sample(&Effect::of(g.effect), now)
        };
        if ends > now {
            fades.until = fades.until.max(ends);
        }
        v
    })
}

// ------------------------------------------------------------ the host's two

/// Whether any fade started so far will still be moving at `now` — "the
/// host owes one more frame".
///
/// FALSE AT REST, which is the point: nothing in this module asks for a
/// redraw, so a screen where nothing changed is drawn exactly as often as
/// it is today. An upper bound, never a promise: a fade retargeted to
/// where it already stood, or one landing early under a shortened effect,
/// can leave this true for a few frames more than strictly necessary.
///
/// # NOBODY IN THIS REPOSITORY CALLS IT, and that is not an oversight
///
/// This is an API FOR THE HOST, and libnacelle has no frame loop to be
/// its reader: the toolkit is called to draw, it never decides when.
/// Stating it plainly is the point — a seam quietly assumed to be wired
/// is worse than one openly waiting.
///
/// It is also not load-bearing yet, which is why stone 2 could land
/// without it. nacelle-desktop drives an unconditional 60 Hz cadence
/// (`src/main.rs`, the `next_frame`/`FRAME` block that calls
/// `request_redraw` on every screen), so today every fade gets its
/// frames whether or not anyone asks. The day that loop learns to sleep
/// when nothing is moving — stone 3's business — this is the one
/// question it has to put to the registry before it does, and the tests
/// in `tests/state_fades.rs` already pin the answer at both ends.
///
/// Until then the only readers are those tests.
pub fn pending(now: f64) -> bool {
    FADES.with(|cell| now < cell.borrow().until)
}

/// How many controls the registry is tracking — for the test that proves
/// a control which left the screen leaves nothing behind. Not a number
/// any drawing code has business reading.
pub fn tracked() -> usize {
    FADES.with(|cell| {
        let f = cell.borrow();
        f.tracks.len() + f.gates.len()
    })
}

/// Forgets every fade in flight, as if nothing had been drawn yet.
///
/// For tests, and for a host that has just torn a world down: the next
/// ask is a first sighting, so it is born settled and nothing fades in
/// from a screen that no longer exists.
pub fn forget_fades() {
    FADES.with(|cell| {
        *cell.borrow_mut() = Fades::new();
    });
}

// ---------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Every curve is anchored at 0 and 1 and clamps its progress —
    /// `Custom` included, which is the new arrival.
    #[test]
    fn every_easing_word_runs_from_zero_to_one() {
        for e in [
            Easing::Linear,
            Easing::EaseOut,
            Easing::EaseIn,
            Easing::EaseInOut,
            Easing::Sine,
            Easing::Step { duty: 0.5, floor: 0.0 },
            Easing::Custom([0.25, 0.10, 0.25, 1.00]),
        ] {
            assert_eq!(e.at(0.0), 0.0);
            assert_eq!(e.at(1.0), 1.0);
            assert!(e.at(-3.0) >= 0.0 && e.at(9.0) <= 1.0, "progress is clamped");
        }
    }

    /// The words pick the spec's formulas (§5.22), and an unknown word
    /// is the enum's own linear fallback.
    #[test]
    fn the_word_table_matches_the_spec() {
        let p = [0.25, 0.10, 0.25, 1.00];
        assert_eq!(easing_of("linear", 0.0, 0.0, p), Easing::Linear);
        assert_eq!(easing_of("ease_out", 0.0, 0.0, p), Easing::EaseOut);
        assert_eq!(easing_of("ease_in", 0.0, 0.0, p), Easing::EaseIn);
        assert_eq!(easing_of("ease_in_out", 0.0, 0.0, p), Easing::EaseInOut);
        assert_eq!(easing_of("sine", 0.0, 0.0, p), Easing::Sine);
        assert_eq!(easing_of("step", 0.4, 0.1, p), Easing::Step { duty: 0.4, floor: 0.1 });
        assert_eq!(easing_of("custom", 0.0, 0.0, p), Easing::Custom(p));
        assert_eq!(easing_of("bounce", 0.0, 0.0, p), Easing::Linear);
        let t = 0.25;
        assert_eq!(Easing::EaseOut.at(t), 1.0 - (1.0 - t) * (1.0 - t));
        assert_eq!(Easing::EaseIn.at(t), t * t);
        assert_eq!(Easing::EaseInOut.at(t), t * t * (3.0 - 2.0 * t));
        assert_eq!(Easing::Sine.at(t), 0.5 - 0.5 * (std::f32::consts::PI * t).cos());
    }

    /// `sine` on a one-shot is policed to linear; every other word
    /// passes through untouched.
    #[test]
    fn a_one_shot_refuses_sine() {
        let p = [0.25, 0.10, 0.25, 1.00];
        assert_eq!(one_shot_easing_of("sine", 0.0, 0.0, p, "test"), Easing::Linear);
        assert_eq!(one_shot_easing_of("ease_out", 0.0, 0.0, p, "test"), Easing::EaseOut);
        assert_eq!(one_shot_easing_of("custom", 0.0, 0.0, p, "test"), Easing::Custom(p));
    }

    /// The default `easing_p` is monotone and finite across the run —
    /// four Newton iterations are enough for the shipped points.
    #[test]
    fn the_default_custom_curve_is_monotone() {
        let e = Easing::Custom([0.25, 0.10, 0.25, 1.00]);
        let mut prev = 0.0f32;
        for i in 0..=100 {
            let v = e.at(i as f32 / 100.0);
            assert!(v.is_finite());
            assert!(v >= prev - 1e-3, "custom curve dipped at {i}: {v} < {prev}");
            prev = v;
        }
    }

    /// Degenerate control points stall Newton instead of feeding NaN
    /// into an alpha: every answer is finite, whatever the points.
    #[test]
    fn a_degenerate_bezier_never_answers_nan() {
        for p in [
            [0.0, 0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0, 1.0],
            [0.0, 5.0, 1.0, -5.0],
            [1.0, 1.0, 1.0, 1.0],
            [f32::NAN, 0.0, 0.5, 1.0],
        ] {
            for i in 0..=20 {
                let v = Easing::Custom(p).at(i as f32 / 20.0);
                assert!(v.is_finite(), "points {p:?} answered {v} at step {i}");
            }
        }
    }

    /// Which `motion.<id>` a move between two rungs runs under: the rung
    /// that CHANGED names it, and the outermost change wins when two
    /// moved at once.
    #[test]
    fn the_rung_that_moved_names_the_effect() {
        assert_eq!(effect_for(State::Idle, State::Hover), "hover");
        assert_eq!(effect_for(State::Hover, State::Idle), "hover");
        assert_eq!(effect_for(State::Selected, State::SelectedHover), "hover");
        assert_eq!(effect_for(State::Hover, State::Press), "press");
        assert_eq!(effect_for(State::Dragging, State::Idle), "press");
        assert_eq!(effect_for(State::Press, State::Dragging), "hover", "both are pressed");
        assert_eq!(effect_for(State::Idle, State::Selected), "select");
        assert_eq!(effect_for(State::SelectedHover, State::Hover), "select");
        // Disabled is the outermost rule: it wins whatever else moved.
        assert_eq!(effect_for(State::Press, State::Disabled), "disable");
        assert_eq!(effect_for(State::Disabled, State::SelectedHover), "disable");
    }

    /// The identity of a drawn control is its class and its box, rounded
    /// to whole pixels: a control that moved a hair is the same control,
    /// one that moved a row is not, and two classes never collide in one
    /// box.
    #[test]
    fn the_key_is_the_class_and_the_box() {
        // The key carries the live viewport, so make sure the engine has
        // published one before any two keys are compared: a theme loading
        // on another test's thread halfway through would otherwise be a
        // difference this test never asked about.
        let _ = theme::resolved();
        let r = Rect::new(10.0, 20.0, 30.0, 40.0);
        assert_eq!(key_of("button", r), key_of("button", Rect::new(10.2, 19.8, 30.0, 40.1)));
        assert_ne!(key_of("button", r), key_of("list.item", r));
        assert_ne!(key_of("button", r), key_of("button", Rect::new(10.0, 40.0, 30.0, 40.0)));
        assert_ne!(key_of("button", r), key_of("button", Rect::new(10.0, 20.0, 31.0, 40.0)));
        // A degenerate rectangle is a key like any other, never a panic.
        let _ = key_of("button", Rect::new(f32::NAN, 0.0, -1.0, f32::INFINITY));
    }

    /// …and the SURFACE, which is the half a call site cannot see. The
    /// desktop draws one board once per screen, so without this the same
    /// button in the same rectangle on two monitors is one entry, told
    /// `Hover` by the screen under the pointer and `Idle` by the other
    /// one, every frame, forever.
    ///
    /// The surface is thread-local, so this test cannot disturb another;
    /// it puts back what it found all the same.
    #[test]
    fn the_key_names_the_surface_too() {
        let _ = theme::resolved();
        let r = Rect::new(10.0, 20.0, 30.0, 40.0);
        let outer = set_surface(0);
        let a = key_of("button", r);
        let was = set_surface(0x4450_2d31); // "DP-1", as a host might hash it
        assert_eq!(was, 0, "set_surface did not answer what it replaced");
        assert_eq!(surface(), 0x4450_2d31);
        let b = key_of("button", r);
        assert_ne!(a, b, "one class in one box on two screens is one entry");
        // Same class, same box, same screen: the same control again.
        assert_eq!(key_of("button", r), b);
        set_surface(0);
        assert_eq!(key_of("button", r), a, "the first screen's key did not come back");
        set_surface(outer);
    }

    /// The size guard is a HIGH-WATER MARK. A map over the mark whose
    /// entries are all FRESH cannot be reclaimed from, so sweeping it
    /// again on the next ask is pure cost — and the next ask is the next
    /// control, this frame. One sweep per crowd, and the clock keeps its
    /// own trigger.
    ///
    /// Mutation: restore `len() >= SWEEP_FLOOR` as the guard and the
    /// stale entry planted below is gone by the second ask.
    #[test]
    fn a_full_map_of_fresh_entries_is_swept_once_not_per_ask() {
        let _ = theme::resolved();
        let mut f = Fades::new();
        for i in 0..SWEEP_FLOOR + 10 {
            f.tracks.insert(
                key_of("sweep.test", Rect::new(i as f32, 0.0, 1.0, 1.0)),
                Track::born(State::Idle, 10.0),
            );
        }
        f.swept = 10.0;
        // Over the mark and inside the interval: swept for its size, and
        // nothing is reclaimed because everything was drawn this instant.
        f.sweep(10.0);
        assert_eq!(f.tracks.len(), SWEEP_FLOOR + 10, "a fresh entry was reclaimed");
        assert!(f.sweep_at >= 2 * f.tracks.len(), "the mark was not raised past the crowd");
        // The second ask at the same instant must not walk the map again.
        let stale = key_of("sweep.test", Rect::new(0.0, 0.0, 1.0, 1.0));
        f.tracks.get_mut(&stale).expect("the planted entry").seen = 0.0;
        f.sweep(10.0);
        assert!(f.tracks.contains_key(&stale), "the size guard swept twice for one crowd");
        // The clock is the real trigger, and it does reclaim.
        f.sweep(10.0 + KEEP_SECS);
        assert!(!f.tracks.contains_key(&stale), "the clock's sweep kept a stale entry");
        // A sweep that reclaims brings the mark back down toward the floor.
        f.tracks.clear();
        f.sweep(20.0);
        assert_eq!(f.sweep_at, SWEEP_FLOOR, "the mark never came down");
    }

    /// A mixture is a weighting: it sums to one whatever the progress,
    /// and it is the base at 0 and the target at 1.
    #[test]
    fn a_mixture_always_sums_to_one() {
        let mut t = Track::born(State::Idle, 0.0);
        t.base = [0.25, 0.75, 0.0, 0.0, 0.0, 0.0, 0.0];
        t.to = State::Press;
        for i in 0..=10 {
            let m = t.mix_at(i as f32 / 10.0);
            let sum: f32 = State::ALL.into_iter().map(|s| m.weight(s)).sum();
            assert!((sum - 1.0).abs() < 1e-5, "the weights lost mass at {i}: {sum}");
        }
        assert_eq!(t.mix_at(0.0).weight(State::Hover), 0.75);
        assert!(t.mix_at(1.0).is_settled());
        assert_eq!(t.mix_at(1.0).weight(State::Press), 1.0);
        assert_eq!(t.dominant(), State::Hover, "the heavier rung is the one it came from");
    }

    /// The blend's ENDS are exact — a theme's colour arrives as the theme
    /// wrote it — and the middle keeps its hue: mixing something with
    /// nothing must not drag it toward black, which is what a straight
    /// average of an RGBA pair does.
    #[test]
    fn a_mix_keeps_its_ends_and_its_hue() {
        let cyan = Color { r: 0.0, g: 0.9, b: 1.0, a: 1.0 };
        let none = Color::TRANSPARENT;
        assert_eq!(mix_color(none, cyan, 0.0), none);
        assert_eq!(mix_color(none, cyan, 1.0), cyan);
        assert_eq!(mix_color(none, cyan, -3.0), none);
        assert_eq!(mix_color(none, cyan, 9.0), cyan);
        let half = mix_color(none, cyan, 0.5);
        assert!((half.a - 0.5).abs() < 1e-6, "alpha did not halve: {}", half.a);
        assert!(half.g == cyan.g && half.b == cyan.b && half.r == cyan.r, "the hue moved");
        // Two solid colours average the ordinary way.
        let red = Color { r: 1.0, g: 0.0, b: 0.0, a: 1.0 };
        let mid = mix_color(red, cyan, 0.5);
        assert!((mid.r - 0.5).abs() < 1e-6 && (mid.g - 0.45).abs() < 1e-6 && mid.a == 1.0);
    }

    /// A key nobody has seen is born settled on the rung it was asked
    /// for, and a second ask AT THE SAME INSTANT is one frame asked
    /// twice — a jump, not a transition. Both answers are reached
    /// without reading a token, which is what this can assert without a
    /// theme.
    #[test]
    fn a_first_sighting_and_a_still_clock_both_settle() {
        forget_fades();
        let r = Rect::new(0.0, 0.0, 7.0, 7.0);
        let m = state_mix("motion.test.born", r, State::Hover, 3.0);
        assert!(m.is_settled() && m.target() == State::Hover);
        let m = state_mix("motion.test.born", r, State::Press, 3.0);
        assert!(m.is_settled() && m.target() == State::Press, "one instant, two rungs, no fade");
        let m = state_mix("motion.test.born", r, State::Idle, 2.5);
        assert!(m.is_settled(), "a clock that went backwards is not a frame boundary");
        assert!(!pending(3.0), "a settled registry asked the host for a frame");
        forget_fades();
        assert_eq!(tracked(), 0);
    }

    /// §5.22's header states an ARITHMETIC — so many effects, so many
    /// keys each — and a header that states arithmetic is a header that
    /// can be wrong. It was: it read `18 effects x 8 keys + 2 globals =
    /// 146` while the body declared 152, because `board_ride` carries six
    /// numbers no other entry has and the shape's multiplication does not
    /// know about them.
    ///
    /// The six are NOT a leak in a closed catalogue. Every one of them has
    /// a reader — `perspective`, `gesture_frac`, `rubber_gain`,
    /// `rubber_max` and `epsilon` in nacelle-desktop's `main.rs`, `void` in
    /// this crate's `deco.rs` — and they are named in the table below by
    /// hand, so growing a seventh means editing this test and saying why.
    ///
    /// Read from the FILE, not from a resolved theme: the baker fills
    /// every declared token whether the file wrote it or not, so a
    /// resolved theme cannot tell a key the master declares from a key the
    /// schema invented. The count is a fact about the document.
    #[test]
    fn the_catalogue_is_closed_and_counted() {
        /// The eight keys §5.22 says EVERY entry carries.
        const SHAPE: [&str; 8] = [
            "duration_ms",
            "period_ms",
            "amplitude",
            "floor",
            "duty",
            "easing",
            "easing_p",
            "enabled",
        ];
        /// The one entry that carries more, and exactly what more.
        const RIDE_EXTRAS: [&str; 6] = [
            "perspective",
            "gesture_frac",
            "rubber_gain",
            "rubber_max",
            "epsilon",
            "void",
        ];
        /// `[motion]` itself: the two globals, which are not an effect.
        const GLOBALS: [&str; 2] = ["scale", "idle_cap"];

        let mut section = String::new();
        let mut seen: Vec<(String, Vec<String>)> = Vec::new();
        for line in crate::theme::master_source().lines() {
            let s = line.trim();
            if let Some(rest) = s.strip_prefix('[') {
                section = rest.split(']').next().unwrap_or_default().to_string();
                if section == "motion" || section.starts_with("motion.") {
                    seen.push((section.clone(), Vec::new()));
                }
                continue;
            }
            if s.starts_with('#') || !s.contains('=') {
                continue;
            }
            if section == "motion" || section.starts_with("motion.") {
                let key = s.split('=').next().unwrap_or_default().trim().to_string();
                seen.last_mut().expect("a key outside every section").1.push(key);
            }
        }

        let globals = seen.iter().find(|(s, _)| s == "motion").expect("[motion] itself");
        assert_eq!(globals.1, GLOBALS, "the two globals are scale and idle_cap");
        let effects: Vec<_> = seen.iter().filter(|(s, _)| s != "motion").collect();
        assert_eq!(effects.len(), 18, "the catalogue is CLOSED at eighteen effects");

        for (name, keys) in &effects {
            let id = name.strip_prefix("motion.").expect("an effect is motion.<id>");
            let mut want: Vec<&str> = SHAPE.to_vec();
            if id == "board_ride" {
                want.extend(RIDE_EXTRAS);
            }
            assert_eq!(
                keys.iter().map(String::as_str).collect::<Vec<_>>(),
                want,
                "motion.{id} does not carry §5.22's keys, in §5.22's order"
            );
        }

        let total: usize = seen.iter().map(|(_, k)| k.len()).sum();
        assert_eq!(
            total,
            18 * 8 + RIDE_EXTRAS.len() + GLOBALS.len(),
            "the catalogue's key count moved; §5.22's header says 152"
        );
        assert_eq!(total, 152, "and 152 is the number the header prints");
        assert!(
            crate::theme::master_source().contains("+ 2 globals = 152"),
            "the header's arithmetic no longer matches the body it describes"
        );
    }

    /// Retargeting mid-flight does not move the sampled value at the
    /// moment of the retarget — the pure-arithmetic half of the contract
    /// (the themed half runs in `tests/motion_effects.rs`, where the
    /// process may own the resolved theme).
    #[test]
    fn a_crossfade_at_rest_is_its_target() {
        let cf = Crossfade::new(0.4);
        assert_eq!(cf.target(), 0.4);
        // No theme is loaded here and none is needed: `from == target`
        // makes the sample the target whatever the effect answers.
        let cf2 = Crossfade { from: 0.7, target: 0.7, since: 0.0 };
        assert_eq!(cf2.target(), 0.7);
    }
}
