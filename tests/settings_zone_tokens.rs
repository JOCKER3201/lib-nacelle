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

    // The subpage column of the three-panel layout (ANEKS): a fraction,
    // wider than the rail (it carries subpage names, not group headers),
    // and the two together leave the content panel its majority.
    let subrail = px("settings.subrail_w_frac");
    assert!(
        subrail > 0.0 && subrail < 1.0,
        "subrail_w_frac must bake to a 0..1 fraction, got {subrail}"
    );
    assert!(subrail >= rail, "the subrail must not be narrower than the rail");
    assert!(
        rail + subrail < 0.5,
        "rail + subrail must leave the content panel the majority, got {}",
        rail + subrail
    );
}
