//! The row already in force, marked in the open list.
//!
//! With the anchor wearing the LIST'S OWN NAME, the open list is the
//! only place left that can say which member of the set is standing —
//! and until `AccordionStyle::current` there was no rung on it that
//! could. This binary proves the mark exists, that it travels with the
//! index it is given, and that every pixel of it comes off the
//! `menu.item` class's own ladder rather than a colour written into the
//! object.
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

/// How one row was dressed: the wash over the opaque menu bed, and the
/// ring's colour and width. Three numbers is the whole of what a rung
/// can move on a row that draws no shape of its own.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Dress {
    fill: [f32; 4],
    edge: [f32; 4],
    stroke: f32,
}

fn rgba(c: nacelle::theme::Color) -> [f32; 4] {
    [c.r, c.g, c.b, c.a]
}

/// The list drawn once, read back row by row. The pointer is off-screen,
/// so nothing here is hovering and the rungs under test are the resting
/// ones.
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
    // A row is bed, wash, ring — in that order, once each — so the
    // commands come back in threes and the walk needs no row index.
    let mut out = Vec::new();
    let mut fill = None;
    for c in dl.cmds() {
        match c {
            DrawCmd::Rect { color, .. } => fill = Some(rgba(*color)),
            DrawCmd::RectOutline { stroke, color, .. } => {
                let f = fill.take().expect("a ring before any wash: the row order changed");
                out.push(Dress { fill: f, edge: rgba(*color), stroke: *stroke });
            }
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
    // nobody. Three rows, one dress.
    let none = dressed(&mut fonts, None);
    assert_eq!(none.len(), 3, "one bed / wash / ring per row, three rows");
    assert_eq!(none[0], none[1]);
    assert_eq!(none[1], none[2]);

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
    // no colour and no width of its own, so a theme moving the rung
    // moves the mark.
    let t = theme::resolved();
    let class = theme::class_id("menu.item").expect("the master declares the menu.item class");
    let rung = t.class_state(class, State::Selected);
    assert_eq!(mid[1].fill, rgba(rung.fill), "the wash is menu.item's selected fill");
    assert_eq!(mid[1].edge, rgba(rung.edge), "the ring is menu.item's selected edge");
    assert_eq!(
        mid[1].stroke,
        rung.edge_width,
        "the ring's WIDTH off the same rung as its colour — the channel the \
         master thickens for a selection, and the one that keeps the mark \
         legible in a theme whose washes sit close together"
    );
    // The resting rows answer the same way, one rung down: proof the
    // whole row and not just the marked one reads from the ladder.
    let idle = t.class_state(class, State::Idle);
    assert_eq!(none[0].edge, rgba(idle.edge));
    assert_eq!(none[0].stroke, idle.edge_width);
    // And the master really does make the two rungs different — a
    // ladder whose selected rung equalled its idle one would pass every
    // assertion above and show the user nothing.
    assert_ne!(rgba(rung.edge), rgba(idle.edge));
}
