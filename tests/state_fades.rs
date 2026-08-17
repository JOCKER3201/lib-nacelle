//! The interaction states, reached over TIME.
//!
//! `crate::motion`'s unit tests prove the arithmetic; what this binary
//! proves is the seam between the shared fade registry and a LIVE theme:
//! that a fade arrives at the ladder's own ink and not a rounding of it,
//! that an interrupted one does not jump, that `motion.scale = 0` leaves
//! the picture bit for bit what it was, that the effect is chosen by
//! which rung moved, and that a control which left the screen leaves
//! nothing behind.
//!
//! Time is a PARAMETER throughout — `state_ink(..., now, ...)` — so
//! every clock below is a literal wound by hand. Nothing sleeps and
//! nothing reads `Instant::now()`; that is the module's contract and it
//! is also why these assertions are exact rather than approximate.
//!
//! One test in a binary of its own, for `motion_effects.rs`'s reason:
//! the resolved theme is process-wide, so a test that swaps themes must
//! not run beside the five hundred that read them. The registry is
//! thread-local and shared, so every stage below opens with
//! `forget_fades` — a stage that inherited the previous stage's tracks
//! would be measuring the wrong control.

use nacelle::motion::{self, state_ink, state_mix};
use nacelle::theme::parse::State;
use nacelle::theme::Color;
use nacelle::view::surface::StateInk;
use nacelle::{theme, Rect};

/// Loads a fixture theme whose base is the master, so every token but
/// the ones in `body` is the master's own.
fn skin(body: &str) {
    let path = std::env::temp_dir().join(format!("nacelle-fades-{}.theme", std::process::id()));
    std::fs::write(
        &path,
        format!("[meta]\nschema = 1\nname = \"Fixture\"\nbase = \"default\"\n\n{body}"),
    )
    .expect("the fixture theme must be writable");
    let _ = theme::load_with(theme::LoadRequest { path: Some(path.clone()), ..Default::default() });
    let _ = std::fs::remove_file(&path);
    theme::set_viewport(1080.0, 1.0);
    motion::forget_fades();
}

fn master() {
    let _ = theme::load();
    theme::set_viewport(1080.0, 1.0);
    motion::forget_fades();
}

/// The `button` ladder's own ink for one rung — what every fade below is
/// measured against, read straight off the resolved theme.
fn rung(s: State) -> StateInk {
    let c = theme::class_id("button").expect("the master declares a button class");
    StateInk::from(theme::resolved().class_state(c, s))
}

/// The ink the toolkit would draw a button in, asked through the fade
/// registry exactly as `object::button::dress` asks.
fn ink(r: Rect, to: State, now: f64) -> StateInk {
    state_ink("button", r, to, now, rung)
}

fn same(a: Color, b: Color) -> bool {
    a.r == b.r && a.g == b.g && a.b == b.b && a.a == b.a
}

/// Every field, bit for bit — the assertion "today's colours are kept"
/// has to be made about the whole rung, not about the fill alone.
fn identical(a: StateInk, b: StateInk) -> bool {
    same(a.fill, b.fill)
        && same(a.edge, b.edge)
        && same(a.text, b.text)
        && same(a.glyph, b.glyph)
        && a.edge_width == b.edge_width
        && a.glow_radius == b.glow_radius
        && a.glow_alpha == b.glow_alpha
        && a.elevation == b.elevation
}

const BOX: Rect = Rect { x: 40.0, y: 12.0, w: 120.0, h: 28.0 };

// =====================================================================

#[test]
fn the_state_fades_answer_for_the_theme() {
    master();
    a_control_is_born_on_its_rung();
    a_fade_leaves_one_rung_and_lands_exactly_on_the_other();
    an_interrupted_fade_turns_round_where_it_stands();
    reduced_motion_draws_todays_picture();
    a_disabled_effect_is_a_jump();
    which_rung_moved_picks_the_effect();
    a_transition_needs_a_clock_that_moved();
    a_control_that_left_the_screen_leaves_nothing_behind();
    a_moved_control_arrives_already_in_its_state();
    the_resting_rung_is_the_callers_to_state();
    the_focus_ring_has_a_gate_of_its_own();
    the_host_is_told_when_it_owes_another_frame();
}

