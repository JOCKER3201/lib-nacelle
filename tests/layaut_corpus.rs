//! The golden corpus of the `.layaut` format (u3 §6.1): one case per
//! feature, each asserting the parse AND the round-trip — parse →
//! serialise → parse gives an equivalent LayoutDef — which is what
//! protects the save path. The format moved from the desktop's
//! config.rs; not one byte of syntax, not one default, not one
//! tolerance may drift.

use nacelle::base::{self, LayoutMode, Panel};
use nacelle::layout::{layaut, LayoutDef};

fn setup() {
    // FIRST call wins and panel indices bake into every Layout; the
    // corpus resolves names against the builtin twelve.
    base::set_registry(base::builtin_widgets());
}

fn p(name: &str) -> Panel {
    Panel::from_name(name).expect(name)
}

/// Equivalence through the printer: two defs that print the same ARE
/// the same file to every writer in the program. A Flex base prints
/// the built-in written out (with its banner comment) and reparses as
/// the equivalent Custom, so the fixed point is reached after ONE
/// round — from there, parse → print must never drift again.
fn round_trips(def: &LayoutDef) {
    let t1 = layaut::print(def);
    let d1 = layaut::parse(&t1, "corpus");
    let t2 = layaut::print(&d1);
    let d2 = layaut::parse(&t2, "corpus");
    assert_eq!(layaut::print(&d2), t2, "parse(print(def)) drifted");
}

#[test]
fn legacy_fixed_with_comments_and_suffixes() {
    setup();
    let text = "\
# a hand-written file\n\
clock = 1.5 2.0 16.0 7.0\n\
shell = 20 5 60 60 # trailing comment\n";
    let def = layaut::parse(text, "corpus");
    let LayoutMode::Fixed(spec) = &def.base else {
        panic!("legacy rectangles must parse as Fixed")
    };
    let c = spec.p(p("clock"));
    assert_eq!((c.x, c.y, c.w, c.h), (1.5, 2.0, 16.0, 7.0));
    assert!(def.overrides.is_empty() && def.boards.is_empty());
    round_trips(&def);
}

#[test]
fn flexbox_columns_with_sizes() {
    setup();
    let text = "\
units = du\n\
[column]\n\
basis = 16.4\n\
min = 168\n\
max = 340\n\
grow = 0\n\
collapse = 2\n\
gap = 1.0\n\
panel = clock 7.0\n\
panel = cpu 15.5 ref 12.0 min 8.0\n\
[column]\n\
basis = 65\n\
grow = 1\n\
panel = shell 60.3\n";
    let def = layaut::parse(text, "corpus");
    let LayoutMode::Custom(fl) = &def.base else {
        panic!("[column] must parse as Custom")
    };
    assert_eq!(fl.columns.len(), 2);
    assert!(!fl.units_px);
    assert_eq!(fl.columns[0].panels.len(), 2);
    // The per-layout size override rides on the panel line.
    assert!(def
        .sizes
        .iter()
        .any(|(pp, r, m)| *pp == p("cpu") && *r == 12.0 && *m == 8.0));
    round_trips(&def);
}

#[test]
fn base_screen_and_two_override_sections() {
    setup();
    let text = "\
screen = 1920x1080@27\n\
clock = 1.5 2.0 16.0 7.0\n\
[1920x1080@27]\n\
clock = 2.0 2.0 16.0 7.0\n\
[2560x1440@32]\n\
shell = 20 5 60 60\n";
    let def = layaut::parse(text, "corpus");
    assert_eq!(layaut::base_screen_of(text), Some((1920, 1080, 27)));
    assert_eq!(def.overrides.len(), 2);
    assert!(def.pick((2560, 1440, 32)).is_some());
    assert!(def.pick((1024, 768, 17)).is_none());
    round_trips(&def);
}

#[test]
fn board_headers_one_and_two_numbers() {
    setup();
    let text = "\
clock = 1 1 10 10\n\
[board -1]\n\
shell = 10 10 50 50\n\
[board 2]\n\
[board 0 -1]\n\
cpu = 5 5 20 20\n\
[board 0 1]\n\
memory = 5 5 20 20\n";
    let def = layaut::parse(text, "corpus");
    let ids: Vec<_> = def.boards.iter().map(|(k, _)| *k).collect();
    // Horizontal arm renumbers contiguously: the hand-written [board 2]
    // with nothing at 1 becomes the first board on the right.
    assert!(ids.contains(&(-1, 0)));
    assert!(ids.contains(&(1, 0)), "gap must close: {ids:?}");
    assert!(ids.contains(&(0, -1)) && ids.contains(&(0, 1)));
    round_trips(&def);
}

#[test]
fn hostile_input_degrades_and_never_panics() {
    setup();
    let text = "\
[board 0 0]\n\
[board 1 1]\n\
[board 9]\n\
clock = nan inf 3\n\
unknownpanel = 1 2 3 4\n\
[column]\n\
basis = what\n\
unknownkey = 7\n";
    let def = layaut::parse(text, "corpus");
    // [board 0 0] is home and refused; [board 1 1] is a diagonal no
    // gesture reaches; [board 9] normalises to the first free slot.
    let ids: Vec<_> = def.boards.iter().map(|(k, _)| *k).collect();
    assert!(!ids.contains(&(0, 0)));
    assert!(!ids.contains(&(1, 1)));
    round_trips(&def);
}

#[test]
fn overrides_only_uses_the_builtin_base() {
    setup();
    let text = "\
[1920x1080@27]\n\
clock = 2.0 2.0 16.0 7.0\n";
    let def = layaut::parse(text, "corpus");
    assert!(matches!(def.base, LayoutMode::Flex));
    assert_eq!(def.overrides.len(), 1);
    round_trips(&def);
}

#[test]
fn all_columns_empty_degrades_to_default() {
    setup();
    let text = "\
[column]\n\
basis = 20\n\
[column]\n\
basis = 30\n";
    let def = layaut::parse(text, "corpus");
    // No panel lines anywhere: parse_flex refuses and the base
    // degrades to the built-in default, saying so on stderr.
    assert!(matches!(def.base, LayoutMode::Flex));
    round_trips(&def);
}

#[test]
fn units_px_survives_the_round_trip() {
    setup();
    let text = "\
units = px\n\
[column]\n\
basis = 16.4\n\
min = 168\n\
panel = clock 7.0\n";
    let def = layaut::parse(text, "corpus");
    let LayoutMode::Custom(fl) = &def.base else { panic!() };
    assert!(fl.units_px);
    round_trips(&def);
}
