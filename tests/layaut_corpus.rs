//! The golden corpus of the `.layaut` format (u3 §6.1): one case per
//! feature, each asserting the parse AND the round-trip — parse →
//! serialise → parse gives an equivalent LayoutDef — which is what
//! protects the save path. Not one byte of syntax, not one default, not
//! one tolerance may drift.
//!
//! Half the corpus is written in version 1, the grammar from before a
//! placement carried an identity. Those cases are the migration's
//! contract: an existing file must still load, and must still mean what
//! it meant.

use nacelle::base::{self, LayoutMode, Panel, WidgetCategory, WidgetDef};
use nacelle::layout::{layaut, InstanceId, LayoutDef};
use nacelle::widget::registry;

/// The corpus resolves panel names against a FIXTURE registry of its
/// own. There is no built-in set to borrow: a registry is whatever an
/// installation's addons declare, so a test that needs names has to
/// bring them. FIRST call wins and panel indices bake into every
/// Layout, so this is the one registry this binary ever has.
fn setup() {
    base::set_registry(
        ["clock", "cpu", "memory", "shell"]
            .iter()
            .map(|name| {
                // An addon that declares nothing but its heights: the
                // corpus is about the file format, not about placement.
                let mut def: WidgetDef = registry::bare_def((*name).to_string());
                def.ref_h_vh = 10.0;
                def.min_h_vh = 6.0;
                assert_eq!(def.category, WidgetCategory::Board);
                def
            })
            .collect(),
    );
}

fn p(name: &str) -> Panel {
    Panel::from_name(name).expect(name)
}