/// A key the registry has never seen is SETTLED on the rung it was asked
/// for: a control appearing under the pointer is hovered, not fading in
/// from a state it was never in.
fn a_control_is_born_on_its_rung() {
    master();
    let m = state_mix("button", BOX, State::Hover, 10.0);
    assert!(m.is_settled(), "a first sighting owes no fade");
    assert_eq!(m.target(), State::Hover);
    assert_eq!(m.weight(State::Hover), 1.0);
    assert!(identical(ink(BOX, State::Hover, 10.0), rung(State::Hover)), "born off its rung");
}

/// The master's `motion.hover`: 90 ms of `ease_out`. Nothing has moved at
/// the instant the pointer arrives, everything has by 90 ms, and the ink
/// at the end is the ladder's own — not a float that rounds to it.
fn a_fade_leaves_one_rung_and_lands_exactly_on_the_other() {
    master();
    assert!(identical(ink(BOX, State::Idle, 1.0), rung(State::Idle)), "the resting frame moved");
    // The pointer arrives on the next frame.
    let at_start = ink(BOX, State::Hover, 1.016);
    assert!(identical(at_start, rung(State::Idle)), "the fade jumped at its own start");
    // Halfway: between the two rungs, and neither of them.
    let half = ink(BOX, State::Hover, 1.061);
    assert!(!identical(half, rung(State::Idle)) && !identical(half, rung(State::Hover)));
    let (i, h) = (rung(State::Idle).fill, rung(State::Hover).fill);
    let lo = i.a.min(h.a);
    let hi = i.a.max(h.a);
    assert!(half.fill.a > lo && half.fill.a < hi, "the wash left the two rungs it is between");
    // And it lands. 90 ms after the fade began, to the millisecond.
    assert!(
        identical(ink(BOX, State::Hover, 1.106), rung(State::Hover)),
        "the fade did not land on the theme's own hover ink"
    );
    // …and stays landed, at no cost: the track has collapsed.
    assert!(identical(ink(BOX, State::Hover, 9.0), rung(State::Hover)));
    assert!(state_mix("button", BOX, State::Hover, 9.5).is_settled());
}

/// The pointer leaving mid-hover fades back from WHERE THE FADE HAD
/// REACHED. The sampled ink either side of the turn is the same ink —
/// that is the whole of the no-jump contract, and it holds for a rung
/// crossfade exactly as it does for the scalar one.
fn an_interrupted_fade_turns_round_where_it_stands() {
    master();
    let _ = ink(BOX, State::Idle, 1.0);
    let _ = ink(BOX, State::Hover, 1.016);
    let before = ink(BOX, State::Hover, 1.061);
    let after = ink(BOX, State::Idle, 1.061 + 1e-9);
    assert!(
        (before.fill.a - after.fill.a).abs() < 1e-4 && same(before.edge, after.edge),
        "the turn jumped: {:?} -> {:?}",
        before.fill,
        after.fill
    );
    // The way back sets off from there and arrives at the resting rung.
    assert!(
        identical(ink(BOX, State::Idle, 1.061 + 0.1), rung(State::Idle)),
        "the way back did not arrive"
    );
    // A third target mid-flight is the same story: no jump, and it lands.
    let _ = ink(BOX, State::Hover, 2.0);
    let mid = ink(BOX, State::Hover, 2.03);
    let turned = ink(BOX, State::Press, 2.03 + 1e-9);
    assert!((mid.fill.a - turned.fill.a).abs() < 1e-4, "a third rung jumped");
    assert!(identical(ink(BOX, State::Press, 2.5), rung(State::Press)), "press did not arrive");
}

/// `motion.scale = 0` is §5.22's reduced motion: a JUMP to the end
/// state. Every ask is the ladder's own ink at the instant it is asked
/// for — so a build with the fades and a build without them draw the
/// same pixels, which is the promise the whole stone rests on.
fn reduced_motion_draws_todays_picture() {
    skin("[motion]\nscale = 0.0\n");
    let mut t = 5.0;
    for s in [State::Idle, State::Hover, State::Press, State::Selected, State::Disabled] {
        let got = ink(BOX, s, t);
        assert!(identical(got, rung(s)), "reduced motion drew a blend for {}", s.name());
        assert!(state_mix("button", BOX, s, t).is_settled());
        t += 0.016;
    }
    assert!(!motion::pending(t), "reduced motion asked the host for a frame");
    master();
}

