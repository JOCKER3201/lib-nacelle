//! A cache keyed on the theme's CONTENT must survive a screen swap.
//!
//! Two consumers added by the 2026-08-17 token audit hold a one-slot memo
//! keyed on an epoch: `num.rs`'s text tokens (`decimal_sep`, `group_sep`,
//! `unit.text_gap` — found by a linear scan of the cold-path diagnostics,
//! and read once per number on a draw path) and `panel.rs`'s `rung()` (a
//! dozen name lookups and four enum interns per panel per frame). Both
//! first shipped keyed on [`theme::epoch`], and both were wrong for the
//! reason `theme::content_epoch`'s own doc gives: `epoch` names WHICH
//! BAKE is published, a desktop whose monitors are unequal heights has
//! two live bakes published in turn, and its value therefore alternates
//! every frame forever. A one-slot cache keyed on it misses every time.
//!
//! That is not a small remark. It is the exact shape of the fault that
//! put `--desktop` at 100 % CPU: `FontSystem::begin_frame` guarded its
//! face reload with `epoch` and re-walked the font directories sixty
//! times a second. `content_epoch` was split off for it, and `font.rs`
//! reads that one now.
//!
//! So the property both keys stand on is stated here, once: a viewport
//! swap moves the published bake and does NOT move the content counter.
//! Nothing else in the suite says so — `theme_preview.rs` makes the
//! neighbouring claim about a PREVIEW, not about a screen.
//!
//! Its own test binary, because `set_viewport` is process-wide.

use nacelle::theme;

/// Two heights that cannot round to one `u`: the engine re-uses a bake
/// when a height produces the same unit size, and a claim about swapping
/// bakes needs two bakes to exist.
const SHORT: f32 = 1080.0;
const TALL: f32 = 2160.0;

#[test]
fn a_screen_swap_is_not_a_content_change() {
    let _ = theme::load();

    theme::set_viewport(SHORT, 1.0);
    let content_at_start = theme::content_epoch();
    let epoch_short = theme::epoch();
    let key_short = theme::viewport_key();

    theme::set_viewport(TALL, 1.0);
    let epoch_tall = theme::epoch();
    let key_tall = theme::viewport_key();

    assert_ne!(
        key_short, key_tall,
        "the two heights bake to one viewport, so this file cannot tell a counter that \
         follows the screen from one that does not"
    );
    assert_ne!(
        epoch_short, epoch_tall,
        "a second bake was published and `epoch` — which names WHICH bake — stood still"
    );
    assert_eq!(
        theme::content_epoch(),
        content_at_start,
        "`content_epoch` moved for a screen swap. Every one-slot cache keyed on it — the \
         font system's face reload above all — now misses on every frame of a desktop \
         with two monitor heights, which is the 100 % CPU fault it was split off to fix"
    );

    // And back again, because alternating is the case that matters: the
    // desktop does not swap once, it swaps per screen per frame.
    theme::set_viewport(SHORT, 1.0);
    assert_eq!(theme::viewport_key(), key_short, "the same height must bake the same viewport");
    assert_eq!(
        theme::content_epoch(),
        content_at_start,
        "`content_epoch` moved on the way back — a cache keyed on it alternates with the \
         screens, which is exactly what it exists to stop"
    );

    // A LOAD is a content change, so the counter has to move for that or
    // it would be a constant and every cache keyed on it would go stale
    // instead of merely thrashing.
    let _ = theme::load();
    assert_ne!(
        theme::content_epoch(),
        content_at_start,
        "a load renames the face slots and every text token; a counter that does not move \
         for it is not a change detector at all"
    );
}
