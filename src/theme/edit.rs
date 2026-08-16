//! THE MODEL BEHIND THE THEME EDITOR: what a control means in tokens.
//!
//! The editor shows four controls — a border kind, a border colour, a
//! background kind, and a background colour. None of those is a token. Each
//! is a NAME for a set of tokens that have to move together, and this module
//! is the only place that knows the sets. The view picks names and drags
//! sliders; the file is written from what comes out of here.
//!
//! WHY A MODULE AND NOT A FEW LINES IN THE SETTINGS WINDOW: the same question
//! — "what does `neon` mean" — is asked when the editor OPENS (to show the
//! current state), while a slider MOVES (to preview), and when SAVE writes.
//! Three callers, one answer, or they drift.
//!
//! # What this module deliberately does NOT offer
//!
//! Only what the renderer actually draws. The theme language declares far more
//! than the code reads — the `[elev.*]` ladder has nine rungs of about thirty
//! keys and seven of them reach the screen — and a control wired to a token
//! nobody reads is worse than a missing control, because it looks like it
//! works. Measured 2026-08-16, with the anchors kept next to each set below.
//!
//! The gap narrows as the renderer learns: on 2026-08-16 the panel rung's
//! glass became real (`elev::Level` and `window::frame` both read
//! `elev.panel.glass.*`, and `glass.rank` gained its first reader), so the
//! background sets below write it. The fixture rung keeps its own hand-made
//! path in `deco.rs` and is deliberately NOT addressed here — the owner's
//! scope for a background is windows and widgets, never the desktop's own
//! decoration.

use super::color::Oklch;

/// WHERE an edit lands.
///
/// One value today, and the type exists anyway. The owner's plan is per-widget
/// and per-window settings later, and the difference between "write these
/// tokens" and "write these tokens IN SCOPE S" is a rewrite of the save path
/// if it arrives late and one more match arm if it arrives early. The editor
/// shows no scope picker; the model already has one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Scope {
    /// Every surface at once — the whole theme.
    Theme,
}

/// The two borders the editor offers.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Border {
    /// A ring and nothing else.
    Line,
    /// The same ring with a halo around it.
    ///
    /// The halo has NO COLOUR OF ITS OWN. `object/window.rs:107` passes the
    /// ring's colour into `glow_ring`, and `glow.panel_edge.color` is declared
    /// in the master and read by nobody. So one colour drives both, which is
    /// why the editor has one set of colour sliders for the border rather
    /// than two.
    Neon,
}

/// The three backgrounds the editor offers.
///
/// These are PRESETS over the glass pair, not tokens: nothing in the theme
/// language selects "blur" as a word. Glass is two quads — `glass.tint`
/// multiplies (so it can only darken and hue-shift) and `glass.wash` lays
/// over with alpha (the only one that can brighten) — and the master says a
/// single "glass colour" token would be a bug. A preset is the honest way to
/// give the three names the owner asked for: each one is a shape of that pair.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Glass {
    /// No glass: the surface's own fill, opaque.
    Solid,
    /// The scene behind, blurred, with the tint left neutral.
    Blur,
    /// Blurred, tinted and washed, so nothing reads through.
    Frosted,
}

/// One token, and the text to write for it.
///
/// The value is TEXT, not a parsed value, because it is going into a file that
/// is patched byte by byte — a save replaces the bytes of a value span and
/// leaves every comment and every other byte where it was. Handing a `Color`
/// to the writer would mean the writer decides how a colour is spelled, and
/// then two places know.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Edit {
    pub token: &'static str,
    pub value: String,
}

impl Edit {
    fn new(token: &'static str, value: impl Into<String>) -> Self {
        Self { token, value: value.into() }
    }
}

