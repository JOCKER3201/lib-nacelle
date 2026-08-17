//! One SURFACE LEVEL, drawn in one place.
//!
//! `[elev.backdrop]` … `[elev.fixture]` is the master's ladder of
//! surfaces (§5.12). Every rung states the same dictionary — a body
//! (`fill`), a shape (`corner`, `radius`), a ring (`edge.color`,
//! `edge.width`, and the two-stop pair `edge.mode` / `edge.color2` /
//! `edge.axis`), and beyond them the glass pair, the two glows, the
//! drop shadow and the reflection. An object that assembles its rung out
//! of primitives at its own call site therefore owns a PRIVATE COPY of
//! those rules, and the copies drift: `panel.rs` reads the fill as a bed
//! and guards it on alpha, `window.rs` does not; whichever of them the
//! next level's author reads is the one that level will resemble.
//!
//! [`Level`] is the one reader. A consumer names a rung once — `"elev.
//! popover"` — and gets the whole dictionary, so when the glass ranks
//! and the shadow 9-slice land they land for every rung at once instead
//! of for whichever object was being edited that week.
//!
//! What is NOT here is any decision: no fallback colour, no minimum
//! radius, no "if the theme says nothing draw a hairline". A rung whose
//! `fill` is `none` and whose `edge.width` is `0` draws nothing, which
//! is the raw look the governing principle asks for.

use crate::corner::Cuts;
use crate::draw::Corner;
use crate::theme::{self, Color, TokenId};
use crate::{Ctx, Rect};
use std::sync::OnceLock;

/// The engine's colour, in the draw list's clothes.
fn col(c: theme::ThemeColor) -> Color {
    Color { r: c.r, g: c.g, b: c.b, a: c.a }
}

/// The keys of one `[elev.*]` rung, resolved to ids once.
///
/// Ids and enum vocabularies are both stable for the life of the process
/// — [`theme::id`] says so of the first, and the second is interned out
/// of the master, which no user theme replaces — so a `Level` is built
/// inside a `OnceLock` at the call site exactly like the bare `TokenId`
/// statics everywhere else. What is NOT cached is any resolved VALUE:
/// the colour, the radius and the corner word are read from the live
/// [`theme::ResolvedTheme`] on every draw, so a theme swap moves them.
pub(crate) struct Level {
    fill: TokenId,
    corner: TokenId,
    radius: TokenId,
    edge_color: TokenId,
    edge_color2: TokenId,
    edge_mode: TokenId,
    edge_axis: TokenId,
    edge_width: TokenId,
    glass_rank: TokenId,
    glass_tint: TokenId,
    glass_wash: TokenId,
    /// Where each cut's word sits in `corner`'s vocabulary — see
    /// [`crate::corner::Cuts`], which is the one reader of it.
    words: Cuts,
    /// `edge.mode`'s index for the word `gradient`.
    mode_gradient: Option<u16>,
    /// `edge.axis`'s indices for `x, y, diag_down, diag_up`, in that
    /// order, against [`AXES`].
    axis_words: [Option<u16>; 4],
}

/// What each word of `edge.axis` means as a direction, y DOWN — the same
/// screen space [`crate::draw::DrawList::rect_grad`] projects in.
///
/// Definitions of the four words, not lengths or colours: `diag_down`
/// travels down as it travels right, which is what the word says. The
/// vectors are not normalised because the ring normalises `t` against the
/// box's own extent, so only the DIRECTION is read here.
const AXES: [(&str, [f32; 2]); 4] =
    [("x", [1.0, 0.0]), ("y", [0.0, 1.0]), ("diag_down", [1.0, 1.0]), ("diag_up", [1.0, -1.0])];