/// A theme that switches one effect off freezes THAT transition and no
/// other: the hover snaps, the press still fades.
fn a_disabled_effect_is_a_jump() {
    skin("[motion.hover]\nenabled = false\n");
    let _ = ink(BOX, State::Idle, 3.0);
    assert!(
        identical(ink(BOX, State::Hover, 3.016), rung(State::Hover)),
        "a disabled hover still faded"
    );
    let _ = ink(BOX, State::Idle, 4.0);
    let mid = ink(BOX, State::Press, 4.016);
    assert!(!identical(mid, rung(State::Press)), "press was switched off with hover");
    master();
}

/// The catalogue names STATES, not pairs, so the rung that changed is
/// what names the effect. The master gives them different durations —
/// hover 90 ms, press 150 ms, select 120 ms, disable 160 ms — and the
/// duration is what this measures: each transition is still moving one
/// millisecond before its own end and landed one after it.
fn which_rung_moved_picks_the_effect() {
    master();
    let arrives_by = |from: State, to: State, secs: f64, at: Rect| {
        // The control stands on `from`, and the fade sets off on the NEXT
        // frame — which is the frame the duration is counted from.
        let _ = ink(at, from, 20.0);
        let start = 20.001;
        let _ = ink(at, to, start);
        assert!(
            !identical(ink(at, to, start + secs - 0.002), rung(to)),
            "{} -> {} landed before its effect was over",
            from.name(),
            to.name()
        );
        assert!(
            identical(ink(at, to, start + secs + 0.002), rung(to)),
            "{} -> {} outlived its effect",
            from.name(),
            to.name()
        );
    };
    // Each pair gets a box of its own: the box IS the identity.
    arrives_by(State::Idle, State::Hover, 0.090, Rect::new(0.0, 0.0, 10.0, 10.0));
    arrives_by(State::Hover, State::Press, 0.150, Rect::new(20.0, 0.0, 10.0, 10.0));
    arrives_by(State::Idle, State::Selected, 0.120, Rect::new(40.0, 0.0, 10.0, 10.0));
    arrives_by(State::Idle, State::Disabled, 0.160, Rect::new(60.0, 0.0, 10.0, 10.0));
    // Disabled is the outermost rule — it wins even when the pointer
    // moved at the same time.
    arrives_by(State::Disabled, State::Hover, 0.160, Rect::new(80.0, 0.0, 10.0, 10.0));
    // A drag is a sustained press, and leaving one runs `press` too.
    arrives_by(State::Dragging, State::Idle, 0.150, Rect::new(100.0, 0.0, 10.0, 10.0));
    // Selection under the pointer is still selection: hover is what moved.
    arrives_by(State::Selected, State::SelectedHover, 0.090, Rect::new(120.0, 0.0, 10.0, 10.0));
}

/// Two asks at ONE instant are one frame asked twice, not a change over
/// time. The second jumps — which is what keeps a view drawn twice in a
/// frame, and every test that draws at a fixed clock, exactly what it
/// was.
fn a_transition_needs_a_clock_that_moved() {
    master();
    let r = Rect::new(4.0, 4.0, 30.0, 12.0);
    assert!(identical(ink(r, State::Idle, 7.0), rung(State::Idle)));
    assert!(identical(ink(r, State::Hover, 7.0), rung(State::Hover)), "one instant, two rungs");
    assert!(identical(ink(r, State::Press, 7.0), rung(State::Press)));
    // A clock that goes BACKWARDS is not a frame boundary either.
    assert!(identical(ink(r, State::Idle, 6.5), rung(State::Idle)));
    // …and the next real frame fades again.
    assert!(!identical(ink(r, State::Hover, 7.016), rung(State::Hover)));
}

/// The registry is bounded by what is on screen. A hundred rows scrolled
/// away stop being asked about, and the sweep reclaims them; nothing
/// keeps an entry alive but being drawn.
fn a_control_that_left_the_screen_leaves_nothing_behind() {
    master();
    for i in 0..100 {
        let _ = ink(Rect::new(0.0, i as f32 * 20.0, 50.0, 18.0), State::Hover, 30.0);
    }
    assert_eq!(motion::tracked(), 100, "a control was not tracked");
    // A second later only one of them is still being drawn.
    let _ = ink(Rect::new(0.0, 0.0, 50.0, 18.0), State::Hover, 31.0);
    assert_eq!(motion::tracked(), 1, "the sweep left {} entries behind", motion::tracked());
}

