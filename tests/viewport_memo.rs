//! The theme engine is ONE global behind a `OnceLock`, and its viewport is
//! part of that global. A test that moves the viewport therefore moves it
//! for every test running beside it — and the unit suite has 498 of them,
//! all of which read lengths the viewport decides.
//!
//! So this file holds exactly one test. Cargo gives every integration test
//! file its own binary, and a file with one test has nothing to interleave
//! with: the engine it touches is its own process's.
//!
//! (Written after the obvious version of this test — a unit test beside the
//! others — was added and promptly broke `text_input`, which had measured a
//! line at a height this test had just changed underneath it.)

use nacelle::theme;

/// Two monitors of unequal height alternate the viewport on every frame, and
/// `set_viewport` only ever dropped a REPEAT of the last one — two heights
/// taking turns are never a repeat. Every alternation therefore fell through
/// to a full resolve of the theme, because the `u` that keys the bake cache
/// is something a resolve has to PRODUCE before the cache can be asked.
///
/// That was `--desktop` pinned at 100 % CPU on a mixed-height desktop while
/// idling at 5 % on a single screen: ~120 resolves of a 2697-token theme per
/// second, and not one observable saying so.
#[test]
fn alternating_two_screen_heights_resolves_twice_not_twice_a_frame() {
    // The desktop this was found on: 2560x1440 beside 3840x2160.
    let (short, tall) = (1440.0, 2160.0);

    // Warm both, so what follows measures the steady state a running
    // program is in and not the first sight of either height.
    theme::set_viewport(short, 1.0);
    theme::set_viewport(tall, 1.0);

    let before = theme::resolves();
    for _ in 0..50 {
        theme::set_viewport(short, 1.0);
        theme::set_viewport(tall, 1.0);
    }
    let spent = theme::resolves() - before;

    assert_eq!(
        spent, 0,
        "100 alternations between two warmed screen heights resolved the \
         theme {spent} times — the per-viewport memo is not holding, and \
         a mixed-height desktop is back to resolving every frame"
    );

    // And the memo must not have flattened the two heights into one answer:
    // each screen still gets the bake baked for ITS viewport.
    theme::set_viewport(short, 1.0);
    let a = theme::resolved() as *const _;
    theme::set_viewport(tall, 1.0);
    let b = theme::resolved() as *const _;
    assert_ne!(
        a, b,
        "both heights answered with the same bake — the memo is keyed too \
         loosely and one screen is drawing at the other's unit size"
    );

    // A height never seen before still costs its one resolve: the memo is a
    // cache, not a lid.
    let before = theme::resolves();
    theme::set_viewport(1234.0, 1.0);
    assert_eq!(
        theme::resolves() - before,
        1,
        "an unseen viewport did not resolve — the memo is answering for a \
         height it never baked"
    );
}
