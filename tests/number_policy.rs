//! `[num]` decides how a reading is written down — measured on the
//! instrument the master names.
//!
//! §5.17 opens with a sentence about itself: **"THE THEME DECIDES, not a
//! locale guess"**. Until 2026-08-17 two of its sixteen keys had a reader
//! and both were about the figure BOX; the reading itself came out of
//! `format!("{v:.0}%")` in `ui.rs`, so a theme could not move the decimal
//! mark, the thousands separator, the number of places or the letters of
//! the unit. This file asks the gauge — the one instrument the master's
//! own comments point at (`decimals_compact`: "temperatures, gauge
//! readouts") — what it draws under themes that differ in one key each.
//!
//! Every stage names ONE key and requires the drawing to follow it. The
//! master's own picture is measured first, so that a stage which changes
//! nothing fails instead of passing quietly.
//!
//! ONE test function, on purpose: the resolved theme is process-wide, so
//! a test that switches it must not run beside a test that reads it — the
//! same ruling `tests/gauge_role_bindings.rs` makes. The three chains
//! below are functions of that one test and not tests of their own.

use nacelle::draw::{DrawCmd, DrawList};
use nacelle::font::FontSystem;
use nacelle::pointer::Pointer;
use nacelle::theme::{self, LoadRequest};
use nacelle::ui::{self, GaugeKind, GaugeLabels, GaugeStyle, GaugeValueFmt};
use nacelle::{Ctx, Rect};

const W: f32 = 1920.0;
const H: f32 = 1080.0;

/// Runs one question on a thread of its own: the toolkit memoises text
/// tokens, resolved roles and enum words per THREAD and per epoch, and a
/// reload renumbers the open word sets a binding lives in.
fn fresh<T: Send>(f: impl FnOnce() -> T + Send) -> T {
    std::thread::scope(|s| s.spawn(f).join().expect("the drawing thread panicked"))
}

/// Loads the master, or a theme based on it that rewrites a few keys.
fn apply(fixture: Option<&str>) {
    match fixture {
        None => {
            let _ = theme::load();
        }
        Some(text) => {
            let path = std::env::temp_dir()
                .join(format!("nacelle-number-policy-{}.theme", std::process::id()));
            std::fs::write(&path, text).expect("the fixture theme must be writable");
            let _ = theme::load_with(LoadRequest { path: Some(path), ..Default::default() });
        }
    }
}

const HEAD: &str = "[meta]\nschema = 1\nname = \"Number policy fixture\"\nbase = \"default\"\n\n";

