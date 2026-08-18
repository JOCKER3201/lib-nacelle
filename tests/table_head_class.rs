//! A sortable table heading is a control, and a control needs a class.
//!
//! `ui::table_surface` has asked the theme for `table.head` since the
//! table learned to sort (F2 §2.1), and the master's own class matrix
//! documents that ladder — hover, press, and `selected` for the SORTED
//! column. The `[class]` block never declared the row. `theme::class_id`
//! answered `None`, every rung fell back to `StateStyle::RAW`, and
//! `RAW.fill` is transparent: the heading under the pointer drew no
//! plate, the heading under a press drew no plate, and the column the
//! table was sorted by was marked by nothing but its arrow.
//!
//! So this file asks the drawing, not the dictionary: put the pointer on
//! a heading and require a filled rectangle where the heading is. A
//! declaration alone would be a token nobody reads; a rectangle is the
//! whole claim.
//!
//! Drawn through a [`Surface`] probe, so it needs no window and no font
//! atlas — the same trick `tests/list_view.rs` turns.

use nacelle::theme::parse::State;
use nacelle::theme::{self, Color};
use nacelle::ui::{
    table_surface, Align, CellKind, ColWidth, Column, TableStyle, TableView,
};
use nacelle::view::surface::{StateInk, Surface};
use nacelle::view::table::TableState;
use nacelle::view::Hits;
use nacelle::Rect;

/// A surface that answers the REAL theme and records the rectangles it
/// was asked to fill. Text measures at half an em a character: wrong
/// about fonts, right about monotonicity, which is all the column solver
/// asks of it.
#[derive(Default)]
struct Probe {
    rects: Vec<(Rect, Color)>,
    mouse: (f32, f32),
}

impl Surface for Probe {
    fn rect(&mut self, r: Rect, c: Color) {
        self.rects.push((r, c));
    }
    fn rect_outline(&mut self, _r: Rect, _w: f32, _c: Color) {}
    fn line(&mut self, _x0: f32, _y0: f32, _x1: f32, _y1: f32, _w: f32, _c: Color) {}
    fn polyline(&mut self, _pts: &[[f32; 2]], _w: f32, _c: Color, _closed: bool) {}
    fn text(&mut self, _f: u8, _px: f32, _x: f32, _y: f32, _s: &str, _c: Color, _t: f32, _a: Align) {
    }
    fn measure(&mut self, _face: u8, px: f32, s: &str, _track: f32) -> f32 {
        s.chars().count() as f32 * px * 0.5
    }
    fn clip(&mut self, _r: Rect) -> bool {
        true
    }
    fn unclip(&mut self) {}
    fn has_token(&mut self, name: &str) -> bool {
        theme::id(name).is_some()
    }
    fn px(&mut self, name: &str) -> f32 {
        theme::resolved().px(theme::id(name).unwrap_or(theme::TokenId::MISSING))
    }
    fn color(&mut self, name: &str) -> Color {
        theme::resolved().color(theme::id(name).unwrap_or(theme::TokenId::MISSING))
    }
    fn bed(&mut self, name: &str) -> Color {
        theme::resolved().bed(theme::id(name).unwrap_or(theme::TokenId::MISSING))
    }
    fn flag(&mut self, name: &str) -> bool {
        theme::resolved().flag(theme::id(name).unwrap_or(theme::TokenId::MISSING))
    }
    fn word(&mut self, name: &str) -> String {
        theme::id(name).and_then(theme::enum_word_of).unwrap_or_default()
    }
    /// The real theme's answer, like every other kind above. A probe that
    /// answered nothing here would say "this theme states no trim
    /// marker", which is a case worth its own test and not the state a
    /// probe of the SHIPPED master should be in.
    fn theme_text(&mut self, name: &str) -> String {
        theme::diagnostics().text(name).unwrap_or_default().to_string()
    }
    fn class_state(&mut self, class: &str, state: State) -> StateInk {
        match theme::class_id(class) {
            Some(c) => StateInk::from(theme::resolved().class_state(c, state)),
            None => StateInk::raw(),
        }
    }
    fn epoch(&mut self) -> u32 {
        theme::epoch()
    }
    fn now(&self) -> f64 {
        0.0
    }
    fn mouse(&self) -> (f32, f32) {
        self.mouse
    }
    fn scale(&self) -> f32 {
        1.0
    }
}

const AREA: Rect = Rect { x: 10.0, y: 20.0, w: 400.0, h: 200.0 };

fn columns() -> Vec<Column> {
    ["PID", "COMMAND"]
        .into_iter()
        .map(|title| Column {
            title: title.to_string(),
            align: Align::Left,
            kind: CellKind::Text,
            width: ColWidth::Content,
        })
        .collect()
}

