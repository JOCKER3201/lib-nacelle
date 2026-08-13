//! An open drop-down is ONE OBJECT: one frame, one bed, one inset —
//! and the frame unfolds with the list rather than appearing around it.
//!
//! The owner's report, from a screenshot of the settings window's open
//! THEMES list: "every drop-down is to have the same frame and the same
//! background as the window". What the picture showed was a list with no
//! frame AT ALL, whose rows ran wider than both the anchor they hung
//! from and the THEMES EDITOR button above them — because every row
//! painted its own rectangle of `component.menu.fill` and nothing was
//! ever drawn around the whole. A stack of rectangles has no outline to
//! stroke.
//!
//! It has one now, and it needed no new token to get one: the list
//! occupies `[elev.popover]` — Elev 5, the master's own "menu, tooltip,
//! dropdown, context menu, drag ghost" — whose `edge.color` is
//! `@component.panel.border`, the SAME token `[elev.focused]` states for
//! the window the list opens in.
//!
//! Every claim below is read out of a recording [`DrawList`]: the
//! commands the object actually issued, and the geometry they carry.
//!
//! * the box is ONE shaped fill and ONE ring, and not one per row;
//! * both come off `[elev.popover]`, and follow a theme that moves it;
//! * the rows sit inside it by `[menu].pad`, the inset they had none of;
//! * the box at unfold `p` is exactly `p` of the box at 1 — a frame at
//!   full size around a half-open list is the error this one is written
//!   against, and the animation is next in the queue;
//! * a row never crosses the box's corner, under all THREE corner
//!   languages — with the unclipped shape measured beside it, so the
//!   claim has the failure it prevents standing next to it.
//!
//! One test in a binary of its own: the resolved theme is process-wide
//! (§7.1 hands every draw path the same `&'static ResolvedTheme`), so a
//! test that swaps themes must not run beside one that reads them.

use nacelle::draw::{Corner, CornerStyle, DrawCmd, DrawList};
use nacelle::font::FontSystem;
use nacelle::object::dropdown::{self, AccordionStyle};
use nacelle::theme::{self, Color};
use nacelle::{Ctx, Rect};

const W: f32 = 1920.0;
const H: f32 = 1080.0;
const ROW_H: f32 = 30.0;
/// The anchor the list hangs from — wide enough that `menu.min_w`
/// cannot move it, and clear of the screen edges.
const ANCHOR: Rect = Rect { x: 200.0, y: 300.0, w: 400.0, h: 36.0 };
/// Nine, because nine is what the owner's screenshot held.
const NAMES: [&str; 9] = [
    "DEFAULT", "COCKPIT", "INSTRUMENT", "AURORA", "GRAPHITE", "SIGNAL", "VELLUM", "NOCTURNE",
    "EMBER",
];
/// Off screen: nothing hovers unless a case says so.
const AWAY: (f32, f32) = (-1.0, -1.0);

fn names() -> Vec<String> {
    NAMES.iter().map(|s| s.to_string()).collect()
}

/// The master plus `body`, so every token the fixture does not name is
/// the master's own.
fn skin(body: &str) {
    let path =
        std::env::temp_dir().join(format!("nacelle-popover-frame-{}.theme", std::process::id()));
    std::fs::write(
        &path,
        format!("[meta]\nschema = 1\nname = \"Fixture\"\nbase = \"default\"\n\n{body}"),
    )
    .expect("the fixture theme must be writable");
    let _ = theme::load_with(theme::LoadRequest { path: Some(path.clone()), ..Default::default() });
    let _ = std::fs::remove_file(&path);
    theme::set_viewport(H, 1.0);
}

fn master() {
    let _ = theme::load();
    theme::set_viewport(H, 1.0);
}

fn px_of(name: &str) -> f32 {
    let t = theme::resolved();
    t.px(theme::id(name).unwrap_or_else(|| panic!("the master declares {name}")))
}

fn color_of(name: &str) -> Color {
    let t = theme::resolved();
    t.color(theme::id(name).unwrap_or_else(|| panic!("the master declares {name}")))
}

fn rgba(c: Color) -> [f32; 4] {
    [c.r, c.g, c.b, c.a]
}

