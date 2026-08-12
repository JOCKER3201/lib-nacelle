//! A token id asked before anything has touched the theme.
//!
//! `theme::id` reads the schema, and there is no schema until a theme has
//! been loaded — `theme::resolved` is what loads it. Almost every caller
//! memoises the answer in a `'static OnceLock`: `ui::tok`, `theme::plate`,
//! `term_ansi`, `data_series`, and a dozen private copies of `tok` across
//! the objects. So an id asked one moment too early is not a miss that the
//! next frame repairs — it is pinned for the life of the process, and the
//! consumer quietly keeps drawing with whatever it falls back to. This is
//! the shape of the `border.width` defect the theme suite already records:
//! the borders kept their hard-coded thickness for a whole release and the
//! theme looked like it only changed colour.
//!
//! This file exists to ask FIRST. It is a test binary of its own precisely
//! so that nothing else has a chance to load the theme before the question
//! is put — inside a shared binary the first test to draw anything would
//! initialise the engine and hide the whole hazard.

use nacelle::theme;

#[test]
fn an_id_asked_before_any_theme_is_loaded_still_finds_the_token() {
    // Deliberately the very first line of the process to mention the theme.
    // No `resolved()`, no `load()`, no draw: exactly the position a widget's
    // `static CELL: OnceLock<TokenId>` is in the first time it is read.
    let id = theme::id("layout.panel_gutter");
    assert!(
        id.is_some(),
        "a token the master declares must resolve on the first ask, before \
         anything else has loaded the theme — otherwise every OnceLock in the \
         tree pins MISSING for the life of the process"
    );

    // And it is the same token the engine hands out once it is warm, rather
    // than some placeholder minted to satisfy the first caller.
    let warm = theme::id("layout.panel_gutter");
    assert_eq!(id, warm);

    // The value behind it is real, which is what says the schema was built
    // rather than merely allocated.
    let px = theme::resolved().px(id.expect("checked above"));
    assert!(px > 0.0, "the gutter must bake to a real length, got {px}");

    // A name the master does not declare still answers None: forcing the
    // load must not invent tokens.
    assert!(theme::id("no.such.token.in.the.master").is_none());
}
