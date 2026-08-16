//! `tabular` stops being a token nobody reads — measured, not assumed.
//!
//! §5.16 gives all twenty-four type roles a `tabular` bool and §5.17 says
//! what it means: every figure is stepped by the widest of them and
//! centred in that step, so a number does not change width when its
//! content changes. `fontdue` exposes no OpenType features, so there is no
//! `tnum` to ask the face for and the toolkit computes the box itself.
//!
//! Until this suite existed the token was declared on every role and read
//! by nobody, which is why `network.rhai` reached for the `data` role — a
//! MONOSPACE role — purely to stop an IP address shivering, and inherited
//! a size 74 % smaller as the price. The thing it actually wanted is the
//! thing measured below.
//!
//! Every assertion here comes in a pair: the proportional control and the
//! tabular case. The control is not decoration — it is what makes the test
//! FAIL if the box is switched off, rather than pass vacuously on a face
//! whose digits happen to be uniform.

use nacelle::draw::DrawList;
use nacelle::font::{Figures, FontSystem, FONT_UI};
use nacelle::theme::Color;

/// §5.17's default `num.tabular_set`. Spelled here rather than read from
/// the theme because this suite is about the MECHANISM: the theme wiring
/// is proved separately, at the bottom.
const DIGITS: &str = "0123456789";

/// A size well clear of `type.min_px`, so nothing here is testing a floor.
const PX: f32 = 24.0;

fn ink() -> Color {
    Color { r: 1.0, g: 1.0, b: 1.0, a: 1.0 }
}

/// The x of the left edge of every glyph quad in the list, in draw order.
/// A run's geometry is exactly this sequence: where each glyph landed.
fn pen_stops(dl: &DrawList) -> Vec<f32> {
    dl.verts.chunks(6).map(|q| q[0].pos[0]).collect()
}

fn draw(fs: &mut FontSystem, text: &str, fig: &Figures) -> DrawList {
    let mut dl = DrawList::new();
    dl.text_fig(fs, FONT_UI, PX, 100.0, 50.0, text, ink(), 0.0, fig);
    dl
}

// --------------------------------------------------------- the box itself

#[test]
fn the_box_is_the_widest_figure_and_no_wider() {
    let mut fs = FontSystem::new();
    let fig = fs.figures(FONT_UI, PX, DIGITS, true);
    assert!(fig.is_on(), "a set of ten digits must produce a box");

    let widest = DIGITS
        .chars()
        .map(|c| fs.glyph(FONT_UI, PX, c).unwrap().advance)
        .fold(0.0f32, f32::max);
    assert_eq!(fig.advance(), widest);

    // Every digit is a member and is stepped by the box, whatever the
    // face gave it.
    // A member of the set proper is boxed wherever it stands, so the
    // neighbours it is asked about make no difference to it.
    for c in DIGITS.chars() {
        assert_eq!(fig.advance_of(None, c, None), Some(widest), "{c}");
        assert_eq!(fig.advance_of(Some('X'), c, Some('X')), Some(widest), "{c}");
    }
    // A letter is not: proportional text keeps the advance it was drawn
    // with, which is the half of the rule that makes the other half safe.
    for c in "ABCXYZabcxyz".chars() {
        assert_eq!(fig.advance_of(None, c, None), None, "{c}");
        assert_eq!(fig.advance_of(Some('1'), c, Some('1')), None, "{c}");
    }
}

#[test]
fn punctuation_joins_the_box_but_never_widens_it() {
    let mut fs = FontSystem::new();
    let bare = fs.figures(FONT_UI, PX, DIGITS, false);
    let with_punct = fs.figures(FONT_UI, PX, DIGITS, true);

    // `num.tabular_punct` is what stops `21:57:30` shivering on the colon
    // — the master says so in the comment beside the token. The colon is
    // asked about standing where a clock puts it, between two figures,
    // because that is the only place the mark is part of a number.
    assert_eq!(bare.advance_of(Some('1'), ':', Some('5')), None);
    assert_eq!(
        with_punct.advance_of(Some('1'), ':', Some('5')),
        Some(with_punct.advance())
    );

    // '%' is wider than any digit in most faces. Letting the punctuation
    // into the maximum would grow every number on screen the moment the
    // flag went on, so the box is measured from the SET alone.
    assert_eq!(
        bare.advance(),
        with_punct.advance(),
        "turning tabular_punct on must not resize the figure box"
    );
}

// ------------------------------------------------- the owner's measurement

