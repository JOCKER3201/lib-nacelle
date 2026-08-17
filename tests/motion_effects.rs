//! The shared motion resolver against the LIVE theme: `crate::motion`'s
//! pure arithmetic is proven in its own unit tests; what this binary
//! proves is the seam between the resolver and `motion.*`'s tokens —
//! the freeze rules, the word table read from a THEME, the `easing_p`
//! reader, the global scale on a cyclic period, and the crossfade's
//! no-jump retarget under a real effect.
//!
//! Time is a PARAMETER everywhere below — `one_shot(started, now)`,
//! `cyclic(now)` — so every clock in this file is a literal the test
//! winds by hand. Nothing sleeps and nothing reads `Instant::now()`,
//! which is the module's own contract.
//!
//! One test in a binary of its own: the resolved theme is process-wide
//! (§7.1 hands every draw path the same `&'static ResolvedTheme`), so a
//! test that swaps themes must not run beside the ~500 that read them.

use nacelle::motion::{easing_of, Crossfade, Easing, Effect};
use nacelle::{motion, theme};

/// Loads a fixture theme whose base is the master, so every token but
/// the ones in `body` is the master's own.
fn skin(body: &str) {
    let path = std::env::temp_dir().join(format!("nacelle-motion-{}.theme", std::process::id()));
    std::fs::write(
        &path,
        format!("[meta]\nschema = 1\nname = \"Fixture\"\nbase = \"default\"\n\n{body}"),
    )
    .expect("the fixture theme must be writable");
    let _ = theme::load_with(theme::LoadRequest { path: Some(path.clone()), ..Default::default() });
    let _ = std::fs::remove_file(&path);
    theme::set_viewport(1080.0, 1.0);
}

fn master() {
    let _ = theme::load();
    theme::set_viewport(1080.0, 1.0);
}

// =====================================================================

/// One test in the binary, and every stage inside it: `skin` swaps the
/// process-wide resolved theme, so two stages running in parallel
/// threads would each be measuring the other's fixture.
#[test]
fn the_motion_resolver_answers_for_the_theme() {
    master();
    a_one_shot_runs_the_master_s_unfold();
    every_freeze_answers_fully_visible();
    the_theme_s_word_picks_the_spec_s_formula();
    sine_on_a_one_shot_runs_linear();
    easing_p_finally_has_a_reader();
    a_cyclic_source_matches_blink_factor_and_scales();
    a_crossfade_retargets_without_jumping();
    an_unknown_effect_is_reported_and_ignored();
    the_a11y_switch_reaches_the_scale();
    a_breath_swings_about_the_number_the_theme_wrote();
}

/// The master's `menu_unfold`: 150 ms of `ease_out`, run on a hand-wound
/// clock. Progress 0 at the moment of opening, the spec's curve halfway,
/// 1.0 from the duration onward — and monotone in between, because a
/// menu that re-folds mid-open is a defect whatever the curve.
fn a_one_shot_runs_the_master_s_unfold() {
    master();
    let e = Effect::of("menu_unfold");
    assert_eq!(e.one_shot(10.0, 10.0), 0.0, "just opened: nothing has unfolded");
    assert_eq!(e.one_shot(10.0, 10.15), 1.0, "the master's 150 ms are up");
    assert_eq!(e.one_shot(10.0, 99.0), 1.0, "and it stays open");
    // Halfway: t01 = 0.5 through ease_out = 1-(1-t)^2.
    let half = e.one_shot(10.0, 10.075);
    assert!((half - 0.75).abs() < 1e-3, "ease_out at 0.5 is 0.75, got {half}");
    let mut prev = 0.0;
    for i in 0..=30 {
        let v = e.one_shot(10.0, 10.0 + i as f64 * 0.005);
        assert!(v >= prev - 1e-4, "the unfold went backwards at step {i}");
        prev = v;
    }
    assert!((e.one_shot_secs() - 0.15).abs() < 1e-4, "the clock the host may integrate");
}

/// The freeze rules, one by one: reduced motion, a disabled effect and
/// a zero duration all answer 1.0 AT ONCE — a jump to the end state,
/// never a run in 0 ms (§5.22), and never "never opens".
fn every_freeze_answers_fully_visible() {
    skin("[motion]\nscale = 0.0\n");
    assert_eq!(Effect::of("menu_unfold").one_shot(5.0, 5.0), 1.0, "reduced motion jumps");
    assert_eq!(Effect::of("value_blink").cyclic(0.7), 1.0, "and freezes a source visible");
    assert_eq!(Effect::of("board_ride").one_shot_secs(), 0.0, "the ride is a hard cut");

    skin("[motion.menu_unfold]\nenabled = false\n");
    assert_eq!(Effect::of("menu_unfold").one_shot(5.0, 5.0), 1.0, "disabled means already open");

    skin("[motion.menu_unfold]\nduration_ms = 0ms\n");
    assert_eq!(Effect::of("menu_unfold").one_shot(5.0, 5.0), 1.0, "zero duration means at once");

    skin("[motion.value_blink]\nenabled = false\n");
    assert_eq!(Effect::of("value_blink").cyclic(0.7), 1.0, "a disabled source stays visible");
    master();
}

