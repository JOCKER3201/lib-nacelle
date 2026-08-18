//! The zone tokens of the settings window (spec §2), proved to bake.
//!
//! Krok 1 of the settings-window plan adds a band grammar (`Zone`) whose
//! every distance and threshold lives in the master, not in Rust. The
//! walker is not written yet, so nothing draws through these tokens —
//! this file is what stands between "declared" and "a comment with an
//! equals sign in it": each name must resolve to a `TokenId` and bake to
//! the number the spec computed, under the same metrics the spec used
//! (1080p, u = 5.4 px).
//!
//! It is a binary of its own for the reason `token_id_before_load` is:
//! it loads the theme, and the resolved theme is process-wide — a test
//! that loads must not run beside a test that merely reads.

use nacelle::theme;

/// |a - b| within half a device pixel — bake rounds strokes, not spaces,
/// so the tolerance is for float arithmetic, not for policy.
fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.5
}

fn px(name: &str) -> f32 {
    let id = theme::id(name)
        .unwrap_or_else(|| panic!("the master must declare `{name}` — spec §2 puts it there"));
    theme::resolved().px(id)
}

#[test]
fn the_zone_tokens_of_the_settings_window_bake_to_the_specs_numbers() {
    let _ = theme::load();
    // The spec's arithmetic assumes 1080p at scale 1: u = 5.4 px.
    theme::set_viewport(1080.0, 1.0);

    // Gutters: both gaps ride @space.6 / @space.4, so they bake to real
    // lengths, and zone_gap = section_gap is the "one rhythm" the spec
    // wrote into its own comment.
    assert!(near(px("settings.col_gap"), 21.6), "col_gap: {}", px("settings.col_gap"));
    assert!(near(px("settings.zone_gap"), 21.6), "zone_gap: {}", px("settings.zone_gap"));
    assert!(
        near(px("settings.zone_gap"), px("settings.section_gap")),
        "zone_gap must equal section_gap — one rhythm, spec §2"
    );
    assert!(near(px("settings.bar_gap"), 10.8), "bar_gap: {}", px("settings.bar_gap"));

    // The column threshold and its device-px floor (companion, 3.2).
    // 72u = 388.8 px at 1080p; the floor is written in device px and
    // bakes exactly as written.
    assert!(near(px("settings.col_min_w"), 388.8), "col_min_w: {}", px("settings.col_min_w"));
    assert!(
        near(px("settings.col_min_w_min_px"), 360.0),
        "col_min_w_min_px: {}",
        px("settings.col_min_w_min_px")
    );

    // The editor's section rail (krok 3): a fraction 0..1 after bake,
    // patterned on back_w_frac and equal to it by declaration.
    let rail = px("settings.rail_w_frac");
    assert!(rail > 0.0 && rail < 1.0, "rail_w_frac must bake to a 0..1 fraction, got {rail}");
    assert!(
        near(rail, px("settings.back_w_frac")),
        "rail_w_frac is declared `wzorem back_w_frac` — both 22%"
    );
    assert!(near(px("settings.rail_w_min"), 70.2), "rail_w_min: {}", px("settings.rail_w_min"));
    assert!(
        near(px("settings.rail_w_min_min_px"), 70.0),
        "rail_w_min_min_px: {}",
        px("settings.rail_w_min_min_px")
    );

    // …and it leaves the page the majority on its own. The second
    // navigation column it used to share the window with is gone
    // (2026-08-18): a section's pages unfold UNDER it now, so
    // `settings.subrail_w_frac` describes nothing and is not declared.
    assert!(rail < 0.5, "the rail must leave the page the majority, got {rail}");
    assert!(
        theme::id("settings.subrail_w_frac").is_none(),
        "`settings.subrail_w_frac` is still declared, and the column it sized no \
         longer exists — a knob that turns nothing reads as a knob"
    );

    // THE RAIL'S OWN RHYTHM. A navigation column is a dense list of
    // names, not a run of controls, so its break is its own and is
    // TIGHTER than the form's — and it has to be, because the rail
    // carries the open section's pages as well and has no scroll to put
    // the overflow in.
    let rail_gap = px("settings.rail_row_gap");
    assert!(near(rail_gap, 5.4), "rail_row_gap: {rail_gap}");
    assert!(rail_gap > 0.0, "entries with no break at all are one entry");
    assert!(
        rail_gap < px("modal.row_gap"),
        "the rail's break ({rail_gap}) is not under the form's ({}) — the column \
         that has to hold every section AND the open one's pages is spending a \
         page's rhythm",
        px("modal.row_gap")
    );

    // THE UNFOLDED SECTION. Its pages stand `rail_indent` in from it,
    // propped against a hairline standing `rail_guide_x` of the way
    // across that step. All three are the theme's, because all three are
    // the whole of what the second column used to say by standing
    // somewhere else.
    let indent = px("settings.rail_indent");
    assert!(near(indent, 16.2), "rail_indent: {indent}");
    // A step the eye can read, and one the rail can afford: a section's
    // own pages are buttons with words on them, so an indent that ate
    // the column would leave them unreadable rather than nested.
    assert!(indent > 0.0, "an indent of nothing is not an indent");
    assert!(
        indent < px("settings.rail_w_min") / 2.0,
        "the indent ({indent}) takes more than half the narrowest rail ({}) — its \
         pages would have less room than the step that marks them",
        px("settings.rail_w_min")
    );
    let guide = px("settings.rail_guide_w");
    assert!(guide > 0.0, "a guide of no width is a guide nobody sees: {guide}");
    assert!(
        guide < indent,
        "the guide ({guide}) is as wide as the step it stands in ({indent}) — that \
         is a second column's edge, which is the shape the one rail replaced"
    );
    let at = px("settings.rail_guide_x");
    assert!(
        (0.0..=1.0).contains(&at),
        "rail_guide_x must bake to a 0..1 fraction of the indent, got {at}"
    );

    // The air a navigation column's bed keeps around what stands on it
    // (owner, 2026-08-18: "żadnych paddingów nie ma, totalna amatorka").
    // Both axes ride @space.4, so the plate reads square rather than
    // tilted, and both are real lengths and not the zero the window had
    // before there was a token at all.
    let pad_x = px("settings.band_pad_x");
    let pad_y = px("settings.band_pad_y");
    assert!(near(pad_x, 10.8), "band_pad_x: {pad_x}");
    assert!(near(pad_y, 10.8), "band_pad_y: {pad_y}");
    assert!(pad_x > 0.0 && pad_y > 0.0, "a bed with no air is the fault, not the fix");
    // And the padding stays SMALLER than the gutter beside it: a button
    // must read as further from the next COLUMN than from its own bed's
    // edge, or the two columns fuse into one.
    assert!(
        pad_x < px("settings.col_gap"),
        "the bed's own air ({pad_x}) is not under the gutter between columns ({})",
        px("settings.col_gap")
    );
}