impl Level {
    /// The rung named by `prefix`, e.g. `"elev.popover"`.
    ///
    /// A name and not a `TokenId` because a rung is a DICTIONARY: the
    /// caller states which surface it is, once, and the five keys under
    /// it are this module's business. A key the master does not declare
    /// degrades through [`TokenId::MISSING`], which is the engine's raw
    /// look and not a design.
    pub(crate) fn of(prefix: &str) -> Level {
        let id = |key: &str| theme::id(&format!("{prefix}.{key}")).unwrap_or(TokenId::MISSING);
        let corner = id("corner");
        let edge_mode = id("edge.mode");
        let edge_axis = id("edge.axis");
        Level {
            fill: id("fill"),
            corner,
            radius: id("radius"),
            edge_color: id("edge.color"),
            edge_color2: id("edge.color2"),
            edge_mode,
            edge_axis,
            edge_width: id("edge.width"),
            glass_rank: id("glass.rank"),
            glass_tint: id("glass.tint"),
            glass_wash: id("glass.wash"),
            words: Cuts::of(corner),
            mode_gradient: theme::enum_index(edge_mode, "gradient"),
            axis_words: [
                theme::enum_index(edge_axis, AXES[0].0),
                theme::enum_index(edge_axis, AXES[1].0),
                theme::enum_index(edge_axis, AXES[2].0),
                theme::enum_index(edge_axis, AXES[3].0),
            ],
        }
    }

    /// The COMPONENT this rung is worn by, where its keys are older than
    /// the ladder's.
    ///
    /// `elev::Level` is the one place a surface is drawn, but the two
    /// oldest floating surfaces in the toolkit — the menu and the tooltip
    /// — were written before the ladder existed and the master still
    /// spells their body, ring and cut under their own names
    /// (`component.menu.fill`, `[menu].corner` for a RADIUS where the
    /// ladder's `corner` is a CUT MODE). Renaming those keys would break
    /// every theme and every embedder that names them; keeping a private
    /// copy of the drawing rules is what this module exists to stop. So
    /// the object states which of its own tokens stand in for which of the
    /// rung's, once, here — and everything it does NOT name (the glass
    /// pair, and every key the ladder grows after this line) comes from
    /// the rung, which is what "participates in the elevation hierarchy"
    /// means.
    ///
    /// The ring's SECOND colour, its mode and its axis stay on the rung on
    /// purpose: a two-stop edge is a property of the surface class, the
    /// component names only the ring it already had, and a component key
    /// that does not exist would resolve to `MISSING` — whose colour is
    /// the engine's raw ink, which is not what "the theme said nothing"
    /// should paint.
    pub(crate) fn worn_as(
        mut self,
        fill: &str,
        corner: &str,
        radius: &str,
        edge_color: &str,
        edge_width: &str,
    ) -> Level {
        let id = |name: &str| theme::id(name).unwrap_or(TokenId::MISSING);
        self.fill = id(fill);
        self.corner = id(corner);
        self.radius = id(radius);
        self.edge_color = id(edge_color);
        self.edge_width = id(edge_width);
        self.words = Cuts::of(self.corner);
        self
    }

    /// The far end of a two-stop ring and the direction it travels, or
    /// `None` for the flat ring every rung draws by default.
    ///
    /// Three tokens have to agree, and the master says so at each of them:
    /// `edge.mode` must be the word `gradient`; `edge.color2` must hold a
    /// COLOUR (its default `same_as_parent` is a §5.0 sentinel, which bakes
    /// to a negative scalar and means "copy edge.color", i.e. a flat ring);
    /// and `edge.axis` must be one of [`AXES`]' four words. A direction the
    /// vocabulary does not name is not a direction, so the ring stays flat
    /// rather than being drawn along a guess — the same degradation
    /// [`Cuts::read`] applies to a corner word.
    ///
    /// What is NOT read is `edge.gradient`, the NAMED multi-stop slot
    /// (`@grad.<name>`). Measured 2026-08-17: the engine bakes no stops at
    /// all. `[grad]`'s `<name>.stops` is an array of `[position, colour]`
    /// pairs, `cascade.rs` declares each pair as a token, and `bake.rs`'s
    /// `Value::Array(_) => {}` drops it — so `ResolvedTheme` has no place
    /// to hold a stop list and no accessor to answer one with. Adding it
    /// is a theme-engine job (bake the eight `[grad].samples` RGBA stops
    /// per slot, plus a `stops(id)` accessor); until then the sugar pair is
    /// the whole of what a theme can ask for here, which is what the
    /// master's own comment calls "the color/color2 pair".
    fn edge_gradient(&self, t: &theme::ResolvedTheme) -> Option<(Color, [f32; 2])> {
        if self.mode_gradient? != t.enum_of(self.edge_mode) {
            return None;
        }
        // A word in the colour slot, not a colour: §5.0's sentinels fold to
        // a negative scalar and leave the colour empty.
        if t.px(self.edge_color2) < 0.0 {
            return None;
        }
        let axis = t.enum_of(self.edge_axis);
        let i = self.axis_words.iter().position(|w| *w == Some(axis))?;
        Some((col(t.color(self.edge_color2)), AXES[i].1))
    }

