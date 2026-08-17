//! The terminal's cell grid comes out of the theme, and the user's own
//! multiplier stands above it.
//!
//! `terminal.cell_font`, `terminal.min_px` and `terminal.line_height` sat
//! in the master with no reader at all until 2026-08-17: the ABI measured
//! a cell with `vh(1.45)`, floored it at a literal `8.0` and multiplied
//! the font's line box by nothing. A theme could write any of the three
//! and the grid did not move (audit 2026-08-17, Z03 and Z18).
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

    // A width no plausible cell divides evenly, so a changed cell has to
    // change the column count rather than round back onto it.
    const W: f32 = 1234.0;

    let base = Grid::measure(&mut fonts, 1.0);
    let cols = base.cols(W);
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
        big.cols(W) < cols,
        "a cell twice as wide still fit {} columns, the same as {cols}",
        big.cols(W)
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
    assert!(airy.rows(W) < base.rows(W), "a row twice as tall still fit as many rows");
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
}