/// One drawing of the list, recorded, with the rectangles the object
/// answered.
fn shoot(
    fonts: &mut FontSystem,
    current: Option<usize>,
    p: f32,
) -> (DrawList, Vec<(Rect, bool)>) {
    shoot_names(fonts, &names(), current, p)
}

/// The same shot with the list's contents given, so a list of NOTHING can
/// be asked what it draws.
fn shoot_names(
    fonts: &mut FontSystem,
    names: &[String],
    current: Option<usize>,
    p: f32,
) -> (DrawList, Vec<(Rect, bool)>) {
    let mut dl = DrawList::recording();
    let rows = {
        let mut ctx = Ctx {
            dl: &mut dl,
            fonts,
            w: W,
            h: H,
            t: 0.0,
            mouse: AWAY,
            term_font_scale: 1.0,
            ui_font_scale: 1.0,
            panel_scale: 1.0,
            focus: None,
            tips: None,
        };
        dropdown::accordion(
            &mut ctx,
            ANCHOR,
            ROW_H,
            &names,
            p,
            &AccordionStyle { current, ..AccordionStyle::default() },
        )
    };
    (dl, rows)
}

/// The BOX: the first shaped fill on the list, which is the bed the
/// whole object stands on. Its rect, its corner and its colour.
fn bed(dl: &DrawList) -> ([f32; 4], Corner, Color) {
    dl.cmds()
        .iter()
        .find_map(|c| match c {
            DrawCmd::RingFill { r, corners, color } => Some((*r, corners[0], *color)),
            _ => None,
        })
        .expect("the list drew no bed of its own")
}

/// Its RING, if the theme states one.
fn ring(dl: &DrawList) -> Option<([f32; 4], Corner, f32, Color)> {
    dl.cmds().iter().find_map(|c| match c {
        DrawCmd::Ring { r, corners, stroke, color } => Some((*r, corners[0], *stroke, *color)),
        _ => None,
    })
}

fn count<F: Fn(&DrawCmd) -> bool>(dl: &DrawList, f: F) -> usize {
    dl.cmds().iter().filter(|c| f(c)).count()
}

fn is_fill(c: &DrawCmd) -> bool {
    matches!(c, DrawCmd::RingFill { .. })
}
fn is_ring(c: &DrawCmd) -> bool {
    matches!(c, DrawCmd::Ring { .. })
}
fn is_rect(c: &DrawCmd) -> bool {
    matches!(c, DrawCmd::Rect { .. })
}
fn is_text(c: &DrawCmd) -> bool {
    matches!(c, DrawCmd::Text { .. })
}

/// Every shaped fill EXCEPT the bed — i.e. the plates under marked rows.
fn plates(dl: &DrawList) -> Vec<([f32; 4], [Corner; 4])> {
    dl.cmds()
        .iter()
        .filter_map(|c| match c {
            DrawCmd::RingFill { r, corners, .. } => Some((*r, *corners)),
            _ => None,
        })
        .skip(1)
        .collect()
}

// ---------------------------------------------------------------------
// Geometry, written HERE and out of the shapes' definitions rather than
// out of the generator the object drew with — two readings that share a
// generator cannot disagree, which is the same as not measuring.
// ---------------------------------------------------------------------

/// How far a cut reaches into its box along the corner's diagonal.
fn depth(c: Corner) -> f32 {
    let s = c.size.max(0.0);
    match c.style {
        CornerStyle::Square => 0.0,
        CornerStyle::Round => s * (std::f32::consts::SQRT_2 - 1.0),
        CornerStyle::Chamfer => s / std::f32::consts::SQRT_2,
    }
}

