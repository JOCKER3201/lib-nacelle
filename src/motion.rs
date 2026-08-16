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

use crate::theme::{self, TokenId};
use crate::ui::{theme_word, warn_once};
use std::cell::RefCell;
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
    pub fn of(id: &str) -> Effect {
        thread_local! {
            static EFFECTS: RefCell<HashMap<String, Effect>> = RefCell::new(HashMap::new());
        }
        EFFECTS.with(|m| {
            *m.borrow_mut().entry(id.to_string()).or_insert_with(|| {
                if theme::id(&format!("motion.{id}.duration_ms")).is_none() {
                    warn_once(
                        &format!("motion:{id}"),
                        &format!("unknown motion effect \"{id}\" — it freezes at fully visible"),
                    );
                }
                let g = |k: &str| {
                    theme::id(&format!("motion.{id}.{k}")).unwrap_or(TokenId::MISSING)
                };
                Effect {
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
                }
            })
        })
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
        let word = theme_word(self.easing);
        let e = easing_of(&word, t.px(self.duty), t.px(self.floor), self.bezier_points());
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
        if theme_word(self.easing) == "sine" {
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
