//! The tooltip's CONSUMERS — the half of F2 §8.1 that makes the manager
//! more than a module.
//!
//! `tests/tooltip_view.rs` proves the box is placed, sized and delayed
//! correctly when something asks for it. This one proves something asks:
//! that an interactive table's trimmed heading and trimmed cell, and a
//! tab strip's trimmed label, file the request themselves while they
//! draw — so that resting the pointer on text the ellipsis cut short
//! really does put the whole of it on screen.
//!
//! Everything runs through the real master theme and the real fonts,
//! because "was this trimmed?" is a question only a real measure can
//! answer.

use nacelle::draw::DrawList;
use nacelle::font::FontSystem;
use nacelle::object::tooltip::Tooltips;
use nacelle::object::{segmented, tabs};
use nacelle::theme;
use nacelle::ui::{self, Align, CellKind, ColWidth, Column, TableStyle, TableView};
use nacelle::view::{Hit, Hits, TableState};
use nacelle::{Ctx, Rect};

const W: f32 = 1920.0;
const H: f32 = 1080.0;

/// A name no narrow column can show in full.
const LONG: &str = "a-very-long-process-name-that-no-column-will-ever-show-in-full";

/// One frame: the caller draws, the manager answers at the end of it —
/// exactly the order the desktop keeps. Gives back whatever the drawing
/// returned and the text that reached the screen, if any.
fn frame<R, F>(
    tips: &mut Tooltips,
    fonts: &mut FontSystem,
    mouse: (f32, f32),
    t: f64,
    body: F,
) -> (R, Option<String>)
where
    F: FnOnce(&mut Ctx) -> R,
{
    let mut dl = DrawList::new();
    let mut ctx = Ctx {
        dl: &mut dl,
        fonts,
        w: W,
        h: H,
        t,
        mouse,
        term_font_scale: 1.0,
        ui_font_scale: 1.0,
        panel_scale: 1.0,
        focus: None,
        tips: Some(tips),
    };
    let out = body(&mut ctx);
    // Taken out before it is drawn, as the desktop does: the manager
    // cannot be lent to the frame and draw into it at the same time.
    let m = ctx.tips.take().expect("the manager was lent to this frame");
    m.draw(&mut ctx);
    (out, m.shown().map(|s| s.to_string()))
}

fn columns() -> Vec<Column> {
    vec![
        Column {
            title: "PROCESS IDENTIFIER".into(),
            align: Align::Right,
            kind: CellKind::Text,
            width: ColWidth::Content,
        },
        Column {
            title: "NAME".into(),
            align: Align::Left,
            kind: CellKind::Text,
            width: ColWidth::Content,
        },
    ]
}

fn rows() -> Vec<Vec<String>> {
    vec![
        vec!["1471".into(), LONG.into()],
        vec!["7".into(), "sh".into()],
    ]
}

fn style() -> TableStyle {
    TableStyle { elastic: 1, zebra: false, severity_col: None, shrink: 1.0 }
}

/// Draws the table into `r` with the view options the shipped process
/// widget uses, and records where everything landed.
fn table(
    ctx: &mut Ctx,
    r: Rect,
    state: &mut TableState,
    hits: &mut Hits,
    explain: bool,
) {
    hits.clear();
    ui::table_view(
        ctx,
        r,
        &columns(),
        &rows(),
        &style(),
        TableView {
            state,
            hits,
            id: 0,
            generation: 1,
            interactive: true,
            select: true,
            key_col: Some(0),
            scroll: false,
            tooltip: explain,
        },
    );
}

// ---- the table -------------------------------------------------------