/// Is `p` inside the rect `r` cut by `c` at all four corners?
///
/// `tol` is the slack a point ON the boundary is allowed: a row clipped
/// to exactly the box's own cut lies on it, which is inside and not out.
/// The cut is capped at half the short side, which is the cap the ring
/// generator applies too — a shape cannot be cut deeper than it is.
fn inside(p: [f32; 2], r: [f32; 4], c: Corner, tol: f32) -> bool {
    let (x, y, w, h) = (r[0], r[1], r[2], r[3]);
    if p[0] < x - tol || p[0] > x + w + tol || p[1] < y - tol || p[1] > y + h + tol {
        return false;
    }
    let s = c.size.max(0.0).min(w.min(h) * 0.5);
    if s <= 0.0 || c.style == CornerStyle::Square {
        return true;
    }
    // Distance from each corner, measured inward along both edges.
    for (dx, dy) in [
        (p[0] - x, p[1] - y),
        (x + w - p[0], p[1] - y),
        (x + w - p[0], y + h - p[1]),
        (p[0] - x, y + h - p[1]),
    ] {
        if dx >= s || dy >= s {
            continue; // not in this corner's square
        }
        match c.style {
            // The 45° face: x + y = s, keep the far side.
            CornerStyle::Chamfer => {
                if dx + dy < s - tol * std::f32::consts::SQRT_2 {
                    return false;
                }
            }
            // Inside the quarter circle centred s in from both edges.
            CornerStyle::Round => {
                let (ux, uy) = (s - dx, s - dy);
                if (ux * ux + uy * uy).sqrt() > s + tol {
                    return false;
                }
            }
            CornerStyle::Square => {}
        }
    }
    true
}

/// The boundary of the rect `r` cut by `corners`, sampled densely:
/// every corner's arc or face plus the straight runs between them.
///
/// Sampled rather than tessellated, because the question is not "did the
/// same generator produce the same triangles" — it is "does the shape
/// this command DESCRIBES stay inside the shape that command describes".
fn boundary(r: [f32; 4], corners: &[Corner; 4]) -> Vec<[f32; 2]> {
    let (x, y, w, h) = (r[0], r[1], r[2], r[3]);
    let cap = (w.min(h) * 0.5).max(0.0);
    // corner point, direction back along the incoming edge, along the outgoing
    let geo: [([f32; 2], [f32; 2], [f32; 2]); 4] = [
        ([x, y], [0.0, 1.0], [1.0, 0.0]),
        ([x + w, y], [-1.0, 0.0], [0.0, 1.0]),
        ([x + w, y + h], [0.0, -1.0], [-1.0, 0.0]),
        ([x, y + h], [1.0, 0.0], [0.0, -1.0]),
    ];
    let mut out = Vec::new();
    for (i, &(pc, ein, eout)) in geo.iter().enumerate() {
        let s = corners[i].size.max(0.0).min(cap);
        let a = [pc[0] + s * ein[0], pc[1] + s * ein[1]];
        let b = [pc[0] + s * eout[0], pc[1] + s * eout[1]];
        match corners[i].style {
            CornerStyle::Square => out.push(pc),
            CornerStyle::Chamfer => {
                for k in 0..=32 {
                    let u = k as f32 / 32.0;
                    out.push([a[0] + (b[0] - a[0]) * u, a[1] + (b[1] - a[1]) * u]);
                }
            }
            CornerStyle::Round => {
                let centre = [pc[0] + s * (ein[0] + eout[0]), pc[1] + s * (ein[1] + eout[1])];
                for k in 0..=32 {
                    let th = std::f32::consts::FRAC_PI_2 * k as f32 / 32.0;
                    // from the incoming endpoint round to the outgoing one
                    let (vx, vy) = (a[0] - centre[0], a[1] - centre[1]);
                    // rotate (vx,vy) toward b; the sign follows the winding
                    let (sn, cs) = th.sin_cos();
                    let rot = [vx * cs - vy * sn, vx * sn + vy * cs];
                    let alt = [vx * cs + vy * sn, -vx * sn + vy * cs];
                    let pick = if dist([centre[0] + rot[0], centre[1] + rot[1]], b)
                        <= dist([centre[0] + alt[0], centre[1] + alt[1]], b)
                    {
                        rot
                    } else {
                        alt
                    };
                    out.push([centre[0] + pick[0], centre[1] + pick[1]]);
                }
            }
        }
        // the straight run to the next corner's entry point
        let next = (i + 1) % 4;
        let ns = corners[next].size.max(0.0).min(cap);
        let (npc, nein, _) = geo[next];
        let n = [npc[0] + ns * nein[0], npc[1] + ns * nein[1]];
        for k in 1..16 {
            let u = k as f32 / 16.0;
            out.push([b[0] + (n[0] - b[0]) * u, b[1] + (n[1] - b[1]) * u]);
        }
    }
    out
}