fn rows() -> Vec<Vec<String>> {
    [["1471", "firefox"], ["22", "nacelle-desktop"]]
        .into_iter()
        .map(|r| r.iter().map(|s| s.to_string()).collect())
        .collect()
}

/// One drawing, on a thread of its own.
///
/// The state crossfade keeps its tracks in a THREAD-LOCAL registry
/// (`motion::state_mix`), and a first sighting is born settled at the
/// rung it is asked for. Two draws of one table on one thread would
/// therefore be a first sighting and a transition, and the transition
/// starts at what the last frame drew — which is exactly the fade this
/// file must not measure. One thread per question keeps every draw a
/// first sighting.
fn draw(mouse: (f32, f32), interactive: bool) -> Vec<(Rect, Color)> {
    std::thread::scope(|s| {
        s.spawn(move || {
            theme::load();
            let mut sf = Probe { mouse, ..Probe::default() };
            let mut state = TableState::default();
            let mut hits = Hits::new();
            let cols = columns();
            let body = rows();
            let st = TableStyle {
                elastic: 1,
                zebra: false,
                severity_col: None,
                shrink: 1.0,
            };
            table_surface(
                &mut sf,
                AREA,
                &cols,
                &body,
                &st,
                Some(TableView {
                    state: &mut state,
                    hits: &mut hits,
                    id: 1,
                    generation: 0,
                    interactive,
                    select: false,
                    key_col: None,
                    scroll: false,
                    tooltip: false,
                }),
            );
            sf.rects
        })
        .join()
        .expect("the drawing thread panicked")
    })
}

/// The heading band starts at the table's top edge and is `table.head_gap`
/// tall (`ui.rs`: `Rect::new(x, r.y, w, band_h)`), so a rectangle drawn
/// AT that edge is a heading's plate and nothing else — the rule below it
/// is a line, and the first body row starts a `head_gap_below` further
/// down.
fn plates_on_the_heading_row(rects: &[(Rect, Color)]) -> Vec<(Rect, Color)> {
    rects
        .iter()
        .filter(|(r, c)| (r.y - AREA.y).abs() < 0.01 && c.a > 0.0)
        .copied()
        .collect()
}

#[test]
fn the_heading_under_the_pointer_wears_the_table_head_class() {
    // The dictionary half: the master must declare the class the code has
    // always asked for. Stated first because if this fails the drawing
    // below fails for a reason nobody could read off the pixels.
    let declared = std::thread::scope(|s| {
        s.spawn(|| {
            theme::load();
            theme::class_id("table.head").is_some()
        })
        .join()
        .expect("the loading thread panicked")
    });
    assert!(
        declared,
        "`ui::table_surface` asks the theme for the class `table.head`; the master's \
         [class] block does not declare it, so class_id answers None and every rung of \
         a heading falls back to StateStyle::RAW"
    );

    // The drawing half. Away from the table first: a resting heading has
    // never drawn a plate and must not start now.
    let resting = plates_on_the_heading_row(&draw((-1000.0, -1000.0), true));
    assert!(
        resting.is_empty(),
        "a table nobody is pointing at grew a band under its headings: {resting:?}"
    );

    // Now on the first heading — inside the band, which runs from the
    // table's top edge down by `table.head_gap`.
    let hovered = plates_on_the_heading_row(&draw((AREA.x + 4.0, AREA.y + 4.0), true));
    assert!(
        !hovered.is_empty(),
        "the heading under the pointer drew no plate at all: with `table.head` \
         undeclared the hover rung is StateStyle::RAW, whose fill is transparent, so \
         the wash the class matrix promises never reaches a pixel"
    );
    let (band, fill) = hovered[0];
    assert!(
        band.x <= AREA.x + 4.0 && band.x + band.w > AREA.x + 4.0,
        "the plate is not the heading the pointer is on: {band:?}"
    );
    assert!(
        fill.a > 0.0,
        "the heading's plate was drawn in a transparent fill, which is RAW wearing \
         the class's clothes: {fill:?}"
    );

    // And the same heading under the same pointer draws NOTHING when the
    // table was never told it is interactive: the class answers for a
    // control, and a heading that cannot be clicked is not one. (This is
    // what keeps the master's own read-only tables looking untouched.)
    let inert = plates_on_the_heading_row(&draw((AREA.x + 4.0, AREA.y + 4.0), false));
    assert!(
        inert.is_empty(),
        "a table without `interactive` answered the pointer anyway: {inert:?}"
    );
}
