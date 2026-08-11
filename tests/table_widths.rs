//! The column-width solver was MOVED, not rewritten — and this file is
//! the proof.
//!
//! `ui::table` computed its widths inline (u2 §2.7: measure, then the
//! slack ladder, then the elastic column's leftover). Phase 2 lifts that
//! arithmetic into `view::table::solve_widths` so a second drawing path
//! and a plugin-side table can reach it. The condition of that move is
//! that the master renders the same pixels afterwards, and a table's
//! pixels are decided by these floats.
//!
//! So the old arithmetic is written out below, once, exactly as it stood
//! in `ui::table` — with `ctx.fonts.measure` replaced by numbers, since
//! measuring text needs a font system and this test needs no window —
//! and every case runs through both. Not "close enough": the same bits.
//! If a future change to `solve_widths` is meant to change the look, it
//! changes this file too, deliberately, and the diff says so.

use nacelle::view::table::{solve_widths, ColMeasure, TableTokens};

/// `ui::table`'s width arithmetic as it stood before the extraction.
/// `head[i]`, `content[i]`, `bar[i]` are what the measuring loop found.
fn old_widths(
    head: &[f32],
    content: &[f32],
    bar: &[bool],
    avail: f32,
    elastic: usize,
    col_gap: f32,
    cell_pad: f32,
    bar_w: f32,
    elastic_min_w: f32,
    col_min_w: f32,
) -> Vec<f32> {
    let n = head.len();
    let extra = col_gap + cell_pad;
    let mut widths: Vec<f32> = Vec::with_capacity(n);
    let mut bar_slack: Vec<f32> = vec![0.0; n];
    let mut content_slack: Vec<f32> = vec![0.0; n];
    for i in 0..n {
        let h = head[i];
        let mut w = h;
        if i != elastic {
            // The content measure: `w.max(cell)` over the shown rows.
            w = w.max(content[i]);
            content_slack[i] = w - h;
        }
        if bar[i] && i != elastic {
            bar_slack[i] = bar_w + col_gap;
            w += bar_w + col_gap;
        }
        widths.push(w + extra);
    }
    let sum_fixed = |ws: &[f32]| -> f32 {
        ws.iter()
            .enumerate()
            .filter(|(i, _)| *i != elastic)
            .map(|(_, w)| *w)
            .sum()
    };
    let elastic_min = elastic_min_w + extra;
    let mut deficit = elastic_min - (avail - sum_fixed(&widths));
    for slack in [&bar_slack, &content_slack] {
        if deficit <= 0.0 {
            break;
        }
        let total: f32 = slack.iter().sum();
        if total <= 0.0 {
            continue;
        }
        let k = (deficit / total).min(1.0);
        for (w, s) in widths.iter_mut().zip(slack.iter()) {
            *w -= s * k;
        }
        deficit -= total * k;
    }
    let leftover = avail - sum_fixed(&widths);
    if let Some(w) = widths.get_mut(elastic) {
        *w = leftover.max(col_min_w + extra);
    }
    widths
}

/// The master's `table.*`, at shrink 1: col_gap 2.4u, cell_pad 0.6u,
/// bar_w 6u, elastic_min_w 9u, col_min_w 2.6u at the reference u.
const COL_GAP: f32 = 13.0;
const CELL_PAD: f32 = 3.2;
const BAR_W: f32 = 32.4;
const ELASTIC_MIN_W: f32 = 48.6;
const COL_MIN_W: f32 = 14.0;

fn tokens(shrink: f32) -> TableTokens {
    // The shrink asymmetry `ui::table` has always had, reproduced here
    // on purpose: the gaps shrink with the type, the two minima do not.
    TableTokens {
        col_gap: COL_GAP * shrink,
        cell_pad: CELL_PAD * shrink,
        bar_w: BAR_W * shrink,
        elastic_min_w: ELASTIC_MIN_W,
        col_min_w: COL_MIN_W,
    }
}