/// The proof the owner asked for: the same string with `1`s and with `8`s
/// measures the same width, and it does NOT without the box.
#[test]
fn one_and_eight_measure_the_same_width_only_under_the_box() {
    let mut fs = FontSystem::new();
    let fig = fs.figures(FONT_UI, PX, DIGITS, true);

    for (ones, eights) in [
        ("11111111", "88888888"),
        // The clock of image 1, which ticks once a second.
        ("11:11:11", "88:88:88"),
        // The address `network.rhai` went to a mono role to hold still.
        ("192.168.1.1", "192.868.8.8"),
    ] {
        let loose = (
            fs.measure(FONT_UI, PX, ones, 0.0),
            fs.measure(FONT_UI, PX, eights, 0.0),
        );
        // The control. If this ever stops holding, the face's digits are
        // already uniform and the test below proves nothing — so it is an
        // assertion, not a comment.
        assert_ne!(
            loose.0, loose.1,
            "proportional figures must differ, or this test is vacuous: {ones} vs {eights}"
        );

        let boxed = (
            fs.measure_fig(FONT_UI, PX, ones, 0.0, &fig),
            fs.measure_fig(FONT_UI, PX, eights, 0.0, &fig),
        );
        assert_eq!(boxed.0, boxed.1, "{ones} vs {eights}");
    }
}

/// Measuring is not drawing. Every character the two strings SHARE must
/// also land on the same pixel — a width that agrees over a run whose
/// separators walk apart is a clock that still shivers on the colon.
///
/// The digits themselves are deliberately not compared: a '1' is centred
/// in its box and an '8' fills it, so their ink starts at different x by
/// design. That is the box working, not failing — what must not move is
/// everything around them.
#[test]
fn what_two_readings_share_lands_on_the_same_pixel() {
    let mut fs = FontSystem::new();
    let fig = fs.figures(FONT_UI, PX, DIGITS, true);

    // The colons of the clock and a sentinel at the end of the run, which
    // is where a reflow shows up first.
    let shared = |dl: &DrawList| {
        let s = pen_stops(dl);
        vec![s[2], s[5], s[8]]
    };

    let loose = (
        shared(&draw(&mut fs, "11:11:11X", &Figures::NONE)),
        shared(&draw(&mut fs, "88:88:88X", &Figures::NONE)),
    );
    assert_ne!(loose.0, loose.1, "the control must differ or nothing is proved");

    let boxed = (
        shared(&draw(&mut fs, "11:11:11X", &fig)),
        shared(&draw(&mut fs, "88:88:88X", &fig)),
    );
    assert_eq!(boxed.0, boxed.1);
}

/// A figure is CENTRED in its box, not flushed left in it: a narrow '1'
/// beside a wide '8' has to keep the column's optical rhythm. The half of
/// §5.17 that a fixed advance alone would not give.
#[test]
fn a_narrow_figure_is_centred_in_its_box() {
    let mut fs = FontSystem::new();
    let fig = fs.figures(FONT_UI, PX, DIGITS, true);
    let one = fs.glyph(FONT_UI, PX, '1').unwrap().advance;
    let widest = fig.advance();
    assert!(one < widest, "the face's '1' must be narrower, or nothing is centred");
    assert_eq!(Figures::centre_in(widest, one), (widest - one) / 2.0);
    // The widest figure fills its box exactly and is not nudged.
    assert_eq!(Figures::centre_in(widest, widest), 0.0);
}

/// A seven-digit pid is the process table's version of the same claim:
/// every pid of the same length occupies the same column width, whatever
/// the digits turn out to be.
#[test]
fn every_pid_of_a_length_measures_the_same() {
    let mut fs = FontSystem::new();
    let fig = fs.figures(FONT_UI, PX, DIGITS, true);
    let w = |fs: &mut FontSystem, s: &str| fs.measure_fig(FONT_UI, PX, s, 0.0, &fig);

    let reference = w(&mut fs, "1471000");
    for pid in ["1888888", "9999999", "1010101", "4004004"] {
        assert_eq!(w(&mut fs, pid), reference, "{pid}");
    }
    // §5.17's arithmetic claim: the width of an all-figure string is
    // `len x advance`, which is what makes a right-aligned numeric column
    // free — no atlas is touched to know it. Compared within a float
    // ulp or two, because one side sums seven advances and the other
    // multiplies by seven; the CLAIM is the arithmetic, not the summation
    // order.
    assert!(
        (reference - 7.0 * fig.advance()).abs() < 1e-3,
        "{reference} vs {}",
        7.0 * fig.advance()
    );
}

// ------------------------------------------------- text without figures