/// The curve is picked by the THEME's word, read live: one fixture per
/// word, each sampled a quarter of the way through a 1000 ms run, against
/// the formulas §5.22 prints.
fn the_theme_s_word_picks_the_spec_s_formula() {
    let quarter = |body: &str| {
        skin(&format!("[motion.menu_unfold]\nduration_ms = 1000ms\n{body}"));
        Effect::of("menu_unfold").one_shot(0.0, 0.25)
    };
    assert!((quarter("easing = linear\n") - 0.25).abs() < 1e-4);
    assert!((quarter("easing = ease_out\n") - 0.4375).abs() < 1e-4);
    assert!((quarter("easing = ease_in\n") - 0.0625).abs() < 1e-4);
    assert!((quarter("easing = ease_in_out\n") - 0.15625).abs() < 1e-4);
    // step: below the duty the factor is the floor, above it 1 — a hard
    // cut, which is a legitimate thing for a theme to ask for.
    skin("[motion.menu_unfold]\nduration_ms = 1000ms\neasing = step\nduty = 0.5\nfloor = 0.2\n");
    let e = Effect::of("menu_unfold");
    assert!((e.one_shot(0.0, 0.25) - 0.2).abs() < 1e-4, "before the duty: the floor");
    assert_eq!(e.one_shot(0.0, 0.75), 1.0, "past the duty: fully open");
    master();
}

/// `sine` is cyclic-only (§5.22's table): a one-shot whose theme writes
/// it runs LINEAR — the two hand-rolled resolvers this module replaced
/// accepted it silently.
fn sine_on_a_one_shot_runs_linear() {
    skin("[motion.menu_unfold]\nduration_ms = 1000ms\neasing = sine\n");
    let v = Effect::of("menu_unfold").one_shot(0.0, 0.25);
    assert!((v - 0.25).abs() < 1e-4, "sine on a one-shot must fall back to linear, got {v}");
    let sine_would_be = 0.5 - 0.5 * (std::f32::consts::PI * 0.25).cos();
    assert!((v - sine_would_be).abs() > 0.05, "…and this build ran the sine anyway");
    master();
}

/// `custom` and its `easing_p` — the four control points had no reader
/// in Rust until the shared resolver. The defaults' curve matches the
/// enum's own arithmetic, and a fixture that moves the points moves the
/// answer, which is what proves the tokens are being read.
fn easing_p_finally_has_a_reader() {
    skin("[motion.menu_unfold]\nduration_ms = 1000ms\neasing = custom\n");
    let v = Effect::of("menu_unfold").one_shot(0.0, 0.25);
    let want = Easing::Custom([0.25, 0.10, 0.25, 1.00]).at(0.25);
    assert!((v - want).abs() < 1e-4, "the master's easing_p is not what ran: {v} vs {want}");

    // CSS's ease-in-out points: symmetric, so the middle is exactly a
    // half and the first quarter lags linear.
    skin(
        "[motion.menu_unfold]\nduration_ms = 1000ms\neasing = custom\n\
         easing_p = [0.42, 0.0, 0.58, 1.0]\n",
    );
    let e = Effect::of("menu_unfold");
    let mid = e.one_shot(0.0, 0.5);
    assert!((mid - 0.5).abs() < 1e-3, "a symmetric bezier's middle is a half, got {mid}");
    assert!(e.one_shot(0.0, 0.25) < 0.25, "ease-in-out lags linear early on");
    // And the pure table agrees about which curve the word names.
    assert_eq!(
        easing_of("custom", 0.0, 0.0, [0.42, 0.0, 0.58, 1.0]),
        Easing::Custom([0.42, 0.0, 0.58, 1.0])
    );
    master();
}

/// The cyclic path: bit-for-bit the `blink_factor` the runs consume —
/// one resolver now, so they cannot drift — and a period that stretches
/// under `motion.scale`, which is what makes reduced-but-not-zero motion
/// slow the blink rather than ignore the setting.
fn a_cyclic_source_matches_blink_factor_and_scales() {
    master();
    let e = Effect::of("value_blink");
    for i in 0..40 {
        let t = i as f64 * 0.13;
        assert_eq!(
            e.cyclic(t),
            nacelle::ui::blink_factor("value_blink", t),
            "the run's blink and the resolver disagree at t={t}"
        );
    }
    // The master's value_blink: 1000 ms, duty 0.5, floor 0 — visible on
    // the first half-beat, gone on the second.
    assert_eq!(e.cyclic(0.25), 1.0);
    assert_eq!(e.cyclic(0.75), 0.0);
    // Doubled scale, doubled period: 0.75 s is now inside the ON phase.
    skin("[motion]\nscale = 2.0\n");
    assert_eq!(Effect::of("value_blink").cyclic(0.75), 1.0, "the period did not scale");
    assert_eq!(Effect::of("value_blink").cyclic(1.25), 0.0, "…but the beat still ends");
    master();
}

