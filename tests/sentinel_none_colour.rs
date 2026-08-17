//! `none` on a colour key, read through BOTH doors: the master's own
//! declaration and a value laid over it by [`theme::set_preview`].
//!
//! The bug this file pins down was recorded as an OVERLAY fault — "the word
//! is right in the master and bakes to opaque black through the overlay" —
//! and the theme editor carries a workaround written against that reading
//! (`.gap-program/obalone-naprawy.md`). Measured, the two doors behaved
//! identically and both were wrong: `elev.panel.glass.wash = none`, straight
//! out of the shipped master, answered `color()` with rgba(0, 0, 0, 1).
//! Nothing had noticed because the master also ships `glass.rank = 0` on
//! every `[elev.*]` rung, so no frame ever asked the wash for its colour
//! until the editor's BLUR/FROSTED raised the rank.
//!
//! Its own file because it previews, and a preview is ONE global that every
//! other test in the same process would see — the rule `theme_preview.rs`
//! and `viewport_memo.rs` are already written to.

use nacelle::theme;

/// The word means the same thing whichever door it comes through, and what
/// it means is "there is no colour here".
#[test]
fn none_is_not_a_colour_from_the_master_nor_from_an_overlay() {
    theme::load();

    // Door one: the master's own. `[elev.panel] glass.wash = none`, whose
    // comment in `default.theme` reads "none = the second quad is not
    // drawn" — and whose readers (`object/elev.rs`, `object/window.rs`)
    // spell that as `if wash.a > 0.0`.
    let wash = theme::id("elev.panel.glass.wash").expect("elev.panel.glass.wash");
    let from_master = theme::resolved().color(wash);
    assert_eq!(
        from_master.a, 0.0,
        "the master's own `none` answers colour() with alpha {} — an opaque \
         quad over the glass, where the theme said there was nothing to draw",
        from_master.a
    );

    // §5.0 is untouched by the repair: the word still folds to its `f32` on
    // the scalar side, which is where a consumer tells the sentinels apart.
    assert_eq!(
        theme::resolved().px(wash),
        0.0,
        "the sentinel stopped folding to its own scalar"
    );

    // Door two: the same word, arriving from a preview, on a key the master
    // fills with a real colour — so the assertion cannot pass by inheriting
    // the answer from door one.
    let tint = theme::id("elev.panel.glass.tint").expect("elev.panel.glass.tint");
    let opaque = theme::resolved().color(tint);
    assert!(
        opaque.a > 0.0,
        "the master no longer gives glass.tint a colour; this test needs a key that has one"
    );

    let refused = theme::set_preview(&[("elev.panel.glass.tint", "none")]);
    assert!(refused.is_empty(), "the engine refused the word `none`: {refused:?}");
    let from_overlay = theme::resolved().color(tint);
    let overlay_px = theme::resolved().px(tint);
    theme::clear_preview();

    assert_eq!(
        (from_overlay.r, from_overlay.g, from_overlay.b, from_overlay.a),
        (from_master.r, from_master.g, from_master.b, from_master.a),
        "`none` through an overlay is not the same colour as `none` in the master"
    );
    assert_eq!(overlay_px, 0.0, "the previewed sentinel did not fold to its scalar");

    // And the thing the editor had to write INSTEAD of the word — a colour
    // with nothing in it — is now exactly what the word says. This is the
    // assertion that licenses `theme::edit::glass_edits` to drop its
    // workaround and write what it means.
    let refused = theme::set_preview(&[(
        "elev.panel.glass.tint",
        "oklch(0.0000, 0.0000, 0.00 / 0.000)",
    )]);
    assert!(refused.is_empty(), "{refused:?}");
    let spelled_out = theme::resolved().color(tint);
    theme::clear_preview();
    assert_eq!(
        (spelled_out.r, spelled_out.g, spelled_out.b, spelled_out.a),
        (from_overlay.r, from_overlay.g, from_overlay.b, from_overlay.a),
        "the word `none` and a fully transparent literal disagree, so the \
         editor's workaround is still load-bearing"
    );

    // A colour that IS a colour still arrives, so the repair has not simply
    // emptied the preview path.
    let refused = theme::set_preview(&[(
        "elev.panel.glass.tint",
        "oklch(0.8000, 0.2000, 30.00)",
    )]);
    assert!(refused.is_empty(), "{refused:?}");
    assert!(
        theme::resolved().color(tint).a > 0.0,
        "an ordinary previewed colour lost its alpha"
    );
    theme::clear_preview();

    // CANCEL puts the master back.
    assert_eq!(theme::resolved().color(tint).a, opaque.a);
}