/// The other half of the rule, and the reason typography has two figure
/// sets at all: a run with no figures in it must come out of the box path
/// byte for byte as it came out of the proportional one.
#[test]
fn a_run_without_figures_is_untouched() {
    let mut fs = FontSystem::new();
    let fig = fs.figures(FONT_UI, PX, DIGITS, false);

    for text in ["WIDOK", "Zakonczone", "eth0", "PID"] {
        assert_eq!(
            fs.measure(FONT_UI, PX, text, 0.0),
            fs.measure_fig(FONT_UI, PX, text, 0.0, &fig),
            "{text}"
        );
        let loose = draw(&mut fs, text, &Figures::NONE);
        let boxed = draw(&mut fs, text, &fig);
        assert_eq!(loose.verts.len(), boxed.verts.len(), "{text}");
        for (a, b) in loose.verts.iter().zip(boxed.verts.iter()) {
            assert_eq!(a.pos, b.pos, "{text}");
            assert_eq!(a.uv, b.uv, "{text}");
        }
    }
}

/// And the letters of a MIXED run keep the face's advance: only the
/// figures are boxed, so `eth0 192.168.1.1` does not turn into a grid.
#[test]
fn only_the_figures_of_a_mixed_run_are_boxed() {
    let mut fs = FontSystem::new();
    // No punctuation in the box, so the space between the two words is
    // the face's own and the letters are provably untouched.
    let fig = fs.figures(FONT_UI, PX, DIGITS, false);
    let prefix = "eth";

    let loose = pen_stops(&draw(&mut fs, prefix, &Figures::NONE));
    let boxed = pen_stops(&draw(&mut fs, "eth0", &fig));
    assert_eq!(&boxed[..loose.len()], &loose[..], "the letters must not move");

    // ...and the figure after them still sits in its box: replacing it
    // with a wider digit does not move anything.
    let a = pen_stops(&draw(&mut fs, "eth0X", &fig));
    let b = pen_stops(&draw(&mut fs, "eth1X", &fig));
    assert_eq!(a.len(), b.len());
    assert_eq!(a.last(), b.last(), "the glyph AFTER a figure must not move");
}

// ----------------------------------------------------- the theme wiring

/// The mechanism is only worth having if the master reaches it. These are
/// the six roles §5.16 declares `tabular` on, and the four the owner's two
/// widgets actually stand on.
#[test]
fn the_master_reaches_the_box_through_its_roles() {
    for name in ["display.clock", "display.date", "value", "value.large", "data", "data.dump"] {
        assert!(nacelle::ui::role(name).tabular(), "type.{name}.tabular");
    }
    // Running text keeps proportional figures — the token is a system, so
    // the roles that say `false` have to be read as `false` too.
    for name in ["body", "body.dim", "title.panel", "caption", "button", "tooltip"] {
        assert!(!nacelle::ui::role(name).tabular(), "type.{name}.tabular");
    }
}

/// A role that asks for the box gets one; a role that does not gets
/// [`Figures::NONE`], and drawing under `NONE` is drawing as before.
#[test]
fn a_role_resolves_its_own_box() {
    let mut fs = FontSystem::new();
    let boxed = nacelle::ui::role("value").figures(&mut fs, FONT_UI, PX);
    let loose = nacelle::ui::role("body").figures(&mut fs, FONT_UI, PX);
    assert!(boxed.is_on(), "type.value.tabular = true");
    assert!(!loose.is_on(), "type.body.tabular = false");
    assert_eq!(loose.advance_of(None, '7', None), None);
    assert_eq!(boxed.advance_of(None, '7', None), Some(boxed.advance()));
}

/// The register is the image guard's witness, and the box is geometry:
/// two runs of the same string at the same size that occupy different
/// widths may not record the same line.
#[test]
fn the_register_records_the_box() {
    let mut fs = FontSystem::new();
    let fig = fs.figures(FONT_UI, PX, DIGITS, true);

    let mut loose = DrawList::recording();
    loose.text(&mut fs, FONT_UI, PX, 0.0, 0.0, "1471", ink(), 0.0);
    let mut boxed = DrawList::recording();
    boxed.text_fig(&mut fs, FONT_UI, PX, 0.0, 0.0, "1471", ink(), 0.0, &fig);

    let l = loose.cmds()[0].to_string();
    let b = boxed.cmds()[0].to_string();
    assert_ne!(l, b);
    // The proportional line is the line it has always been, so the
    // recorded corpus stays comparable across this change.
    assert!(!l.contains("figure"), "{l}");
    assert!(b.contains("figure"), "{b}");
}
