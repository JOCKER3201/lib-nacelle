//! The tab strip and the segmented control, actually drawn.
//!
//! The unit tests hold the arithmetic still; this one runs the drawing
//! through the real master theme and looks at what came out. It needs no
//! window and no font atlas because both objects draw through a
//! [`Surface`] — a probe that records quads is as good a surface as a
//! GPU.

use nacelle::object::{segmented, tabs};
use nacelle::theme::parse::State;
use nacelle::theme::{self, Color};
use nacelle::ui::Align;
use nacelle::view::surface::{StateInk, Surface};
use nacelle::view::{Hit, Hits};
use nacelle::Rect;

/// A surface that answers the REAL theme and records what it was asked
/// to draw. Text is measured at half an em a character: wrong about
/// fonts, right about monotonicity, which is all the trimming asks.
#[derive(Default)]
struct Probe {
    rects: Vec<(Rect, Color)>,
    quads: Vec<([[f32; 2]; 4], Color)>,
    /// Chamfered fills and frames: rect, cut, stroke (0 for a fill).
    chamfers: Vec<(Rect, f32, f32)>,
    texts: Vec<(f32, f32, String, Align)>,
    lines: Vec<(f32, f32, f32, f32, f32, Color)>,
    polys: Vec<Vec<[f32; 2]>>,
    mouse: (f32, f32),
}

impl Surface for Probe {
    fn rect(&mut self, r: Rect, c: Color) {
        self.rects.push((r, c));
    }
    fn rect_outline(&mut self, _r: Rect, _w: f32, _c: Color) {}
    fn line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, w: f32, c: Color) {
        self.lines.push((x0, y0, x1, y1, w, c));
    }
    fn polyline(&mut self, pts: &[[f32; 2]], _w: f32, _c: Color, _closed: bool) {
        self.polys.push(pts.to_vec());
    }
    fn quad(&mut self, pts: [[f32; 2]; 4], c: Color) {
        self.quads.push((pts, c));
    }
    fn text(&mut self, _px: f32, x: f32, y: f32, s: &str, _c: Color, _t: f32, a: Align) {
        self.texts.push((x, y, s.to_string(), a));
    }
    fn measure(&mut self, px: f32, s: &str, _track: f32) -> f32 {
        s.chars().count() as f32 * px * 0.5
    }
    fn clip(&mut self, _r: Rect) -> bool {
        true
    }
    fn unclip(&mut self) {}
    fn chamfer_fill(&mut self, r: Rect, cut: f32, _c: Color) {
        self.chamfers.push((r, cut, 0.0));
    }
    fn chamfer_frame(&mut self, r: Rect, cut: f32, w: f32, _c: Color) {
        self.chamfers.push((r, cut, w));
    }
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

fn px(name: &str) -> f32 {
    theme::resolved().px(theme::id(name).unwrap())
}

fn ladder(class: &str, state: State) -> StateInk {
    StateInk::from(theme::resolved().class_state(theme::class_id(class).unwrap(), state))
}

const AREA: Rect = Rect { x: 10.0, y: 20.0, w: 600.0, h: 60.0 };

// ------------------------------------------------------------ tab strip

#[test]
fn a_strip_wants_a_tab_the_gap_under_it_and_the_rule() {
    let mut sf = Probe::default();
    assert_eq!(
        tabs::natural_h(&mut sf),
        px("tab.h") + px("tab.rule_gap") + px("tab.rule"),
        "the master draws the rule, so the box is taller than one tab",
    );
}

#[test]
fn tabs_are_measured_from_their_labels_and_laid_from_the_left() {
    let labels = ["ONE", "TWO AND A HALF", "III"];
    let mut sf = Probe::default();
    let cells = tabs::strip(
        &mut sf,
        AREA,
        &labels,
        &tabs::StripState::new(0),
        &tabs::StripStyle::default(),
        None,
    );
    assert_eq!(cells.len(), 3);
    // Top of the box, the theme's own height.
    for c in &cells {
        assert_eq!(c.y, AREA.y);
        assert!((c.h - px("tab.h")).abs() < 0.01);
    }
    // The middle label is the longest, so its tab is the widest — a
    // strip is content-measured, not divided.
    assert!(cells[1].w > cells[0].w && cells[1].w > cells[2].w);
    // Laid left to right from the box's left edge, `tab.gap` apart.
    assert_eq!(cells[0].x, AREA.x);
    let gap = px("tab.gap");
    assert!((cells[1].x - (cells[0].right() + gap)).abs() < 0.01);
    assert!((cells[2].x - (cells[1].right() + gap)).abs() < 0.01);
    // Nothing stretched: three tabs are three tabs wide, not the box's.
    let used = cells[2].right() - cells[0].x;
    assert!(used < AREA.w, "a short strip does not fill its box: {used}");
    // One label a tab, centred, untrimmed — there was room for all of it.
    assert_eq!(sf.texts.len(), 3);
    for (i, l) in labels.iter().enumerate() {
        assert_eq!(sf.texts[i].2, *l);
        assert_eq!(sf.texts[i].3, Align::Center);
        assert!((sf.texts[i].0 - cells[i].cx()).abs() < 0.01);
    }
}