/// The instances of one board as (widget name, rectangle) — what the
/// old per-widget table used to be able to say, so the version 1 cases
/// can be checked in the terms they were written in.
fn placed(def: &LayoutDef, board: (i32, i32)) -> Vec<(&'static str, [f32; 4])> {
    def.board_instances(board)
        .into_iter()
        .map(|i| {
            let r = i.rect.expect("a rectangle board's instance has a rectangle");
            (i.widget.name(), [r.x, r.y, r.w, r.h])
        })
        .collect()
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
    assert!(matches!(def.base, LayoutMode::Rects), "rectangles must parse as Rects");
    assert_eq!(
        placed(&def, (0, 0)),
        [("clock", [1.5, 2.0, 16.0, 7.0]), ("shell", [20.0, 5.0, 60.0, 60.0])]
    );
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
    assert_eq!(def.base_screen, Some((1920, 1080, 27)));
    assert_eq!(layaut::base_screen_of(text), Some((1920, 1080, 27)));
    assert_eq!(def.overrides.len(), 2);
    assert!(def.pick((2560, 1440, 32)).is_some());
    assert!(def.pick((1024, 768, 17)).is_none());
    // A version 1 override named a widget; it can only ever have meant
    // the one instance of it on home, and that is what it now moves.
    let clock = def.instances.first_of(p("clock")).unwrap();
    assert_eq!(def.pick((1920, 1080, 27)).unwrap().rects[0].0, clock);
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
    // And the widgets went with their boards, not with their positions.
    assert_eq!(placed(&def, (-1, 0)), [("shell", [10.0, 10.0, 50.0, 50.0])]);
    assert_eq!(placed(&def, (0, -1)), [("cpu", [5.0, 5.0, 20.0, 20.0])]);
    assert_eq!(placed(&def, (0, 1)), [("memory", [5.0, 5.0, 20.0, 20.0])]);
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
    // The generated base places one instance per installed widget, so
    // an override written against a widget name still finds one.
    assert_eq!(def.overrides[0].rects.len(), 1);
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

// ---------------------------------------------------------------------
// Version 2: placements carry an identity
// ---------------------------------------------------------------------

/// The whole point of the grammar: one widget, two rectangles, on one
/// board. A version 1 file could not write this line down at all.
#[test]
fn one_widget_twice_on_one_board() {
    setup();
    let text = "\
version = 2\n\
next_instance = 9\n\
shell#4 = 0 0 45 90\n\
shell#7 = 50 0 45 90\n";
    let def = layaut::parse(text, "corpus");
    assert_eq!(def.instances.count_of(p("shell")), 2);
    let rects = placed(&def, (0, 0));
    assert_eq!(rects, [("shell", [0.0, 0.0, 45.0, 90.0]), ("shell", [50.0, 0.0, 45.0, 90.0])]);
    // The two are told apart by identity, and the identities are the
    // file's own.
    let ids: Vec<u32> = def.instances.iter().map(|i| i.id.get()).collect();
    assert_eq!(ids, [4, 7]);
    round_trips(&def);
}

/// The same, in a flexbox column: two terminals stacked in the work
/// surface, each with its own weight.
#[test]
fn one_widget_twice_in_one_column() {
    setup();
    let text = "\
version = 2\n\
next_instance = 3\n\
units = du\n\
[column]\n\
basis = 65\n\
grow = 1\n\
panel = shell#1 60\n\
panel = shell#2 40\n";
    let def = layaut::parse(text, "corpus");
    let LayoutMode::Custom(fl) = &def.base else { panic!() };
    assert_eq!(fl.columns[0].panels.len(), 2);
    assert_eq!(fl.columns[0].panels[0].id, InstanceId::new(1));
    assert_eq!(fl.columns[0].panels[1].id, InstanceId::new(2));
    assert_eq!(fl.columns[0].panels[1].weight, 40.0);
    round_trips(&def);
}

/// Identities survive a save and a load unchanged — which is the only
/// reason anything else may hold on to one.
#[test]
fn identities_are_stable_across_save_and_load() {
    setup();
    let text = "\
version = 2\n\
next_instance = 40\n\
clock#11 = 1 1 10 10\n\
shell#22 = 20 5 60 60\n\
[board 1]\n\
cpu#33 = 5 5 20 20\n";
    let def = layaut::parse(text, "corpus");
    let before: Vec<(u32, &str, (i32, i32))> = def
        .instances
        .iter()
        .map(|i| (i.id.get(), i.widget.name(), i.board))
        .collect();
    let again = layaut::parse(&layaut::write_file(&def), "corpus");
    let after: Vec<(u32, &str, (i32, i32))> = again
        .instances
        .iter()
        .map(|i| (i.id.get(), i.widget.name(), i.board))
        .collect();
    assert_eq!(before, after);
    assert_eq!(before, [(11, "clock", (0, 0)), (22, "shell", (0, 0)), (33, "cpu", (1, 0))]);
    // And the promise about the free ids travels with the file.
    assert_eq!(again.instances.next_free(), 40);
}

/// Removing the middle instance moves nobody, and its id stays retired
/// across the file — the property an index into a vector cannot give.
#[test]
fn removing_the_middle_instance_leaves_the_rest_alone() {
    setup();
    let mut def = layaut::parse(
        "version = 2\nnext_instance = 4\nclock#1 = 0 0 10 10\ncpu#2 = 0 20 10 10\nshell#3 = 0 40 10 10\n",
        "corpus",
    );
    assert!(def.instances.remove(InstanceId::new(2)));
    let again = layaut::parse(&layaut::write_file(&def), "corpus");
    assert_eq!(
        placed(&again, (0, 0)),
        [("clock", [0.0, 0.0, 10.0, 10.0]), ("shell", [0.0, 40.0, 10.0, 10.0])]
    );
    let ids: Vec<u32> = again.instances.iter().map(|i| i.id.get()).collect();
    assert_eq!(ids, [1, 3], "the survivors keep their identities");
    // The freed id is not reused: the next widget dragged out gets 4.
    let mut list = again.instances.clone();
    assert_eq!(list.add(p("cpu"), (0, 0), None), InstanceId::new(4));
}

/// The GENERATED arrangement is composed, never written down: its
/// placements carry identities of their own, but a file that keeps the
/// generated base names them the way it always did — by widget — so
/// installing an addon still changes what home shows.
#[test]
fn a_composed_placement_is_written_by_name_and_a_saved_one_by_identity() {
    setup();
    let def = layaut::parse("[1920x1080@27]\nclock = 2 2 16 7\n", "corpus");
    assert!(matches!(def.base, LayoutMode::Flex));
    assert!(
        def.instances.iter().all(|i| i.id.is_generated()),
        "a generated base has no saved identities"
    );
    let text = layaut::write_file(&def);
    assert!(text.contains("clock = 2.00"), "written by name: {text}");
    assert!(!text.contains("clock#"), "no composed id may reach the file: {text}");
    // The counter is untouched by composing, so the first widget the
    // user drags out is instance 1 and not instance 5.
    assert_eq!(def.instances.next_free(), 1);
    // Reading it back composes the very same identities again.
    let again = layaut::parse(&text, "corpus");
    let ids = |d: &LayoutDef| -> Vec<u32> { d.instances.iter().map(|i| i.id.get()).collect() };
    assert_eq!(ids(&def), ids(&again));
}

/// Arranging a board yourself is what turns its composed placements
/// into saved ones: each gets an ordinary identity, and everything in
/// the definition that named it follows. A board still showing the
/// generated arrangement keeps composing, so installing an addon goes
/// on changing what it shows.
#[test]
fn materialising_only_touches_the_boards_that_save_their_own() {
    setup();
    let mut def = layaut::parse("[1920x1080@27]\nclock = 2 2 16 7\n", "corpus");
    assert!(def.materialize().is_empty(), "a generated base composes, it does not save");
    assert!(def.instances.iter().all(|i| i.id.is_generated()));

    // The user arranges home himself: rectangles, and now they are his.
    let was = def.overrides[0].rects[0].0;
    def.base = LayoutMode::Rects;
    for id in def.instances.iter().map(|i| i.id).collect::<Vec<_>>() {
        def.instances.set_rect(id, Some(base::PanelSpec { x: 1.0, y: 1.0, w: 10.0, h: 10.0 }));
    }
    let map = def.materialize();
    assert_eq!(map.len(), def.instances.len());
    assert!(def.instances.iter().all(|i| !i.id.is_generated()));
    let now = map.iter().find(|(w, _)| *w == was).unwrap().1;
    assert_eq!(def.overrides[0].rects[0].0, now, "the section followed its instance");
    // Saved identities survive the write, unlike the composed ones.
    let again = layaut::parse(&layaut::write_file(&def), "corpus");
    assert!(again.instances.get(now).is_some());
    round_trips(&again);
}

/// A per-screen section in the new grammar moves ONE instance — the
/// second terminal on the 4K monitor, not "the terminal".
#[test]
fn a_screen_section_moves_one_instance() {
    setup();
    let text = "\
version = 2\n\
next_instance = 3\n\
shell#1 = 0 0 45 90\n\
shell#2 = 50 0 45 90\n\
[3840x2160@32]\n\
shell#2 = 60 0 38 90\n";
    let def = layaut::parse(text, "corpus");
    let ov = def.pick((3840, 2160, 32)).expect("the section");
    assert_eq!(ov.rects.len(), 1);
    assert_eq!(ov.rects[0].0, InstanceId::new(2));
    // And the solve puts that one — and only that one — where the
    // section says.
    let t = base::size_table();
    let lay = def.solve(3840.0, 2160.0, 8.0, (3840, 2160, 32), &t);
    assert!((lay.of(InstanceId::new(2)).x - 3840.0 * 0.60).abs() < 0.5);
    assert!((lay.of(InstanceId::new(1)).x - 0.0).abs() < 0.5);
}
