//! The row already in force, marked in the open list.
//!
//! With the anchor wearing the LIST'S OWN NAME, the open list is the
//! only place left that can say which member of the set is standing —
//! and until `AccordionStyle::current` there was no rung on it that
//! could. This binary proves the mark exists, that it travels with the
//! index it is given, and that every pixel of it comes off the
//! `list.item` class's own ladder rather than a colour written into the
//! object.
//!
//! The mark is a PLATE laid under the row, not a ring drawn around it:
//! a ring is a box, and a column of boxes is what an open list must not
//! look like. Which shape the plate is cut to, and that a resting row
//! wears none at all, is [`dropdown_list_dress`]'s subject; this one is
//! about WHICH row wears it.
//!
//! A binary of its own because the resolved theme is process-wide (§7.1
//! hands every draw path the same `&'static ResolvedTheme`): a test that
//! loads a theme must not run beside one that swaps it.

use nacelle::draw::{DrawCmd, DrawList};
use nacelle::font::FontSystem;
use nacelle::object::dropdown::{self, AccordionStyle};
use nacelle::theme::{self, parse::State};
use nacelle::{Ctx, Rect};

const W: f32 = 1920.0;
const H: f32 = 1080.0;
const ROW_H: f32 = 30.0;
/// The anchor the list hangs from, wide enough that no width floor can
/// move it and clear of the screen edges.
const ANCHOR: Rect = Rect { x: 200.0, y: 300.0, w: 400.0, h: 36.0 };

/// How one row was dressed: the plate laid over the opaque bed — `None`
/// for a row the ladder marks in no way — and the ink its label is set
/// in. Two channels is the whole of what a rung can move on a row that
/// draws no shape of its own.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Dress {
    plate: Option<[f32; 4]>,
    text: [f32; 4],
}

fn rgba(c: nacelle::theme::Color) -> [f32; 4] {
    [c.r, c.g, c.b, c.a]
}

/// The list drawn once, read back row by row. The pointer is off-screen,
/// so nothing here is hovering and the rungs under test are the resting
/// ones.
///
/// A row is bed, then at most one plate, then its label — in that
/// order — so a new bed opens a new row and the walk needs no index.
fn dressed(fonts: &mut FontSystem, current: Option<usize>) -> Vec<Dress> {
    let names: Vec<String> = ["ALPHA", "BETA", "GAMMA"].iter().map(|s| s.to_string()).collect();
    let mut dl = DrawList::recording();
    {
        let mut ctx = Ctx {
            dl: &mut dl,
            fonts,
            w: W,
            h: H,
            t: 0.0,
            mouse: (-1.0, -1.0),
            term_font_scale: 1.0,
            ui_font_scale: 1.0,
            panel_scale: 1.0,
            focus: None,
            tips: None,
        };
        dropdown::accordion(
            &mut ctx,
            ANCHOR,
            ROW_H,
            &names,
            1.0,
            &AccordionStyle { current, ..AccordionStyle::default() },
        );
    }
    let mut out: Vec<Dress> = Vec::new();
    let mut plate: Option<[f32; 4]> = None;
    for c in dl.cmds() {
        match c {
            // A bed: the row before this one is finished.
            DrawCmd::Rect { .. } => plate = None,
            DrawCmd::RingFill { color, .. } => plate = Some(rgba(*color)),
            DrawCmd::Text { color, .. } => out.push(Dress { plate, text: rgba(*color) }),
            _ => {}
        }
    }
    out
}

#[test]
fn the_row_in_force_wears_the_ladders_selected_rung_and_its_neighbours_do_not() {
    let _ = theme::load();
    theme::set_viewport(H, 1.0);
    let mut fonts = FontSystem::new();

    // Nothing in force: a set with no standing member says so by marking
    // nobody. Three rows, one dress, and that dress is bare.
    let none = dressed(&mut fonts, None);
    assert_eq!(none.len(), 3, "one bed / label per row, three rows");
    assert_eq!(none[0], none[1]);
    assert_eq!(none[1], none[2]);
    assert_eq!(none[0].plate, None, "a row nothing is true of still wears a plate");

    // The middle one in force: it and only it changes.
    let mid = dressed(&mut fonts, Some(1));
    assert_eq!(mid[0], none[0], "an untouched row must not move");
    assert_eq!(mid[2], none[2]);
    assert_ne!(
        mid[1], none[1],
        "the row in force is drawn exactly like the rows that are not — \
         which is the window with no current theme visible anywhere in it"
    );

    // ...and the mark travels with the index rather than sticking to a
    // position, which a mark drawn from the wrong end would not.
    let last = dressed(&mut fonts, Some(2));
    assert_eq!(last[2], mid[1], "the mark is the same dress wherever it lands");
    assert_eq!(last[0], none[0]);
    assert_eq!(last[1], none[1]);

    // Every channel of it off the class's own ladder: the object states
    // no colour of its own, so a theme moving the rung moves the mark.
    let t = theme::resolved();
    let class = theme::class_id("list.item").expect("the master declares the list.item class");
    let rung = t.class_state(class, State::Selected);
    assert_eq!(
        mid[1].plate,
        Some(rgba(rung.fill)),
        "the plate is not list.item's selected fill"
    );
    assert_eq!(mid[1].text, rgba(rung.text), "the label is not list.item's selected ink");
    // The resting rows answer the same way, one rung down: proof the
    // whole row and not just the marked one reads from the ladder.
    let idle = t.class_state(class, State::Idle);
    assert_eq!(none[0].text, rgba(idle.text));
    // And the master really does make the two rungs different — a
    // ladder whose selected rung equalled its idle one would pass every
    // assertion above and show the user nothing.
    assert_ne!(rgba(rung.fill), rgba(idle.fill));
    assert_ne!(rgba(rung.text), rgba(idle.text));
}