fn dist(a: [f32; 2], b: [f32; 2]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}

/// How far outside `hull` the worst point of `shape` falls; `<= 0` means
/// the shape is contained.
fn escape(shape: &[[f32; 2]], hull: [f32; 4], cut: Corner) -> f32 {
    let mut worst: f32 = 0.0;
    for p in shape {
        if inside(*p, hull, cut, 0.02) {
            continue;
        }
        // Bisect on the tolerance to say HOW far out it is — a number the
        // failure message can carry.
        let (mut lo, mut hi) = (0.02f32, 64.0f32);
        for _ in 0..24 {
            let mid = (lo + hi) * 0.5;
            if inside(*p, hull, cut, mid) {
                hi = mid;
            } else {
                lo = mid;
            }
        }
        worst = worst.max(hi);
    }
    worst
}

// ---------------------------------------------------------------------

#[test]
fn an_open_drop_down_is_one_framed_box_that_unfolds_with_its_rows() {
    master();
    let mut fonts = FontSystem::new();

    // ================================================================
    // 1 · ONE frame and ONE bed — not one per row
    // ================================================================
    // A list of NOTHING draws nothing. Before the box existed the row loop
    // simply never ran, so this held by accident; now the box is drawn
    // before the rows are counted, and a frame around no rows is a frame
    // around nothing.
    let (empty, empty_rows) = shoot_names(&mut fonts, &[], None, 1.0);
    assert!(empty_rows.is_empty(), "an empty list returned rows");
    assert_eq!(
        empty.cmds().len(),
        0,
        "an empty list drew {} command(s) — a frame around nothing",
        empty.cmds().len()
    );

    let (open, rows) = shoot(&mut fonts, None, 1.0);
    assert_eq!(rows.len(), NAMES.len(), "the list drew {} of 9 rows", rows.len());
    assert_eq!(count(&open, is_text), NAMES.len(), "a list that drew no labels proves nothing");

    // The picture the owner reported was nine axis-aligned rectangles,
    // one `component.menu.fill` per row, and no outline anywhere. Both
    // halves of that are gone: no row paints a bed, and the whole wears
    // exactly one.
    assert_eq!(
        count(&open, is_rect),
        0,
        "a row still paints a rectangle of its own — nine of those, with nothing \
         around them, is the stack of strips the report was about"
    );
    assert_eq!(
        count(&open, is_fill),
        1,
        "the list laid {} beds; a list is one object and stands on one",
        count(&open, is_fill)
    );
    assert_eq!(
        count(&open, is_ring),
        1,
        "the list drew {} rings where it owes exactly one — its own",
        count(&open, is_ring)
    );

    // The one ring is around the WHOLE: same rect as the bed, and that
    // rect starts at the anchor's bottom edge and is as wide as it.
    let (bed_r, bed_c, bed_col) = bed(&open);
    let (ring_r, ring_c, ring_w, ring_col) = ring(&open).expect("the master states a popover ring");
    assert_eq!(bed_r, ring_r, "the ring is not drawn around the bed it is supposed to frame");
    assert_eq!(bed_c, ring_c, "the ring and the bed disagree about the shape of one box");
    let skew = px_of("button.skew");
    let pad = px_of("menu.pad");
    assert!(pad > 0.0, "the master keeps no room inside the menu box — §3 below cannot bite");
    assert_eq!(
        [bed_r[0], bed_r[1], bed_r[2]],
        [ANCHOR.x, ANCHOR.bottom() + px_of("menu.anchor_gap"), ANCHOR.w - skew],
        "the box does not hang off the anchor's bottom edge"
    );
    // TWO COMPLETE FRAMES, NOT ONE SHAPE WITH A SEAM. Flush, the anchor and
    // the box closed their outlines on the SAME line: the ring was stroked
    // twice there, each corner curved into the other so the rounding read as
    // cancelled, and the vertical edge stepped from 2 px at 0.95 alpha (the
    // anchor wears the `selected` rung while its list is open) to 1 px at
    // 0.78 halfway down what looks like one line. The owner asked for the
    // frame of the control and the frame of the list, and nothing running
    // out of one into the other.
    //
    // Asserted as a STRICT gap and as the token, not as 2.7 px: a theme may
    // widen the air, and may not close it.
    let gap = px_of("menu.anchor_gap");
    assert!(
        gap > 0.0,
        "menu.anchor_gap is {gap} — flush again, and the two frames share an edge"
    );
    assert!(
        bed_r[1] > ANCHOR.bottom(),
        "the box starts at {} and the anchor ends at {} — they touch",
        bed_r[1],
        ANCHOR.bottom()
    );
    assert!(
        (bed_r[3] - (ROW_H * NAMES.len() as f32 + 2.0 * pad)).abs() < 0.01,
        "the finished box is {} tall where its nine rows and its two pads come to {}",
        bed_r[3],
        ROW_H * NAMES.len() as f32 + 2.0 * pad
    );

    // ================================================================
    // 2 · the box is `[elev.popover]`, and its ring is the window's
    // ================================================================
    assert_eq!(
        rgba(bed_col),
        rgba(color_of("elev.popover.fill")),
        "the box's bed is not the Elev 5 material"
    );
    assert_eq!(
        rgba(ring_col),
        rgba(color_of("elev.popover.edge.color")),
        "the box's ring is not the Elev 5 edge"
    );
    assert_eq!(
        ring_w,
        px_of("elev.popover.edge.width"),
        "the ring's weight is not the one Elev 5 states"
    );
    // The owner asked for "the same frame as the window". The window is
    // Elev 4, and the two levels name ONE token for their edge — which
    // is why this took no new token at all. Asserted, because the whole
    // answer to the report rests on it.
    assert_eq!(
        rgba(color_of("elev.popover.edge.color")),
        rgba(color_of("elev.focused.edge.color")),
        "[elev.popover] and [elev.focused] no longer state one edge — the list \
         and the window it opens in are framed differently"
    );

    // Negative controls: each of the three keys, moved by one line, and
    // the picture moves with it. A colour written into this object would
    // sit still through all three.
    //
    // The body is turned off by its ALPHA and not by the word `none`:
    // in this engine a colour written `none` bakes to opaque black
    // (measured, not assumed — `[elev.popover] glow.inner.color = none`
    // in the master reads back as 0,0,0,1), so alpha is what a level's
    // reader can act on and alpha is what it reads.
    skin("[elev.popover]\nfill = #000000 / 0.0\n");
    let (no_bed, _) = shoot(&mut fonts, None, 1.0);
    assert_eq!(
        count(&no_bed, is_fill),
        0,
        "`[elev.popover] fill = none` and the box still painted a bed"
    );
    skin("[elev.popover]\nedge.width = 0px\n");
    let (no_ring, _) = shoot(&mut fonts, None, 1.0);
    assert_eq!(
        count(&no_ring, is_ring),
        0,
        "`[elev.popover] edge.width = 0px` and the box still stroked a ring"
    );
    skin("[elev.popover]\nedge.color = #FF00FF / 1.0\n");
    let (magenta, _) = shoot(&mut fonts, None, 1.0);
    assert_eq!(
        rgba(ring(&magenta).expect("the fixture states a ring").3),
        rgba(color_of("elev.popover.edge.color")),
        "the ring did not follow `[elev.popover] edge.color` — it is written into \
         the object, not read from the level"
    );

    // ================================================================
    // 3 · the rows sit INSIDE the box, by the inset it keeps
    // ================================================================
    master();
    let (_, rows) = shoot(&mut fonts, None, 1.0);
    let pad = px_of("menu.pad");
    for (i, (r, _)) in rows.iter().enumerate() {
        assert!(
            (r.x - (bed_r[0] + pad)).abs() < 0.01,
            "row {i} starts at {} where the box's inside starts at {}",
            r.x,
            bed_r[0] + pad
        );
        assert!(
            (r.right() - (bed_r[0] + bed_r[2] - pad)).abs() < 0.01,
            "row {i} runs to {} where the box's inside ends at {} — this is the \
             report itself: rows wider than the anchor and the button above them",
            r.right(),
            bed_r[0] + bed_r[2] - pad
        );
    }
    assert!(
        (rows[0].0.y - (bed_r[1] + pad)).abs() < 0.01,
        "the first row touches the box's own top edge instead of its inside"
    );
    assert!(
        (rows[NAMES.len() - 1].0.bottom() - (bed_r[1] + bed_r[3] - pad)).abs() < 0.01,
        "the last row touches the box's own bottom edge instead of its inside"
    );

    // The negative control for the inset: it is `[menu].pad`'s, so a
    // theme that keeps no room gets the flush rows back.
    skin("[menu]\npad = 0u\n");
    assert_eq!(px_of("menu.pad"), 0.0, "the fixture's own pad did not bake");
    let (flush_dl, flush) = shoot(&mut fonts, None, 1.0);
    let flush_box = bed(&flush_dl).0;
    assert_eq!(flush[0].0.x, flush_box[0], "`[menu].pad = 0u` and the rows kept an inset");
    assert_eq!(flush[0].0.w, flush_box[2], "`[menu].pad = 0u` and the rows kept an inset");

    // ================================================================
    // 4 · the frame UNFOLDS — it does not appear
    // ================================================================
    master();
    let full = bed(&shoot(&mut fonts, None, 1.0).0).0;
    let mut last_h = -1.0f32;
    for step in 0..=10 {
        let p = step as f32 / 10.0;
        let (dl, rows) = shoot(&mut fonts, None, p);
        if p == 0.0 {
            // A closed list is not a box of zero height, it is no box.
            assert_eq!(count(&dl, is_fill), 0, "a closed list still drew a box");
            assert!(rows.is_empty(), "a closed list drew rows");
            last_h = 0.0;
            continue;
        }
        let b = bed(&dl).0;
        assert_eq!([b[0], b[1], b[2]], [full[0], full[1], full[2]], "the box moved sideways \
             while it unfolded — only its height is a function of p");
        assert!(
            (b[3] - p * full[3]).abs() < 0.01,
            "at p = {p} the box is {} tall where p of the finished box is {} — a \
             frame that is not p of itself is a frame appearing around a list that \
             is still opening, which is exactly what the owner will see next",
            b[3],
            p * full[3]
        );
        assert!(b[3] > last_h, "the box did not grow between p = {} and p = {p}", p - 0.1);
        last_h = b[3];
        // …and it always CONTAINS what it is opening on: no row is ever
        // outside the frame that is supposed to be around it.
        let pad = px_of("menu.pad");
        for (i, (r, _)) in rows.iter().enumerate() {
            assert!(
                r.y >= b[1] + pad - 0.01 && r.bottom() <= b[1] + b[3] - pad + 0.01,
                "at p = {p} row {i} spans {}..{} and the box's inside is {}..{}",
                r.y,
                r.bottom(),
                b[1] + pad,
                b[1] + b[3] - pad
            );
        }
    }
    // The failure this section exists for, stated as a number: a frame
    // drawn at full size around a half-open list.
    let half = bed(&shoot(&mut fonts, None, 0.5).0).0;
    assert!(
        half[3] < full[3] - 0.5,
        "the box at half unfold is as tall as the finished one ({} against {})",
        half[3],
        full[3]
    );

    // ================================================================
    // 5 · a row never crosses the box's corner — all three languages
    // ================================================================
    // The fixture takes the room away (`pad = 0u`, so the rows sit ON
    // the box's boundary) and takes the row's own cut away (`[list]
    // corner = 0u`, so a square row is pressed into a cut corner). That
    // is the worst case the object can be handed, and it is the one that
    // used to poke out.
    for word in ["round", "chamfer", "square"] {
        skin(&format!(
            "[corner]\nmode = {word}\n\n[menu]\npad = 0u\n\n[list]\ncorner = 0u\n"
        ));
        let (dl, _) = shoot(&mut fonts, Some(0), 1.0);
        let (box_r, box_c, _) = bed(&dl);
        let marked = plates(&dl);
        assert_eq!(marked.len(), 1, "one row in force, {} plates", marked.len());
        let (plate_r, plate_c) = marked[0];

        // What the object DREW, measured against the box it drew it in.
        let drawn = escape(&boundary(plate_r, &plate_c), box_r, box_c);
        assert!(
            drawn <= 0.0,
            "under `@corner.mode = {word}` the top row escapes its own box by \
             {drawn} px"
        );

        // The negative control, computed rather than claimed: THE SAME
        // ROW with the clip undone — the cut `[list]` states for it,
        // which the fixture has set to nothing. That is the shape this
        // object drew before there was a box to be clipped against.
        // Under a cut box it is outside; under a square one nothing can
        // be, and the case says so instead of passing quietly.
        assert_eq!(px_of("list.corner"), 0.0, "the fixture's own row cut did not bake");
        let own = Corner { style: box_c.style, size: 0.0 };
        let unclipped = escape(&boundary(plate_r, &[own; 4]), box_r, box_c);
        // A cut is compared by DEPTH and not by `size`: the three styles
        // are not one scale, and a `round` row inside a `round` box
        // would compare equal on style alone whether or not it had been
        // clipped at all.
        let deep = depth(box_c);
        if word == "square" {
            assert_eq!(box_c.style, CornerStyle::Square);
            assert_eq!(deep, 0.0, "a square box reaches into itself");
            assert!(unclipped <= 0.0, "a square box cut a row out of nothing");
            for (i, c) in plate_c.iter().enumerate() {
                assert_eq!(
                    depth(*c),
                    0.0,
                    "corner {i} was cut under a box that cuts nothing"
                );
            }
        } else {
            assert!(deep > 0.5, "the fixture's box is barely cut — nothing here bites");
            assert!(
                unclipped > 0.5,
                "under `@corner.mode = {word}` the row's own cut already stayed \
                 inside the box (escape {unclipped} px), so this case proves \
                 nothing about the clip — the fixture has stopped biting"
            );
            // The two corners the top row SHARES with the box are the
            // box's; the two it does not are still its own — which the
            // fixture has made nothing, so they measure as nothing.
            for i in [0usize, 1] {
                assert!(
                    depth(plate_c[i]) >= deep - 0.01,
                    "the top row's corner {i} reaches {} where the box reaches {deep}",
                    depth(plate_c[i])
                );
            }
            for i in [2usize, 3] {
                assert_eq!(
                    depth(plate_c[i]),
                    0.0,
                    "the top row's BOTTOM corner {i} was cut too, and it stands on \
                     nothing there — the clip reached past the boundary it is a \
                     clip against"
                );
            }
        }
    }

    // The last row is clipped at the bottom for the same reason, and a
    // row in the middle is clipped nowhere: the box only overrules a row
    // where the row is actually standing on it.
    skin("[corner]\nmode = round\n\n[menu]\npad = 0u\n\n[list]\ncorner = 0u\n");
    let (dl, _) = shoot(&mut fonts, Some(NAMES.len() - 1), 1.0);
    let (box_r, box_c, _) = bed(&dl);
    let deep = depth(box_c);
    let (last_r, last_c) = plates(&dl)[0];
    assert!(
        escape(&boundary(last_r, &last_c), box_r, box_c) <= 0.0,
        "the LAST row escapes the bottom of its box"
    );
    for i in [2usize, 3] {
        assert!(
            depth(last_c[i]) >= deep - 0.01,
            "the last row's bottom corner {i} did not take the box's cut"
        );
    }
    for i in [0usize, 1] {
        assert_eq!(
            depth(last_c[i]),
            0.0,
            "the last row was cut at its top corner {i} too, where it stands on nothing"
        );
    }

    let (dl, _) = shoot(&mut fonts, Some(4), 1.0);
    let (_, mid_c) = plates(&dl)[0];
    for (i, c) in mid_c.iter().enumerate() {
        assert_eq!(
            depth(*c),
            0.0,
            "corner {i} of a row in the MIDDLE of the list took the box's cut — \
             the clip is not a clip, it is a second set of clothes"
        );
    }

    master();
}