#[test]
fn a_cell_the_ellipsis_cut_short_says_the_whole_of_itself() {
    theme::set_viewport(H, 1.0);
    let mut fonts = FontSystem::new();
    let mut tips = Tooltips::new();
    let mut state = TableState::new();
    let mut hits = Hits::new();
    let r = Rect::new(40.0, 60.0, 300.0, 400.0);

    // A frame with the pointer nowhere near it, to learn where the rows
    // landed — the same thing a click does between frames.
    frame(&mut tips, &mut fonts, (0.0, 0.0), 0.0, |ctx| {
        table(ctx, r, &mut state, &mut hits, true);
    });
    let row = hits
        .rect_of(&Hit::Row { id: 0, key: "1471".into() })
        .expect("the table records a rectangle for every row it drew");
    // The elastic column is the last one, so the table's right edge is
    // inside it whatever the first column measured.
    let at = (r.right() - 10.0, row.y + row.h / 2.0);

    // Resting starts the clock and shows nothing.
    let (_, now) = frame(&mut tips, &mut fonts, at, 0.0, |ctx| {
        table(ctx, r, &mut state, &mut hits, true);
    });
    assert_eq!(now, None, "a tooltip before the delay is a tooltip in the way");

    // A second later, the whole name.
    let (_, now) = frame(&mut tips, &mut fonts, at, 1.0, |ctx| {
        table(ctx, r, &mut state, &mut hits, true);
    });
    assert_eq!(now.as_deref(), Some(LONG));
}

#[test]
fn a_cell_that_fits_explains_nothing() {
    theme::set_viewport(H, 1.0);
    let mut fonts = FontSystem::new();
    let mut tips = Tooltips::new();
    let mut state = TableState::new();
    let mut hits = Hits::new();
    // Room for the whole name: nothing was trimmed, so there is nothing
    // to add, and a tooltip repeating what is on screen is noise.
    let r = Rect::new(40.0, 60.0, 1600.0, 400.0);

    frame(&mut tips, &mut fonts, (0.0, 0.0), 0.0, |ctx| {
        table(ctx, r, &mut state, &mut hits, true);
    });
    let row = hits
        .rect_of(&Hit::Row { id: 0, key: "1471".into() })
        .expect("the table records a rectangle for every row it drew");
    let at = (r.right() - 10.0, row.y + row.h / 2.0);

    for t in [0.0, 1.0, 2.0] {
        let (_, now) = frame(&mut tips, &mut fonts, at, t, |ctx| {
            table(ctx, r, &mut state, &mut hits, true);
        });
        assert_eq!(now, None, "an untrimmed cell has nothing to say");
    }
}

#[test]
fn a_table_that_was_not_asked_to_explain_itself_stays_quiet() {
    theme::set_viewport(H, 1.0);
    let mut fonts = FontSystem::new();
    let mut tips = Tooltips::new();
    let mut state = TableState::new();
    let mut hits = Hits::new();
    let r = Rect::new(40.0, 60.0, 300.0, 400.0);

    frame(&mut tips, &mut fonts, (0.0, 0.0), 0.0, |ctx| {
        table(ctx, r, &mut state, &mut hits, false);
    });
    let row = hits
        .rect_of(&Hit::Row { id: 0, key: "1471".into() })
        .expect("the table records a rectangle for every row it drew");
    let at = (r.right() - 10.0, row.y + row.h / 2.0);

    for t in [0.0, 1.0, 2.0] {
        let (_, now) = frame(&mut tips, &mut fonts, at, t, |ctx| {
            table(ctx, r, &mut state, &mut hits, false);
        });
        assert_eq!(now, None, "`tooltip` is opt-in, like every other view option");
    }
}

#[test]
fn a_heading_squeezed_by_a_dragged_width_says_what_it_is() {
    theme::set_viewport(H, 1.0);
    let mut fonts = FontSystem::new();
    let mut tips = Tooltips::new();
    let mut state = TableState::new();
    let mut hits = Hits::new();
    let r = Rect::new(40.0, 60.0, 600.0, 400.0);
    // The user dragged the first column down to a sliver: its heading no
    // longer fits, which is the one case where a heading needs saying.
    state.set_width(0, Some(30.0));

    frame(&mut tips, &mut fonts, (0.0, 0.0), 0.0, |ctx| {
        table(ctx, r, &mut state, &mut hits, true);
    });
    let head = hits
        .rect_of(&Hit::TableHead { id: 0, col: 0 })
        .expect("an interactive table records a rectangle for every heading");
    let at = (head.x + 2.0, head.y + head.h / 2.0);

    let (_, now) = frame(&mut tips, &mut fonts, at, 0.0, |ctx| {
        table(ctx, r, &mut state, &mut hits, true);
    });
    assert_eq!(now, None);
    let (_, now) = frame(&mut tips, &mut fonts, at, 1.0, |ctx| {
        table(ctx, r, &mut state, &mut hits, true);
    });
    assert_eq!(now.as_deref(), Some("PROCESS IDENTIFIER"));
}