/// The crossfade's contract: retargeting mid-flight starts the new fade
/// from the VALUE the old one had reached — no jump at the moment of
/// the retarget — and under reduced motion a sample IS the target, so
/// the property can never be stranded half-way.
fn a_crossfade_retargets_without_jumping() {
    skin("[motion.menu_unfold]\nduration_ms = 1000ms\neasing = linear\n");
    let e = Effect::of("menu_unfold");
    let mut cf = Crossfade::new(0.0);
    assert_eq!(cf.sample(&e, 0.0), 0.0, "at rest a crossfade is its value");
    cf.retarget(&e, 1.0, 0.0);
    assert_eq!(cf.target(), 1.0);
    assert!((cf.sample(&e, 0.5) - 0.5).abs() < 1e-4, "half the fade at half the time");
    // Turn round mid-flight: the sample the instant before and the
    // instant after the retarget are the same number.
    let before = cf.sample(&e, 0.5);
    cf.retarget(&e, 0.0, 0.5);
    let after = cf.sample(&e, 0.5);
    assert!((before - after).abs() < 1e-4, "the retarget jumped: {before} -> {after}");
    // …and the way back sets off from there.
    assert!((cf.sample(&e, 1.0) - 0.25).abs() < 1e-3, "halfway back down from 0.5");
    assert!((cf.sample(&e, 1.5) - 0.0).abs() < 1e-4, "and arrives");
    // A retarget to the standing target is a no-op, not a restart.
    let mut steady = Crossfade::new(1.0);
    steady.retarget(&e, 1.0, 7.0);
    assert_eq!(steady.sample(&e, 7.0), 1.0, "re-aiming at the target restarted the fade");

    // Reduced motion: the sample is the target the moment it is set.
    skin("[motion]\nscale = 0.0\n");
    let e = Effect::of("menu_unfold");
    let mut cf = Crossfade::new(0.0);
    cf.retarget(&e, 1.0, 3.0);
    assert_eq!(cf.sample(&e, 3.0), 1.0, "reduced motion left the property mid-way");
    master();
}

/// Prohibition 6: an id outside the closed catalogue is reported (once,
/// on stderr) and IGNORED — every ask freezes at fully visible, exactly
/// as `blink_factor` always answered for a run naming a ghost.
fn an_unknown_effect_is_reported_and_ignored() {
    master();
    let e = Effect::of("no_such_effect");
    assert_eq!(e.one_shot(0.0, 0.001), 1.0);
    assert_eq!(e.cyclic(0.7), 1.0);
    assert_eq!(e.one_shot_secs(), 0.0);
    assert_eq!(nacelle::ui::blink_factor("also_not_real", 0.3), 1.0);
}

/// `a11y.reduced_motion` — declared in §5.23 since the file was written
/// and read by NOTHING until now, which left §5.22's whole reduced-motion
/// contract reachable only through a theme that hand-wrote
/// `motion.scale = 0`. That is not a setting an accessible program can
/// ask its user to find.
///
/// The three words, each measured against a `motion.scale` the fixture
/// leaves at the master's 1.0 — so what is being measured is the SWITCH,
/// never the multiplier.
fn the_a11y_switch_reaches_the_scale() {
    // The platform half starts quiet, and this test owns it: it is
    // process-wide, and this binary is one test.
    let _ = motion::set_platform_reduce_motion(false);

    // `on` — the theme decides, and every one-shot is at its end state on
    // the frame it begins, which is a JUMP and not a run in zero ms.
    skin("[a11y]\nreduced_motion = on\n");
    let e = Effect::of("menu_unfold");
    assert_eq!(e.one_shot(5.0, 5.0), 1.0, "reduced_motion = on did not freeze the unfold");
    assert_eq!(e.one_shot_secs(), 0.0, "…and the host's clock is a hard cut");
    assert_eq!(Effect::of("value_blink").cyclic(0.75), 1.0, "a source did not freeze visible");
    assert!(motion::reduce_motion());

    // The master's own value is `system`, and with no host to ask the
    // answer is "no preference known" — the toolkit animates.
    skin("[a11y]\nreduced_motion = system\n");
    assert!(!motion::reduce_motion(), "an unasked platform suppressed motion");
    assert_eq!(Effect::of("menu_unfold").one_shot(5.0, 5.0), 0.0, "the unfold was frozen anyway");

    // A host that HAS asked: the same theme, the other answer.
    let was = motion::set_platform_reduce_motion(true);
    assert!(!was, "set_platform_reduce_motion did not answer what it replaced");
    assert!(motion::reduce_motion(), "the platform's preference was not honoured");
    assert_eq!(Effect::of("menu_unfold").one_shot(5.0, 5.0), 1.0);

    // `off` is a DECISION, not an absence of one: a theme saying it wants
    // animation is a user overriding their desktop for this program, and
    // it wins over the platform.
    skin("[a11y]\nreduced_motion = off\n");
    assert!(!motion::reduce_motion(), "the theme's `off` lost to the platform");
    assert_eq!(Effect::of("menu_unfold").one_shot(5.0, 5.0), 0.0);

    // A word this build does not know falls to the platform, never to
    // `off`: an accessibility switch fails toward the stated preference.
    skin("[a11y]\nreduced_motion = someday\n");
    assert!(motion::reduce_motion(), "an unknown word turned the suppression off");

    // …and `on` still wins when the platform says nothing.
    motion::set_platform_reduce_motion(false);
    skin("[a11y]\nreduced_motion = on\n");
    assert!(motion::reduce_motion());
    master();
    assert!(!motion::reduce_motion(), "the master's own file suppresses motion");
}

