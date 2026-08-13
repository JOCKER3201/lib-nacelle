//! A drop-down's rows are LIST rows, and the mark on one is a plate.
//!
//! The owner's report was about the settings window's THEMES list: a
//! rounded anchor, and under it nine square strips each carrying its own
//! hairline ring, so every seam between two neighbours showed two lines
//! stacked on each other. The cause was one line of binding —
//! `dropdown.rs` took the `menu.item` class, whose ladder states an
//! `edge_width` per rung, and stroked that ring around EVERY row.
//!
//! Everything below is measured out of a recording [`DrawList`], which
//! is the picture itself and not a claim about it:
//!
//! * a resting list puts NO ring and NO outline around a ROW, and the
//!   class it draws from still states a width — so the absence is this
//!   object's decision, not a theme that happens to ask for nothing.
//!   The one ring the list does draw is around the WHOLE — its
//!   `[elev.popover]` box, whose subject is [`dropdown_popover_frame`],
//!   and every count here subtracts it by its rectangle so a claim
//!   about rows cannot read the container;
//! * the row in force gets ONE plate, cut to `[list].corner_style`, and
//!   the rows beside it get none;
//! * the label's px is the px it always was, because both the binding
//!   this object used to read and the one it reads now name `body`;
//! * the face comes off the ROLE — a theme that moves `type.body.face`
//!   moves the row's font slot, which a hard-coded `FONT_UI` could not;
//! * rows line up with their anchor's own edge, skew and all.
//!
//! One test in a binary of its own: the resolved theme is process-wide
//! (§7.1 hands every draw path the same `&'static ResolvedTheme`), so a
//! test that swaps themes must not run beside one that reads them.

use nacelle::draw::{Corner, CornerStyle, DrawCmd, DrawList};
use nacelle::font::FontSystem;
use nacelle::object::dropdown::{self, AccordionStyle};
use nacelle::theme::{self, parse::State};
use nacelle::{Ctx, Rect};

const W: f32 = 1920.0;
const H: f32 = 1080.0;
const ROW_H: f32 = 30.0;
/// The anchor the list hangs from, wide enough that no width floor can
/// move it and clear of the screen edges.
const ANCHOR: Rect = Rect { x: 200.0, y: 300.0, w: 400.0, h: 36.0 };
const NAMES: [&str; 3] = ["ALPHA", "BETA", "GAMMA"];
/// Off screen: nothing is hovering unless a case says so.
const AWAY: (f32, f32) = (-1.0, -1.0);

fn names() -> Vec<String> {
    NAMES.iter().map(|s| s.to_string()).collect()
}