/// Every string a gauge block draws, in the order the runs were laid
/// down. A cell gauge draws its unit first and its number second — the
/// run is laid out from its right edge, because the unit hangs off the
/// number's end.
fn runs(values: &[f32], fmt: GaugeValueFmt) -> Vec<String> {
    let values = values.to_vec();
    fresh(move || {
        let mut fonts = FontSystem::new();
        let mut dl = DrawList::recording();
        {
            let mut c = Ctx {
                dl: &mut dl,
                fonts: &mut fonts,
                w: W,
                h: H,
                t: 0.0,
                mouse: Pointer::new(-1.0, -1.0),
                term_font_scale: 1.0,
                ui_font_scale: 1.0,
                panel_scale: 1.0,
                focus: None,
                tips: None,
            };
            let st = GaugeStyle {
                cols: 1,
                kind: GaugeKind::Cell,
                labels: GaugeLabels::None,
                value_fmt: fmt,
                shrink: 1.0,
            };
            ui::gauge_grid(&mut c, Rect::new(40.0, 40.0, 400.0, 320.0), &values, &st);
        }
        dl.cmds()
            .iter()
            .filter_map(|c| match c {
                DrawCmd::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    })
}

/// The reading of a single-gauge block: the number run, without its unit.
fn number(values: &[f32], fmt: GaugeValueFmt) -> String {
    let all = runs(values, fmt);
    all.iter()
        .find(|s| !s.contains('%'))
        .cloned()
        .unwrap_or_else(|| panic!("no number run in {all:?}"))
}

// ------------------------------------------------------- the decimal mark

/// The owner's first question of `[num]`: a theme that writes `12,50`
/// gets `12,50`.
///
/// Three stages, because two would not separate the two keys involved.
/// The master writes its gauge readouts whole (`decimals_compact = 0`),
/// so a mark cannot show until a theme asks for a fraction — which is
/// itself the second key of this block with no reader before today.
#[test]
fn the_theme_decides_how_a_reading_is_written_down() {
    the_decimal_mark_and_how_many_places();
    where_the_thousands_open_up();
    the_unit_is_a_run_and_not_an_appended_character();
}

fn the_decimal_mark_and_how_many_places() {
    // ---- the master: a whole number and no mark ----------------------
    apply(None);
    let plain = number(&[12.5], GaugeValueFmt::Percent);
    assert_eq!(plain, "12", "the master's gauge readout is whole (decimals_compact = 0)");

    // ---- `decimals_compact` alone: the fraction appears ---------------
    apply(Some(&format!("{HEAD}[num]\ndecimals_compact = 2\n")));
    let with_places = number(&[12.5], GaugeValueFmt::Percent);
    assert_eq!(
        with_places, "12.50",
        "num.decimals_compact moved and the gauge readout did not follow it"
    );

    // ---- and the mark itself -----------------------------------------
    apply(Some(&format!("{HEAD}[num]\ndecimals_compact = 2\ndecimal_sep = \",\"\n")));
    let comma = number(&[12.5], GaugeValueFmt::Percent);
    assert_eq!(
        comma, "12,50",
        "num.decimal_sep = ',' did not reach the gauge readout — the mark is \
         still the one `format!` puts there"
    );
    assert_ne!(comma, with_places, "the two fixtures must differ or nothing is proved");

    apply(None);
}

// ----------------------------------------------------------- thousands

/// `num.group_min` is the length at which an integer starts being
/// grouped, and `num.group_sep` is what it is grouped with.
///
/// The master ships a minimum of five and a thin space, so `1234` stands
/// bare and `12345` becomes `12 345` — the exact pair the key's own
/// comment gives as its example.
fn where_the_thousands_open_up() {
    apply(None);
    let sep = theme::diagnostics().text("num.group_sep").unwrap_or_default().to_string();
    assert!(!sep.is_empty(), "the master ships a thousands separator");

    // Raw, because a percentage never reaches four figures — and the
    // block is about integers, not about the unit.
    let short = number(&[1234.0], GaugeValueFmt::Raw);
    assert_eq!(short, "1234", "four digits are under the master's group_min");
    let long = number(&[12345.0], GaugeValueFmt::Raw);
    assert_eq!(long, format!("12{sep}345"), "five digits are grouped");

    // ---- moved up: the same reading closes back up --------------------
    apply(Some(&format!("{HEAD}[num]\ngroup_min = 9\n")));
    assert_eq!(
        number(&[12345.0], GaugeValueFmt::Raw),
        "12345",
        "num.group_min moved and the grouping did not follow it"
    );

    // ---- and moved down: four digits open up --------------------------
    apply(Some(&format!("{HEAD}[num]\ngroup_min = 3\n")));
    assert_eq!(
        number(&[1234.0], GaugeValueFmt::Raw),
        format!("1{sep}234"),
        "num.group_min = 3 did not open up a four-digit reading"
    );

    // ---- the separator is the theme's too -----------------------------
    apply(Some(&format!("{HEAD}[num]\ngroup_sep = \"'\"\n")));
    assert_eq!(
        number(&[12345.0], GaugeValueFmt::Raw),
        "12'345",
        "num.group_sep did not reach the reading"
    );

    apply(None);
}

// ---------------------------------------------------------------- the unit

/// The unit is a RUN of its own — its own size, its own letters, its own
/// place — which is the half of §5.17 that a string could never carry.
fn the_unit_is_a_run_and_not_an_appended_character() {
    apply(None);

    // The master's `unit.case = upper`, on a unit that has a lower-case
    // form to move: the percent sign has none, so the claim is made
    // through the byte formatter, whose units are letters.
    let upper = fresh(|| nacelle::telemetry::fmt_bytes(2 * 1024 * 1024 * 1024));
    assert_eq!(upper, "2.00 GIB", "num.unit.case = upper did not reach the unit");

    apply(Some(&format!("{HEAD}[num]\nunit.case = none\n")));
    let none = fresh(|| nacelle::telemetry::fmt_bytes(2 * 1024 * 1024 * 1024));
    assert_eq!(none, "2.00 GiB", "num.unit.case = none still upper-cased the unit");

    // The joint between the two, where they are one string: a text token,
    // because a string carries no ems.
    apply(Some(&format!("{HEAD}[num]\nunit.text_gap = \"\"\n")));
    assert_eq!(
        fresh(|| nacelle::telemetry::fmt_bytes(2 * 1024 * 1024 * 1024)),
        "2.00GIB",
        "num.unit.text_gap did not close the joint"
    );

    // `percent_attached` is the same joint on the drawn side: the gauge
    // sets its unit as a second run, and the master closes the gap up
    // before a percent sign.
    apply(None);
    let both = runs(&[12.0], GaugeValueFmt::Percent);
    assert_eq!(both.len(), 2, "a percent reading is a number run and a unit run: {both:?}");
    assert!(both.iter().any(|s| s == "%"), "the unit is its own run: {both:?}");

    apply(None);
}
