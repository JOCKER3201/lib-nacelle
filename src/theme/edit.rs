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
//! The gap is a real task, not a permanent shape: `glass.rank` has no reader
//! at all, and `glass.tint`/`glass.wash` are read only for the fixture rung,
//! by hand, in `deco.rs`. When the ladder is honoured for every rung, the sets
//! here widen and nothing else about this module changes.

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

/// Tokens that carry the background, for one scope.
///
/// The glass pair is read for the fixture rung only (`deco.rs:84-88`), so that
/// is the rung addressed here. `elev.panel.fill` is what a panel actually
/// draws when there is no glass (`object/elev.rs:97-100`, and the master's own
/// note at the `fill` key: "used INSTEAD of the glass pair while rank = 0").
///
/// `glass.rank` is NOT written. It has no reader anywhere in the workspace —
/// grep finds comments and nothing else — so a rank in the file would be a
/// promise the renderer never keeps.
pub fn glass_edits(scope: Scope, kind: Glass, tint: Oklch, wash: Oklch) -> Vec<Edit> {
    let Scope::Theme = scope;
    match kind {
        // No glass: the surface's own fill carries the colour, and the wash
        // is cleared so a previous frosted state does not sit on top of it.
        Glass::Solid => vec![
            Edit::new("elev.panel.fill", oklch_literal(wash)),
            Edit::new("elev.fixture.glass.wash", "none"),
        ],
        // Blurred, tint left alone: multiplying by a colour can only darken,
        // so a blur that wants to stay honest to the scene behind it does not
        // tint at all.
        Glass::Blur => vec![
            Edit::new("elev.fixture.glass.tint", oklch_literal(Oklch { alpha: 1.0, ..tint })),
            Edit::new("elev.fixture.glass.wash", oklch_literal(wash)),
        ],
        // Both quads carry colour: the tint pulls the scene toward the glass
        // and the wash lays over it until nothing reads through.
        Glass::Frosted => vec![
            Edit::new("elev.fixture.glass.tint", oklch_literal(tint)),
            Edit::new("elev.fixture.glass.wash", oklch_literal(wash)),
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
        let colour = c(0.7, 0.15, 200.0, 1.0);
        let mut all = Vec::new();
        for kind in [Border::Line, Border::Neon] {
            all.extend(border_edits(Scope::Theme, kind, colour, false));
            all.extend(border_edits(Scope::Theme, kind, colour, true));
        }
        all.push(border_colour_edit(Scope::Theme, colour));
        for kind in [Glass::Solid, Glass::Blur, Glass::Frosted] {
            all.extend(glass_edits(Scope::Theme, kind, colour, colour));
        }
        for e in &all {
            assert!(
                !DEAD.contains(&e.token),
                "the model wrote `{}`, which no renderer reads",
                e.token
            );
            assert!(
                !e.token.ends_with("glass.rank"),
                "the model wrote a glass rank; nothing reads one"
            );
        }
    }

    #[test]
    fn blur_leaves_the_tint_opaque_and_frosted_does_not() {
        // Multiplying by a translucent tint is not "less tint" — the blur quad
        // is a multiply, so alpha there means something other than strength.
        // Blur keeps it at 1.0 on purpose and the difference is worth pinning.
        let tint = c(0.6, 0.05, 220.0, 0.5);
        let wash = c(0.2, 0.02, 220.0, 0.34);
        let blur = glass_edits(Scope::Theme, Glass::Blur, tint, wash);
        let frosted = glass_edits(Scope::Theme, Glass::Frosted, tint, wash);
        let tint_of = |v: &Vec<Edit>| {
            v.iter().find(|e| e.token.ends_with("glass.tint")).unwrap().value.clone()
        };
        assert!(!tint_of(&blur).contains('/'), "blur tinted with an alpha");
        assert!(tint_of(&frosted).contains('/'), "frosted lost the tint's alpha");
    }
}
