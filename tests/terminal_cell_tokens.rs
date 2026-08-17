//! The terminal's cell grid comes out of the theme, and the user's own
//! multiplier stands above it.
//!
//! `terminal.cell_font`, `terminal.min_px` and `terminal.line_height` sat
//! in the master with no reader at all until 2026-08-17: the ABI measured
//! a cell with `vh(1.45)`, floored it at a literal `8.0` and multiplied
//! the font's line box by nothing. A theme could write any of the three
//! and the grid did not move (audit 2026-08-17, Z03 and Z18).
//!
//! The other half of moving the cell into the theme is the bound that
//! has to move with it: the `8.0` floor was also what kept a window from
//! reporting a grid of millions of cells, and a token cannot keep that
//! promise, so `Grid::span` bounds the two axes together.
//!
//! One test in the file, and the file is its own process: a preview lays
//! values over the ONE global theme, so anything else running beside it
//! would measure a grid it did not ask for — the same reason
//! `theme_preview.rs` and `viewport_memo.rs` are each alone.

use nacelle::font::FontSystem;
use nacelle::term::Grid;
use nacelle::theme;

/// Close enough for a measurement that went through a font's metrics.
fn near(a: f32, b: f32) -> bool {
    (a - b).abs() <= 0.01 * b.abs().max(1.0)
}