    /// The cut this rung makes on the box `r`, and the tessellation of
    /// its arcs.
    ///
    /// Through [`Corner::sized`] and not a clamp: §5.0's `pill` is a
    /// word about a box ("as round as this one can be") and bakes to a
    /// negative sentinel, so a floor at zero answers a master writing
    /// `pill` with the square it wrote to avoid.
    pub(crate) fn cut(&self, t: &theme::ResolvedTheme, r: Rect) -> ([Corner; 4], u8) {
        static SEGMENTS: OnceLock<TokenId> = OnceLock::new();
        let style = self.words.read(t, self.corner);
        let c = Corner::sized(style, t.px(self.radius), r);
        ([c; 4], super::window::corner_segments(t, &SEGMENTS, c.size))
    }

    /// Material, ring, and family A's bloom over the ring.
    ///
    /// Answers the shape it drew, because a caller that has to fit
    /// content INSIDE the rung — a drop-down's rows, a panel's content
    /// box — would otherwise settle the same cut a second time and be
    /// free to settle it differently.
    pub(crate) fn draw(&self, ctx: &mut Ctx, r: Rect) -> ([Corner; 4], u8) {
        self.draw_in(ctx.dl, theme::resolved(), r, ctx.t)
    }

    /// [`Level::draw`] with the theme and the clock in hand and no frame
    /// around it.
    ///
    /// A rung touches nothing of a `Ctx` but its draw list and its clock,
    /// and taking the theme as an argument is what lets one rung be drawn
    /// from a theme that is not the published one — which is how the
    /// picture this rung makes is put under test at all, gradient ring
    /// included, without a test reaching into the process-wide theme every
    /// other test is reading at the same time.
    ///
    /// `now` is `Ctx::t`, seconds since application start, and it exists
    /// here for one reader: the edge bloom breathes on `motion.glow_pulse`
    /// and a cyclic effect has to be told what time it is. A caller with no
    /// frame around it — every test below — passes a time of its own
    /// choosing, which is the only way a pulse can be sampled at a stated
    /// phase instead of at whenever the suite happened to run.
    pub(crate) fn draw_in(
        &self,
        dl: &mut crate::draw::DrawList,
        t: &theme::ResolvedTheme,
        r: Rect,
        now: f64,
    ) -> ([Corner; 4], u8) {
        let (c, seg) = self.cut(t, r);
        // Glass INSTEAD of the fill, never on top of it — the master's own
        // contract at the `fill` key ("used INSTEAD of the glass pair while
        // rank = 0") and at the ladder's head: glass is TWO quads, the tint
        // that multiplies the blurred scene (it can only darken) and the
        // wash that lays over with alpha (the only knob that brightens).
        // This is the rank's FIRST reader: until 2026-08-16 the token was
        // declared on every rung and read by nobody, so a theme asking for
        // glass on a panel got a flat fill and no word about it.
        let rank = t.px(self.glass_rank).clamp(0.0, 3.0);
        if rank > 0.0 {
            dl.glass_fill(r, &c, seg, rank, col(t.color(self.glass_tint)));
            let wash = col(t.color(self.glass_wash));
            if wash.a > 0.0 {
                dl.ring_fill(r, &c, seg, wash);
            }
        } else {
            let fill = col(t.bed(self.fill));
            if fill.a > 0.0 {
                dl.ring_fill(r, &c, seg, fill);
            }
        }
        let edge = col(t.color(self.edge_color));
        let width = t.px(self.edge_width).max(0.0);
        if edge.a > 0.0 && width > 0.0 {
            // Until 2026-08-17 this read `edge.color` and nothing else, so
            // a theme that wrote `edge.mode = gradient` beside a second
            // colour got a flat ring and no word about it — the master
            // declares the pair at every one of the nine rungs.
            match self.edge_gradient(t) {
                Some((far, dir)) => dl.ring_grad(r, &c, seg, width, edge, far, dir),
                None => dl.ring(r, &c, seg, width, edge),
            }
            // The bloom keeps taking the ring's OWN colour, the near end:
            // `glow_ring` is one additive sprite ring with one vertex
            // colour, so a gradient halo is not a thing this call can carry
            // and inventing a midpoint here would be a decision made in
            // Rust. Its ALPHA breathes on `motion.glow_pulse`, which is
            // what the clock is for; a two-colour ring and a breathing
            // bloom are orthogonal — the gradient decides the ring's two
            // ends, the pulse decides how brightly the halo over it is
            // laid, and neither reads the other.
            super::window::panel_edge_glow(dl, t, r, &c, seg, edge, now);
        }
        (c, seg)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::draw::{DrawCmd, DrawList};

    /// The clock every proof in and under this module draws at.
    ///
    /// A stated instant, not "whenever the suite ran": `draw_in`'s only
    /// reader of the clock is the edge bloom's breath on
    /// `motion.glow_pulse`, and a picture compared against another picture
    /// has to be taken at the same phase as it. The master ships
    /// `glow.panel_edge.enabled = false`, so under it the bloom returns
    /// before the pulse is ever sampled and the number does not matter —
    /// which is exactly why it must be written down rather than left to a
    /// theme that turns the glow on later.
    pub(crate) const AT_REST: f64 = 0.0;

    // ------------------------------------------- the no-move proof
    //
    // Shared with `menu.rs` and `tooltip.rs`, whose claim is not about a
    // gradient at all: that JOINING the ladder moved no pixel. Written
    // once, here, because two copies of what counts as proof is the same
    // mistake in the test suite that this module exists to undo in the
    // drawing code.

    /// What an object drew before it joined the ladder — the `ring_fill`
    /// + `ring` pair from its own five tokens, transcribed from the
    /// private copies `menu.rs` and `tooltip.rs` carried until
    /// 2026-08-17. It is a TRANSCRIPT, so it keeps their two departures
    /// from the rung: the body is drawn whatever its alpha, and the ring
    /// is drawn on the width alone.
    pub(crate) fn the_private_copy(
        dl: &mut DrawList,
        t: &theme::ResolvedTheme,
        r: Rect,
        fill: &str,
        corner_mode: &str,
        radius: &str,
        edge: &str,
        width: &str,
    ) {
        static SEG: OnceLock<TokenId> = OnceLock::new();
        let id = |n: &str| theme::id(n).unwrap_or(TokenId::MISSING);
        let mode = id(corner_mode);
        let style = Cuts::of(mode).read(t, mode);
        let c = [Corner::sized(style, t.px(id(radius)), r); 4];
        let seg = super::super::window::corner_segments(t, &SEG, c[0].size);
        dl.ring_fill(r, &c, seg, col(t.bed(id(fill))));
        let bw = t.px(id(width)).max(0.0);
        if bw > 0.0 {
            dl.ring(r, &c, seg, bw, col(t.color(id(edge))));
        }
    }

    /// Two lists that are the same picture, checked the way the frame
    /// guard checks one: the command register AND the vertices under it.
    /// The register alone would miss a colour the commands agree on and
    /// the geometry does not; the vertices alone would miss a command
    /// that emitted none.
    pub(crate) fn same_picture(was: &DrawList, now: &DrawList) {
        let dump = |dl: &DrawList| {
            dl.cmds().iter().map(|c| c.to_string()).collect::<Vec<_>>().join("\n")
        };
        assert_eq!(dump(was), dump(now));
        let verts = |dl: &DrawList| {
            dl.verts.iter().map(|v| (v.pos, v.uv, v.color)).collect::<Vec<_>>()
        };
        assert_eq!(verts(was).len(), verts(now).len(), "the vertex count moved");
        assert_eq!(verts(was), verts(now));
    }

    /// The rung every popover wears, undressed — the ladder's own key
    /// spellings, which is what a gradient is written against.
    fn popover() -> Level {
        Level::of("elev.popover")
    }

    fn box_() -> Rect {
        Rect::new(20.0, 12.0, 160.0, 40.0)
    }

    /// The one command a ring draws, whichever kind it is.
    fn ring_cmd(dl: &DrawList) -> DrawCmd {
        let rings: Vec<_> = dl
            .cmds()
            .iter()
            .filter(|c| matches!(c, DrawCmd::Ring { .. } | DrawCmd::RingGrad { .. }))
            .cloned()
            .collect();
        assert_eq!(rings.len(), 1, "a rung strokes its ring once: {rings:?}");
        rings[0].clone()
    }

    /// An `[elev.popover]` override, with the two vocabularies restated.
    ///
    /// Restated because a re-declaration in the SAME stage replaces the
    /// token whole, `enum:` list included (`cascade.rs`'s `declare`), and
    /// an enum's baked value is an INDEX into that list — so an override
    /// that dropped the list would number its own single word 0 and mean
    /// something else than the same word means in the master.
    fn overlay(mode: &str, color2: &str, axis: &str) -> String {
        format!(
            "[elev.popover]\n\
             edge.mode = {mode}    # · enum: solid | gradient ·\n\
             edge.color2 = {color2}\n\
             edge.axis = {axis}    # · enum: x | y | diag_down | diag_up ·\n"
        )
    }

    /// A popover rung whose ring is `mode`/`color2`/`axis`, drawn once.
    fn ring_under(mode: &str, color2: &str, axis: &str) -> DrawCmd {
        let t = theme::bake_over_master(&overlay(mode, color2, axis));
        let mut dl = DrawList::recording();
        popover().draw_in(&mut dl, &t, box_(), AT_REST);
        ring_cmd(&dl)
    }

    /// The declaration this whole path stood on, and stood on badly until
    /// 2026-08-17: `edge.mode`'s vocabulary is the master's `enum:` list,
    /// not the words a theme happens to have used. Without the list the
    /// vocabulary grows from use, `solid` is the only word ever used, and
    /// `gradient` — the one word the key exists to carry — could never be
    /// delivered by any theme, so no reader here could have fired.
    #[test]
    fn the_master_owns_the_words_this_ring_is_switched_by() {
        for rung in ["elev.backdrop", "elev.board", "elev.panel", "elev.raised",
            "elev.focused", "elev.popover", "elev.inset", "elev.overlay", "elev.fixture"] {
            let mode = theme::id(&format!("{rung}.edge.mode")).unwrap();
            assert_eq!(theme::enum_index(mode, "solid"), Some(0), "{rung}");
            assert_eq!(theme::enum_index(mode, "gradient"), Some(1), "{rung}");
            let axis = theme::id(&format!("{rung}.edge.axis")).unwrap();
            for (i, (word, _)) in AXES.iter().enumerate() {
                assert_eq!(theme::enum_index(axis, word), Some(i as u16), "{rung} {word}");
            }
        }
    }

    /// USTERKA 2. A gradient written in the theme reaches the ring as a
    /// gradient: two ends, and the axis the theme named.
    #[test]
    fn a_gradient_edge_is_drawn_as_one() {
        let t = theme::bake_over_master(&overlay("gradient", "#FF00FF / 1.0", "diag_down"));
        let mut dl = DrawList::recording();
        popover().draw_in(&mut dl, &t, box_(), AT_REST);
        match ring_cmd(&dl) {
            DrawCmd::RingGrad { near, far, dir, stroke, .. } => {
                // The near end is `edge.color`, untouched: the sugar pair
                // is color -> color2, in that order.
                let want = t.color(theme::id("elev.popover.edge.color").unwrap());
                assert!((near.r - want.r).abs() < 1e-6, "near {near:?} is not edge.color");
                assert!((near.a - want.a).abs() < 1e-6, "near {near:?} is not edge.color");
                // A hair, not equality: the far end went round the sRGB
                // transfer on its way through the bake.
                for (got, want) in [(far.r, 1.0), (far.g, 0.0), (far.b, 1.0), (far.a, 1.0)] {
                    assert!((got - want).abs() < 1e-6, "far {far:?} is not #FF00FF");
                }
                assert_eq!(dir, [1.0, 1.0]);
                assert!(stroke > 0.0, "the ring still takes its width from the theme");
            }
            other => panic!("the theme asked for a gradient and got {other}"),
        }
    }

    /// Each of the four words is a different direction, and `y` is DOWN —
    /// the screen's axis, not the plotter's.
    #[test]
    fn every_axis_word_is_its_own_direction() {
        for (word, dir) in AXES {
            match ring_under("gradient", "#FF00FF / 1.0", word) {
                DrawCmd::RingGrad { dir: got, .. } => assert_eq!(got, dir, "{word}"),
                other => panic!("{word} drew {other}"),
            }
        }
    }

    /// The master's own default stays FLAT, which is the whole reason the
    /// picture did not move when this reader landed: `same_as_parent` is a
    /// §5.0 sentinel — "copy edge.color" — and a copy of one colour is not
    /// a gradient. Three shipped spellings, one flat ring.
    #[test]
    fn the_master_default_and_its_neighbours_stay_flat() {
        for (mode, color2, axis) in [
            // What every one of the nine rungs ships.
            ("solid", "same_as_parent", "x"),
            // A theme that asked for a gradient and named no far end.
            ("gradient", "same_as_parent", "x"),
            // …and one that named a direction the vocabulary does not
            // have: not a direction, so not a gradient, rather than a
            // guess made in Rust.
            ("gradient", "#FF00FF / 1.0", "sideways"),
        ] {
            match ring_under(mode, color2, axis) {
                DrawCmd::Ring { .. } => {}
                other => panic!("{mode}/{color2}/{axis} drew {other}"),
            }
        }
    }

    /// A gradient ring costs what a flat one costs — the master's own
    /// claim at `[grad]` ("the same 24 verts a solid border costs"), which
    /// is why a gradient border was affordable enough to declare on all
    /// nine rungs in the first place.
    #[test]
    fn a_gradient_ring_costs_what_a_flat_ring_costs() {
        let t = theme::resolved();
        let flat = {
            let mut dl = DrawList::new();
            popover().draw_in(&mut dl, t, box_(), AT_REST);
            dl.verts.len()
        };
        let grad = {
            let g = theme::bake_over_master(&overlay("gradient", "#FF00FF / 1.0", "x"));
            let mut dl = DrawList::new();
            popover().draw_in(&mut dl, &g, box_(), AT_REST);
            dl.verts.len()
        };
        assert_eq!(flat, grad);
    }

    /// The two ends land ON the box, not somewhere inside it: `t` is
    /// normalised against the rect's own projected extent, so the near
    /// colour is exactly at the least-projected corner and the far colour
    /// exactly at the most-projected one. Read off the VERTICES, since
    /// that is what the rasteriser interpolates between.
    #[test]
    fn the_two_ends_reach_the_ends_of_the_box() {
        // `fill = none` leaves the RING alone in the list: the body under
        // it reaches the same two edges in a different colour, so a
        // reading taken over the whole list would be measuring the fill.
        let t = theme::bake_over_master(&format!(
            "{}fill = none\n",
            overlay("gradient", "#FF00FF / 1.0", "x")
        ));
        let mut dl = DrawList::new();
        popover().draw_in(&mut dl, &t, box_(), AT_REST);
        let r = box_();
        let left = dl
            .verts
            .iter()
            .filter(|v| (v.pos[0] - r.x).abs() < 1e-3)
            .map(|v| v.color[0])
            .fold(f32::INFINITY, f32::min);
        let right = dl
            .verts
            .iter()
            .filter(|v| (v.pos[0] - (r.x + r.w)).abs() < 1e-3)
            .map(|v| v.color[0])
            .fold(f32::NEG_INFINITY, f32::max);
        let near = t.color(theme::id("elev.popover.edge.color").unwrap());
        assert!(!dl.verts.is_empty(), "the ring drew nothing to measure");
        assert!((left - near.r).abs() < 1e-6, "left end {left} is not edge.color");
        assert!((right - 1.0).abs() < 1e-6, "right end {right} is not edge.color2");
    }
}
