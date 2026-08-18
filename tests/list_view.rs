//! The list and the tree, actually drawn.
//!
//! The unit tests hold the arithmetic still; this one runs the drawing
//! itself — through the real master theme, through the real window
//! arithmetic, through the real hit list — and looks at what came out.
//! It needs no window and no font atlas because the view draws through a
//! [`Surface`], which is the whole reason that trait exists: a probe
//! that records rectangles is as good a surface as a GPU.

use nacelle::theme::parse::State;
use nacelle::theme::{self, Color};
use nacelle::ui::Align;
use nacelle::view::list::{ListState, ListStyle, ListView};
use nacelle::view::surface::{StateInk, Surface};
use nacelle::view::tree::{MemNode, MemTree};
use nacelle::view::{FlatTree, Hit, Hits, RowBuf, Rows};
use nacelle::Rect;

/// A surface that answers the REAL theme and records what it was asked
/// to draw. Text is measured at half an em a character: wrong about
/// fonts, right about monotonicity, which is all the trimming asks.
#[derive(Default)]
struct Probe {
    rects: Vec<(Rect, Color)>,
    texts: Vec<(f32, f32, String, Align)>,
    lines: Vec<(f32, f32, f32, f32)>,
    polys: Vec<Vec<[f32; 2]>>,
    clips: Vec<Rect>,
    depth: i32,
    deepest: i32,
    mouse: (f32, f32),
}

impl Probe {
    fn label_at(&self, i: usize) -> &str {
        &self.texts[i].2
    }
}