/// A colour as the theme language spells it: `oklch(L, C, H)`.
///
/// Written rather than assembled from a `Color`, so the three sliders land in
/// the file as the three numbers the owner moved. The alternative — convert to
/// sRGB and write a hex triple — throws the chroma and hue away and makes the
/// next edit start from a rounded value.
///
/// The `/ a` tail is the FUNCTION's alpha and never a component suffix
/// (`parse.rs:1087`), so it is only written when the colour is not opaque.
pub fn oklch_literal(c: Oklch) -> String {
    if c.alpha >= 1.0 {
        format!("oklch({:.4}, {:.4}, {:.2})", c.l, c.c, c.h)
    } else {
        format!("oklch({:.4}, {:.4}, {:.2} / {:.3})", c.l, c.c, c.h, c.alpha)
    }
}

// ---------------------------------------------------------------- the sets

/// Tokens that carry the border, for one scope.
///
/// `elev.panel.edge.color` and `.width` are what `elev::Level` reads
/// (`object/elev.rs:68-69`); the glow keys are what `panel_edge_glow` reads
/// (`object/window.rs:97-104`). The four other `edge.*` keys the master
/// declares — `color2`, `mode`, `gradient`, `axis` — have no reader in Rust
/// and are NOT written here. Writing them would put a value in the file that
/// changes nothing, which is exactly what makes `cockpit.theme:154-156` ask
/// for a gradient border today and get a flat one.
/// The one edit that changes the border's COLOUR and nothing else.
///
/// Split out for the state the editor opens in: no border kind chosen yet.
/// Sending a kind then would not be neutral — LINE's switch turns the halo
/// off — so a colour slider moved before the list is touched must move the
/// colour alone. (Verified finding, 2026-08-16: the earlier shape mapped
/// "no choice" to LINE and a colour drag switched five themes' halos off
/// as a side effect.)
pub fn border_colour_edit(scope: Scope, colour: Oklch) -> Edit {
    let Scope::Theme = scope;
    Edit::new("elev.panel.edge.color", oklch_literal(colour))
}

/// `halo_dressed` answers "does the theme already draw a visible halo" —
/// resolved radius AND alpha both above zero. The caller reads it off the
/// live theme; this function stays pure so the tests need no engine.
pub fn border_edits(scope: Scope, kind: Border, colour: Oklch, halo_dressed: bool) -> Vec<Edit> {
    let mut out = vec![border_colour_edit(scope, colour)];
    match kind {
        // The theme's own radius and alpha are left standing: LINE only
        // takes the halo away, and `enabled = false` is the whole of that
        // (`window.rs:97` returns before either is read).
        Border::Line => out.push(Edit::new("glow.panel_edge.enabled", "false")),
        // NEON dresses the halo ONLY where the theme has not: the default
        // master ships `radius = 0u` and `alpha = 0.0` and `window.rs:104`
        // returns at zero, so a bare switch was invisible there. A theme
        // that has dressed its own halo — Cockpit 1.6u/0.34, aurora
        // 0.70u/0.35, azure 0.6u/0.16, spring 1.1u/0.30, instrument
        // 0.7u/0.22 — keeps its dress: writing the seeds over those five
        // was the earlier shape's mistake, found in verification, and the
        // comment that excused it had checked exactly one theme.
        Border::Neon => {
            out.push(Edit::new("glow.panel_edge.enabled", "true"));
            if !halo_dressed {
                out.push(Edit::new("glow.panel_edge.radius", "1.6u"));
                out.push(Edit::new("glow.panel_edge.alpha", "0.34"));
            }
        }
    }
    out
}