// ---- the tab strip ---------------------------------------------------

#[test]
fn a_tab_too_narrow_for_its_page_gives_the_name_in_full() {
    theme::set_viewport(H, 1.0);
    let mut fonts = FontSystem::new();
    let mut tips = Tooltips::new();
    let st = tabs::StripState::new(0);
    let labels = ["TELEMETRY AND DIAGNOSTICS", "SHELL"];
    // Narrow enough that the solver floors both plates and the first
    // label is cut short.
    let r = Rect::new(0.0, 0.0, 160.0, 120.0);

    let (cells, _) = frame(&mut tips, &mut fonts, (0.0, 0.0), 0.0, |ctx| {
        tabs::draw(ctx, r, &labels, &st)
    });
    let cell = cells[0];
    let at = (cell.x + cell.w / 2.0, cell.y + cell.h / 2.0);

    let (_, now) = frame(&mut tips, &mut fonts, at, 0.0, |ctx| {
        tabs::draw(ctx, r, &labels, &st)
    });
    assert_eq!(now, None);
    let (_, now) = frame(&mut tips, &mut fonts, at, 1.0, |ctx| {
        tabs::draw(ctx, r, &labels, &st)
    });
    assert_eq!(now.as_deref(), Some(labels[0]));
}

#[test]
fn a_tab_with_room_for_its_label_stays_quiet() {
    theme::set_viewport(H, 1.0);
    let mut fonts = FontSystem::new();
    let mut tips = Tooltips::new();
    let st = tabs::StripState::new(0);
    let labels = ["ONE", "TWO"];
    let r = Rect::new(0.0, 0.0, 900.0, 120.0);

    let (cells, _) = frame(&mut tips, &mut fonts, (0.0, 0.0), 0.0, |ctx| {
        tabs::draw(ctx, r, &labels, &st)
    });
    let cell = cells[0];
    let at = (cell.x + cell.w / 2.0, cell.y + cell.h / 2.0);

    for t in [0.0, 1.0, 2.0] {
        let (_, now) = frame(&mut tips, &mut fonts, at, t, |ctx| {
            tabs::draw(ctx, r, &labels, &st)
        });
        assert_eq!(now, None, "a label that fits is already saying everything");
    }
}

// ---- the segmented control -------------------------------------------

#[test]
fn a_segment_too_narrow_for_its_choice_gives_the_word_in_full() {
    theme::set_viewport(H, 1.0);
    let mut fonts = FontSystem::new();
    let mut tips = Tooltips::new();
    let st = segmented::StripState::new(0);
    let labels = ["EVERYTHING AT ONCE", "SOME", "NONE"];
    let r = Rect::new(0.0, 0.0, 150.0, 80.0);

    let (cells, _) = frame(&mut tips, &mut fonts, (0.0, 0.0), 0.0, |ctx| {
        segmented::draw(ctx, r, &labels, &st)
    });
    let cell = cells[0];
    let at = (cell.x + cell.w / 2.0, cell.y + cell.h / 2.0);

    let (_, now) = frame(&mut tips, &mut fonts, at, 0.0, |ctx| {
        segmented::draw(ctx, r, &labels, &st)
    });
    assert_eq!(now, None);
    let (_, now) = frame(&mut tips, &mut fonts, at, 1.0, |ctx| {
        segmented::draw(ctx, r, &labels, &st)
    });
    assert_eq!(now.as_deref(), Some(labels[0]));
}