/// A control that MOVED is a new key, born settled — so a scrolling list
/// and a dragged thumb read instantly instead of smearing a fade behind
/// them, which is the reason the identity is allowed to be the box.
fn a_moved_control_arrives_already_in_its_state() {
    master();
    let a = Rect::new(200.0, 10.0, 60.0, 20.0);
    let b = Rect::new(200.0, 30.0, 60.0, 20.0);
    let _ = ink(a, State::Idle, 40.0);
    assert!(identical(ink(b, State::Dragging, 40.016), rung(State::Dragging)), "the move smeared");
}

/// The RESTING rung is the caller's to state, which is what lets a list
/// row fade its plate out to nothing instead of into the master's
/// `idle.fill` — and, at rest, draw no plate at all, exactly as today.
fn the_resting_rung_is_the_callers_to_state() {
    master();
    let r = Rect::new(300.0, 0.0, 80.0, 16.0);
    let bare = |s: State| match s {
        State::Idle => StateInk::CLEAR,
        s => rung(s),
    };
    let at = |to, now| state_ink("list.item", r, to, now, bare);
    assert_eq!(at(State::Idle, 50.0).fill.a, 0.0, "a resting row grew a plate");
    // The pointer arrives on the next frame; halfway through the
    // master's 90 ms is 45 ms after that.
    let _ = at(State::Hover, 50.016);
    let half = at(State::Hover, 50.061);
    assert!(half.fill.a > 0.0 && half.fill.a < rung(State::Hover).fill.a, "the plate did not fade");
    // Premultiplied, so a plate fading in out of nothing keeps its HUE
    // all the way: a wash that darkened toward black on the way in would
    // be a bruise, not a highlight.
    let hovered = rung(State::Hover).fill;
    assert!(
        (half.fill.r - hovered.r).abs() < 1e-3
            && (half.fill.g - hovered.g).abs() < 1e-3
            && (half.fill.b - hovered.b).abs() < 1e-3,
        "the fade dragged the colour toward black: {:?} vs {:?}",
        half.fill,
        hovered
    );
    assert!(identical(at(State::Hover, 50.2), rung(State::Hover)));
    // …and back out to nothing at all.
    let _ = at(State::Idle, 50.216);
    assert_eq!(at(State::Idle, 50.5).fill.a, 0.0, "the plate never left");
}

/// `motion.focus` finally has a reader. Focus is not a ladder rung
/// (§5.21), so the ring rides a GATE — 0 and 1 exactly at the ends, the
/// master's 120 ms in between, and the same three rules as a track.
fn the_focus_ring_has_a_gate_of_its_own() {
    master();
    let r = Rect::new(500.0, 4.0, 40.0, 40.0);
    assert_eq!(motion::gate("focus.ring", r, false, "focus", 60.0), 0.0, "born lit");
    assert!(motion::gate("focus.ring", r, true, "focus", 60.016) < 1.0, "the ring snapped on");
    let half = motion::gate("focus.ring", r, true, "focus", 60.076);
    assert!(half > 0.0 && half < 1.0, "the gate is not between its ends: {half}");
    assert_eq!(motion::gate("focus.ring", r, true, "focus", 60.14), 1.0, "the ring never arrived");
    assert_eq!(motion::gate("focus.ring", r, true, "focus", 61.0), 1.0);
    // Turning round mid-flight does not jump, and it does reach zero.
    assert!(motion::gate("focus.ring", r, false, "focus", 61.016) > 0.9);
    assert_eq!(motion::gate("focus.ring", r, false, "focus", 61.2), 0.0, "the ring never left");
    // Reduced motion is a jump here too.
    skin("[motion]\nscale = 0.0\n");
    let r = Rect::new(500.0, 4.0, 40.0, 40.0);
    assert_eq!(motion::gate("focus.ring", r, false, "focus", 70.0), 0.0);
    assert_eq!(motion::gate("focus.ring", r, true, "focus", 70.016), 1.0, "reduced motion faded");
    master();
}

/// The one thing the host has to be told: whether another frame is owed.
/// FALSE AT REST — nothing in the registry asks for a redraw, which is
/// what keeps an idle desktop as cheap as it is today.
fn the_host_is_told_when_it_owes_another_frame() {
    master();
    let r = Rect::new(600.0, 0.0, 30.0, 30.0);
    let _ = ink(r, State::Idle, 80.0);
    assert!(!motion::pending(80.0), "a resting control asked for a frame");
    let _ = ink(r, State::Hover, 80.016);
    assert!(motion::pending(80.016), "a fade in flight did not ask for a frame");
    assert!(motion::pending(80.1), "the fade stopped asking before it landed");
    assert!(!motion::pending(80.2), "the fade kept asking after it landed");
}