#[test]
fn every_tab_is_a_sheared_plate_and_the_rule_runs_under_the_whole_strip() {
    let mut sf = Probe::default();
    let cells = tabs::strip(
        &mut sf,
        AREA,
        &["ONE", "TWO"],
        &tabs::StripState::new(0),
        &tabs::StripStyle::default(),
        None,
    );
    let skew = px("tab.skew");
    assert!(skew > 0.0);
    assert_eq!(sf.quads.len(), 2, "one plate a tab");
    let (q, _) = sf.quads[0];
    assert_eq!(q[0], [cells[0].x + skew, cells[0].y], "the top edge leans right");
    assert_eq!(q[3], [cells[0].x, cells[0].bottom()]);
    // The rule spans the tabs that were drawn, at `tab.rule_gap` under
    // them — exactly where the shell's own strip puts it.
    let rule = sf
        .lines
        .iter()
        .find(|l| (l.4 - px("tab.rule")).abs() < 0.01)
        .expect("the master draws a rule");
    assert!((rule.1 - (AREA.y + px("tab.h") + px("tab.rule_gap"))).abs() < 0.01);
    assert_eq!(rule.0, cells[0].x);
    assert!((rule.2 - cells[1].right()).abs() < 0.01);
}

#[test]
fn the_showing_tab_wears_the_selected_rung_and_its_underline() {
    let mut sf = Probe::default();
    let st = tabs::StripState { active: 1, hover: Some(0), flash: None };
    let cells = tabs::strip(
        &mut sf,
        AREA,
        &["ONE", "TWO", "THREE"],
        &st,
        &tabs::StripStyle::default(),
        None,
    );
    // Every plate's fill is the rung the state says, straight from the
    // `tab` ladder: hover, selected, idle.
    for (i, want) in [State::Hover, State::Selected, State::Idle].iter().enumerate() {
        let got = sf.quads[i].1;
        let ink = ladder("tab", *want).fill;
        assert_eq!((got.r, got.g, got.b, got.a), (ink.r, ink.g, ink.b, ink.a), "tab {i}");
    }
    // Exactly one underline, on the showing tab, inside its own plate.
    let uw = px("tab.underline_active");
    let unders: Vec<_> = sf.lines.iter().filter(|l| (l.4 - uw).abs() < 0.01).collect();
    assert_eq!(unders.len(), 1);
    let u = unders[0];
    assert_eq!(u.0, cells[1].x);
    assert!((u.2 - (cells[1].right() - px("tab.skew"))).abs() < 0.01);
    assert!((u.1 - (cells[1].bottom() - uw / 2.0)).abs() < 0.01);
}

#[test]
fn a_crowded_strip_trims_its_labels_and_floors_the_tabs() {
    // Ten long labels in a box that fits maybe three of them.
    let labels: Vec<String> = (0..10).map(|i| format!("SESSION NUMBER {i}")).collect();
    let refs: Vec<&str> = labels.iter().map(|s| s.as_str()).collect();
    let narrow = Rect::new(0.0, 0.0, 200.0, 60.0);
    let mut sf = Probe::default();
    let cells = tabs::strip(
        &mut sf,
        narrow,
        &refs,
        &tabs::StripState::new(0),
        &tabs::StripStyle::default(),
        None,
    );
    assert_eq!(cells.len(), 10, "no tab is dropped — every page keeps its plate");
    let min_w = px("tab.min_w");
    for c in &cells {
        assert!(c.w >= min_w - 0.01, "floored at tab.min_w: {}", c.w);
    }
    // The floor is a floor: with ten of them the strip is wider than the
    // box, and says so rather than pretending.
    assert!(cells[9].right() > narrow.right());
    // Every label was trimmed to what its tab could hold.
    assert!(sf.texts.iter().all(|t| t.2.ends_with('\u{2026}')), "{:?}", sf.texts);
}

#[test]
fn a_strip_records_a_rectangle_for_every_tab() {
    let mut hits = Hits::new();
    let mut sf = Probe::default();
    let cells = tabs::strip(
        &mut sf,
        AREA,
        &["ONE", "TWO", "THREE"],
        &tabs::StripState::new(0),
        &tabs::StripStyle::default(),
        Some(tabs::StripView { hits: &mut hits, id: 7 }),
    );
    assert_eq!(hits.len(), 3);
    let mid = cells[1];
    assert_eq!(
        hits.at(mid.cx(), mid.cy_probe()),
        Some(&Hit::Tab { id: 7, index: 1 }),
    );
    // The object's own hit test answers the same question.
    assert_eq!(tabs::hit(&cells, mid.cx(), mid.y + 1.0), Some(1));
    assert_eq!(tabs::hit(&cells, AREA.x - 5.0, mid.y + 1.0), None);
}

