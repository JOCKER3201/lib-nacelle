//! Hit testing: rectangles recorded while drawing, tested when input
//! arrives.
//!
//! The filesystem widget has done this by hand since it grew tiles —
//! `hits: Vec<(Rect, usize)>`, filled in `draw`, searched in `click`.
//! The idea is right and the reason is structural: a view knows where it
//! put things only once it has drawn them, because the theme decides the
//! metrics and the model decides the count. What the hand-written
//! version lacks is a TYPE — with one, a single vector serves a table's
//! headers, its dividers, its rows, the tree's expanders and the
//! scrollbar, and the caller reads a name instead of decoding an index.

use crate::Rect;

/// What the pointer landed on. Every variant carries the `id` of the
/// view that recorded it, so one [`Hits`] may serve every view in a
/// widget without them having to agree on anything else.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Hit {
    /// A column heading — a click sorts by it.
    TableHead { id: u32, col: usize },
    /// The grip between two headings — a drag resizes the column.
    TableDivider { id: u32, col: usize },
    /// A row of a table, list or tree. The key is the model's own
    /// identity for the row, not its index: a model that reorders
    /// between frames must not move the selection.
    Row { id: u32, key: String },
    /// A tree row's expander.
    Disclosure { id: u32, key: String },
    /// The scrollbar's thumb.
    Thumb { id: u32 },
    /// The scrollbar's track, beside the thumb: a page toward the click.
    Track { id: u32, toward_end: bool },
    Tab { id: u32, index: usize },
    Segment { id: u32, index: usize },
}

impl Hit {
    /// The view that recorded this rectangle.
    pub fn id(&self) -> u32 {
        match self {
            Hit::TableHead { id, .. }
            | Hit::TableDivider { id, .. }
            | Hit::Row { id, .. }
            | Hit::Disclosure { id, .. }
            | Hit::Thumb { id }
            | Hit::Track { id, .. }
            | Hit::Tab { id, .. }
            | Hit::Segment { id, .. } => *id,
        }
    }
}

/// The rectangles one frame recorded, in the order they were drawn.
#[derive(Clone, Debug, Default)]
pub struct Hits(Vec<(Rect, Hit)>);

impl Hits {
    pub fn new() -> Self {
        Hits(Vec::new())
    }

    /// Drop the previous frame's rectangles. Called at the top of a
    /// draw: a stale rectangle is a click that lands on a row which is
    /// no longer there.
    pub fn clear(&mut self) {
        self.0.clear();
    }

    pub fn push(&mut self, r: Rect, hit: Hit) {
        self.0.push((r, hit));
    }

    /// What is under the pointer, or `None`.
    ///
    /// The LAST matching rectangle wins, because the draw list is
    /// immediate and draw order is z-order: an overlay scrollbar is
    /// drawn over the rows it covers, so it must also take their
    /// clicks.
    pub fn at(&self, x: f32, y: f32) -> Option<&Hit> {
        self.find(x, y).map(|(_, h)| h)
    }

    /// [`Hits::at`] with the rectangle that matched — for a caller that
    /// needs the geometry as well, such as a thumb about to be grabbed.
    pub fn find(&self, x: f32, y: f32) -> Option<(Rect, &Hit)> {
        self.0.iter().rev().find(|(r, _)| r.contains(x, y)).map(|(r, h)| (*r, h))
    }

    /// The rectangle recorded for a hit, if one was.
    pub fn rect_of(&self, hit: &Hit) -> Option<Rect> {
        self.0.iter().rev().find(|(_, h)| h == hit).map(|(r, _)| *r)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &(Rect, Hit)> {
        self.0.iter()
    }
}

// ---------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn row(key: &str) -> Hit {
        Hit::Row { id: 1, key: key.to_string() }
    }

    #[test]
    fn the_pointer_finds_the_row_it_is_over() {
        let mut h = Hits::new();
        h.push(Rect::new(0.0, 0.0, 100.0, 20.0), row("a"));
        h.push(Rect::new(0.0, 20.0, 100.0, 20.0), row("b"));
        assert_eq!(h.at(10.0, 25.0), Some(&row("b")));
        assert_eq!(h.at(10.0, 5.0), Some(&row("a")));
        assert_eq!(h.at(10.0, 45.0), None, "below every row");
        assert_eq!(h.at(200.0, 5.0), None, "beside every row");
    }

    #[test]
    fn what_was_drawn_last_takes_the_click() {
        // The overlay scrollbar covers the right edge of a row; it was
        // drawn after it, so it is what the pointer is pointing at.
        let mut h = Hits::new();
        h.push(Rect::new(0.0, 0.0, 100.0, 20.0), row("a"));
        h.push(Rect::new(94.0, 0.0, 6.0, 12.0), Hit::Thumb { id: 1 });
        assert_eq!(h.at(96.0, 5.0), Some(&Hit::Thumb { id: 1 }));
        assert_eq!(h.at(50.0, 5.0), Some(&row("a")));
        assert_eq!(h.find(96.0, 5.0).map(|(r, _)| r.x), Some(94.0));
    }

    #[test]
    fn a_cleared_frame_hits_nothing() {
        let mut h = Hits::new();
        h.push(Rect::new(0.0, 0.0, 10.0, 10.0), Hit::Tab { id: 2, index: 0 });
        assert_eq!(h.len(), 1);
        assert_eq!(h.at(1.0, 1.0).map(Hit::id), Some(2));
        h.clear();
        assert!(h.is_empty());
        assert_eq!(h.at(1.0, 1.0), None);
    }

    #[test]
    fn every_kind_of_hit_names_its_view() {
        assert_eq!(Hit::TableHead { id: 7, col: 2 }.id(), 7);
        assert_eq!(Hit::TableDivider { id: 7, col: 2 }.id(), 7);
        assert_eq!(Hit::Disclosure { id: 7, key: "x".into() }.id(), 7);
        assert_eq!(Hit::Track { id: 7, toward_end: true }.id(), 7);
        assert_eq!(Hit::Segment { id: 7, index: 1 }.id(), 7);
    }
}