#[test]
fn the_cell_is_the_theme_s_and_the_user_s_multiplier_rides_on_top() {
    theme::load();
    // A viewport of its own, so `u` is a number this file can predict
    // rather than whatever the last test to touch the global left behind.
    theme::set_viewport(1080.0, 1.0);
    let mut fonts = FontSystem::new();

    // A rectangle no plausible cell divides evenly, so a changed cell has
    // to change the grid rather than round back onto it.
    const W: f32 = 1234.0;
    const H: f32 = 987.0;

    let base = Grid::measure(&mut fonts, 1.0);
    let (cols, rows) = base.span(W, H);
    assert!(base.px > 0.0, "the master declares terminal.cell_font and it measured nothing");
    assert!(cols > 4, "a 1234 px wide terminal came out {cols} columns wide");

    // ---- terminal.cell_font moves the column count --------------------
    //
    // Twice the master's 2.9u. A monospace advance is proportional to the
    // size it is rasterised at, so twice the size is half the columns.
    let refused = theme::set_preview(&[("terminal.cell_font", "5.8u")]);
    assert!(refused.is_empty(), "the engine refused a size it should take: {refused:?}");
    let big = Grid::measure(&mut fonts, 1.0);
    assert!(
        near(big.px, base.px * 2.0),
        "terminal.cell_font doubled and the cell went from {} px to {} px",
        base.px,
        big.px
    );
    assert!(
        big.span(W, H).0 < cols,
        "a cell twice as wide still fit {} columns, the same as {cols}",
        big.span(W, H).0
    );
    theme::clear_preview();
    assert_eq!(Grid::measure(&mut fonts, 1.0).px, base.px, "clearing the preview left the cell");

    // ---- terminal.min_px is the floor under it ------------------------
    //
    // A size far under the floor, and a floor far over anything the
    // master ships, so the answer can only have come from the floor.
    let refused = theme::set_preview(&[
        ("terminal.cell_font", "0.1u"),
        ("terminal.min_px", "40px"),
    ]);
    assert!(refused.is_empty(), "the engine refused a floor it should take: {refused:?}");
    assert!(
        near(Grid::measure(&mut fonts, 1.0).px, 40.0),
        "terminal.min_px = 40px let a 0.1u cell through at {} px",
        Grid::measure(&mut fonts, 1.0).px
    );
    theme::clear_preview();

    // ---- terminal.line_height opens the rows up -----------------------
    //
    // The width must not move with it: this token multiplies the line
    // box, and a grid whose columns drifted when its rows were opened
    // would have stopped agreeing with the PTY.
    let refused = theme::set_preview(&[("terminal.line_height", "2.0")]);
    assert!(refused.is_empty(), "the engine refused a line height it should take: {refused:?}");
    let airy = Grid::measure(&mut fonts, 1.0);
    assert!(
        near(airy.cell_h, base.cell_h * 2.0),
        "terminal.line_height = 2.0 took the row from {} px to {} px",
        base.cell_h,
        airy.cell_h
    );
    assert_eq!(airy.cell_w, base.cell_w, "a taller row changed the column width");
    assert!(airy.span(W, H).1 < rows, "a row twice as tall still fit as many rows");
    theme::clear_preview();

    // ---- and the user still stands above the token --------------------
    //
    // `TermFontSize=` scales what the theme chose; it does not replace
    // it. Both halves are checked, because a reader that dropped the
    // multiplier and a reader that dropped the token look the same from
    // one measurement.
    let scaled = Grid::measure(&mut fonts, 2.0);
    assert!(
        near(scaled.px, base.px * 2.0),
        "TermFontSize 2.0 over a {} px cell gave {} px",
        base.px,
        scaled.px
    );
    let refused = theme::set_preview(&[("terminal.cell_font", "1.45u")]);
    assert!(refused.is_empty(), "the engine refused a size it should take: {refused:?}");
    assert!(
        near(Grid::measure(&mut fonts, 2.0).px, base.px),
        "half the base at twice the user's scale did not come back to the base"
    );
    theme::clear_preview();

    // ---- a collapsed cell still reports a grid this build can hold ----
    //
    // The floor under the cell is a TOKEN now, and a token is a line in
    // a user's file: `terminal.min_px = 0px` is a file that asked for no
    // floor at all, `terminal.line_height = 0.0` is one that asked for
    // no row, and both land on the arithmetic floor of one device pixel
    // that `measure` keeps so the divisions survive. On a 4K screen a
    // cell of one pixel is 3840 columns of 2160 rows — eight million
    // cells, allocated and walked once a frame by every widget the ABI
    // hands them to. What the engine owes here is not a readable grid,
    // which is what the theme asks ITSELF for with `terminal.min_px`,
    // but a finite one.
    const UHD: (f32, f32) = (3840.0, 2160.0);
    // A larger window of the same shape. A grid still tracking its
    // window answers more columns for it; a grid that has reached the
    // bound answers the same ones. It is only a little larger because
    // with a cell of one pixel the per-axis bound sits 4096 px away,
    // and past that a different clamp would be doing the answering.
    const WIDER: (f32, f32) = (4096.0, 2304.0);

    let refused =
        theme::set_preview(&[("terminal.cell_font", "0u"), ("terminal.min_px", "0px")]);
    assert!(refused.is_empty(), "the engine refused a size it should take: {refused:?}");
    let collapsed = Grid::measure(&mut fonts, 1.0);
    assert_eq!(
        (collapsed.cell_w, collapsed.cell_h),
        (1.0, 1.0),
        "a theme that asked for no size left a cell of {} by {} px",
        collapsed.cell_w,
        collapsed.cell_h
    );
    let (c4, r4) = collapsed.span(UHD.0, UHD.1);
    let (cw_, rw_) = collapsed.span(WIDER.0, WIDER.1);
    assert!(c4 >= 2 && r4 >= 2, "the bound cut the grid to nothing: {c4} by {r4}");
    assert!(
        c4 as f32 * r4 as f32 <= UHD.0 * UHD.1 / 4.0,
        "a cell of one pixel reported {c4} by {r4} cells across a 4K screen"
    );
    // Within one on each axis: the same grid, floored twice.
    assert!(
        cw_ <= c4 + 1 && rw_ <= r4 + 1,
        "a wider window reported {cw_} by {rw_} cells where 4K reported {c4} by {r4}"
    );
    // The window's own shape survives the bound — it scales the grid
    // rather than cropping one axis and leaving the other.
    assert!(
        (c4 as f32 / r4 as f32 - UHD.0 / UHD.1).abs() < 0.01,
        "the bounded grid came back {c4} by {r4}, which is not the window's shape"
    );
    // And the bound is on the PAIR, not on some cell size the theme is
    // held to: the master's own cell is nowhere near it and comes back
    // covering its window exactly.
    theme::clear_preview();
    let (c, r) = base.span(UHD.0, UHD.1);
    assert_eq!(
        (c, r),
        ((UHD.0 / base.cell_w) as u32, (UHD.1 / base.cell_h) as u32),
        "the master's own cell came back bounded across a 4K screen"
    );
}
