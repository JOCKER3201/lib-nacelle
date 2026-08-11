//! What a view asks of the data behind it.
//!
//! A list, a tree and a table all draw ROWS; what differs is where the
//! rows come from. [`RowModel`] is that seam, and it is deliberately
//! pull-based and index-addressed: a virtualised view draws forty rows
//! out of four thousand, so a model that had to hand over a `Vec` would
//! materialise the other three thousand nine hundred and sixty for
//! nothing.
//!
//! The row itself arrives in a [`RowBuf`] the view owns and reuses, for
//! the same reason: forty rows a frame, sixty frames a second, is two
//! thousand four hundred allocations a second saved by one buffer.
//!
//! Nothing here decides what a row LOOKS like. A row carries its label,
//! its trailing status, the script's judgement of it (a severity — an
//! index into the closed set, never a colour) and an optional fraction
//! for the second reading a bar gives; the theme decides the rest.

use crate::ui::Sev;

/// One row, as the model describes it and the view draws it.
///
/// Filled through `&mut` rather than returned, so the view's buffer is
/// reused: [`RowBuf::reset`] clears it without freeing the strings'
/// capacity.
#[derive(Clone, Debug, Default)]
pub struct RowBuf {
    /// The row's identity across a data refresh. Selection is by this
    /// string and never by index — the model is rebuilt every snapshot
    /// and an index means nothing across two of them.
    pub key: String,
    /// The row's main text.
    pub label: String,
    /// The trailing text, drawn small at the right edge — the
    /// `(Zakończone)` of the reference images. Empty for none.
    pub status: String,
    /// The model's judgement of this row, as an index into the closed
    /// severity set. Colours the chip and, where there is one, the bar.
    pub severity: Option<Sev>,
    /// A 0..1 fraction drawn as a thin bar under the label — a SECOND
    /// reading of a value the row already states, never a replacement
    /// for it. `None` for a row that is not a quantity.
    pub bar: Option<f32>,
    /// How deep in a tree this row sits; 0 for a flat list.
    pub depth: u16,
    /// Whether the row can be expanded — a tree node with children.
    pub has_children: bool,
    /// Whether it currently is.
    pub expanded: bool,
}

impl RowBuf {
    pub fn new() -> RowBuf {
        RowBuf::default()
    }

    /// Empties the buffer without giving its allocations back, so the
    /// next row writes into the same strings.
    pub fn reset(&mut self) {
        self.key.clear();
        self.label.clear();
        self.status.clear();
        self.severity = None;
        self.bar = None;
        self.depth = 0;
        self.has_children = false;
        self.expanded = false;
    }
}

/// A sequence of rows a view can draw without owning them.
///
/// `generation` is what tells a cached sort or a cached flattening "new
/// data" from "the same data again"; a model with nothing to say leaves
/// it at 0 and its readers rebuild only when their own state moves.
pub trait RowModel {
    fn len(&self) -> usize;

    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Writes row `index` into `out`. Out of range: leave `out` reset —
    /// a view mid-resize legitimately asks for a row that has just gone.
    fn row(&self, index: usize, out: &mut RowBuf);

    /// The model's rewrite counter.
    fn generation(&self) -> u64 {
        0
    }

    /// The row's key alone, for a caller that wants identity without
    /// paying for the rest of the row (restoring a selection, say). The
    /// default fills a scratch buffer, which is correct everywhere and
    /// cheap enough for the once-per-click callers that use it.
    fn key(&self, index: usize) -> String {
        let mut buf = RowBuf::new();
        self.row(index, &mut buf);
        buf.key
    }
}

/// The simplest model there is: rows already in memory.
///
/// What a script's `list` element produces — the answer is an array of
/// at most `max_array_size` entries, so materialising it is what already
/// happened by the time the renderer sees it.
#[derive(Clone, Debug, Default)]
pub struct Rows {
    rows: Vec<RowBuf>,
    generation: u64,
}

impl Rows {
    pub fn new(rows: Vec<RowBuf>) -> Rows {
        Rows { rows, generation: 0 }
    }

    /// The same rows, carrying the snapshot counter they were built from.
    pub fn with_generation(mut self, generation: u64) -> Rows {
        self.generation = generation;
        self
    }

    pub fn push(&mut self, row: RowBuf) {
        self.rows.push(row);
    }

    pub fn as_slice(&self) -> &[RowBuf] {
        &self.rows
    }
}

impl RowModel for Rows {
    fn len(&self) -> usize {
        self.rows.len()
    }

    fn row(&self, index: usize, out: &mut RowBuf) {
        out.reset();
        if let Some(r) = self.rows.get(index) {
            out.clone_from(r);
        }
    }

    fn generation(&self) -> u64 {
        self.generation
    }

    fn key(&self, index: usize) -> String {
        self.rows.get(index).map(|r| r.key.clone()).unwrap_or_default()
    }
}

// ---------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn row(key: &str) -> RowBuf {
        RowBuf { key: key.into(), label: key.into(), ..RowBuf::default() }
    }

    #[test]
    fn a_reused_buffer_keeps_nothing_of_the_row_before_it() {
        let model = Rows::new(vec![
            RowBuf {
                key: "a".into(),
                label: "alpha".into(),
                status: "done".into(),
                severity: Some(Sev(0)),
                bar: Some(0.5),
                depth: 2,
                has_children: true,
                expanded: true,
            },
            row("b"),
        ]);
        let mut buf = RowBuf::new();
        model.row(0, &mut buf);
        assert_eq!(buf.status, "done");
        model.row(1, &mut buf);
        assert_eq!(buf.key, "b");
        assert_eq!(buf.status, "", "the previous row's status must not linger");
        assert_eq!(buf.severity, None);
        assert_eq!(buf.bar, None);
        assert_eq!(buf.depth, 0);
        assert!(!buf.has_children);
        assert!(!buf.expanded);
    }

    #[test]
    fn a_row_past_the_end_is_empty_rather_than_a_panic() {
        let model = Rows::new(vec![row("a")]);
        let mut buf = RowBuf::new();
        model.row(0, &mut buf);
        model.row(9, &mut buf);
        assert_eq!(buf.key, "");
        assert_eq!(model.key(9), "");
    }
}