/// `cyclic_amplitude` — the SECOND cyclic path, and the reason it is a
/// second one: a blink freezes fully visible, a breath freezes at its
/// mean. `glow_pulse` is the only entry in the catalogue that declares an
/// amplitude, and neither the key nor the arithmetic had a reader.
fn a_breath_swings_about_the_number_the_theme_wrote() {
    let _ = motion::set_platform_reduce_motion(false);

    // The master ships the pulse OFF, so the multiplier is exactly one at
    // every clock — not approximately one.
    master();
    let e = Effect::of("glow_pulse");
    for i in 0..40 {
        assert_eq!(e.cyclic_amplitude(i as f64 * 0.11), 1.0, "a disabled breath moved");
    }

    // Turned on, with the master's 1600 ms of sine at amplitude 0.25:
    // the floor at the start of a period, the ceiling in the middle, and
    // EXACTLY the mean at each quarter — which is what makes a picture
    // drawn there identical to a frozen one.
    skin("[motion.glow_pulse]\nenabled = true\n");
    let e = Effect::of("glow_pulse");
    assert!((e.cyclic_amplitude(0.0) - 0.75).abs() < 1e-5, "the floor is 1 - amplitude");
    assert!((e.cyclic_amplitude(0.8) - 1.25).abs() < 1e-5, "the ceiling is 1 + amplitude");
    assert_eq!(e.cyclic_amplitude(0.4), 1.0, "a quarter through, the swing is the mean");
    assert_eq!(e.cyclic_amplitude(1.2), 1.0, "and three quarters through");
    // …and it stays inside the band all the way round, which is what an
    // alpha multiplier has to promise.
    let mut sum = 0.0f64;
    for i in 0..1600 {
        let v = e.cyclic_amplitude(i as f64 / 1000.0);
        assert!((0.75..=1.25).contains(&v), "the breath left its band at {i} ms: {v}");
        sum += v as f64;
    }
    assert!((sum / 1600.0 - 1.0).abs() < 1e-3, "the breath is not centred on one");

    // The blinks do not share this path and cannot have moved.
    assert_eq!(Effect::of("value_blink").cyclic(0.25), 1.0);
    assert_eq!(Effect::of("value_blink").cyclic(0.75), 0.0);

    // Every freeze answers the MEAN, which is one: reduced motion by
    // either road, and an amplitude of nothing.
    skin("[motion.glow_pulse]\nenabled = true\n\n[motion]\nscale = 0.0\n");
    for t in [0.0, 0.4, 0.8] {
        assert_eq!(Effect::of("glow_pulse").cyclic_amplitude(t), 1.0, "scale = 0 breathed");
    }
    skin("[motion.glow_pulse]\nenabled = true\n\n[a11y]\nreduced_motion = on\n");
    for t in [0.0, 0.4, 0.8] {
        assert_eq!(Effect::of("glow_pulse").cyclic_amplitude(t), 1.0, "reduced motion breathed");
    }
    skin("[motion.glow_pulse]\nenabled = true\namplitude = 0.0\n");
    assert_eq!(Effect::of("glow_pulse").cyclic_amplitude(0.0), 1.0, "a swing of nothing swung");
    assert_eq!(Effect::of("glow_pulse").amplitude(), 0.0, "the raw key is readable too");

    // A period of zero is no cycle to stand on: the mean again.
    skin("[motion.glow_pulse]\nenabled = true\nperiod_ms = 0ms\n");
    assert_eq!(Effect::of("glow_pulse").cyclic_amplitude(0.3), 1.0, "a zero period breathed");
    master();
}
