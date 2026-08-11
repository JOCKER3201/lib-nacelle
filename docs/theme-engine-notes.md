# Implementation notes on the theme engine

Where the code deliberately differs from `theme-engine.md`, and what the
specification got wrong. The spec is the design; this file is the record of what
survived contact with a compiler. Read both.

## Deviations decided by the lead

**`default.theme` is the schema.** §2.1 asked for a generated `tokens.rs` holding
enums for ~2190 tokens and §7 for a `ResolvedTheme` with one named field each — a
second copy of the catalogue to be kept byte-identical with `default.theme` by hand,
forever. Instead the master file IS the token table: the set of tokens that exist is
the set of keys it declares, a token's type is inferred from the form of its default
value, and its id is its position in declaration order, interned at load. An unknown
key in another theme falls out of the design rather than needing a second table, and
adding a token is one line in one file. `ResolvedTheme` is four flat arrays
(`colors`, `scalars`, `flags`, `enums`) indexed by a `u16` `TokenId`; a hot draw path
holds its ids in a `OnceLock` resolved once by name, so a per-frame read is one
bounds-checked slice index — the cost §7 promised, without the duplication.

**§4.1 stage 1 could not survive that.** A compiled `const FALLBACK: ResolvedTheme`
has nothing to be generated from once there is no hand-written token table. It is
replaced by `resolve::fallback()`, a value per *kind* (grey, 0.0, false, word 0),
which keeps the guarantee that matters: stage 2 failing still yields a running
program.

## Defects found in the specification

- **The EBNF cannot spell the catalogue.** §3.2 has `ident = lower { lower | digit |
  "_" }`, but §5.4's ladders are `space.0`, `size.2xl`, `size.3xl`. A path segment
  must be allowed to start with a digit; the parser allows it.
- **Bare words carry dots.** §3.2 has `enum-word = ident`, yet every `*_role` token's
  value is a dotted role name (`title.window`, `label.section`). The parser accepts
  dots in a bare word — unambiguous, because a reference starts with `@` and a call is
  caught by its parenthesis.
- **`px` on a whole last segment.** §3.2 says `px` is legal only on a name ending
  `_min_px`/`_max_px`; the catalogue also writes `type.min_px` and
  `type.display.clock.max_px`, where the suffix is the entire segment. Both forms are
  accepted.
- **`contrast_on` prose contradicts its definition.** §6 claims crimson's `#FF2A35`
  chip needs light text; under WCAG 2.x that chip is 5.63 against black and 3.73
  against white, so the function correctly returns dark. The definition was
  implemented; the prose is wrong.
- **Worked hex values are not reproducible.** §6's `mix(black, accent, 0.06) =
  #141E1A` looks computed in sRGB, not linear; §4.4's composites land one 8-bit step
  off. Tests assert the properties with a one-step tolerance instead of the literals.

## A bug the engine's own tests caught

`Schema::from_default` originally interned `[mood.*]` and `[variant.*]` keys from the
master into the dense schema, which would have made `default.theme`'s shipped
`lockdown` mood everyone's ordinary colours. Overlay sections are skipped when
building the schema — they are stage 4, and may only re-declare tokens that already
exist. Regression test:
`cascade::tests::a_mood_inside_the_master_does_not_become_everyones_default`.

## Deliberately not built yet

`enforce.rs` (contrast floors and separation repair), `encode.rs` (output encoding per
swapchain format — `bake.rs` does the sRGB encode inline meanwhile), `abi.rs` (the
nineteen appended `HostApi` entries), `mask.rs` and `plate.rs` (both need renderer
work from Appendix B). The structured views of §7.1 — `ClassStyle[49][7]`,
`SeverityStyle`, `Motion`, `Type`, `ShapeSpec`, `Gradient`, `IconDef` — are not
materialised; `ResolvedTheme` is the flat form. `em` lengths bake to their bare
multiplier until `Ctx` carries a per-panel type cache. §5.10's severity generation and
§5.11's ANSI generation are unimplemented, and every slot they would fill is an
addressable token today.

Each absence is named in the module docs of the module that will call it.

## Glow adoption (10.08, after the sprite fleet)

`glow.panel_edge` is live in the toolkit: `object::window::panel_edge_glow`
draws the additive sprite ring after every panel-class stroke it owns —
`window::frame` (settings, editor, popups) and `winframe`'s outer ring.
Three deferrals, each deliberate:

- **Plugin panels can't glow yet.** The four widget plugins draw their frames
  with `polyline` over the ABI; the sprite path needs the mask uv and the
  additive image handle, neither of which crosses ABI 5. This joins the
  enum-word gap on the ABI 6 list.
- **`focus_ring` waits for the focus system.** `winframe` swaps its edge
  colour on focus (§5.21) and the glow follows that colour through the
  `element` rule, but the separate `[glow] focus_ring` class has no consumer
  until containers know they are focused. No shipped theme enables it.
- **`panel_edge.color` only honours `element`.** The master types the token
  by its default, a bare word; the colour arm of the union is unimplemented
  and no shipped theme uses it.