/// Tokens that carry the background, for one scope: WINDOWS AND WIDGETS,
/// never the desktop's decoration.
///
/// The seam is exact and it was found by reading, not chosen for comfort:
/// `component.panel.fill` is read directly by `window::frame` and inherited
/// by the panel rung through the master's derivation (`[elev.panel] fill =
/// @component.panel.fill`), so ONE token colours both. Writing
/// `elev.panel.fill` instead would sever that derivation for good — the
/// windows would stop following. The glass trio lives on the panel rung
/// (`elev.panel.glass.*`, underived, safe to write) and is read by BOTH
/// drawers since 2026-08-16. The fixture's own glass (`elev.fixture.*`,
/// `deco.rs`) is not touched from here, which is what keeps the board's
/// decoration out of the editor's reach by construction.
/// `opacity`, `depth` and `coverage` are the kind's own knobs, 0..1 and
/// 1..=3: opacity scales the whole effect (a translucent SOLID lets the
/// scene through sharp; a translucent tint blends the blur with the sharp
/// base beneath it), depth picks the pyramid rank, and coverage is the
/// wash's alpha — a slider now, where an opening literal stood before
/// verification called it out.
pub fn glass_edits(
    scope: Scope,
    kind: Glass,
    tint: Oklch,
    wash: Oklch,
    opacity: f32,
    depth: f32,
    coverage: f32,
) -> Vec<Edit> {
    let Scope::Theme = scope;
    let op = opacity.clamp(0.0, 1.0);
    // Fractional on purpose: the emitter mixes two pyramid rungs by the
    // fraction, so 1.7 is a real depth and not a rounding of 2.
    let rank = format!("{:.2}", depth.clamp(1.0, 3.0));
    match kind {
        Glass::Solid => vec![
            Edit::new("component.panel.fill", oklch_literal(Oklch { alpha: op, ..wash })),
            Edit::new("elev.panel.glass.rank", "0"),
        ],
        Glass::Blur => vec![
            Edit::new("elev.panel.glass.rank", rank),
            Edit::new("elev.panel.glass.tint", oklch_literal(Oklch { alpha: op, ..tint })),
            // A fully transparent colour, NOT the word `none`. The master
            // may declare the key with `none`, but the same word arriving
            // through an overlay bakes to OPAQUE BLACK and paints itself
            // over the glass — measured 2026-08-16, three screenshots: the
            // set with `none` renders black panels, the same set without
            // the line renders glass. The resolver's overlay handling of
            // the sentinel is a real bug, recorded in the plan; until it
            // is fixed, the editor writes what it means: no wash, as a
            // colour with nothing in it.
            Edit::new(
                "elev.panel.glass.wash",
                oklch_literal(Oklch { l: 0.0, c: 0.0, h: 0.0, alpha: 0.0 }),
            ),
        ],
        Glass::Frosted => vec![
            Edit::new("elev.panel.glass.rank", rank),
            Edit::new("elev.panel.glass.tint", oklch_literal(Oklch { alpha: op, ..tint })),
            Edit::new(
                "elev.panel.glass.wash",
                oklch_literal(Oklch { alpha: coverage.clamp(0.0, 1.0), ..wash }),
            ),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(l: f32, ch: f32, h: f32, a: f32) -> Oklch {
        Oklch { l, c: ch, h, alpha: a }
    }

    #[test]
    fn an_opaque_colour_is_written_without_an_alpha_tail() {
        // The tail is the function's own alpha and reads as a fourth argument;
        // writing `/ 1.000` on every colour would put noise in a file whose
        // diffs a person is meant to read.
        let s = oklch_literal(c(0.232, 0.121, 210.5, 1.0));
        assert_eq!(s, "oklch(0.2320, 0.1210, 210.50)");
        assert!(!s.contains('/'), "an opaque colour grew an alpha tail");
    }

    #[test]
    fn a_translucent_colour_keeps_its_alpha() {
        let s = oklch_literal(c(0.232, 0.121, 210.5, 0.82));
        assert_eq!(s, "oklch(0.2320, 0.1210, 210.50 / 0.820)");
    }

    #[test]
    fn the_border_colour_is_written_once_and_the_halo_wears_it() {
        // `glow.panel_edge.color` exists in the master and has no reader, so
        // a second colour here would be a value that changes nothing. If a
        // reader is ever added, THIS test is where the second write belongs.
        let neon = border_edits(Scope::Theme, Border::Neon, c(0.7, 0.15, 200.0, 1.0), false);
        let colours: Vec<_> = neon.iter().filter(|e| e.token.ends_with("color")).collect();
        assert_eq!(
            colours.len(),
            1,
            "the border wrote {} colours; the halo has none of its own",
            colours.len()
        );
        assert_eq!(colours[0].token, "elev.panel.edge.color");
    }

    #[test]
    fn neon_dresses_the_halo_and_line_does_not_touch_it() {
        let colour = c(0.7, 0.15, 200.0, 1.0);
        let line = border_edits(Scope::Theme, Border::Line, colour, false);
        let neon = border_edits(Scope::Theme, Border::Neon, colour, false);
        let neon_dressed = border_edits(Scope::Theme, Border::Neon, colour, true);
        // A theme that has dressed its own halo keeps it: aurora's 0.70u
        // must not become Cockpit's 1.6u because someone chose NEON.
        for k in ["glow.panel_edge.radius", "glow.panel_edge.alpha"] {
            assert!(
                !neon_dressed.iter().any(|e| e.token == k),
                "NEON overwrote {k} on a theme that had already dressed its halo"
            );
        }
        // NEON must write a radius and an alpha, because the default master
        // ships both at zero and the renderer draws nothing at zero — a
        // switch alone was measured invisible on default and inert on
        // Cockpit, which ships the halo already on.
        for k in ["glow.panel_edge.radius", "glow.panel_edge.alpha"] {
            assert!(
                neon.iter().any(|e| e.token == k),
                "NEON did not write {k}; on the default theme it is invisible"
            );
            // And LINE must NOT: the theme's own halo dress survives a trip
            // through LINE, so switching back to NEON finds it as it was.
            assert!(
                !line.iter().any(|e| e.token == k),
                "LINE wrote {k}, flattening the theme's own halo"
            );
        }
        let of = |v: &Vec<Edit>| v.iter().find(|e| e.token.ends_with("enabled")).unwrap().value.clone();
        assert_eq!(of(&line), "false");
        assert_eq!(of(&neon), "true");
    }

    #[test]
    fn no_set_writes_a_token_nothing_reads() {
        // The whole point of the module. These four are declared by the master
        // and read by no Rust in the workspace (measured 2026-08-16); writing
        // them would produce a file that asks for a gradient border and gets a
        // flat one, which is what `cockpit.theme` does today.
        const DEAD: [&str; 5] = [
            "elev.panel.edge.color2",
            "elev.panel.edge.mode",
            "elev.panel.edge.gradient",
            "elev.panel.edge.axis",
            "glow.panel_edge.color",
        ];
        // `glass.rank` left this list on 2026-08-16, the day it gained its
        // first reader (`elev::Level::draw`, `window::frame`).
        let colour = c(0.7, 0.15, 200.0, 1.0);
        let mut all = Vec::new();
        for kind in [Border::Line, Border::Neon] {
            all.extend(border_edits(Scope::Theme, kind, colour, false));
            all.extend(border_edits(Scope::Theme, kind, colour, true));
        }
        all.push(border_colour_edit(Scope::Theme, colour));
        for kind in [Glass::Solid, Glass::Blur, Glass::Frosted] {
            all.extend(glass_edits(Scope::Theme, kind, colour, colour, 1.0, 2.0, 0.42));
        }
        for e in &all {
            assert!(
                !DEAD.contains(&e.token),
                "the model wrote `{}`, which no renderer reads",
                e.token
            );
        }
    }

    /// The scope is windows and widgets, and the tokens are the proof: the
    /// shared fill goes through `component.panel.fill` (windows read it
    /// directly, panels inherit it), NEVER through `elev.panel.fill` —
    /// writing the rung's own fill would sever the derivation and the
    /// windows would stop following the colour. And nothing here may touch
    /// the fixture: the desktop's decoration is out of the editor's reach
    /// by construction, which this test keeps true.
    #[test]
    fn the_background_lands_on_the_shared_seam_and_never_on_the_fixture() {
        let colour = c(0.3, 0.05, 220.0, 1.0);
        for kind in [Glass::Solid, Glass::Blur, Glass::Frosted] {
            for e in glass_edits(Scope::Theme, kind, colour, colour, 1.0, 2.0, 0.42) {
                assert!(
                    !e.token.starts_with("elev.fixture"),
                    "{:?} wrote {} — the desktop's decoration is not the editor's",
                    kind,
                    e.token
                );
                assert_ne!(
                    e.token, "elev.panel.fill",
                    "{kind:?} wrote the rung's own fill, severing the derivation \
                     that keeps windows following the colour"
                );
            }
        }
        let solid = glass_edits(Scope::Theme, Glass::Solid, colour, colour, 1.0, 2.0, 0.42);
        assert!(
            solid.iter().any(|e| e.token == "component.panel.fill"),
            "SOLID does not colour the seam both windows and panels read"
        );
        assert!(
            solid.iter().any(|e| e.token == "elev.panel.glass.rank" && e.value == "0"),
            "SOLID left a previous glass standing"
        );
    }

    /// The OPACITY knob owns the tint's alpha for BOTH glassy kinds, and
    /// the COVERAGE knob owns the wash's — the sliders' own alphas are
    /// discarded on purpose, so the file always carries what the knobs say
    /// and never a stale channel smuggled in through a seeded colour.
    #[test]
    fn the_knobs_own_the_alphas_and_the_colours_do_not() {
        let tint = c(0.6, 0.05, 220.0, 0.5);
        let wash = c(0.2, 0.02, 220.0, 0.34);
        let tint_of = |v: &Vec<Edit>| {
            v.iter().find(|e| e.token.ends_with("glass.tint")).unwrap().value.clone()
        };
        // Full opacity: no alpha tail, whatever the seeded colour carried.
        let blur = glass_edits(Scope::Theme, Glass::Blur, tint, wash, 1.0, 2.0, 0.42);
        assert!(!tint_of(&blur).contains('/'), "full opacity grew an alpha tail");
        // Dialled down: the tail is the KNOB's number, for blur and frosted
        // alike — a translucent tint blends the blur with the sharp scene.
        for kind in [Glass::Blur, Glass::Frosted] {
            let v = glass_edits(Scope::Theme, kind, tint, wash, 0.6, 2.0, 0.42);
            assert!(
                tint_of(&v).contains("/ 0.600"),
                "{kind:?}: the tint's alpha is not the opacity knob's 0.6"
            );
        }
        // And the wash follows coverage, not the seeded colour's 0.34.
        let frosted = glass_edits(Scope::Theme, Glass::Frosted, tint, wash, 1.0, 2.0, 0.7);
        let wash_of = frosted
            .iter()
            .find(|e| e.token.ends_with("glass.wash"))
            .unwrap();
        assert!(
            wash_of.value.contains("/ 0.700"),
            "the wash's alpha is not the coverage knob's 0.7"
        );
        // Depth lands in the rank, clamped to the pyramid the renderer has.
        let deep = glass_edits(Scope::Theme, Glass::Blur, tint, wash, 1.0, 9.0, 0.42);
        assert!(
            deep.iter().any(|e| e.token.ends_with("glass.rank") && e.value == "3.00"),
            "a depth past the pyramid was not clamped"
        );
        // And a fraction survives to the file — the whole point of the
        // two-fan emitter.
        let mid = glass_edits(Scope::Theme, Glass::Blur, tint, wash, 1.0, 1.7, 0.42);
        assert!(
            mid.iter().any(|e| e.token.ends_with("glass.rank") && e.value == "1.70"),
            "a fractional depth was rounded away"
        );
    }
}