#[test]
fn a_strip_with_nothing_in_it_draws_nothing() {
    let mut sf = Probe::default();
    let cells = tabs::strip(
        &mut sf,
        AREA,
        &[],
        &tabs::StripState::new(0),
        &tabs::StripStyle::default(),
        None,
    );
    assert!(cells.is_empty());
    assert!(sf.quads.is_empty() && sf.texts.is_empty() && sf.lines.is_empty());
    // A box with no height in it draws nothing either.
    let flat = Rect::new(0.0, 0.0, 300.0, 0.0);
    let cells = tabs::strip(
        &mut sf,
        flat,
        &["ONE"],
        &tabs::StripState::new(0),
        &tabs::StripStyle::default(),
        None,
    );
    assert!(cells.is_empty());
    assert!(sf.quads.is_empty());
}

// --------------------------------------------------- segmented control

#[test]
fn segments_are_content_measured_floored_and_left_aligned() {
    let mut sf = Probe::default();
    let cells = segmented::control(
        &mut sf,
        AREA,
        &["A", "LONGER CHOICE", "C"],
        &segmented::StripState::new(0),
        &segmented::StripStyle::default(),
        None,
    );
    assert_eq!(cells.len(), 3);
    let min = px("segmented.min_cell_w");
    let h = px("segmented.h");
    for c in &cells {
        assert!(c.w >= min - 0.01, "floored at segmented.min_cell_w: {}", c.w);
        assert!((c.h - h).abs() < 0.01);
        // Vertically centred in the box it was given.
        assert!((c.y - (AREA.y + (AREA.h - h) / 2.0)).abs() < 0.01);
    }
    // The one-letter choices are AT the floor; the long one is not.
    assert!((cells[0].w - min).abs() < 0.01);
    assert!(cells[1].w > min);
    assert_eq!(cells[0].x, AREA.x);
    let gap = px("segmented.gap");
    assert!((cells[1].x - (cells[0].right() + gap)).abs() < 0.01);
    // The control is as wide as its choices, and says so before it draws.
    let mut probe = Probe::default();
    let want = segmented::natural_w(
        &mut probe,
        &["A", "LONGER CHOICE", "C"],
        &segmented::StripStyle::default(),
    );
    assert!((cells[2].right() - AREA.x - want).abs() < 0.01);
}

#[test]
fn the_chosen_segment_wears_the_heavier_ring_and_the_selected_rung() {
    let mut sf = Probe::default();
    let st = segmented::StripState { active: 1, hover: None, flash: None };
    let cells = segmented::control(
        &mut sf,
        AREA,
        &["A", "B", "C"],
        &st,
        &segmented::StripStyle::default(),
        None,
    );
    let cut = px("segmented.corner");
    // Fill then frame, cell by cell: the chamfer is the theme's corner.
    let frames: Vec<_> = sf.chamfers.iter().filter(|c| c.2 > 0.0).collect();
    assert_eq!(frames.len(), 3, "one ring a cell");
    assert!((frames[0].2 - px("segmented.border")).abs() < 0.01);
    assert!((frames[1].2 - px("segmented.border_active")).abs() < 0.01);
    assert!((frames[2].2 - px("segmented.border")).abs() < 0.01);
    for f in &frames {
        assert!((f.1 - cut).abs() < 0.01, "the cut is segmented.corner");
    }
    let fills: Vec<_> = sf.chamfers.iter().filter(|c| c.2 == 0.0).collect();
    assert_eq!(fills.len(), 3);
    assert_eq!(fills[1].0.x, cells[1].x);
    // The chosen cell stands on the button ladder's Selected rung — the
    // class the 5.27 matrix lends this control.
    let sel = ladder("button", State::Selected).fill;
    let idle = ladder("button", State::Idle).fill;
    assert_ne!(
        (sel.r, sel.g, sel.b, sel.a),
        (idle.r, idle.g, idle.b, idle.a),
        "the master's own ladder must distinguish the chosen cell",
    );
}

#[test]
fn a_segmented_control_records_a_rectangle_for_every_cell() {
    let mut hits = Hits::new();
    let mut sf = Probe::default();
    let cells = segmented::control(
        &mut sf,
        AREA,
        &["A", "B"],
        &segmented::StripState::new(0),
        &segmented::StripStyle::default(),
        Some(segmented::StripView { hits: &mut hits, id: 3 }),
    );
    assert_eq!(hits.len(), 2);
    assert_eq!(
        hits.at(cells[1].cx(), cells[1].y + 1.0),
        Some(&Hit::Segment { id: 3, index: 1 }),
    );
}

/// `Rect` has no vertical centre of its own; the tests want one.
trait Cy {
    fn cy_probe(&self) -> f32;
}

impl Cy for Rect {
    fn cy_probe(&self) -> f32 {
        self.y + self.h / 2.0
    }
}