impl Surface for Probe {
    fn rect(&mut self, r: Rect, c: Color) {
        self.rects.push((r, c));
    }
    fn rect_outline(&mut self, _r: Rect, _w: f32, _c: Color) {}
    fn line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, _w: f32, _c: Color) {
        self.lines.push((x0, y0, x1, y1));
    }
    fn polyline(&mut self, pts: &[[f32; 2]], _w: f32, _c: Color, _closed: bool) {
        self.polys.push(pts.to_vec());
    }
    fn text(&mut self, _face: u8, _px: f32, x: f32, y: f32, s: &str, _c: Color, _t: f32, a: Align) {
        self.texts.push((x, y, s.to_string(), a));
    }
    fn measure(&mut self, _face: u8, px: f32, s: &str, _track: f32) -> f32 {
        s.chars().count() as f32 * px * 0.5
    }
    fn clip(&mut self, r: Rect) -> bool {
        self.clips.push(r);
        self.depth += 1;
        self.deepest = self.deepest.max(self.depth);
        true
    }
    fn unclip(&mut self) {
        self.depth -= 1;
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

fn row_h() -> f32 {
    theme::resolved().px(theme::id("list.row_h").unwrap())
}

fn rows(labels: &[&str]) -> Rows {
    Rows::new(
        labels
            .iter()
            .map(|l| RowBuf {
                key: (*l).into(),
                label: (*l).into(),
                ..RowBuf::default()
            })
            .collect(),
    )
}

const AREA: Rect = Rect { x: 10.0, y: 20.0, w: 300.0, h: 100.0 };
/// Room for six rows, for the tree tests that need more than three.
const TALL: Rect = Rect { x: 10.0, y: 20.0, w: 300.0, h: 200.0 };

#[test]
fn a_plain_list_draws_whole_rows_from_the_top_and_stops_at_the_edge() {
    let model = rows(&["alpha", "beta", "gamma", "delta", "epsilon"]);
    let mut sf = Probe::default();
    nacelle::view::list::list(&mut sf, AREA, &model, &ListStyle::default(), None);
    // As many whole rows as fit, and no more: the master's gap is zero,
    // so that is `floor(h / row_h)`.
    let fits = (AREA.h / row_h()).floor() as usize;
    assert!(fits >= 3 && fits < 5, "the master's row height puts {fits} rows in 100px");
    assert_eq!(sf.texts.len(), fits, "one label a row, none past the edge");
    assert_eq!(sf.label_at(0), "alpha");
    assert_eq!(sf.texts[0].3, Align::Left);
    // The labels march down by exactly one row.
    let step = sf.texts[1].1 - sf.texts[0].1;
    assert!((step - row_h()).abs() < 0.01, "one row apart, not {step}");
    // Nothing was clipped: there is no offset to clip against.
    assert_eq!(sf.deepest, 0);
    assert_eq!(sf.depth, 0);
}

#[test]
fn a_scrolled_list_draws_only_its_window_and_clips_the_partial_rows() {
    let labels: Vec<String> = (0..200).map(|i| format!("row {i}")).collect();
    let model = Rows::new(
        labels
            .iter()
            .map(|l| RowBuf { key: l.clone(), label: l.clone(), ..RowBuf::default() })
            .collect(),
    );
    let mut state = ListState::new();
    // Half a row down, so the window starts part-way through one.
    state.scroll.set_offset(row_h() * 4.5);
    let mut hits = Hits::new();
    let mut sf = Probe::default();
    nacelle::view::list::list(
        &mut sf,
        AREA,
        &model,
        &ListStyle::default(),
        Some(ListView {
            state: &mut state,
            hits: &mut hits,
            id: 3,
            select: true,
            scroll: true,
            tree: false,
            tooltip: false,
        }),
    );
    // Two hundred rows, a handful drawn.
    let fits = (AREA.h / row_h()).ceil() as usize + 1;
    assert!(sf.texts.len() <= fits, "{} rows drawn of 200", sf.texts.len());
    assert!(sf.texts.len() >= 4);
    assert_eq!(sf.label_at(0), "row 4", "the row the offset lands in");
    // The first row starts ABOVE the viewport, which is what the clip is
    // for — and the clip was balanced.
    assert!(sf.texts[0].1 < AREA.y, "the partial row hangs over the top edge");
    assert_eq!(sf.deepest, 1, "the body was clipped");
    assert_eq!(sf.depth, 0, "and unclipped again");
    assert_eq!((sf.clips[0].x, sf.clips[0].y, sf.clips[0].w, sf.clips[0].h),
               (AREA.x, AREA.y, AREA.w, AREA.h));
    // Every drawn row answers the pointer, by KEY.
    let mid = sf.texts[2].1 + row_h() / 2.0;
    match hits.at(AREA.x + 20.0, mid) {
        Some(Hit::Row { id, key }) => {
            assert_eq!(*id, 3);
            assert_eq!(key, "row 6");
        }
        other => panic!("expected a row under the pointer, got {other:?}"),
    }
    // The offset was clamped against the content, not left where it was
    // put: 200 rows is more than the viewport, so it stands.
    assert!(state.extent.scrollable);
    assert!(state.extent.content > state.extent.viewport);
}

#[test]
fn a_selected_row_is_washed_and_a_hovered_one_too() {
    let model = rows(&["alpha", "beta", "gamma"]);
    let mut state = ListState::new();
    state.select(Some("beta".into()));
    let mut hits = Hits::new();
    let mut sf = Probe::default();
    // The pointer sits on the third row.
    sf.mouse = (AREA.x + 5.0, AREA.y + row_h() * 2.5);
    nacelle::view::list::list(
        &mut sf,
        AREA,
        &model,
        &ListStyle::default(),
        Some(ListView {
            state: &mut state,
            hits: &mut hits,
            id: 0,
            select: true,
            scroll: false,
            tree: false,
            tooltip: false,
        }),
    );
    // Two washes: the selected row and the hovered one, each the full
    // width of the list.
    let washes: Vec<&(Rect, Color)> = sf.rects.iter().filter(|(r, _)| r.w == AREA.w).collect();
    assert_eq!(washes.len(), 2, "one selected row, one hovered row");
    assert!((washes[0].0.y - (AREA.y + row_h())).abs() < 0.01, "beta is the second row");
    assert!((washes[1].0.y - (AREA.y + row_h() * 2.0)).abs() < 0.01);

    // With `select` off nothing is washed — and that is the render the
    // master ships, because no shipped script asks for selection.
    let mut plain = Probe::default();
    plain.mouse = sf.mouse;
    nacelle::view::list::list(&mut plain, AREA, &model, &ListStyle::default(), None);
    assert!(
        plain.rects.iter().all(|(r, _)| r.w != AREA.w),
        "an unselectable list draws no row washes"
    );
}

#[test]
fn a_row_carries_its_chip_its_status_and_its_bar() {
    let model = Rows::new(vec![
        RowBuf {
            key: "cpu".into(),
            label: "CPU".into(),
            status: "hot".into(),
            severity: nacelle::ui::sev_of("critical"),
            bar: Some(0.5),
            ..RowBuf::default()
        },
        RowBuf { key: "bare".into(), label: "bare".into(), ..RowBuf::default() },
    ]);
    let mut sf = Probe::default();
    nacelle::view::list::list(&mut sf, AREA, &model, &ListStyle::default(), None);
    // Two labels and one status.
    let texts: Vec<&String> = sf.texts.iter().map(|t| &t.2).collect();
    assert!(texts.contains(&&"CPU".to_string()));
    assert!(texts.contains(&&"hot".to_string()));
    assert!(texts.contains(&&"bare".to_string()));
    // The status is right-aligned at the row's right padding.
    let status = sf.texts.iter().find(|t| t.2 == "hot").unwrap();
    assert_eq!(status.3, Align::Right);
    assert!(status.0 < AREA.right() && status.0 > AREA.right() - 20.0);
    // The chip and the bar are both drawn for the row that has them, and
    // neither for the row that does not: the severity chip is a SECOND
    // reading of the row's own judgement, never an invention.
    let chip = theme::resolved().px(theme::id("list.glyph").unwrap());
    assert!(
        sf.rects.iter().any(|(r, _)| (r.w - chip).abs() < 0.01),
        "the severity chip"
    );
    let bar_h = theme::resolved().px(theme::id("list.bar_h").unwrap());
    assert!(
        sf.rects.iter().any(|(r, _)| r.h > 0.0 && r.h <= bar_h && r.w > chip),
        "the bar's fill"
    );
    // The labelled-but-plain row starts further left than the chipped
    // one, because it reserves no chip.
    let cpu = sf.texts.iter().find(|t| t.2 == "CPU").unwrap().0;
    let bare = sf.texts.iter().find(|t| t.2 == "bare").unwrap().0;
    assert!(bare < cpu, "no chip, no chip column");
}

#[test]
fn a_tree_indents_by_depth_and_only_a_parent_gets_an_expander() {
    let model = MemTree::new(vec![
        MemNode::leaf("usr").with_children(vec![MemNode::leaf("share"), MemNode::leaf("lib")]),
        MemNode::leaf("etc"),
    ]);
    let mut flat = FlatTree::new(model);
    flat.expand("usr");
    flat.sync();
    let mut state = ListState::new();
    let mut hits = Hits::new();
    let mut sf = Probe::default();
    nacelle::view::list::list(
        &mut sf,
        TALL,
        &flat,
        &ListStyle::default(),
        Some(ListView {
            state: &mut state,
            hits: &mut hits,
            id: 1,
            select: true,
            scroll: false,
            tree: true,
            tooltip: false,
        }),
    );
    assert_eq!(sf.texts.len(), 4, "usr, share, lib, etc");
    assert_eq!(sf.label_at(0), "usr");
    assert_eq!(sf.label_at(1), "share");
    // A child's label starts one indent further right than its parent's.
    let indent = theme::resolved().px(theme::id("tree.indent").unwrap());
    assert!(
        (sf.texts[1].0 - sf.texts[0].0 - indent).abs() < 0.01,
        "one level of nesting is one tree.indent"
    );
    // One expander, on the one row that has children, drawn as the
    // three-point polyline every icon in this project is drawn with.
    assert_eq!(sf.polys.len(), 1);
    assert_eq!(sf.polys[0].len(), 3);
    // And it answers the pointer as a Disclosure, not as the row.
    let expander = sf.polys[0][0];
    match hits.at(expander[0] + 1.0, TALL.y + row_h() / 2.0) {
        Some(Hit::Disclosure { id, key }) => {
            assert_eq!(*id, 1);
            assert_eq!(key, "usr");
        }
        other => panic!("expected the expander, got {other:?}"),
    }
    // The rest of the row is still the row: opening a node and picking
    // it are two different gestures on two different pixels.
    match hits.at(TALL.right() - 5.0, TALL.y + row_h() / 2.0) {
        Some(Hit::Row { key, .. }) => assert_eq!(key, "usr"),
        other => panic!("expected the row, got {other:?}"),
    }
}

#[test]
fn collapsing_redraws_without_the_descendants_and_keeps_the_selection() {
    let model = MemTree::new(vec![MemNode::leaf("usr").with_children(vec![
        MemNode::leaf("share").with_children(vec![MemNode::leaf("fonts")]),
    ])]);
    let mut flat = FlatTree::new(model);
    flat.expand("usr");
    flat.expand("usr/share");
    flat.sync();
    let mut state = ListState::new();
    state.select(Some("usr/share/fonts".into()));

    let draw = |flat: &FlatTree<MemTree>, state: &mut ListState| -> Probe {
        let mut hits = Hits::new();
        let mut sf = Probe::default();
        nacelle::view::list::list(
            &mut sf,
            TALL,
            flat,
            &ListStyle::default(),
            Some(ListView {
                state,
                hits: &mut hits,
                id: 0,
                select: true,
                scroll: false,
                tree: true,
                tooltip: false,
            }),
        );
        sf
    };

    let open = draw(&flat, &mut state);
    assert_eq!(open.texts.len(), 3);
    // The selected row is washed, three levels down.
    assert_eq!(
        open.rects.iter().filter(|(r, _)| r.w == TALL.w).count(),
        1,
        "the selected row, and nothing else"
    );

    flat.collapse("usr");
    flat.sync();
    let closed = draw(&flat, &mut state);
    assert_eq!(closed.texts.len(), 1, "only the root is left");
    assert!(state.is_selected("usr/share/fonts"), "which does not unpick it");
    assert_eq!(
        closed.rects.iter().filter(|(r, _)| r.w == TALL.w).count(),
        0,
        "the selected row is simply not on screen"
    );

    flat.expand("usr");
    flat.sync();
    let reopened = draw(&flat, &mut state);
    assert_eq!(reopened.texts.len(), 3, "and reopening puts the shape back");
    assert_eq!(reopened.rects.iter().filter(|(r, _)| r.w == TALL.w).count(), 1);
}

#[test]
fn a_surface_that_cannot_clip_scrolls_by_whole_rows_instead() {
    /// The old-host case: `Surface::clip` refuses, so the view must not
    /// paint half a row outside its box — it snaps the offset instead,
    /// which is exactly what the file panel does today.
    struct NoClip(Probe);
    impl Surface for NoClip {
        fn rect(&mut self, r: Rect, c: Color) {
            self.0.rect(r, c)
        }
        fn rect_outline(&mut self, r: Rect, w: f32, c: Color) {
            self.0.rect_outline(r, w, c)
        }
        fn line(&mut self, a: f32, b: f32, c: f32, d: f32, w: f32, col: Color) {
            self.0.line(a, b, c, d, w, col)
        }
        fn polyline(&mut self, p: &[[f32; 2]], w: f32, c: Color, closed: bool) {
            self.0.polyline(p, w, c, closed)
        }
        fn text(&mut self, _face: u8, px: f32, x: f32, y: f32, s: &str, c: Color, t: f32, a: Align) {
            self.0.text(_face, px, x, y, s, c, t, a)
        }
        fn measure(&mut self, _face: u8, px: f32, s: &str, t: f32) -> f32 {
            self.0.measure(_face, px, s, t)
        }
        fn clip(&mut self, _r: Rect) -> bool {
            false
        }
        fn unclip(&mut self) {
            panic!("a refused clip must never be undone");
        }
        fn can_clip(&self) -> bool {
            false
        }
        fn has_token(&mut self, n: &str) -> bool {
            self.0.has_token(n)
        }
        fn px(&mut self, n: &str) -> f32 {
            self.0.px(n)
        }
        fn color(&mut self, n: &str) -> Color {
            self.0.color(n)
        }
        fn bed(&mut self, n: &str) -> Color {
            self.0.bed(n)
        }
        fn flag(&mut self, n: &str) -> bool {
            self.0.flag(n)
        }
        fn word(&mut self, n: &str) -> String {
            self.0.word(n)
        }
        fn class_state(&mut self, c: &str, s: State) -> StateInk {
            self.0.class_state(c, s)
        }
        fn epoch(&mut self) -> u32 {
            self.0.epoch()
        }
        fn now(&self) -> f64 {
            self.0.now()
        }
        fn mouse(&self) -> (f32, f32) {
            self.0.mouse()
        }
        fn scale(&self) -> f32 {
            self.0.scale()
        }
    }

    let labels: Vec<String> = (0..50).map(|i| format!("row {i}")).collect();
    let model = Rows::new(
        labels
            .iter()
            .map(|l| RowBuf { key: l.clone(), label: l.clone(), ..RowBuf::default() })
            .collect(),
    );
    let mut state = ListState::new();
    state.scroll.set_offset(row_h() * 4.5);
    let mut hits = Hits::new();
    let mut sf = NoClip(Probe::default());
    nacelle::view::list::list(
        &mut sf,
        AREA,
        &model,
        &ListStyle::default(),
        Some(ListView {
            state: &mut state,
            hits: &mut hits,
            id: 0,
            select: false,
            scroll: true,
            tree: false,
            tooltip: false,
        }),
    );
    // The half row was rounded away: the offset landed on a whole row,
    // and the view drew exactly what it would have drawn had the caller
    // asked for that row in the first place.
    assert!(!sf.0.texts.is_empty());
    assert_eq!(sf.0.label_at(0), "row 5", "4.5 rows rounds to 5");
    assert!((state.scroll.offset() - row_h() * 5.0).abs() < 0.01);

    let mut whole = ListState::new();
    whole.scroll.set_offset(row_h() * 5.0);
    let mut hits2 = Hits::new();
    let mut clipping = Probe::default();
    nacelle::view::list::list(
        &mut clipping,
        AREA,
        &model,
        &ListStyle::default(),
        Some(ListView {
            state: &mut whole,
            hits: &mut hits2,
            id: 0,
            select: false,
            scroll: true,
            tree: false,
            tooltip: false,
        }),
    );
    assert_eq!(clipping.label_at(0), "row 5");
    assert!(
        (clipping.texts[0].1 - sf.0.texts[0].1).abs() < 0.01,
        "the snapped view sits exactly where the whole-row view does"
    );
}

#[test]
fn the_table_draws_through_the_same_wall_the_list_does() {
    // The point of `Surface`: `ui::table` is no longer welded to the
    // host's draw list. If this compiles and draws, a plugin holding an
    // `AbiSurface` can draw the very same table across the ABI — which
    // is the difference between one implementation and two.
    use nacelle::ui::{CellKind, ColWidth, Column, TableStyle};
    let columns = vec![
        Column {
            title: "PID".into(),
            align: Align::Right,
            kind: CellKind::Text,
            width: ColWidth::Content,
        },
        Column {
            title: "COMMAND".into(),
            align: Align::Left,
            kind: CellKind::Text,
            width: ColWidth::Content,
        },
    ];
    let rows: Vec<Vec<String>> = (0..40)
        .map(|i| vec![format!("{}", 1000 + i), format!("process-{i}")])
        .collect();
    let st = TableStyle { elastic: 1, zebra: false, severity_col: None, shrink: 1.0 };
    let mut sf = Probe::default();
    nacelle::ui::table_surface(&mut sf, TALL, &columns, &rows, &st, None);

    // Two headings, then a body: the header rule is the one line drawn.
    assert_eq!(sf.label_at(0), "PID");
    assert_eq!(sf.label_at(1), "COMMAND");
    assert_eq!(sf.lines.len(), 1, "the hairline under the header");
    assert!((sf.lines[0].0 - TALL.x).abs() < 0.01);
    assert!((sf.lines[0].2 - TALL.right()).abs() < 0.01);
    // Not all forty rows: a table without a view is cut at its box.
    let cells = sf.texts.len() - 2;
    assert!(cells > 0 && cells < 80, "{cells} cells drawn of 80");
    assert_eq!(cells % 2, 0, "whole rows, both columns");
    // The right-aligned column ends before the left-aligned one starts —
    // u2 §2.7's `1471  firefox`, and the reason every width reserves a
    // gap beyond its content.
    let pid = sf.texts.iter().find(|t| t.2 == "1000").unwrap();
    let cmd = sf.texts.iter().find(|t| t.2 == "process-0").unwrap();
    assert_eq!(pid.3, Align::Right);
    assert!(pid.0 < cmd.0, "the pid ends before the command begins");
    assert_eq!(sf.deepest, 0, "an unscrolled table clips nothing");
}