/// Runs one case through both paths and demands identical floats.
fn same(head: &[f32], content: &[f32], bar: &[bool], avail: f32, elastic: usize, shrink: f32) {
    let t = tokens(shrink);
    let old = old_widths(
        head, content, bar, avail, elastic, t.col_gap, t.cell_pad, t.bar_w, t.elastic_min_w,
        t.col_min_w,
    );
    let measured: Vec<ColMeasure> = (0..head.len())
        .map(|i| ColMeasure { head: head[i], content: content[i], bar: bar[i] })
        .collect();
    let new = solve_widths(&measured, avail, elastic, &[], &t);
    assert_eq!(
        new.len(),
        old.len(),
        "column count: {head:?} {content:?} {bar:?} avail={avail} elastic={elastic}"
    );
    for (i, (a, b)) in new.iter().zip(old.iter()).enumerate() {
        assert_eq!(
            a.to_bits(),
            b.to_bits(),
            "column {i}: {a} != {b} (head={head:?} content={content:?} bar={bar:?} \
             avail={avail} elastic={elastic} shrink={shrink})"
        );
    }
}

/// The PROCESSES table of the shipped widget: PID, CPU with a bar, MEM
/// with a bar, NAME elastic — the arrangement the master actually draws,
/// at the panel widths a 1080p and a 4K screen give it.
#[test]
fn the_shipped_process_table_solves_identically() {
    let head = [26.0, 30.0, 34.0, 44.0];
    let content = [48.0, 41.0, 46.0, 44.0];
    let bar = [false, true, true, false];
    for avail in [180.0, 240.0, 320.0, 460.0, 640.0, 1200.0] {
        for shrink in [1.0, 0.86, 0.62] {
            same(&head, &content, &bar, avail, 3, shrink);
        }
    }
}

/// The narrow end, where the slack ladder actually runs: bar
/// reservations first, then the content measure, then the elastic floor.
#[test]
fn every_rung_of_the_slack_ladder_is_identical() {
    let head = [26.0, 30.0, 34.0, 44.0];
    let content = [120.0, 90.0, 88.0, 44.0];
    let bar = [false, true, true, false];
    // From "everything fits" down to "narrower than the headings".
    for avail in [
        900.0, 600.0, 420.0, 380.0, 340.0, 300.0, 260.0, 220.0, 180.0, 140.0, 100.0, 60.0, 20.0,
        0.0,
    ] {
        same(&head, &content, &bar, avail, 3, 1.0);
    }
}

/// Every column measured from its heading (`width: "heading"`), so no
/// content slack exists at all and the ladder skips a rung.
#[test]
fn a_table_with_no_content_slack_solves_identically() {
    let head = [40.0, 55.0, 30.0];
    let content = head; // what the drawer passes for ColWidth::Heading
    let bar = [false, false, false];
    for avail in [400.0, 200.0, 90.0] {
        same(&head, &content, &bar, avail, 2, 1.0);
    }
}

/// The elastic column at either end, one column on its own, and an
/// elastic index the script made up — the edges `ui::table` has always
/// had to survive.
#[test]
fn the_edges_solve_identically() {
    let head = [26.0, 30.0, 34.0];
    let content = [60.0, 70.0, 80.0];
    let bar = [true, false, true];
    for elastic in [0usize, 1, 2, 3, 99] {
        for avail in [500.0, 200.0, 80.0] {
            same(&head, &content, &bar, avail, elastic, 1.0);
        }
    }
    // One column, which is also the elastic one.
    same(&[40.0], &[40.0], &[false], 300.0, 0, 1.0);
    same(&[40.0], &[40.0], &[false], 10.0, 0, 1.0);
    // No columns at all: `ui::table` returns before it measures, but the
    // solver must still answer rather than panic.
    same(&[], &[], &[], 300.0, 0, 1.0);
}

/// Zero and absurd widths: a panel mid-resize really does measure zero,
/// and a script really does ask for a table in a box that is not there.
#[test]
fn degenerate_widths_solve_identically() {
    let head = [20.0, 20.0];
    let content = [200.0, 20.0];
    let bar = [true, false];
    for avail in [0.0, -50.0, 1e6] {
        same(&head, &content, &bar, avail, 1, 1.0);
    }
}
