//! Which rows of a row list are visible through a viewport, and where
//! the first of them starts.
//!
//! Pure arithmetic — no theme, no state, no drawing — so the tests hold
//! it still the way they hold `script::stack_fit`. The drawer iterates
//! `first .. first + count` and nothing else: that is the whole of
//! virtualisation. `ui::table` already draws a window (`take(shown)`);
//! what it has never had is the OFFSET, which is what this file adds.

/// A window onto a row list.
///
/// `y0` is the top of row `first` **relative to the viewport's top edge**
/// and is never positive: a scrolled list starts with a row that is
/// partly above the viewport. Drawing that partial row needs a clip;
/// a caller that cannot clip (an old host across the plugin ABI) snaps
/// its offset with [`snap_offset`] first, and then `y0` is always 0.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RowWindow {
    /// Index of the first row to draw.
    pub first: usize,
    /// How many rows to draw, partial edge rows included.
    pub count: usize,
    /// Top of row `first`, relative to the viewport top (`<= 0`).
    pub y0: f32,
}

impl RowWindow {
    /// The empty window: nothing to draw.
    pub const EMPTY: RowWindow = RowWindow { first: 0, count: 0, y0: 0.0 };

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Row indices to draw, ready for `for i in w.rows()`.
    pub fn rows(&self) -> std::ops::Range<usize> {
        self.first..self.first + self.count
    }

    /// Top of row `index`, relative to the viewport top. Rows outside
    /// the window answer honestly (above or below), so a caller may use
    /// it for a row it decided to draw anyway — a dragged one, say.
    pub fn y_of(&self, index: usize, row_h: f32) -> f32 {
        self.y0 + (index as f32 - self.first as f32) * row_h
    }
}

/// The window `viewport_h` of pixels shows, `offset_px` down a list of
/// `total` rows of `row_h` each.
///
/// Degenerate input answers [`RowWindow::EMPTY`] rather than panicking:
/// a widget mid-resize legitimately measures itself at zero height, and
/// a model may legitimately be empty.
///
/// An offset past the end is treated as the last row sitting at the top.
/// This function does not clamp for real — [`super::scroll::ScrollView`]
/// owns the offset and clamps it against the content — it only refuses
/// to answer with a row index the model does not have.
pub fn row_window(offset_px: f32, viewport_h: f32, row_h: f32, total: usize) -> RowWindow {
    if total == 0 || !row_h.is_finite() || row_h <= 0.0 || !viewport_h.is_finite() || viewport_h <= 0.0
    {
        return RowWindow::EMPTY;
    }
    // NaN compares false everywhere, so `max` answers with the other
    // operand and a NaN offset degrades to the top of the list.
    let off = offset_px.max(0.0).min((total - 1) as f32 * row_h);
    let first = ((off / row_h).floor() as usize).min(total - 1);
    let y0 = first as f32 * row_h - off;
    // `viewport_h - y0` is the span still to cover, including the part
    // of the first row that sits above the viewport; the ceiling is what
    // makes the partial row at the bottom edge part of the window.
    let count = (((viewport_h - y0) / row_h).ceil() as usize).max(1).min(total - first);
    RowWindow { first, count, y0 }
}

/// The height `total` rows occupy — the content height a scroll view
/// measures itself against.
pub fn content_h(row_h: f32, total: usize) -> f32 {
    if !row_h.is_finite() || row_h <= 0.0 {
        return 0.0;
    }
    row_h * total as f32
}

/// The row a scroll offset lands on when whole rows are the only legal
/// stops: `round(offset / row_h)`.
///
/// This is the arithmetic the filesystem widget has used since it grew a
/// scroll (`row_off = round(scroll / row_h)`), kept in ONE place so the
/// generic view and the widget cannot drift apart the way `fit_end`
/// once did.
pub fn snap_row(offset_px: f32, row_h: f32) -> usize {
    if !row_h.is_finite() || row_h <= 0.0 {
        return 0;
    }
    (offset_px / row_h).round().max(0.0) as usize
}

/// [`snap_row`] as an offset: the top of the row the offset rounds to.
pub fn snap_offset(offset_px: f32, row_h: f32) -> f32 {
    if !row_h.is_finite() || row_h <= 0.0 {
        return 0.0;
    }
    snap_row(offset_px, row_h) as f32 * row_h
}

// ---------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_list_at_rest_shows_whole_rows_only() {
        let w = row_window(0.0, 100.0, 25.0, 10);
        assert_eq!(w, RowWindow { first: 0, count: 4, y0: 0.0 });
        assert_eq!(w.rows(), 0..4);
    }

    #[test]
    fn the_window_carries_the_partial_rows_at_both_edges() {
        // Ten pixels down: row 0 is cut at the top, so a fifth row is
        // needed to cover the bottom edge.
        let w = row_window(10.0, 100.0, 25.0, 10);
        assert_eq!(w.first, 0);
        assert_eq!(w.count, 5);
        assert_eq!(w.y0, -10.0);
        assert_eq!(w.y_of(4, 25.0), 90.0, "the last row starts inside the viewport");
    }

    #[test]
    fn a_row_boundary_offset_needs_no_partial_row() {
        let w = row_window(25.0, 100.0, 25.0, 10);
        assert_eq!(w, RowWindow { first: 1, count: 4, y0: 0.0 });
    }

    #[test]
    fn the_window_never_runs_past_the_model() {
        let w = row_window(1000.0, 100.0, 25.0, 10);
        assert_eq!(w.first, 9, "clamped to the last row the model has");
        assert_eq!(w.count, 1);
        assert!(w.rows().end <= 10);
        // A viewport taller than the whole list still asks for ten rows.
        let all = row_window(0.0, 1000.0, 25.0, 10);
        assert_eq!(all.count, 10);
    }

    #[test]
    fn degenerate_input_draws_nothing() {
        assert_eq!(row_window(0.0, 100.0, 25.0, 0), RowWindow::EMPTY);
        assert_eq!(row_window(0.0, 0.0, 25.0, 10), RowWindow::EMPTY);
        assert_eq!(row_window(0.0, 100.0, 0.0, 10), RowWindow::EMPTY);
        assert_eq!(row_window(f32::NAN, 100.0, 25.0, 10).first, 0);
        assert_eq!(content_h(0.0, 10), 0.0);
    }

    #[test]
    fn snapping_is_the_filesystem_arithmetic() {
        let row_h = 27.0_f32;
        for i in 0..40 {
            let off = i as f32 * 7.5;
            assert_eq!(snap_row(off, row_h), (off / row_h).round() as usize);
            assert_eq!(snap_offset(off, row_h), snap_row(off, row_h) as f32 * row_h);
        }
        // Half a row rounds to the next one, as `f32::round` does.
        assert_eq!(snap_row(row_h * 0.5, row_h), 1);
        assert_eq!(snap_row(row_h * 0.49, row_h), 0);
        assert_eq!(snap_row(-5.0, row_h), 0);
    }
}