/// Loads a fixture theme whose base is the master, so every token but
/// the ones in `body` is the master's own. The same harness
/// `control_shape_tokens` uses.
fn skin(body: &str) {
    let path =
        std::env::temp_dir().join(format!("nacelle-list-dress-{}.theme", std::process::id()));
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

/// One drawing of the list, recorded.
fn shoot(fonts: &mut FontSystem, current: Option<usize>, mouse: (f32, f32), p: f32) -> DrawList {
    let names = names();
    let mut dl = DrawList::recording();
    {
        let mut ctx = Ctx {
            dl: &mut dl,
            fonts,
            w: W,
            h: H,
            t: 0.0,
            mouse,
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
        );
    }
    dl
}

/// The popover box the list draws around itself, for the theme loaded
/// right now and this file's anchor at unfold `p`.
///
/// Recomputed from tokens rather than remembered, so it follows every
/// fixture below — and so the claims about ROWS can tell the container's
/// commands from theirs without counting on emission order.
fn popover(p: f32) -> [f32; 4] {
    let w = (ANCHOR.w - px_of("button.skew")).max(px_of("menu.min_w"));
    let content = ROW_H * NAMES.len() as f32 + px_of("list.gap") * (NAMES.len() - 1) as f32;
    // `menu.anchor_gap` below the anchor, not flush with it: two complete
    // rounded frames with background between them, so neither borrows the
    // other's edge.
    [
        ANCHOR.x,
        ANCHOR.bottom() + px_of("menu.anchor_gap"),
        w,
        p * (content + 2.0 * px_of("menu.pad")),
    ]
}

/// Whether a command's rect is the box's — within a hair, because the
/// height is a product.
fn is_popover(r: [f32; 4], p: f32) -> bool {
    let b = popover(p);
    (0..4).all(|i| (r[i] - b[i]).abs() < 0.01)
}

/// Every stroked box in the picture EXCEPT the popover's own ring: the
/// `rect_outline` the row ring used to be, plus the shaped `ring` that
/// would replace it if the object ever strokes a plate's outline.
///
/// The container's ring is subtracted by its RECTANGLE, so this counts
/// what it says it counts: rings around rows. The ring around the whole
/// is the thing the owner asked for.
fn boxes(dl: &DrawList, p: f32) -> usize {
    dl.cmds()
        .iter()
        .filter(|c| match c {
            DrawCmd::RectOutline { .. } => true,
            DrawCmd::Ring { r, .. } => !is_popover(*r, p),
            _ => false,
        })
        .count()
}

/// Every plate: the shaped fill under a ROW, with the rect it covers,
/// its corner and its colour's alpha. The popover's own shaped fill is
/// not a plate and is subtracted the same way.
fn plates_at(dl: &DrawList, p: f32) -> Vec<([f32; 4], Corner, f32)> {
    dl.cmds()
        .iter()
        .filter_map(|c| match c {
            DrawCmd::RingFill { r, corners, color } if !is_popover(*r, p) => {
                Some((*r, corners[0], color.a))
            }
            _ => None,
        })
        .collect()
}

/// [`plates_at`] for the finished list, which is most of this file.
fn plates(dl: &DrawList) -> Vec<([f32; 4], Corner, f32)> {
    plates_at(dl, 1.0)
}

/// Every label: where it sits, in which font slot, at what px.
fn labels(dl: &DrawList) -> Vec<([f32; 2], u8, f32, String)> {
    dl.cmds()
        .iter()
        .filter_map(|c| match c {
            DrawCmd::Text { at, font, px, text, .. } => {
                Some((*at, *font, *px, text.clone()))
            }
            _ => None,
        })
        .collect()
}

/// The figure advance each label was drawn under — zero for a
/// proportional run, the box's width for a boxed one. Read off the
/// command, so it is what the call ASKED for and not an inference from
/// where the glyphs landed.
fn advances(dl: &DrawList) -> Vec<f32> {
    dl.cmds()
        .iter()
        .filter_map(|c| match c {
            DrawCmd::Text { tabular, .. } => Some(*tabular),
            _ => None,
        })
        .collect()
}

fn lines(dl: &DrawList) -> usize {
    dl.cmds().iter().filter(|c| matches!(c, DrawCmd::Line { .. })).count()
}

fn px_of(name: &str) -> f32 {
    let t = theme::resolved();
    t.px(theme::id(name).unwrap_or_else(|| panic!("the master declares {name}")))
}

/// The unfold `p` at which the box has opened far enough to hold
/// `rows` rows' worth of content.
///
/// The box scales as ONE object — the room it keeps is part of what
/// unfolds — so `p` is not the fraction of the content that shows.
/// Every `p` below is written through this, so the fixtures state what
/// they mean ("one and a half rows out") and follow a theme that
/// changes the pad or the gap.
fn unfold(rows: f32) -> f32 {
    let pad = px_of("menu.pad");
    let content = ROW_H * NAMES.len() as f32 + px_of("list.gap") * (NAMES.len() - 1) as f32;
    (rows * (ROW_H + px_of("list.gap")) + 2.0 * pad) / (content + 2.0 * pad)
}

#[test]
fn a_drop_downs_rows_are_list_rows_and_the_mark_on_one_is_a_plate() {
    master();
    let mut fonts = FontSystem::new();

    // ---- 1 · not one ring anywhere -----------------------------------
    // Three rows, resting. Before this change each of them put a
    // `rect_outline` of its own on the screen — three boxes, and in an
    // open THEMES list nine.
    let idle = shoot(&mut fonts, None, AWAY, 1.0);
    assert_eq!(
        boxes(&idle, 1.0),
        0,
        "a resting list still strokes {} box(es) AROUND ROWS — a ring around every \
         row is what makes nine themes read as nine loose boxes",
        boxes(&idle, 1.0)
    );
    // The negative control for that zero: the class the rows draw from
    // DOES state a ring, on every rung the list can stand on. So the
    // empty count above is this object refusing to stroke one, not a
    // ladder with nothing to stroke.
    let t = theme::resolved();
    let class = theme::class_id("list.item").expect("the master declares the list.item class");
    for rung in [State::Idle, State::Hover, State::Selected, State::SelectedHover] {
        assert!(
            t.class_state(class, rung).edge_width > 0.0,
            "list.item's {rung:?} rung states no edge width — this probe cannot \
             tell a refused ring from an absent one"
        );
    }
    // And the rows really were drawn: one label per row.
    assert_eq!(labels(&idle).len(), NAMES.len(), "a list that drew no labels proves nothing");

    // ---- 2 · the mark is a plate, and only the marked row wears one ---
    assert!(plates(&idle).is_empty(), "a resting row wears a plate — then it marks nothing");
    let mid = shoot(&mut fonts, Some(1), AWAY, 1.0);
    let marked = plates(&mid);
    assert_eq!(marked.len(), 1, "one row in force, {} plates", marked.len());
    let (r, corner, alpha) = marked[0];
    // Under the row it belongs to, edge to edge with it — and that row
    // is INSIDE the box, by the room `[menu].pad` keeps on every side.
    let pad = px_of("menu.pad");
    assert!(pad > 0.0, "the master keeps no room inside the menu box — nothing below bites");
    assert_eq!(
        r,
        [
            ANCHOR.x + pad,
            ANCHOR.bottom() + px_of("menu.anchor_gap") + pad + ROW_H,
            ANCHOR.w - px_of("button.skew") - 2.0 * pad,
            ROW_H
        ],
        "the plate is not the second row's own rectangle"
    );
    // In the ladder's own colour, off the `selected` rung and nowhere
    // else: the object states no wash of its own.
    let selected = t.class_state(class, State::Selected);
    assert_eq!(alpha, selected.fill.a, "the plate's wash is not list.item's selected fill");
    // Cut to the shape [list] names, at the radius [list] states.
    assert_eq!(corner.style, CornerStyle::Round, "the master's list.corner_style is round");
    assert!(
        (corner.size - px_of("list.corner")).abs() < 0.01,
        "the plate's radius is {} where [list].corner says {}",
        corner.size,
        px_of("list.corner")
    );
    // A hovered row answers the same way, one rung over — proof the
    // plate is the ladder's and not the `current` flag's.
    let on_first_row =
        (ANCHOR.x + pad + 10.0, ANCHOR.bottom() + px_of("menu.anchor_gap") + pad + 5.0);
    let hovered = shoot(&mut fonts, None, on_first_row, 1.0);
    let hp = plates(&hovered);
    assert_eq!(hp.len(), 1, "the pointer marks {} rows", hp.len());
    assert_eq!(
        hp[0].0[1],
        ANCHOR.bottom() + px_of("menu.anchor_gap") + pad,
        "the plate is not under the pointer's row"
    );
    assert_eq!(hp[0].2, t.class_state(class, State::Hover).fill.a);
    // The row in force keeps its mark under the pointer, one rung up.
    let both = shoot(&mut fonts, Some(0), on_first_row, 1.0);
    assert_eq!(
        plates(&both)[0].2,
        t.class_state(class, State::SelectedHover).fill.a,
        "a hovered current row fell off the selected_hover rung"
    );

    // ---- 3 · rows take the anchor's own inset ------------------------
    // Same left edge, same width as the anchor's bottom edge, and the
    // label centred on the same line the anchor's label is centred on.
    let skew = px_of("button.skew");
    for (i, (at, _, _, name)) in labels(&idle).iter().enumerate() {
        assert_eq!(name, NAMES[i]);
        assert!(
            (at[0] - (ANCHOR.x + (ANCHOR.w - skew) / 2.0)).abs() < 0.01,
            "row {i}'s label sits at {} and the anchor's centre line is {}",
            at[0],
            ANCHOR.x + (ANCHOR.w - skew) / 2.0
        );
    }

    // ---- 4 · the master's [list] spacing: rows touch, nothing between -
    assert_eq!(px_of("list.gap"), 0.0, "the master's [list].gap is not @space.0");
    assert_eq!(lines(&idle), 0, "the master draws a rule between rows where [list].rule = none");

    // ---- 5 · the label's size did not move ---------------------------
    // The owner said this is not about fonts. Both bindings — the one
    // this object read (`menu.item.role`) and the one it reads now
    // (`list.label_role`) — name `body`, so the px is the same number
    // before and after, and the master is what says so.
    let body_px = px_of("type.body.size");
    for (_, _, px, _) in labels(&idle) {
        assert_eq!(px, body_px, "a row's label left type.body's size");
    }

    // ================================================================
    // Fixtures from here down: each is the master plus one token.
    // ================================================================

    // ---- 6 · the face comes off the ROLE, not off this file ----------
    // The control is the point: with `FONT_UI` written into the draw
    // call, both halves of this pair answered slot 0.
    let ui = {
        skin("[type]\nbody.face = ui\n");
        let dl = shoot(&mut fonts, None, AWAY, 1.0);
        labels(&dl)[0].1
    };
    let mono = {
        skin("[type]\nbody.face = mono\n");
        let dl = shoot(&mut fonts, None, AWAY, 1.0);
        labels(&dl)[0].1
    };
    assert_ne!(
        ui, mono,
        "type.body.face moves no row: the slot is written into the object ({ui} both ways)"
    );

    // ---- 7 · the plate's shape is [list]'s to state ------------------
    skin("[list]\ncorner_style = square\n");
    let square = plates(&shoot(&mut fonts, Some(1), AWAY, 1.0));
    assert_eq!(square[0].1.style, CornerStyle::Square, "[list].corner_style does not reach the plate");
    skin("[list]\ncorner = 0u\n");
    let flat = plates(&shoot(&mut fonts, Some(1), AWAY, 1.0));
    assert_eq!(flat[0].1.size, 0.0, "[list].corner does not reach the plate's radius");

    // ---- 8 · the label's role is [list]'s to bind --------------------
    // What `menu.item.role` used to decide. The master points both at
    // `body`, so this is the only way to see the binding move at all.
    skin("[list]\nlabel_role = caption\n");
    let caption_px = labels(&shoot(&mut fonts, None, AWAY, 1.0))[0].2;
    skin("[list]\nlabel_role = body\n");
    let body_again = labels(&shoot(&mut fonts, None, AWAY, 1.0))[0].2;
    assert_ne!(caption_px, body_again, "[list].label_role does not reach a row's label");
    // ...and the binding the object no longer reads no longer moves it:
    // the menu keeps `menu.item.role` and the list does not answer it.
    skin("[menu]\nitem.role = caption\n");
    let under_menu_role = labels(&shoot(&mut fonts, None, AWAY, 1.0))[0].2;
    assert_eq!(
        under_menu_role, body_again,
        "a drop-down row still answers menu.item.role — the two objects are still \
         wearing one outfit"
    );

    // ---- 9 · [list].gap and [list].rule reach the rows ---------------
    skin("[list]\ngap = 4u\n");
    let gapped = shoot(&mut fonts, Some(1), AWAY, 1.0);
    let gap_px = px_of("list.gap");
    assert!(gap_px > 0.0, "the fixture's own gap did not bake");
    assert_eq!(
        plates(&gapped)[0].0[1],
        ANCHOR.bottom() + px_of("menu.anchor_gap") + px_of("menu.pad") + ROW_H + gap_px,
        "[list].gap does not open a seam between two rows"
    );
    skin("[list]\nrule = @stroke.hair\nrule_every = 1\n");
    let ruled = shoot(&mut fonts, None, AWAY, 1.0);
    assert_eq!(lines(&ruled), NAMES.len(), "[list].rule draws no hairline when a theme asks");

    // ---- 10 · the unfold threshold is [list]'s now -------------------
    // One and a half rows out: the first is at full height and the
    // second is half of one, which is under 0.7 of a row either way —
    // so a threshold of 0 is what tells the two fixtures apart.
    skin("[list]\nunfold_text_threshold = 0.7\n");
    let half = unfold(1.5);
    let shy = labels(&shoot(&mut fonts, None, AWAY, half)).len();
    skin("[list]\nunfold_text_threshold = 0.0\n");
    let half = unfold(1.5);
    let eager = labels(&shoot(&mut fonts, None, AWAY, half)).len();
    assert!(
        eager > shy,
        "[list].unfold_text_threshold does not decide when an unfolding row takes \
         its label ({eager} labels against {shy})"
    );

    // ---- 11 · the inset is the anchor's, not a number of this file's --
    // The master shears no button, so the rows sit on the anchor's whole
    // width and nothing above can tell "follows the anchor" from "is as
    // wide as the anchor". A theme that shears its buttons can: the
    // rows narrow by the shear, because they hang off the anchor's
    // BOTTOM edge, and their labels re-centre on that edge's middle.
    skin("[button]\nskew = 3u\n");
    let sheared = px_of("button.skew");
    assert!(sheared > 0.0, "the fixture's own shear did not bake");
    let dl = shoot(&mut fonts, Some(0), AWAY, 1.0);
    assert_eq!(
        plates(&dl)[0].0[2],
        ANCHOR.w - sheared - 2.0 * px_of("menu.pad"),
        "a row kept the anchor's full width under a shear that shortened the \
         edge it hangs from"
    );
    assert!(
        (labels(&dl)[0].0[0] - (ANCHOR.x + (ANCHOR.w - sheared) / 2.0)).abs() < 0.01,
        "the label centred on the anchor's box instead of on the edge the row \
         actually occupies"
    );

    // ---- 12 · the row's figures step by the box its role asks for ----
    // §5.16's `tabular` reached every other object of this batch and
    // stopped here, because `text_center` is `text_center_fig` with the
    // box left out. The master ships `body` proportional, which is the
    // negative control this claim needs: the advance is zero until a
    // theme says otherwise, and non-zero the moment one does.
    skin("[type]\nbody.tabular = false\n");
    let loose = advances(&shoot(&mut fonts, None, AWAY, 1.0));
    assert_eq!(loose.len(), NAMES.len(), "a row that drew no label proves nothing");
    assert!(
        loose.iter().all(|a| *a == 0.0),
        "a row was boxed under `type.body.tabular = false`: {loose:?}"
    );
    skin("[type]\nbody.tabular = true\n");
    let boxed = advances(&shoot(&mut fonts, None, AWAY, 1.0));
    assert!(
        boxed.iter().all(|a| *a > 0.0),
        "`type.body.tabular = true` and the rows were still drawn proportionally: \
         {boxed:?} — a list of versions or addresses steps its digits differently \
         from the boxed label beside it"
    );

    // ---- 13 · the plate is cut to the row it is drawn on --------------
    // The cut is settled once against a row at FULL height, which every
    // row of a finished list is. A row still unfolding is shorter, and
    // `pill` is a word about the box it is made on: half of 30 is 15,
    // and 15 on the 9 px that row passes through is not a capsule but a
    // radius the shape cannot hold.
    skin("[list]\ncorner = @corner.pill\ncorner_style = round\n");
    let half_row = unfold(0.5);
    let opening = shoot(&mut fonts, Some(0), AWAY, half_row);
    let (r, corner, _) = plates_at(&opening, half_row)[0];
    assert!(r[3] < ROW_H, "the fixture's row is at full height, so it proves nothing");
    assert!(
        (corner.size - r[3] / 2.0).abs() < 0.01,
        "the plate on a {} px row was cut at {} — `pill` is half the SHORTER side \
         of the box it is drawn on, which is {}",
        r[3],
        corner.size,
        r[3] / 2.0
    );
    // And the finished list is unchanged: at rest the cut is the full
    // row's, which is what settling it once was for.
    let done = shoot(&mut fonts, Some(0), AWAY, 1.0);
    assert!(
        (plates(&done)[0].1.size - ROW_H / 2.0).abs() < 0.01,
        "a row at full height lost the cut its own height states"
    );

    master();
}
