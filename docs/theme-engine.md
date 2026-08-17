# nacelle theme engine — authoritative specification

Status: **binding**. This document supersedes the seven agent files
(`d1-palette-console.md`, `d2-material-holographic.md`, `d3-typography-ornament.md`,
`u1-layout-metrics.md`, `u2-states-semantics.md`, `s1-gtk-adwaita.md`,
`s2-qt-kde-tokens.md`). Where they disagree, the resolution is recorded here with
its reason, in **[CONFLICT n]** boxes. Implementers follow this file literally; the
agent files are background reading only.

Two documents this spec is subordinate to:

* `reference-images.md` — the visual brief. Every value here is traceable to it.
* `scope-boundary.md` — the owner's line: **the theme changes how things look, never
  where panels are.** Any token that would move a panel rectangle is cut, and several
  are (see [CONFLICT 4]).

Reference resolution throughout: **1920×1080**, at which the metric unit `u` = 5.4 px.
Where a second number appears it is 2560×1440 (`u` = 7.2 px).

---

## 1. GOALS AND NON-GOALS

### 1.1 What this engine must do

1. **One palette re-skins everything.** Changing `palette.accent` — one line — must
   re-derive every border, label, badge, chart line, glyph, gauge, hexagon and gauge
   arc in the interface. Reference images 2/3/4/6 are the same pixels in four hues;
   reproducing them must cost one edit each. Nothing a widget draws may name a
   literal colour.
2. **Cover everything that can be drawn.** Every colour, length, weight, corner,
   glow, gap, duration and case transform currently frozen as a constant in the
   source becomes a token. The inventory found **214 geometric constants** and
   **96 colour decisions** hard-coded across `libnacelle`, `nacelle-desktop` and
   `nacelle-widgets`; §5 accounts for every one of them (or states why it is not a
   token).
3. **Chrome and data separate.** Panel borders red while the hologram is blue
   (image 5) costs one line: `palette.data`.
4. **Semantic colour survives a monochrome theme.** Amber `CONTAINED` inside an
   all-red screen (image 4); severity by brightness alone when a theme has one hue
   (image 3).
5. **Materials are theme properties.** Glass tint and wash, blur level, edge
   brightness, inner and outer glow, drop shadow, reflection, elevation.
6. **Typography is a system.** A display face, a UI face with wide-tracked small
   caps, a monospace, an icon face, a size ladder, per-role tracking, leading, case
   and tabular figures.
7. **Two visual families from one engine.** Family A (opaque console, images 1–6)
   and family B (glass over a live backdrop, images 7–10) differ only in token
   values, never in engine code.
8. **`default` is the master.** It declares 100 % of tokens, is `include_str!`-embedded
   in the binary, and is the documentation: every key carries a comment stating what it
   colours, which reference image shows it, its unit, its legal range and the
   derivation used when omitted. A theme that omits a key inherits `default`'s
   *expression*, not its value — so overriding a root re-derives the leaves.
9. **Never fail to start.** No theme file, however wrong, may prevent the program
   from running or leave a token unset. Every failure degrades to `default`'s value
   and says precisely what it ignored, where, and why.
10. **Zero per-frame cost.** After load: no string, no hash map, no allocation, no
    lock and no branch-on-variant on any draw path. Every lookup is an array index
    into a `#[repr(C)]` POD.

### 1.2 What this engine deliberately will not do

1. **It will not move a panel.** No token reaches `flex.rs`, `.layaut` files,
   `LayoutSpec`/`FlexLayaut`, `outer_layout`, `padded()`, the responsive breakpoints,
   board membership or the per-panel intrinsic sizing pass. See [CONFLICT 4].
2. **It will not override the user's grid settings** — snap, columns, rows, widget
   padding. Those are the user's layout preferences.
3. **No selectors, no specificity, no structural inheritance, no `var()`, no
   `currentColor`, no media queries as a runtime concept.** The cascade is five
   ordered sparse overlays and nothing else. (GTK needs a counting Bloom filter, a
   per-parent style cache and a 64-selector cap to make a real cascade fast; nacelle
   has no node tree to match against, so it would buy the cost with none of the
   benefit.)
4. **No live re-resolve, no partial invalidation.** A reload re-runs the whole load
   path and swaps one `Arc`.
5. **The engine writes a theme file only through the editor, and never loses a
   comment doing it.** *(Amended 2026-08-16 — see question 12, now decided.)*
   This rule used to read "the engine never writes a theme file", with overlay-only
   edits. The owner decided the other way: SAVE writes the theme being edited, and
   a built-in theme is materialised into the user's theme directory on its first
   save. What the old rule was really protecting — authored files and their
   comments staying byte-identical — survives as the constraint on HOW: a write
   patches the bytes of the value spans it changes and touches nothing else. The
   AST cannot be used to regenerate a file, because `strip_comment` removes
   comments before parsing and `Document` has nowhere to keep them.
   The pattern to follow already exists for `.layaut` files in
   `src/layout/store.rs`: `save_full` / `save_overrides`, materialisation on first
   write, a backup before overwriting, and `preserved_base` keeping the user's own
   text.
6. **No general-purpose styling language.** Tokens name *meanings*, never
   construction. A theme may address `panel.border.color`; it may not address the
   vertex layout of a panel. If a theme can reach a rendering internal, the renderer
   can never be refactored.
7. **Things that cannot become triangles do not become tokens.** Per-pixel filters,
   chromatic aberration, group opacity, arbitrary Gaussian box-shadows, N-colour
   radial and conic gradients, mesh gradients, per-glyph animation, dashed strokes as
   a stroke *style* on arbitrary paths. Each is refused by name in §5 with the
   hardware reason.
8. **No build step, no serde, no TOML crate, no SVG rasteriser, no CSS.** The parser
   is hand-written; the whole load path is Rust in `libnacelle`.
9. **No light variant.** nacelle is a dark interface. `high_contrast` exists;
   `light` does not.
10. **The theme never overrides a program's explicit truecolour** in the terminal,
    and never recolours a photograph.

---

## 2. ARCHITECTURE

### 2.1 Where it lives

The engine is **in `libnacelle`**, not in the application. `nacelle-desktop` keeps
only "which theme is selected" and the filesystem search path.

```
libnacelle/src/theme/
  mod.rs        public API, Arc<ResolvedTheme> handle, epoch, variant/mood selection
  color.rs      Color, sRGB<->linear, OKLab/OKLCh, gamut mapping, WCAG + APCA
  tokens.rs     GENERATED: ColorToken / ScalarToken / FlagToken enums, name->id map,
                the token table that also generates the ResolvedTheme field order
  expr.rs       Expr AST, the 14 derivation functions, evaluation
  parse.rs      hand-written .theme parser: sections, keys, values, units, diagnostics
  cascade.rs    sparse overlay merge, [meta] base chain, reset, mood/variant blocks
  resolve.rs    memoised DAG walk, three-colour cycle marking, per-token fallback
  enforce.rs    contrast floors, perceptual-separation repair, honesty lints
  bake.rs       ThemeSpec + (screen_h, density, ui_scale, format) -> ResolvedTheme
  encode.rs     output-encode stage keyed on swapchain format (M9)
  abi.rs        ThemeC (#[repr(C)]), accessors, recipe implementations
  mask.rs       procedural R8 mask baking into the glyph atlas (M0)
  plate.rs      CPU decoration-plate rasteriser + worker thread (M10/M11)
  default.theme the master theme, include_str!-embedded — the ONE compiled-in look
```

*(Amended 2026-08-16: `themes/` — `aurora spring pure crimson lockdown azure
cockpit instrument` — was removed at the owner's decision. `default` is the
only compiled-in look; every other theme is a user file on the search path,
written by hand or by the editor. §8 keeps the eight as a record.)*

Supporting changes outside `theme/`:

| file | change |
|---|---|
| `libnacelle/src/font.rs` | **done:** 8 face slots as `[Font; 8]` (a slot that resolves to nothing aliases rather than staying empty, so no draw path carries an `Option`), `FACE_IDS` + `face_slot(word)` as the one word→slot rule both sides of the ABI use, §5.16's resolution ladder at load (requested weight → Regular + `synthetic_bold` at ≥600 → `fallback` chain, cycles cut at 8 → `FACE_UI`/`FACE_MONO`), `FaceChoice` folding the user's family/weight in as a delta so the master's ladder survives a settings change, `Figures` (§5.17's tabular box) with its own cache beside the glyph cache, atlas 1024²→2048² **with a reserved mask band the shelf packer never allocates from and `reset_atlas()` never clears** (§5.12), `mask()`, dirty-rect atlas upload. **Still open:** `cap_height` in `line_metrics`, `load_default_mono()` must stop panicking |
| `libnacelle/src/draw.rs` | `quad_c`, `rect_grad`, `fan_c`, `image_uv`, `soft_box`, `ring`/`shape` (generalised corners), `chevron_*`, `tab_*`, `hex_*`, `push_clip`/`pop_clip`; `module_title` becomes a thin wrapper over the host's title band (§5.25) and is deprecated |
| `libnacelle/src/base.rs` | `Ctx::u`, `Ctx::gu`, `Ctx::stroke`, `Ctx::ty`, `Ctx::text_role`, `Ctx::measure_role`, `Ctx::severity`, per-panel type cache; `Ctx::theme` becomes `&ResolvedTheme`; `Ctx` hands a widget its **content box**, the host having already drawn the container |
| `libnacelle/src/ui.rs` | `table` (`ui.rs:163-263`) and `columns` (`ui.rs:267-288`) stop naming `base.alpha(k)`: heading, rule, zebra and body cells take `component.table.*` + `table.head_role`/`table.cell_role`; label/value take `component.columns.*` + `columns.label_role`/`columns.value_role` (§5.25, §5.26) |
| `libnacelle/src/script.rs` | the Rhai element vocabulary is tokenised end to end: every element kind gains a colour role, a type role and an optional `severity:` (§5.29); `text()` gains a role form; `title`/`text`/`line`/`meter`/`columns`/`dots`/`table` stop computing `px*k` locally and read `script.*` |
| `libnacelle/src/plugin.rs` | forwards the role-aware text path (`text_role`/`measure_role`) so case, tracking, small caps and tabular advance are resolved host-side; raw `text`/`measure` remain, deprecated |
| `libnacelle/src/runtime.rs` | `HostApi` gains **19** appended entries (§7.4); `ABI_VERSION` 4 → 5 |
| `nacelle-desktop/src/main.rs` | the **host draws the panel container** in the panel loop before `wg.draw()` (§5.25); the two fixture boards (`main.rs:1841-1855`, `1742-1755`) stop hard-wiring `blur(...)` + `rect(bg, frost_wash)` and emit `Elev::Fixture` instead (§5.12); owns the decoration-plate `ImageId`s through the new image registry (§5.15); `BlurRadius=`/`BlurOpacity=` become a ceiling and a multiplier over the theme (§9.3) |
| `nacelle-renderer/src/gfx.rs` | additive pipeline (R1), per-run clip (R2), per-run glass **rank** (R3) plus `glass_ranks() -> u8`; host image registry backed by the existing `create_texture`/`update_texture`/`destroy_texture`; a one-shot diagnostic on both `MAX_VERTS` overflow paths (§5.12) |

### 2.2 The load pipeline, byte by byte

```
 0  bytes on disk / include_str!("default.theme")
    |
 1  parse.rs        -> Document { sections, keys, spans }   per file, no evaluation
    |                  every value becomes an unevaluated Expr with a source span
 2  cascade.rs      -> ThemeSpec { HashMap<TokenId, (Expr, Span)> }
    |                  five ordered sparse overlays (§4.1); later replaces earlier
    |                  WHOLE-NODE: there is no partial merge of a value
 3  resolve.rs      -> Resolved { colors: Vec<Color>, scalars: Vec<f32>, flags }
    |                  memoised DAG walk, three-colour marking, depth cap 32,
    |                  per-token cycle fallback to default's expression
 4  bake.rs         -> ResolvedTheme (geometry final, colours still linear)
    |                  u = f(screen_h, unit_pct_h, min, max, ui_scale)
    |                  every length -> absolute px; strokes rounded; aliases folded;
    |                  ratios multiplied out; class x state materialised
 5  encode.rs       -> ResolvedTheme, encoded for the live swapchain format (M9)
    |
 6  enforce.rs      -> the same, corrected IN THE LIVE ENCODING
    |                  pass A: contrast floors (ensure)
    |                  pass B: perceptual separation repair (deterministic)
    |                  pass C: contrast floors again  (converges; golden-tested)
    |                  pass D: honesty lints (advisory or corrective per §4.5)
 7  Arc<ResolvedTheme> + Arc<ThemeDiagnostics> published, epoch += 1
```

**Enforcement runs after encode, and that ordering is load-bearing.** §4.4 measures a
translucent foreground *composited over its reference background*, and the GPU does
that compositing on the values in the swapchain's own encoding, not in linear light
(`SRC_ALPHA / ONE_MINUS_SRC_ALPHA` in `fs_main`, over sRGB-encoded values on the
`FormatKind::Unorm` path). Enforcing against a linear `over()` would guarantee a
contrast ratio for a pixel the renderer never produces. `enforce.rs` therefore
composites with `composite_as_rendered(fg, bg)` — an internal engine routine that
mirrors the live blend equation in the live encoding, **not** a fifteenth derivation
function and not authorable (§6). `over()` remains the authoring/derivation function
and stays linear.

Steps 1–3 run **once per theme load**. Step 4 re-runs on load, on resize, and on a
density/ui-scale change — never per frame. Steps 5–6 re-run on `set_color_depth` /
swapchain rebuild as a pair; enforcement is colour-only and costs < 0.4 ms, which is
what makes re-running it on a format change affordable. Measured target for 1–6:
**< 5 ms** for the ≈2 190 addressable tokens of §5. There is no file watcher by
default (`[meta] watch = false`; `--check-theme` and `--dump-theme` in §9.5 are the
supported authoring loop, and the `watch` key's comment in `default.theme` tells an
author to turn it on while editing).

`ResolvedTheme` is pure POD and carries **no** strings and no `Vec` (§7.1). The theme's
name table and its warning list live in a separate `Arc<ThemeDiagnostics>` published
beside it, so the POD guarantee is a property of the type rather than a promise about
how it is used.

Moods and contrast variants multiply step 2–6: each declared variant re-runs the
cascade with its overlay appended and produces its own complete `ResolvedTheme`.
Switching is `self.active = i` — one store, no recomputation, no per-draw branch.

### 2.3 Where the C ABI boundary sits

Three tiers, in order of preference:

1. **Statically linked widgets** (the four core widgets are rlibs as well as `.so`)
   read `nacelle::theme::resolved() -> &'static ResolvedTheme` and index Rust structs
   directly. No call, no copy, no marshalling. **The abstraction must never force the
   slow path on the fast case.**
2. **dlopened plugins, hot path**: `HostApi::theme_snapshot(ctx) -> *const ThemeC`,
   a `#[repr(C)]` POD with a leading `{version, size, epoch}` header, valid for the
   frame. The plugin dereferences fields. Zero calls per draw.
3. **dlopened plugins, cold path**: `theme_color(ctx, id) -> ColorC` /
   `theme_scalar(ctx, id) -> f32` with append-only numeric token ids, plus the
   **recipe calls** `surface()`, `glow()`, `shape()`, `icon()`, `badge()`,
   `text_role()`, `measure_role()`. Recipes exist so the *drawing recipe* (a 9-slice, a
   corner ring, an icon's layer stack, a small-caps run, a severity pill) can change in
   any release without breaking a compiled plugin.

**Typography and severity cross the ABI as roles and indices, never as px and hex.**
`HostApi::text`'s `(font, px, colour, spacing)` signature cannot express `case`,
`tracking`, `smallcaps_ratio`, `synthetic_bold` or `tabular`, so every one of §5.16's
typographic properties would be unreachable from `shell`, `keyboard`, `filesystem` and
`control` — that is every terminal tab label, every key cap, every file name. The
role-aware pair `text_role`/`measure_role` resolves all of them host-side from
`roles[role]`, and `badge()`/`severity_style()` do the same for §5.10's seven roles.
The raw `text`/`measure`/`module_title` entries survive at their existing offsets,
deprecated, on exactly the grounds of [CONFLICT 16].

Full declarations in §7.4.

---

## 3. FILE FORMAT

### 3.1 Justification, in three sentences

A TOML-shaped surface syntax is chosen because it is familiar to every author,
syntax-highlights correctly in every editor without a plugin, and — unlike INI — has
nesting, arrays and typed values, which the ANSI palette and the per-component
sub-sections require. It is parsed by a hand-written recursive-descent parser rather
than the `toml` crate because the crate cannot type-check the domain (colour
expressions, `@` references, units, `extends`), produces generic errors without the
`file:line:col` the diagnostics need, and would pull in serde for a format whose
grammar we deliberately want *smaller* than TOML's, not larger. JSON is refused
outright because it has no comments, and `default.theme` is required to be
documentation as much as configuration.

Extension: **`.theme`**. The old CSS costume is deleted — it promised a cascade,
selectors and inheritance that nacelle will never implement, which is worse than an
honest bespoke format.

### 3.2 Grammar (EBNF)

```ebnf
document      = { line } ;
line          = [ section | keyval | include ] [ comment ] newline ;
comment       = "#" { any-char-but-newline } ;

section       = plain-section | overlay-section ;
plain-section = "[" path [ ":" state ] "]" ;
overlay-section = "[" ( "mood" | "variant" ) "." ident "]" ;
path          = ident { "." ident } ;
state         = "idle" | "hover" | "press" | "selected" | "selected_hover"
              | "dragging" | "disabled" ;

include       = "@include" ws string ;      (* relative to this file's directory *)

keyval        = key ws* "=" ws* value ;
key           = path [ index ] [ locale ] ;   (* dotted keys are legal inside a section *)
index         = "[" digit { digit } "]" ;     (* term.ansi[4], data.series[0] *)
locale        = "[" lower lower [ "_" upper upper ] "]" ;   (* name[pl], name[pt_BR] *)

value         = colour | scalar | ratio | bool | enum-word | string
              | array | reference | call | codepoint | quoted-value ;
quoted-value  = '"' value '"' ;   (* re-lexed as the target token's type; see below *)

(* ---- colour ---- *)
colour        = hex | fn-rgb | fn-oklch | ( ( reference | call | hex ) alpha-suffix ) ;
hex           = "#" ( 3 | 4 | 6 | 8 ) hexdigit ;   (* #RGB #RGBA #RRGGBB #RRGGBBAA *)
fn-rgb        = "rgb(" num sep num sep num [ "/" num ] ")" ;
fn-oklch      = "oklch(" num sep num sep num [ "/" num ] ")" ;   (* L 0..1, C, H deg *)
alpha-suffix  = ws "/" ws num ;               (* sugar for alpha(x, num) *)
sep           = ws | "," ws* ;

(* ---- numbers and units ---- *)
scalar        = num unit ;
num           = [ "-" ] digit { digit } [ "." digit { digit } ] ;
unit          = "u"      (* metric units; THE default authoring unit *)
              | "px"     (* device pixels; legal ONLY on *_min_px / *_max_px tokens *)
              | "%"      (* fraction of the host rect on the token's axis; baked 0..1 *)
              | "em"     (* multiple of the owning type role's resolved px *)
              | "deg" | "ms" | "s" | "hz"
              | "ux"     (* DEPRECATED: 1 ux = 1 px at 1440p = 0.13889u; warns *)
              | "vh" | "vw" ;   (* DEPRECATED migration units; warn on use *)
ratio         = num "x" ws reference ;        (* "0.62x @winframe.title_h" *)
bool          = "true" | "false" ;
enum-word     = ident ;                        (* validated against the token's enum *)
string        = '"' { char } '"' ;             (* ONLY for text-typed tokens *)
codepoint     = "U+" hexdigit { hexdigit } ;   (* icon layer glyphs *)
array         = "[" [ value { sep value } [ "," ] ] "]" ;   (* newlines allowed *)

(* ---- references and calls ---- *)
reference     = "@" path [ index ] ;           (* resolved against the MERGED tree *)
call          = fn-name "(" value { sep value } ")" ;
fn-name       = "alpha" | "fade" | "mix" | "over" | "shade" | "tint" | "lum"
              | "lum_min" | "lum_max" | "sat" | "hue" | "ramp"
              | "contrast_on" | "ensure" ;

ident         = lower { lower | digit | "_" } ;
```

**Rules the grammar implies and the parser enforces.**

* **Quotes are optional everywhere and mandatory nowhere except `text`.** A
  double-quoted value is **re-lexed as the target token's type**: `accent = "#FF2A35"`
  and `accent = #FF2A35` are the same theme, and so are `mode = "hue"` / `mode = hue`,
  `fill = "alpha(@severity.critical.text, 0.18)"` / `fill = alpha(...)`. Quotes are
  *required* only for genuinely textual tokens — `meta.name`, `meta.description`,
  `meta.author`, `meta.base`, `face.*.family`, `face.*.file`, `face.icon.codepoints`,
  `num.decimal_sep`, `num.group_sep`, `type.suffix.brackets`, `backdrop.image`,
  `ornament.dump.alphabet`, `mood.*.when` — because there the quoted bytes *are* the
  value. Every example in this document is written **unquoted** except those; an author
  who quotes anyway gets `value quoted where a <type> is expected — quotes ignored
  (note)` and the correct result. This rule exists because the previous draft of this
  grammar listed `string` as a value type coexisting with `colour`, which made the one
  complete worked example (§3.4) a dozen type mismatches.
* **Inside an overlay section every key is an absolute token path.** `[mood.alert]` +
  `palette.accent` is `palette.accent`, **not** `mood.alert.palette.accent`. This is the
  one exception to the concatenation rule below, and it is the whole point of an
  overlay: a mood and a variant are sparse re-declarations of the root tree. The three
  keys that *do* bind to the mood itself are `inherit`, `wash` and `when` (§5.24); they
  are reserved inside `[mood.<m>]` and may not be shadowed by a token of the same name.
  A key inside an overlay that resolves to `mood.<m>.<something>` is reported as
  `key inside a mood overlay must name a top-level token — did you mean
  "palette.accent"?`
* **Units are mandatory on every length.** A bare `8` where a length is expected is a
  diagnostic, not "8 px". This is the single rule that stops raw pixels creeping back
  in. Dimensionless counts (`chart.grid.rows = 4`, `donut.segments = 48`) and
  multipliers (`metric.density_space = 0.9`) have no unit by construction and take a bare
  number.
* **`px` is legal only on a token whose name ends `_min_px` or `_max_px`.** There is no
  `min` keyword and no arithmetic in the language: a floored length is **two tokens**,
  `X` and its companion `X_min_px` (or `X_max_px`), each declared and commented
  separately in `default.theme`. `a11y.min_hit = 4.8u` + `a11y.min_hit_min_px = 24px`
  is discoverable by the same prefix search as everything else, and an author who
  overrides `X` no longer silently loses a floor that was hidden in prose. §5.25 lists
  both rows for every floored token.
* **One reference sigil.** `@` addresses every token, at every layer, including
  `@palette.accent` and `@grad.focus`. There is no separate gradient sigil and no
  separate gradient production: `@grad.focus` is a reference like any other, and the
  type checker rejects it where a gradient is not expected by the same mechanism that
  rejects every other type mismatch. [CONFLICT 1]
* **A `ratio`'s right-hand side is a reference**, so it is never ambiguous:
  `menu.pad = 0.35x @menu.row_h`. A bare path after `x` is accepted for one release,
  resolved **first against the current section's namespace, then absolutely**, and
  warned: `ratio target "row_h" is relative — write "@menu.row_h"`. The referenced
  token must be a length; naming an enum (the old `0.27x container`) is a type error.
* **`%` is a fraction of the token's host rect on the axis named by its suffix** —
  `_w`/`_x`/`_frac` ⇒ width, `_h`/`_y` ⇒ height — unless the token's row in §5.25
  names a different parent (`border.bracket_len` is of the shorter side;
  `filetile.caption_gap` is of the caption block's height). Every row that uses `%`
  states its parent in the `note` column. **`%` is also an accepted spelling for a
  `frac` token**: `decor.vignette.strength = 55%` and `= 0.55` bake identically, so the
  two spellings stop being a type error.
* **One alpha form beyond `#RRGGBBAA`:** the `/ n` suffix, matching CSS Color 4 and
  `oklch(L C H / a)`. `@accent.primary / 0.55` ≡ `alpha(@accent.primary, 0.55)`.
  [CONFLICT 11]
* Case-insensitive hex; `#RGB`/`#RGBA` expand by digit doubling. Alpha in a literal
  is **straight**, never premultiplied.
* Whitespace is insignificant except that a newline ends a key/value.
* Dotted keys inside a *plain* section concatenate: `[panel]` + `title.band_h = 4.6u`
  is `panel.title.band_h`.
* **An indexed token may be written whole or per slot, and the two mean different
  things.** `term.ansi = [ ... ]` (16 values) **replaces the generated row and disables
  the pull/clamp generation rules of §5.11 entirely** — the author now owns all sixteen.
  `term.ansi[4] = #2A7FE0` pins one slot and leaves the other fifteen generated. The
  same holds for `data.series`. Writing an array of the wrong length is a malformed
  value; writing an index past the end is an unknown key.
* `@include` is depth-capped at 4 and may not escape the theme's own directory tree
  (no `..`, no absolute paths). Its only intended use is splitting a large theme into
  `colors.theme` / `material.theme` / `type.theme`.

### 3.3 The metadata header

Always first, always at the top of the file (KDE buries localised names at the bottom
because KConfig sorts sections alphabetically — do not repeat that).

```ini
[meta]
schema        = 1                # engine schema; a mismatch runs a migration, never fails
name          = "Aurora"
name[pl]      = "Zorza"
description   = "Mint console, reference image 1."
description[pl] = "Miętowa konsola, obraz referencyjny 1."
author        = "nacelle"
family        = console          # enum, unquoted: console | holographic — advisory
base          = "default"        # text: inherit from this theme instead of default; depth 8
strict        = false            # true: unknown keys are reported as errors (still loads)
watch         = false            # inotify reload; off by default, never polls.
                                 # Set true while hand-editing; ship it false.
```

`name`, `description`, `author` and `base` are `text` and are quoted; `family`,
`strict`, `watch` and `schema` are not, per §3.2's quoting rule.

`[meta]` is the one string-bearing part of a loaded theme, and **it does not live in
`ResolvedTheme`**: it is published as `Arc<ThemeDiagnostics>` beside the POD struct
(§7.1), so no draw path can reach it and no locale count can move `ResolvedTheme`'s
size.

### 3.4 Worked example — the entire `lockdown` theme

This is a complete, shippable theme file. It reproduces reference image 5 (red chrome,
blue hologram, an all-red alarmed console with an amber `CONTAINED` badge) by writing
**thirteen** substantive lines. Every value is written **unquoted**, per §3.2; every
key inside `[mood.alert]` is an **absolute token path**, per §3.2.

```ini
[meta]
schema = 1
name = "Lockdown"
name[pl] = "Blokada"
description = "Red chrome, blue data. Reference image 5."
family = console

# ---------------------------------------------------------------- palette
# The five seeds. Everything else in the interface derives from these.
[palette]
black   = #08060B      # target of shade(); MUST be a literal
white   = #FFEDEB      # target of tint();  MUST be a literal
accent  = #FF2A35      # chrome: borders, titles, leader lines, MISSION LOGS
data    = #35A7FF      # THE WHOLE POINT OF IMAGE 5: plots, wireframe, planet, orbits
neutral = #74707E      # hue-free grey anchor for offline / disabled

# ---------------------------------------------------------------- severity
# Mode hue keeps canonical severity hues, pulled 0.16 toward the accent and
# clamped to +/-14 deg. Two roles are pinned because image 5 names them.
[severity]
mode = hue
contained.text = #E8B33A   # image 5's amber CONTAINED, inside an all-red screen
warning.text   = #FF7A00   # forced apart from contained; see the dE floor in 4.4

# ---------------------------------------------------------------- render
# Image 5's central render: a grey hull with a red rim, blue wireframe, blue
# orbits. The wireframe and the orbits already follow palette.data through
# @data.line; only the hull and the rim need saying.
[render]
hull = sat(@data.line, 0.0)
rim  = @accent.primary

# ---------------------------------------------------------------- decoration
[decor]
enabled = true
[decor.traces]                 # image 5: red PCB traces on black
enabled = true
color   = @accent.primary
alpha   = 0.08
[decor.vignette]
enabled  = true
strength = 0.55

# ---------------------------------------------------------------- moods
# The launcher's SYSTEM LOCKDOWN button. A mood is a sparse overlay resolved
# into its own complete sibling theme at load and selected by index.
# EVERY KEY HERE IS AN ABSOLUTE TOKEN PATH. "wash" is one of the three keys
# (inherit, wash, when) that bind to the mood itself.
[mood.alert]
motion.alarm_blink.enabled = true
component.alarm_bar.fill   = alpha(@severity.critical.text, 0.18)
glow.panel_edge.enabled    = true
glow.panel_edge.radius     = 1.4u
wash = #FF2A35 / 0.22
```

Everything else — the other ≈2 175 tokens: six surface levels, seven text roles, five
borders, nine accents, twelve data tokens, twenty-eight severity members, twenty-two
terminal colours, ninety component colours, twenty-four type roles, sixteen shapes,
every metric, every state cell — is inherited from `default` **as an expression** and
re-derived in the new hues. That is the mechanism, and it is the whole point.

Two things this example deliberately does **not** contain, both of which an earlier
draft got wrong and both of which the format now catches:

* `palette.accent = #FF2A35` inside `[mood.alert]`. It is a no-op — `[palette] accent`
  already holds that value — and `--dump-theme` (§9.5) reports overlay keys that
  resolve to the value they already had as `note   mood "alert": palette.accent
  re-states the inherited value`.
* `alarm_bar.fill`. There is no such token; the component colour is
  `component.alarm_bar.fill` (§5.26). Written bare inside an overlay it is an unknown
  key, and §4.2's rename/alias table maps it explicitly so the diagnostic names the
  right key rather than guessing by edit distance.

---

## 4. THE CASCADE

### 4.1 Resolution order — exactly five stages

```
 1  compiled-in fallback   const FALLBACK: ResolvedTheme   (Rust, cannot be edited)
 2  default.theme          include_str!, parsed dense: every token Some(Expr)
 3  the selected theme     parsed sparse: Option<Expr> per token
       3a  ... and its [meta] base chain, resolved depth-first first, cap 8
 4  the active [mood.<m>] / [variant.<v>] overlay(s) from the same document
 5  the user overlay       ~/.config/nacelle-desktop/theme.local
```

Within a stage the last declaration of a token wins. Across stages the later stage
wins. **An override replaces a whole node — there is no partial merge of a value.**
Because `default`'s entries are *expressions*, a theme that sets only
`palette.accent` re-derives every dependent token correctly; this is the entire
mechanism behind images 2/3/4/6 being one layout in four hues, and it is the fix for
libadwaita's documented footgun (CSS variables store substituted text and have no
dependency graph, so `--accent-color` "must be manually overridden as well").

Stage 1 exists so that stage 2 itself failing — a corrupted binary, a bad edit during
development — still yields a running program. `FALLBACK` is `default`'s resolved
output, generated by a build-time test and checked in; a unit test asserts that
resolving `default.theme` reproduces it byte for byte.

Mood order: `[mood.<m>]` applies before `[variant.hc]`, so high contrast always wins
over an alarm's decoration. Moods may declare `inherit = "<other mood>"` (depth 8).

**Variant/mood cap: 8 resolved siblings per theme.** Declaring more is reported and
the extras are dropped (not a load failure). Eight is
`{normal, alert, lockdown, one spare} × {plain, high_contrast}` and there is no ninth
thing a mood should be.

**Every inheritance mechanism in the engine, in one table.** There are five, they do
different things, and an author should never have to reconstruct that from five
sections. All depth caps are **8** except `@include`, which is 4 because it is a
file-level splice and a deeper one is a directory layout problem rather than a theme.

| mechanism | syntax | depth | merges | on cycle / overflow |
|---|---|---|---|---|
| file splice | `@include "colors.theme"` | 4 | the included file's lines, in place | warn naming the chain; the include is skipped |
| theme chain | `[meta] base = "aurora"` | 8 | a whole parent document, resolved depth-first before this one | warn naming the chain; the chain restarts at `default` |
| mood chain | `[mood.alert] inherit = "normal"` | 8 | one whole overlay node into another | warn naming the chain; `inherit` is dropped |
| face chain | `face.<f>.fallback = ui` | 8 | one face's resolution attempt into the next | warn; the face aliases to `FACE_UI` / `FACE_MONO` (§5.16) |
| value sentinel | `panel.content_pad_x = same_as_parent` | n/a | one value from its named parent token | not possible: the parent is fixed per token |

The word `inherit` used to mean three unrelated things — a mood key, a scalar
sentinel and a glow colour sentinel. It now means exactly one: **the mood/base
chain**. The scalar sentinel is `same_as_parent` (`panel.content_pad_x`,
`slider.fill_h`, `keyboard.pad`) and the glow colour sentinel is `element`
(`glow.<C>.color = element` — "the colour of the thing being glowed"), which is
additionally no longer stored in an alpha channel at all (§5.13).

### 4.2 The error policy, stated once

> **This program must never fail to start because a theme file is wrong.**
> Every recoverable defect degrades to `default`'s value for the affected token and
> is reported with file, line, column, the offending text and the reason.

Reports go to four places: stderr once at load; `ThemeDiagnostics.warnings:
Vec<String>` — a **separate** `Arc` published beside the POD `ResolvedTheme`, never a
member of it (§7.1); the settings panel, which shows the list under the theme's name;
and `nacelle-desktop --check-theme <path>` (§9.5), which is the loop a hand-author
actually uses. **Silent substitution is forbidden** — it is the failure mode this
project already calls out as the worst kind.

| condition | behaviour |
|---|---|
| **theme file missing / unreadable** | `default` is used; one line naming the path. |
| **missing key** | Inherit stage 2's expression. **Silent — this is the feature.** |
| **renamed / removed key** | Warn + ignore, resolved through the **static alias table below**, which is consulted *before* edit distance. The message names the new key and **the value the token actually holds**, because that is the fact that connects the warning to the wrong-looking pixel. |
| **unknown key** | Warn + ignore. Message names the key, the nearest known token by Levenshtein distance ≤ 3, **and that token's resolved value** (`unknown key "panel.boarder" — did you mean "panel.border"? (ignored; panel.border = 1px)`). Under `[meta] strict = true` it is logged at error level; **the theme still loads.** [CONFLICT 10] |
| **unknown function** | Warn + fall back to `default`'s expression for that token. Names the function and lists the 14 legal names. [CONFLICT 10] |
| **malformed value** (bad hex, missing unit, wrong arity, enum word not in the enum) | Warn + fall back to `default`'s expression for that token. Message states the expected type/unit/enum. |
| **type mismatch** (a colour expression assigned to a scalar token) | Same as malformed. |
| **reference to an unknown token** | Warn + fall back to `default`'s expression for the *referring* token. |
| **reference cycle** | Warn with the **full path** (`accent.hover → text.title → accent.hover`), then evaluate the token using `default`'s expression. If that also cycles (impossible for the shipped default, asserted by a unit test) the token takes its `FALLBACK` value. [CONFLICT 9] |
| **depth > 32** | Treated as a cycle. |
| **missing `[meta] base` parent** | Warn; the chain restarts at `default`. |
| **`base` chain cycle or depth > 8** | Warn naming the chain; the chain restarts at `default`. |
| **state defined on a class that cannot enter it** (§5.27) | Warn + ignore the cell. |
| **contrast / separation floor violated** | Corrected by `ensure()` / the repair pass and logged with before/after hex (§4.4). |
| **a theme sets a non-themeable token** (terminal tracking, terminal case, `image.photo.tint_strength` from a mood, `glow.mode = bloom_pass`) | Warn + ignore. |
| **more than 8 variants**, **> 3 icon layers**, **> 8 gradient stops**, **> `motion.idle_cap` cyclic sources** | Warn + the excess is dropped/frozen at its mean. |
| **a key inside `[mood.<m>]`/`[variant.<v>]` that resolves to `mood.<m>.<x>`** | Warn naming the intended top-level token (§3.2). |
| **a value quoted where a non-`text` type is expected** | **Note**, not a warning: the value is re-lexed as the target type and used. §3.2. |

**The static rename/alias table**, consulted before edit distance. Levenshtein ≤ 3
cannot find `panel.content_pad` from `panel.pad` (distance 8), and those are exactly
the mistakes this specification itself creates. Each entry carries its own message.

| written | resolves to | message |
|---|---|---|
| `panel.pad` | `panel.content_pad` | `renamed in schema 1 — the user's GridPadding owns "pad"` |
| `alarm_bar.*` | `component.alarm_bar.*` | `alarm bar colours live in the component layer` |
| `severity.<r>.fg` | `severity.<r>.text` | `colour members are fill / edge / text / glyph / on (5.0)` |
| `severity.<r>.bg` | `severity.<r>.fill` | " |
| `severity.<r>.border` | `severity.<r>.edge` | " |
| `icon.size_xs` … `icon.size_launcher` | `icon.size.xs` … `icon.size.launcher` | `ladders are dotted (5.0)` |
| `shape.button.alt` | `shape.button_alt` | `preset variants are underscored (5.0)` |
| `suffix.*` | `type.suffix.*` | `the status suffix is a type role, not a family` |
| `glow.strength_scale` | `glow.alpha_scale` | `glow has alpha (0..1) and boost (>1); "strength" was neither` |
| `metric.unit_vh` | `metric.unit_pct_h` | `"vh" is a deprecated unit; this token is a percentage` |
| `density.space` / `density.type` | `metric.density_space` / `metric.density_type` | `the family is metric.*` |
| `layout.*`, `panel.gutter`, `board.pad*`, `column.min_w`, `column.max_w`, `breakpoint.*`, `controlbar.h` | — | `cut: a theme may not move a panel rectangle (scope-boundary.md, CONFLICT 4)` |
| `Look=` , `Style=` (in `.conf`) | `Theme=` , — | §9.3 |
| `<n>ux` | `<n × 0.13889>u` | `deprecated unit` |
| `<n>vh` / `<n>vw` | `u` | `deprecated migration unit` |

Every rename this document performs is in that table. Adding a rename to the catalogue
without adding it here is a review failure, because the two together are the only
reason an author's muscle memory degrades into a message rather than into silence.

### 4.3 Diagnostics format

Parse, type and reference diagnostics print **the source line with a caret under the
offending span** — the parser already keeps the span (§2.2) and throwing it away at the
print site is the one place this engine would be stingy for no reason. Enforcement
notes carry `file:line:col` of the **winning declaration** of the value they changed;
where the value was derived rather than written, they name the derivation site instead.

```
theme "lockdown" (/home/u/.local/share/nacelle-desktop/themes/lockdown.theme)

  lockdown.theme:15:12  warn  unknown key "panel.boarder" — did you mean
                              "panel.border"? (ignored; panel.border = 1px)
     15 | panel.boarder = @accent.primary
        |       ^^^^^^^

  colors.theme:41:18    warn  expected a length with a unit, found "8"
                              (using default: panel.content_pad = 2.8u -> 15px)
     41 | content_pad = 8
        |               ^

  lockdown.theme:63:3   warn  reference cycle:
                              accent.hover -> text.title -> accent.hover
                              (accent.hover uses default's expression:
                               tint(@accent.primary, 0.18) -> #FF5A62)
     63 | hover = @text.title
        | ^^^^^

  lockdown.theme:34:18  note  severity.warning lifted #FF7A00 -> #FF8F48
                              (dE 0.096 -> 0.127 vs contained)
  default.theme:412     note  ansi[4] lifted #1F76DC -> #2A7FE0 (derived)
                              (contrast 4.31 -> 4.58 vs term.bg)
  default.theme:1180    note  face "display": "Orbitron" not installed
                              — using "ui-bold" + synthetic bold
  lockdown.theme:52:1   note  mood "alert": palette.accent re-states the
                              inherited value (no effect)
```

### 4.4 Load-time enforcement — mandatory, in this order

**These passes run after `encode.rs`, on the values the GPU will actually blend**
(§2.2). Compositing uses `composite_as_rendered(fg, bg)`, which mirrors the live blend
equation (`SRC_ALPHA / ONE_MINUS_SRC_ALPHA`) **in the live encoding** — sRGB-encoded on
the `FormatKind::Unorm` path, linear on `ScRgbLinear`. It is an internal engine routine
and not one of §6's fourteen authorable functions. The linear `over()` remains the
*authoring* composite and is what a theme writes; it is not what enforcement measures,
because the two differ by up to a full 8-bit step on the near-black console surfaces
and by considerably more for `surface.scrim` (α 0.66) over a lit family-B backdrop —
which is precisely where an AAA floor is load-bearing. Where §5.5 quotes
`#15201B / 0.82` over `#0B1310` as `#131E1A`, that is the as-rendered value; the
authoring `over()` gives `#141F1A`. Both numbers are correct for their own question and
the spec now says which is which.

**Pass A — contrast floors.** WCAG 2.x relative-luminance ratio, computed after
compositing any translucent foreground over its reference background with
`composite_as_rendered`. Reference background for text is `surface.panel` composited
over `surface.base` — the real thing text sits on — except `text.instrument`, measured
on `surface.base`. A colour below its floor is walked away from the background in
OKLCh lightness (hue and chroma held, 48 bounded steps, then clamp) until it passes.

| token | floor | why |
|---|---|---|
| `text.primary` | **7.0** (AAA) | terminal output and numeric values at small monospace sizes. This program is a terminal; AA is not enough. |
| `text.title`, `text.secondary` | 4.5 | short tracked small caps at 11 px is not WCAG "large text"; no relaxation. Labels carry meaning. |
| `text.muted` | 3.0 | deliberate sub-AA; see the exception below. |
| `text.disabled` | 2.0 | WCAG 1.4.3 exempts inactive controls; the floor stops "inert" becoming "invisible". |
| `text.instrument` | 1.6 | exempt by definition; the floor keeps it visible *as texture*. |
| `severity.*.text` (6 roles) | 4.5 | `CRITICAL` must be readable. |
| `severity.offline.text` | 3.0 | the one state that should recede. |
| `severity.*.on`, `accent.on`, `text.inverse` | 4.5 vs their own fill | text on a solid chip. |
| `accent.primary`, `data.line`, borders, glyphs | 3.0 | WCAG 1.4.11 non-text contrast. |
| `term.fg` vs `term.bg` | 7.0 | measured 15.19–15.52 across the six original console seeds (§8). |
| `term.ansi[1..6]` vs `term.bg` | 4.5 | measured min 4.56. |
| `term.ansi[9..14]` vs `term.bg` | 7.0 | |

**Pass B — perceptual separation repair.** Contrast against the background is not
sufficient: two severities can both clear 7.0 and be indistinguishable from each
other. Minimum OKLab ΔE between colours that must be told apart:

| set | ΔE floor | calibration |
|---|---|---|
| `term.ansi[1..6]` | 0.11 | shipped palettes measured: VGA 0.119, tango 0.111, dracula 0.131 |
| `term.ansi[9..14]` | 0.09 | |
| `data.series[0..7]` | 0.10 | tab10 0.113, Okabe-Ito 0.156, Set2 0.090 |
| `severity.*.text` | 0.115 | |

Repair is deterministic and bounded. Severity roles are placed in priority order
`critical → warning → ok → info → contained → offline → unknown`; **earlier roles
never move** (critical is anchored to the theme's alarm colour). A later role landing
within ΔE of a placed one steps its OKLab lightness away from the collider by 0.018
per step (max 14 steps) and, if that is not enough, rotates hue in 4° steps (max
±28°), re-applying `ensure()` after every candidate and keeping the best. Before this
pass existed, four of six console palettes had severity pairs at ΔE 0.05–0.10 —
visually the same colour.

**Pass C — contrast floors again.** One extra pass suffices; convergence is asserted
by a golden test over a fixture corpus (originally specified over the eight shipped
themes, which were removed 2026-08-16).

**Pass D — honesty lints.** Advisory unless marked corrective:

1. **No two severity roles may be told apart by hue alone.** Every pair must differ by
   OKLab ΔL ≥ 0.08, or a different `glyph` index, or a different `badge_style`.
   *Corrective*: the failing role's `glyph` is reset to the default table.
2. **`state.disabled` may not exceed `state.idle` on any channel.** A disabled control
   that looks more available than an enabled one is a lie the engine can catch
   arithmetically. *Corrective*: the channel is clamped to `idle`.
3. **A MOOD may not falsify a photograph.** `component.image.photo.tint_strength` and
   `component.image.photo.saturation` may not be changed by a `[mood.<m>]` overlay.
   *Corrective*: the override is dropped, named in the log. A **theme** may set both,
   inside the ceilings stated in §5.26 (`tint_strength ≤ 0.35`, `saturation ≥ 0.35`),
   because image 3 requires exactly that — its `LIVE FEED: SECURITY B-16` photo is
   "desaturated toward green" — and §6 cites that same image as the justification for
   keeping `sat()`. A lint that makes one of the ten reference images unreproducible is
   a lint about the wrong thing. The defensible half survives intact: **an alarm must
   not recolour evidence.** The engine cannot tell a photograph from a render; it
   distinguishes `image.photo.*` from `image.render.*` by which token the caller
   reaches for, so this stops honest callers only — which is stated here rather than
   pretended away.
4. **APCA Lc advisory.** Lc is computed for every text pair and reported when below
   the advisory minimum (term fg 90, primary 75, secondary 60, muted/hint/display 45,
   disabled 25–40, non-text edges 25). It is *reported*, not enforced. [CONFLICT 6]
5. **Hue-collision advisory.** `severity.warning` within 20° of `accent.primary` in
   OKLCh is reported; a theme may legitimately want it (image 4 does).

**Deliberate exceptions, and the widget-contract rules they depend on.**

* `text.muted` at 3.0 rather than 4.5. Used for captions, axis labels, legends and
  leader-line labels; the images depend on this dimness for depth. **Binding widget
  rule: no widget may place unique, unrecoverable information in `text.muted`.**
* `text.instrument` at 1.6, fully exempt. The hex dumps in image 9's margins are
  texture, not information. **Binding widget rule: no widget may render user- or
  system-supplied content in `text.instrument`.**
* The engine cannot enforce either rule. Both are restated in the widget contract, or
  the exceptions are unjustified.

### 4.5 What the engine states plainly and does not pretend to fix

Colour-vision-deficiency separation of seven hue-coded severities is **poor**.
Simulated minimum ΔE across the seven roles, measured on the original shipped
palettes (kept as a record — the variants left the binary 2026-08-16):

| theme | protan | deutan | tritan |
|---|---|---|---|
| aurora | 0.056 | 0.060 | 0.110 |
| pure | 0.030 | 0.053 | 0.077 |
| crimson / lockdown | 0.019 | 0.067 | 0.107 |
| azure | 0.020 | 0.024 | 0.108 |

Seven hue-coded roles cannot be made dichromat-safe; that is a property of the colour
space, not of these palettes. Two consequences, both binding:

1. **Severity is always rendered as colour + glyph + label.** No widget may encode
   severity by colour alone. This is what images 1–6 already do. **The plumbing that
   makes this possible is normative and is specified in three places, because a rule
   with no channel to travel down is decoration:** `HostApi::badge()` and
   `severity_style(ctx, i)` for dlopened widgets (§7.4), `Ctx::severity(i)` for
   statically linked ones, and a `severity:` key on `rows`, `meter`, `text`, `badge`
   and table cells for script widgets (§5.29). §5.10 names which existing call sites
   become which severity index.
2. `severity.mode = mono_strict` (an evenly spaced single-hue OKLab ladder, no hue
   nudge) scores ≈0.080–0.086 under all three dichromacies versus 0.019–0.067 for hue
   mode, at the cost of dropping to ≈0.085 for normal vision. It ships as a mode. On
   a **red** accent it still degrades under protanopia (0.038), because protanopes
   lose red luminance — a red-accented theme cannot be made protan-safe by brightness
   either. Stated, not hidden.

---

## 5. THE TOKEN CATALOGUE

This is the contract. Every token below exists in `default.theme` with a comment.

**Two columns, and they are not the same thing.** Tables give **full dotted name ·
type · `default.theme` source · resolved value · what draws it · reference image**,
where *source* is the text literally in the master file and *resolved* is what it
evaluates to for the mint seed. Where a row's source shows a derivation
(`alpha(@accent.primary, 0.55)`), **`default.theme` contains that expression and never
its resolved hex.** That is binding on whoever writes the file, and it is the entire
reason overriding `palette.accent` re-derives the leaves (§1.1(1), §4.1). Where a row's
source shows a literal, the value really is authored and really does not move — the
five palette seeds, `severity.*` pins, `type.*` size steps. A table that quotes one
value quotes the *source*; a header that says "resolved" quotes the other side.
`px` columns are 1920×1080 (`u` = 5.4 px). §5.0b shows what the file itself looks like.

**Total: ≈2 190 addressable tokens**, by family:

| family | § | tokens | | family | § | tokens |
|---|---|---:|---|---|---|---:|
| `meta` | 5.1 | 8 | | `face` + `type` | 5.16 | 340 |
| `palette` | 5.2 | 6 | | `num` + `type.suffix` | 5.17 | 20 |
| `metric` | 5.3 | 7 | | `shape` | 5.18 | 293 |
| ladders | 5.4 | 30 | | `icon` | 5.19 | 89 |
| `surface` | 5.5 | 7 | | `ornament` | 5.20 | 62 |
| `text` | 5.6 | 7 | | `state` + `focus` | 5.21 | 62 |
| `border` | 5.7 | 12 | | `motion` | 5.22 | 146 |
| `accent` | 5.8 | 10 | | `a11y` | 5.23 | 14 |
| `data` | 5.9 | 12 | | `mood` | 5.24 | 3 + sparse |
| `severity` | 5.10 | 60 | | component metrics | 5.25 | ≈425 |
| `term` | 5.11 | 30 | | `component.*` colours | 5.26 | ≈95 |
| `elev` | 5.12 | 217 | | script vocabulary | 5.29 | 34 |
| `glow` | 5.13 | 106 | | | | |
| `grad` | 5.14 | 25 | | | | |
| `backdrop` + `decor` | 5.15 | 61 | | | | |

Of these, ≈174 resolve to colours, ≈470 to scalars; the rest are enums, flags, strings
and structured sub-records. The `class × state` product (64 × 7 = 448 `ClassStyle`
cells) is **derived**, not authored, and is not counted here — a theme writes the
seven-row state ladder of §5.21 once and every class inherits it.

The inventory this catalogue answers found **214 geometric constants** and **96 colour
decisions** across `libnacelle`, `nacelle-desktop` and `nacelle-widgets`. Three drawing
surfaces account for nearly all of them, and each now has a home: the toolkit's own
primitives (§5.25/§5.26), the four dlopened plugins — which reach the same tokens
through the role-aware ABI of §7.4 rather than through raw px and hex — and the **Rhai
script path**, through which eight of the twelve shipped widgets draw *exclusively*
(`clock`, `cpu`, `hardware`, `memory`, `network`, `processes`, `sysinfo`, `uptime`).
That third surface is the largest single one in the program and it has its own section,
**§5.29**. A catalogue that covered the other two and not it would leave two thirds of
the shipped widgets hard-coded after the engine lands, which would falsify goals 1
and 2 on the exact surface they matter most.

### 5.0 Conventions

**Types.** `col` colour · `len` length (u-multiple → px) · `str` stroke (device px,
never panel-scaled, never density-scaled) · `rat` ratio of another token · `frac`
fraction 0..1 · `n` dimensionless count · `f` plain multiplier · `enum` · `bool` ·
`text` string · `list` array · `dur` duration ms · `grad` gradient reference ·
`role` type-role reference · `icon` icon id · `shape` shape-preset reference.

**Three layers, and two more that sit across them.** `palette.*` (raw, author-named,
free-form) → **semantic roles** (closed set, this *is* the ABI) → `component.*` and
per-widget metrics (closed set, each defaulting to an expression over a semantic role).
**A widget never names a literal.** New colours added later go in the component layer
with a default pointing at an existing semantic role — never as a new semantic role,
never as a literal in a widget. `elev.*` (the material ladder, §5.12) and `shape.*`
(the geometry presets, §5.18) are **not a fourth layer**: they are the two consumers
that a drawn container reads, and each of their fields defaults to a component or
semantic token. That is stated here because they are the two largest families in the
catalogue and leaving them outside the architecture is what produced the next problem.

**One owner per drawn property.** *If two tokens could both change one pixel, one of
them is a bug.* Every property below has exactly **one owning token** — the one the
drawing code reads — and a **single default chain** behind it. Every link is
overridable and every link is documented as a link, so an author who edits any of them
sees a change, and `--dump-theme` (§9.5) prints the chain.

| drawn property | owner (read at draw time) | default chain, one link per arrow |
|---|---|---|
| panel edge colour | `elev.panel.edge.color` | → `@component.panel.border` → `@border.default` → `alpha(@accent.primary, 0.55)` |
| panel edge width | `elev.panel.edge.width` | → `@panel.border` → `@border.width` → `@stroke.hair` |
| panel corner radius | `elev.panel.radius` | → `@panel.corner` → `@corner.md` |
| panel corner style | `elev.panel.corner` | → `@panel.corner_mode` |
| panel fill / glass | `elev.panel.fill`, `.glass.tint`, `.glass.wash` | `fill` → `@component.panel.fill` → `@surface.panel` |
| badge / chip / key / tab edge, fill, corner | `shape.<preset>.*` | → `@component.<c>.*` → semantic role |
| any container that has an `Elev` | `elev.<L>.*` | `shape.<same>.border_color/.fill/.border_width` default to `same_as_parent` and **must not be set** — see below |

`shape.*` and `elev.*` do not compete, because they own **disjoint sets of presets**.
Four presets name a container that also has an elevation — `panel`, `card`, `window`,
`modal` — and for exactly those four, `shape.<p>.fill`, `.border_color` and
`.border_width` default to the sentinel `same_as_parent` and read from `elev.*`.
Setting one of them is legal (a theme may want a card whose ring differs from its
material) and produces a **note**, not a warning: `shape.card.border_color overrides
elev.panel.edge.color for this preset`. The other twelve presets are non-elevated and
`shape.*` is their sole owner. `§7.3`'s `t.classes[Class][state].edge` is not a fifth
path: `ClassStyle` is the **baked product** of the state ladder applied to whichever
of the above owns that class's edge, materialised at bake so the draw path is one
index. Nothing authorable lives there.

**The naming law.** Token names must be guessable from one family to the next, or
≈2 190 of them is a reference manual rather than a format.

1. **Colour members of any group are `fill / edge / text / glyph / on`.** Not
   `fg`/`bg`/`border`. This is why `severity.<r>.fg/.bg/.border` are now
   `severity.<r>.text/.fill/.edge` (§4.2 keeps the old spellings as aliases), and why
   `ClassStyle`, `component.badge.*` and `SeverityStyle` all read the same.
2. **Ladders are dotted, variants are underscored.** `space.1`, `size.xs`,
   `corner.sm`, `stroke.hair`, `icon.size.xs`; `shape.button_alt`, `body_dim`.
3. **A multiplier ends `_scale`; a fraction ends `_frac` or takes `%`; a percentage
   token says so** (`metric.unit_pct_h`, not `unit_vh` — `vh` is a deprecated *unit*
   and meant something else).
4. **A floored length `X` carries a sibling `X_min_px` / `X_max_px`.** There is no
   `min` operator (§3.2).
5. **A family that describes a type role lives under `type.*`** — hence
   `type.suffix.*`, not a top-level `suffix.*` with a role-shaped body.
6. **Glow has exactly two magnitudes:** `alpha` (0..1, always) and `boost` (>1, HDR
   only). "Strength" is not a third thing; the state channel is `glow_alpha` and the
   high-contrast scalar is `glow.alpha_scale`.

**Sentinels — one table, one value each, never overloaded onto a colour channel.**
Baked as `f32` so a consumer testing `if v < 0.0` handles all of them:

| word | baked | means | used by |
|---|---:|---|---|
| `none` | `0.0` | absent; draw nothing | `glow.radius`, `shadow.color`, `border.style` |
| `auto` | `-1.0` | the engine picks by a stated mechanical rule | `glow.mode`, `rhythm.label_col`, `filetile.cols` |
| `pill` | `-2.0` | radius = half the box's shorter side | `corner.pill` |
| `same_as_parent` | `-3.0` | copy the named parent token's baked value | `panel.content_pad_x/_y`, `slider.fill_h`, `keyboard.pad`, the four elevated `shape.*` presets |

`inherit` is **not** in this table. It is the mood/base chain keyword and nothing else
(§4.1). The glow colour sentinel that used to be spelled `inherit` and stored as
`color.a = -1.0` — colliding with `auto`, and one type-pun away from putting a negative
alpha into a `Vertex` under `SRC_ALPHA / ONE_MINUS_SRC_ALPHA` — is now the enum word
`element` backed by an explicit `Glow.inherit: u8` flag (§5.13, §7.1). **No sentinel
can reach a vertex.** No `NaN` is ever baked except from `theme_scalar()` on an unknown
id, which is the one legitimate "no answer".

**Three scaling classes**, a property *of the token*, baked into the token table, not
a decision at the call site:

| class | accessor | applies to |
|---|---|---|
| global | `Ctx::gu(v)` | board chrome, anything between panels, plate geometry |
| panel-scaled | `Ctx::u(v)` | everything a widget draws inside its own box |
| stroke | `Ctx::stroke(v)` | every line width: `max(1, round(v * u_global))` |

This fixes a live inconsistency: today `font_px()` includes `panel_scale` and `vh()`
does not, which is why widget text shrinks in a narrow column while its padding does
not.

---

### 5.0b What `default.theme` actually looks like

`default.theme` is asserted three times in this document to be the documentation
(§1.1(8), §8, the owner's brief). Asserting it is not the same as showing it. This is a
verbatim excerpt covering **one literal colour, one derived colour, one scalar with a
range, one enum with its legal words, one indexed token, one state block, and one
floored length with its companion** — the seven shapes every other key in the file
takes.

**The comment format is a lint, not a style suggestion.** Every key carries exactly one
comment line, in this order, `·`-separated:

```
# <what it draws> · <reference image> · <unit / legal range> · <derivation when omitted>
```

A key with no comment, or with a comment missing a field, fails
`cargo test theme_default_is_documented`, which parses `default.theme` and checks every
one of the ≈2 190 keys. That test is the only thing that keeps the file documentation
after its second year.

```ini
# =========================================================================
# palette — the five seeds. LITERALS. Everything else derives from these.
# =========================================================================
[palette]
black   = #0A100E   # shade() target; all images; sRGB hex; literal by rule (5.2)
white   = #EAF6F1   # tint() target;  all images; sRGB hex; literal by rule (5.2)
accent  = #3FE3AE   # the one hue that re-skins the UI; images 1-6; sRGB hex; literal

# =========================================================================
# text — seven roles. DERIVED. Note that not one of these is a hex: change
# palette.accent and every line below re-derives. That is the whole cascade.
# =========================================================================
[text]
title   = oklch(0.870, 0.55x@chroma.accent, @hue.accent)
                    # panel titles, small caps + wide tracking; images 1-10;
                    # OKLCh, L fixed by the ladder in 5.6; floor 4.5 (4.4)
primary = oklch(0.905, 0.15x@chroma.accent, @hue.accent)
                    # the value half of key:value, terminal output; all images;
                    # OKLCh; floor 7.0 AAA — this program is a terminal (4.4)

# =========================================================================
# metric — the one scalar that drives everything.
# =========================================================================
[metric]
unit_pct_h  = 0.5   # u as a percentage of viewport HEIGHT; all; 0.2 .. 2.0;
                    # no derivation — this is the root of every length
unit_min_px = 4px   # the 720p defence; -; device px, 1 .. 32; companion of unit_pct_h
density     = compact
                    # spacing/type multiplier level; image 1; one of
                    # airy | comfortable | compact | dense | instrument;
                    # sets density_space and density_type together (5.3)

# =========================================================================
# term — indexed. ansi[] is generated (5.11); pin a slot, keep the rest.
# =========================================================================
[term]
ansi[4] = hue(@palette.accent, 240)
                    # ANSI blue: info, git diff, ls directories; -; colour;
                    # generated from the accent when omitted, drift cap 0.42x

# =========================================================================
# a11y — a floored length is TWO tokens. There is no "min" operator (3.2).
# =========================================================================
[a11y]
min_hit        = 4.8u   # the HIT rect, never the drawn one; -; u; 3u .. 10u
min_hit_min_px = 24px   # absolute floor for min_hit; -; device px; WCAG 2.5.5

# =========================================================================
# state — the global ladder. Every class inherits it (5.21). "base" is the
# class's own base colour; "alpha/shade/tint/mix/sat" are the five operators.
# =========================================================================
[state]
idle.fill  = alpha(base, 0.07)   # a resting control's interior; images 1,7,9;
                                 # 0..1; MUST NOT be surface.base — see 5.21(1)
idle.edge  = alpha(base, 0.40)   # a resting control's ring; images 1,7,9; 0..1
hover.fill = alpha(base, 0.22)   # pointer inside the hit rect; image 7; 0..1;
                                 # unified from winframe 0.12 / dropdown 0.25
```

Every §5 table row whose *source* column shows a derivation appears in the file exactly
like `text.title` above. Every row whose source column shows a literal appears like
`palette.black`. There is no third form, and there is no key in the file whose value is
a resolved hex of an expression stated elsewhere in this document — that would be the
libadwaita footgun §4.1 exists to avoid, written into our own master file.

`default.theme` is **one file**, not an `@include` tree: it is the thing an author
greps, and splitting it would mean answering "which of the six files is `panel.*` in?"
before answering any real question. Section order is the order of §5 — the section
banner comments above are the navigation, and the `@include` mechanism exists for
*themes*, which are small, rather than for the master, which is not.

---

### 5.1 `meta.*` — identity (5 + localised)

| token | type | default | what it does |
|---|---|---|---|
| `meta.schema` | n | `1` | engine schema; a mismatch runs a migration, never fails |
| `meta.name` / `name[<lang>]` | text | `"Default"` | shown in settings; the name `Theme=` matches |
| `meta.description` / `[<lang>]` | text | `""` | shown in settings |
| `meta.author` | text | `""` | |
| `meta.family` | enum | `console` | `console \| holographic`; advisory grouping in settings |
| `meta.base` | text | `"default"` | inherit chain, depth 8 |
| `meta.strict` | bool | `false` | unknown keys reported at error level (still loads) |
| `meta.watch` | bool | `false` | inotify reload; never polls |

---

### 5.2 `palette.*` — the raw layer (6 required, unlimited extra)

The whole authored identity of a console theme. Everything else derives from these.

| token | type | default | constraint | image |
|---|---|---|---|---|
| `palette.black` | col | `#0A100E` | **must be a literal** — target of `shade()` | all |
| `palette.white` | col | `#EAF6F1` | **must be a literal** — target of `tint()` | all |
| `palette.accent` | col | `#3FE3AE` | the one hue that re-skins the UI | 1–6 |
| `palette.accent_alt` | col | `hue(@palette.accent, 32)` | secondary chrome hue | 1 (cyan beside mint) |
| `palette.data` | col | `@palette.accent` | hue of plots / renders / holograms | 5 (overridden) |
| `palette.neutral` | col | `#6B7A74` | hue-free grey anchor for offline/disabled | 1–6 |

The literal-only rule on `black`/`white` is not cosmetic: it makes `shade()`/`tint()`
structurally incapable of introducing a cycle, removing the single most likely
authoring mistake from the cycle checker's job. A theme may add any number of extra
`palette.<ident>` entries; unreferenced ones are dropped at load (each costs a
`[f32;4]` otherwise).

---

### 5.3 `metric.*` — the one scalar that drives everything (7)

```
u = clamp(screen_h * metric.unit_pct_h / 100, metric.unit_min_px, metric.unit_max_px)
    * metric.ui_scale
```

| token | type | default | what it does |
|---|---|---|---|
| `metric.unit_pct_h` | f | `0.5` | percent of viewport **height** |
| `metric.unit_min_px` | px | `4px` | the 720p defence |
| `metric.unit_max_px` | px | `10px` | the 4K defence |
| `metric.ui_scale` | f | `1.0` | the user's `UIFontSize=` / 100 |
| `metric.density` | enum | `compact` | `airy \| comfortable \| compact \| dense \| instrument`. **Sets both multipliers below.** |
| `metric.density_space` | f | `1.00` | a point between levels. **Wins over the enum, per axis** — see below |
| `metric.density_type` | f | `1.00` | " |

**Precedence between the enum and the two floats, stated because it cannot be derived
from the cascade rules.** `metric.density` is not a peer of `density_space` /
`density_type`; it is a *generator* of them. Resolution is a DAG walk, not a file scan
(§6.3), so "the last declaration wins" cannot arbitrate between three different tokens
and file order is explicitly irrelevant. The rule instead is:

1. `metric.density` supplies a value for each axis from the level table below.
2. An **explicit** `metric.density_space` or `metric.density_type` — appearing in any
   cascade stage, in any file — replaces the enum-supplied value **for that axis only**.
   The other axis keeps the level's value.
3. `Density=` in the user's `.conf` (§9.3) is a stage-5 override of `metric.density`,
   and therefore loses to an explicit float in the theme. That is intended: a theme
   that has pinned an axis has pinned it for a reason.

So `metric.density = airy` + `metric.density_space = 0.9` is spacing 0.90, type 1.06 —
airy type, compact-ish spacing — regardless of which line comes first. The
`default.theme` comment on `density` says exactly that sentence.

| screen | raw 0.5vh | `u` |
|---|---|---|
| 1280×720 | 3.60 | **4.00** (floored) |
| 1920×1080 | 5.40 | **5.40** (the reference) |
| 2560×1440 | 7.20 | 7.20 |
| 3840×2160 | 10.80 | **10.00** (ceiled) |

*Height, not width*, because every existing metric in this codebase is `vh`-derived
and because a console is a stack of rows; a `vmin` base would shrink type on
ultra-wide monitors for no reason. *Floored at 4 px* because unclamped 720p gives
`size.md` = 18.7 px title bars and 10.8 px badges, below any usable pointer target,
and 720p is the program's declared minimum. *Ceiled at 10 px* because unclamped 4K is
the 1080p interface at 2×: same information, twice the area, nothing gained. A theme
that disagrees writes `metric.unit_max_px = 24px` and gets proportional scaling back
in one line.

**Density levels.**

| level | `metric.density_space` | `metric.density_type` | reads like |
|---|---|---|---|
| `airy` | 1.30 | 1.06 | image 6 |
| `comfortable` | 1.15 | 1.00 | image 7 |
| `compact` | **1.00** | **1.00** | image 1 — the default |
| `dense` | 0.85 | 0.96 | image 8 |
| `instrument` | 0.72 | 0.90 | image 9 |

`metric.density_space` multiplies: the whole `space.*` and `size.*` ladders; every
`*.pad*`, `*.gap*`, `*.inset*`, `*.row_h`, `*.band_h`, `*.block_h`; `rhythm.*`
distances. `metric.density_type` multiplies every `type.<role>.size`.

**Density never touches** `stroke.*`, `corner.*`, every `*.border` / `*.rule` /
`*.stroke`, every icon stroke width, `metric.unit_*`, `type.min_px`, every `*_min_px`
floor, every `%` fraction, and `chart.grid.rows/cols`. A 0.72× hairline is not a
line, and corners are the theme's identity rather than its density.

`terminal.cell_font` follows `metric.density_type` **only** when
`terminal.follow_density = true`, default `false`: a terminal's cell size is a
contract with the program running in it (`COLUMNS`/`LINES`), and silently reflowing
every shell when the user nudges density is hostile.

---

### 5.4 The ladders — `space.*` `size.*` `stroke.*` `corner.*` (30)

**`space.*` — 12 steps**, non-linear (≈×1.4 after `space.3`) so an author picks a step
rather than a number.

| token | value | px | intended use |
|---|---|---|---|
| `space.0` | `0u` | 0 | flush |
| `space.hair` | `0.25u` | 1.4 | optical nudges, icon-to-glyph |
| `space.1` | `0.5u` | 2.7 | inside a badge; bar to rule |
| `space.2` | `1u` | 5.4 | icon-to-label, cell padding |
| `space.3` | `1.5u` | 8.1 | dense list padding |
| `space.4` | `2u` | 10.8 | between rows in a dense list |
| `space.5` | `3u` | 16.2 | between controls in a form (today `vh(1.2)`) |
| `space.6` | `4u` | 21.6 | between groups |
| `space.7` | `6u` | 32.4 | between sections |
| `space.8` | `8u` | 43.2 | around a modal's content |
| `space.9` | `12u` | 64.8 | major separation, dock margins |
| `space.10` | `16u` | 86.4 | full-bleed hero gaps (family B) |

**`size.*` — 7 control heights.**

| token | value | px | what has this height |
|---|---|---|---|
| `size.xs` | `3u` | 16.2 | badge/pill, chart legend row |
| `size.sm` | `4u` | 21.6 | dense list row, chip |
| `size.md` | `5.2u` | 28.1 | **title bar, tab strip** (exactly today's `vh(2.6)`) |
| `size.lg` | `6.5u` | 35.1 | text field, icon-button row |
| `size.xl` | `8.4u` | 45.4 | **primary button** (today's `vh(4.2)`) |
| `size.2xl` | `12u` | 64.8 | dock item, launcher tile |
| `size.3xl` | `18u` | 97.2 | hero / clock block |

`control.button.h` was `win_h * 0.045` = 48.6 px — a *third* height for the same
visual object. **Unified to `size.xl`.** Two button heights differing by 3 px are a
bug wearing a constant's clothing.

**`stroke.*` — 5 weights.** `stroke(x) = max(1, round(x * u_global))`, in physical
pixels, computed from the **global** `u`, never panel-scaled, never density-scaled.

| token | x | @720p | @1080p | @1440p | @4K | use |
|---|---|---|---|---|---|---|
| `stroke.hair` | `0.2` | 1 | **1** | **1** | 2 | panel borders, grid lines, rules — the brief's "thin glowing 1px border" |
| `stroke.thin` | `0.3` | 1 | 2 | 2 | 3 | chart axes, separators, checkbox |
| `stroke.regular` | `0.4` | 2 | 2 | 3 | 4 | focused borders, window frame, icon strokes |
| `stroke.bold` | `0.7` | 3 | 4 | 5 | 7 | active tab underline, emphasis edge |
| `stroke.chart` | `0.45` | 2 | 2 | 3 | 5 | plot line width |

Stroke positions are snapped so an odd width sits on a half-pixel and an even width on
a whole pixel. Without this the 1 px panel borders blur to 2 px grey during a resize
animation — the signature stroke of family A, ruined.

**`corner.*` — 5 sizes + the mode.** A true antialiased radius cannot exist in a
triangle-only pipeline; `round` is a tessellated arc.

| token | value | px | use |
|---|---|---|---|
| `corner.none` | `0u` | 0 | terminal, keyboard keys, table cells |
| `corner.sm` | `0.8u` | 4.3 | badges, chips, icon buttons, checkboxes |
| `corner.md` | `1.2u` | 6.5 | panels, cards (brief: "~8 px at 1440p" → 8.6 px here) |
| `corner.lg` | `2.2u` | 11.9 | modal windows, window frames (today's `vh(1.1)`) |
| `corner.pill` | `pill` | h/2 | pill badges, scrollbar thumbs |
| `corner.segments` | `6` | — | arc tessellation, range 3..16 |

`corner.segments = 6` at an 8 px radius gives a 0.4 px chord error: invisible, and 6
quads instead of 16. Corners are **density-invariant** — making a dense theme change
its corner language would make it a different theme, not a denser one.

---

### 5.5 `surface.*` — six levels and a scrim (7)

Six because some element in images 1–6 sits at each depth and nothing else does.
Placed at fixed OKLab lightness with the accent hue at low chroma:
`void 0.115 / sunken 0.152 / base 0.178 / panel 0.232 / inset 0.283 / raised 0.330`,
chroma `0.22 × C_accent × [0.35, 0.40, 0.45, 0.55, 0.60, 0.62]`.

| token | type | default (mint seed, resolved) | what sits here | image |
|---|---|---|---|---|
| `surface.void` | col | `#020604` | the absolute bed: terminal background, letterbox, the opaque fallback when a family-B theme sets `base` to alpha 0 | 1–10 |
| `surface.sunken` | col | `#060D0A` | recessed beds: progress troughs, chart beds, search-field interiors | 1, 2, 7 |
| `surface.base` | col | `#0B1310` | the screen background where PCB traces and the vignette live | 1, 4 |
| `surface.panel` | col | `#15201B / 0.82` → composited `#131E1A` | the bordered panel fill; **translucent by default** | 1–6 |
| `surface.inset` | col | `#202D27` | a bordered box nested inside a panel: icon buttons, CPU/GPU/NETWORK boxes, badge fills, table headers | 1 |
| `surface.raised` | col | `#2B3933` | floats above panels: menus, tooltips, popovers, taskbar | 9 |
| `surface.scrim` | col | `alpha(@palette.black, 0.66)` | **not a level** — the modal dimmer, composited over an arbitrary level | settings modal |

`surface.void` must stay distinct from `base` so a family-B theme can set
`surface.base` to alpha 0 and still have an opaque fallback.

**`surface.void` is the swapchain clear colour**, and it is named here because with
`theme.bg` deprecated (§7.4) nothing else would own it and an orphan clear colour is a
black screen waiting to happen. Today `main.rs` passes `[theme.bg.rgb, 1.0]`; it now
passes `surface.void` with **alpha forced to 1.0**. That alpha is not cosmetic: the
same value clears the offscreen base-scene target that `fs_blur` samples, and
`fs_blur` returns `textureSample(blurred, suv) * color`, so the emitted alpha is
`blurred.a * tint.a`. Clear the base scene to alpha 0 and every glass quad in both
family-B themes renders fully transparent wherever the backdrop did not paint opaque
pixels — i.e. the entire glass layer disappears. **The base-scene clear alpha is always
1.0 regardless of what the swapchain clear becomes**, and Appendix B R8 carries that as
an explicit prerequisite rather than discovering it in QA.

The `surface.panel` row's second value is the **as-rendered** composite (§4.4): the GPU
blends `#15201B` at α 0.82 over `#0B1310` in the swapchain's encoding and produces
`#131E1A`. The authoring function `over()` works in linear light and gives `#141F1A`.
Enforcement measures the former because that is the pixel; a theme writing
`over(@surface.panel, @surface.base)` gets the latter because that is the physics.

---

### 5.6 `text.*` — seven roles (7)

Five-step legibility ladder plus two off-ladder. Placed at OKLab lightness
`title 0.870 / primary 0.905 / secondary 0.755 / muted 0.590 / disabled 0.435 /
instrument 0.372`, chroma `C_accent × [0.55, 0.15, 0.24, 0.30, 0.22, 0.34]`.

| token | type | default (mint seed) | contrast | what draws it | image |
|---|---|---|---|---|---|
| `text.title` | col | `#9EE6C8` | 11.90 | panel titles: small caps, wide tracking (`MONITOR ZASOBÓW`) | 1–10 |
| `text.primary` | col | `#D2E5DC` | 12.96 | the value / the content; `74%`, `21:57:30`, terminal output | all |
| `text.secondary` | col | `#9AB7AA` | 7.93 | the key half of key:value — `PROCESOR:` before `74%` | 1, 7 |
| `text.muted` | col | `#638677` | 4.24 | dim captions, axis labels, legends, leader labels, photo captions | 1, 2 |
| `text.disabled` | col | `#3F574D` | 2.19 | inert controls | settings |
| `text.inverse` | col | `contrast_on(@accent.primary, @surface.void, @text.primary)` | 4.5 vs fill | text **on** a filled accent/severity chip (`SYSTEM LOCKDOWN`) | 5 |
| `text.instrument` | col | `#22493A` | 1.87 | hex/coordinate dumps in image 9's margins and image 1's panel footer. **Carries no information.** | 1, 9 |

---

### 5.7 `border.*` — five colours + seven geometry keys (12)

The heading used to say "five" and the table listed twelve; the count in §5's summary
was right and the heading was wrong. The two halves are genuinely different kinds of
thing and are now separated, because a reader who skims the colour half misses
`border.style = brackets` — one of the most distinctive features in the system
(image 9) — hiding in a family whose heading counted only its colours.

**`border.<role>` — the five colours.**

| token | type | source | use | image |
|---|---|---|---|---|
| `border.subtle` | col | `alpha(@accent.primary, 0.22)` | hairlines, table rules, chart grid frame | 2 |
| `border.default` | col | `alpha(@accent.primary, 0.55)` | the 1 px glowing panel edge — family A's signature | 1–6 |
| `border.strong` | col | `@accent.primary` | emphasised / alarmed frames | 4 |
| `border.focus` | col | `@accent.focus` | keyboard focus ring | 7 |
| `border.disabled` | col | `alpha(@text.disabled, 0.55)` | inert controls | settings |

**`border.edge.*` — the seven geometry keys.** (The old flat spellings `border.width`,
`border.style`, `border.dash`, `border.gap`, `border.phase`, `border.bracket_len`,
`border.bracket_inset` remain as aliases per §4.2, because `[variant.hc]` writes
`border.width` — as the original shipped themes did — and a rename with no alias is a
silent regression.)

| token | type | source | use | image |
|---|---|---|---|---|
| `border.edge.width` | str | `@stroke.hair` | the global default edge width | all |
| `border.edge.style` | enum | `solid` | `solid \| segmented \| brackets \| none` | 9 (brackets) |
| `border.edge.dash` | len | `1.6u` | `segmented`: dash length along the ring | — |
| `border.edge.gap` | len | `0.8u` | `segmented`: gap length | — |
| `border.edge.phase` | len | `0u` | `segmented`: start offset | — |
| `border.edge.bracket_len` | frac | `18%` | `brackets`: **of the shorter side of the host rect**, clamped `[0.8u, 4.0u]` | 9 |
| `border.edge.bracket_inset` | len | `0u` | | 9 |

`segmented` is a panel-class style only: a 300 px edge at 1440p is ≈13 dashes/side
≈52 quads, fine for a handful of panels and wrong for a list of rows. The resolver
rejects it on `shape.badge` and `shape.icon_tile` with a warning. Corner brackets are
a **border style**, not a separate ornament, so any shape preset can wear them.

---

### 5.8 `accent.*` — chrome (10)

| token | type | default | what draws it |
|---|---|---|---|
| `accent.primary` | col | `@palette.accent` | borders, titles, glyphs, cursors, node dots |
| `accent.secondary` | col | `@palette.accent_alt` | image 1's cyan beside mint |
| `accent.hover` | col | `tint(@accent.primary, 0.18)` | hover edges and glyphs |
| `accent.active` | col | `lum(@accent.primary, 0.84)` | pressed |
| `accent.dim` | col | `lum(@accent.primary, 0.62)` | secondary chrome, icon duotone layer 2 |
| `accent.border` | col | `alpha(@accent.primary, 0.55)` | avatar rings, tile edges |
| `accent.glow` | col | `alpha(@accent.primary, 0.30)` | the **colour** input to every glow; radius lives in `glow.*` |
| `accent.on` | col | `contrast_on(@accent.primary, @surface.void, @text.primary)` | text on a solid accent fill |
| `accent.focus` | col | `@accent.primary` | the focused container's edge. Family B sets `#C04CFF` / `@grad.focus` (image 7's magenta frame) |
| `accent.warm` | col | `@severity.warning.text` | image 8's ORANGE active HOME icon. **[CONFLICT 12]** |

---

### 5.9 `data.*` — separate from chrome (12)

**The single most load-bearing structural point from image 5.** Panel borders red,
hologram blue, one theme, no per-widget editing.

| token | type | default | what draws it | image |
|---|---|---|---|---|
| `data.line` | col | `ensure(@palette.data, @surface.panel, 3.0)` | plot lines, wireframes, gauge arcs, orbit lines | 2, 5, 8 |
| `data.fill` | col | `alpha(@palette.data, 0.22)` | area fills under a line | 2 |
| `data.grid` | col | `alpha(oklch(0.45, 0.30·C_d, h_d), 0.35)` | the faint chart grid | 2, 8 |
| `data.axis` | col | `alpha(oklch(0.55, 0.25·C_d, h_d), 0.60)` | chart axes and ticks | 2 |
| `data.series[0..7]` | col ×8 | generated ramp (below) | categorical chart series, node classes | 2, 8, 9 |

The 8-entry ramp is generated: hue `h_d + 40°·i`, lightness cycling
`[0.800, 0.655, 0.730]`, chroma cycling `[1.00, 0.92, 0.82] × min(C_d, 0.155)`. The
chroma cap keeps every hue on the ramp inside sRGB at those lightnesses; without it
the high-chroma themes clipped and two series collapsed onto each other.

The mint seed resolves to `#35DCA8 #00A3B3 #60AEF2 #B8B4FF #BD6FBE #EA849B #FFA477 #B48A00`.
Deviating from chrome costs a theme exactly **one line**: `palette.data = #35A7FF`.

---

### 5.10 `severity.*` — seven roles × eight members (56 + 4 controls = 60)

| role | means | example |
|---|---|---|
| `ok` | nominal, doing its job | `(Zakończone)` green; `TRYB: PRACA (Optymalny)` |
| `info` | notable, not a problem | `POWIADOMIENIA (4 NOWE)`; `(W toku, 72%)` |
| `warning` | degraded, will become a problem | `ENERGIA: 85%` yellow lightning; image 1's amber alarm half |
| `critical` | failed or failing, act now | `KRYTYCZNY ALARM`, `DAMAGE BREACH`, the `CRITICAL` badges |
| `contained` | a critical condition that has been bounded | image 3's dim-green, image 4's **amber** `CONTAINED` |
| `offline` | not reporting; absent, not zero | shell's empty tab slot; a crew member with no telemetry |
| `unknown` | a value we cannot classify | a severity index past the end of the enum — **the ABI's safety valve** |

`offline` and `unknown` are distinct and both are needed: `offline` says *nobody is
talking*, `unknown` says *somebody is talking and we do not understand*. Conflating
them is the classic dashboard lie — "0 %" for "no reading".

Each role authors **one** colour and the engine generates the rest, using the member
names of §5.0's naming law (`fill / edge / text / glyph / on`). **`<r>` below is the
generator's own placeholder, expanded seven times at load — it is not a metavariable an
author can write** (§6); `default.theme` contains the seven expansions, one per role,
each with its own comment:

```
severity.<r>.text  = <authored or derived>
severity.<r>.glyph = @severity.<r>.text
severity.<r>.edge  = alpha(@severity.<r>.text, 0.60)
severity.<r>.fill  = alpha(shade(@severity.<r>.text, 0.78), 0.88)   # badge fill
severity.<r>.on    = contrast_on(@severity.<r>.text, @surface.void, @text.primary)
```

`.fill` + `.text` is the *outlined* badge (dark pill, coloured 1 px edge, coloured
text — what images 1–6 actually show); `.on` is the text on a *solid* `.text` chip
(image 5's `SYSTEM LOCKDOWN` button). Both forms are needed. `.glyph` is its own member
because §4.4 pass D resets a failing role's glyph independently of its text colour, and
because image 1's leader-line dot is the severity colour at a different alpha from its
label. The old spellings `.fg` / `.bg` / `.border` resolve through §4.2's alias table.

**Which existing call site becomes which severity index.** "Widgets pass a `u32`
severity" (§5.17) is only implementable if every producer knows its index, so the
mapping is named here rather than left to whoever ports each widget. All eight script
widgets reach these through the `severity:` key of §5.29; the four plugins reach them
through `badge()` / `severity_style()` (§7.4).

| call site | today | severity |
|---|---|---|
| `network.rhai` `"STATE", "OFFLINE"` row | `theme.base` | `offline` |
| `network.rhai` `"STATE", "ONLINE"` row | `theme.base` | `ok` |
| `memory.rhai` swap meter ≥ 50 % used | `theme.base` | `warning` |
| `memory.rhai` swap meter ≥ 85 % used | `theme.base` | `critical` |
| `cpu.rhai` / `hardware.rhai` temp over the sensor's `high` / `crit` | `theme.base` | `warning` / `critical` |
| `processes.rhai` a process in state `D` or `Z` | `theme.base` | `warning` |
| `shell` empty tab slot | `theme.base.alpha(0.35)` | `offline` |
| `filesystem` unreadable entry | `theme.base.alpha(0.5)` | `unknown` |
| `sysinfo.rhai` / `uptime.rhai` nominal readouts | `theme.base` | `ok`, written out — a chosen nominal, not a defaulted one |
| an index past the end of the enum | — | `unknown` — the ABI's safety valve |

A producer with genuinely no severity opinion passes **no** `severity:` and gets the
plain type role. That is a different thing from passing `ok`, and the difference is
visible on screen, which is why both spellings exist.

| control | type | default | meaning |
|---|---|---|---|
| `severity.mode` | enum | `hue` | `hue \| mono \| mono_plus_warning \| mono_strict` |
| `severity.pull` | f | `0.16` | how far a canonical hue is pulled toward the accent |
| `severity.pull_clamp` | deg | `14deg` | **hard.** Without it azure's critical became pink `#D9639A` |
| `severity.chroma` | f | `1.00` | global chroma scale for the mono modes |
| `severity.<r>.glyph` | enum | per role | `dot_filled \| dot_hollow \| tri_up \| tri_barred \| box_barred \| dot_slash \| dash` |
| `severity.<r>.badge_style` | enum | per role | `solid \| hollow \| hatched \| hollow_dashed` |
| `severity.<r>.pulse` | bool | `critical` only | participates in `motion.alarm_blink` |

**Modes.**

* `hue` (default) — canonical hues pulled toward the accent by `pull`, clamped.
* `mono` — one hue; roles separated by OKLab lightness on the ladder
  `critical 0.870 / warning 0.800 / ok 0.725 / info 0.660 / unknown 0.625 /
  contained 0.560 / offline 0.520` with chroma scaling and a small hue nudge
  (`crit 0, warn −20°, ok +6°, info +26°, contained −18°, unknown −36°`). *Stated
  plainly:* brightness alone gives about four reliably discriminable steps, not
  seven; the ±36° nudge is the documented deviation and it lifts pure-green's worst
  severity pair from ΔE 0.021 (invisible) to 0.117. The nudge stays inside the hue
  family — `#5AFA63` critical, `#818A16` contained — so the theme still reads as one
  hue. This is image 3.
* `mono_plus_warning` — `mono`, except `warning` and `contained` keep an independent
  amber. This is image 4 as a single token.
* `mono_strict` — evenly spaced single-hue OKLab ladder, **no** hue nudge; the
  CVD-robust option (§4.5).

**Explicit per-role overrides always win over any mode.** Derivation is a
default-generator, not a lock — that is precisely image 4: everything red by
derivation, then `severity.contained.text = #E8B33A` written out.

Default glyph/badge assignment (each distinguishable at 8 px and in greyscale):
`ok` filled circle / hollow · `info` hollow circle / hollow · `warning` filled
triangle apex-up / solid · `critical` filled triangle with a bar / solid ·
`contained` hollow square with a bar (a shut box) / hollow 2 px edge · `offline`
hollow circle with a slash / hollow dashed · `unknown` dash / hatched.

---

### 5.11 `term.*` — the terminal (22 + 8 controls = 30)

| token | type | default | note |
|---|---|---|---|
| `term.fg` | col | `@text.primary` | |
| `term.bg` | col | `@surface.void` | |
| `term.cursor` | col | `@accent.primary` | |
| `term.selection` | col | `alpha(@accent.primary, 0.28)` | |
| `term.selection_fg` | col | `contrast_on(over(@term.selection, @term.bg), @surface.void, @text.primary)` | |
| `term.ansi[0..15]` | col ×16 | generated (below) | |
| `term.dim_factor` | f | `0.60`, floor `0.45` | SGR dim; replaces `term.rs:47`'s literal |
| `term.bold_is_bright` | bool | `true` | names `term.rs:36`'s `index += 8` |
| `term.inverse_mode` | enum | `swap` | `swap \| tint` |
| `term.cursor.style` | enum | `block` | `block \| beam \| underline` |
| `term.cursor.mode` | enum | `invert` | `invert \| tint`. A tinted cursor over a glyph can make that glyph unreadable, and the cursor is exactly where the user is looking. |
| `term.selection.mode` | enum | `invert` | `invert \| tint` |
| `term.follow_density` | bool | `false` | see §5.3 |
| `term.letter_spacing` | — | **0, not themeable** | letter spacing desynchronises the cell grid from the PTY and from `TermViewC.cell_w` |
| `term.case` | — | **None, not themeable** | the terminal draws bytes, not typography |

**How the ANSI sixteen are generated** — the rule that makes palettes
theme-flavoured without breaking terminal semantics:

* `ansi[0]` = OKLab L 0.150 at the accent hue, chroma 0.30·C_accent (a tinted black)
* `ansi[7]` = `@text.secondary`
* `ansi[8]` = L 0.470 at accent hue, chroma 0.26·C_accent
* `ansi[15]` = L 0.960 at accent hue, chroma 0.10·C_accent
* `ansi[1..6]` — canonical sRGB-primary hue rotated toward the accent by `ansi.pull`,
  **clamped per hue**:

| slot | L normal | L bright | chroma | drift cap |
|---|---|---|---|---|
| red | 0.600 | 0.710 | 0.195 | **0.42×** |
| green | 0.712 | 0.815 | 0.175 | 1.00× |
| yellow | 0.848 | 0.910 | 0.158 | 0.90× |
| blue | 0.545 | 0.665 | 0.175 | **0.42×** |
| magenta | 0.658 | 0.762 | 0.190 | 1.00× |
| cyan | 0.782 | 0.870 | 0.115 | 1.00× |

Brights use 0.88× the chroma at the higher L. Then `ensure(·, term.bg, 4.5)` for
1–6 and `7.0` for 9–14.

**Red and blue get a 0.42× tighter drift cap than the rest.** They are the semantic
anchors — errors and info, `git diff`, diagnostics. At a uniform 20° cap the mint
theme produced `#C95C00` for ANSI red: unmistakably orange. This is a correctness
fix, not a taste call.

A monochrome **UI** is a style choice; a monochrome **terminal palette** breaks `ls`,
`git diff` and `vim`. Even image 3's pure-green look keeps a chromatic ANSI row; the
pure seed's identity came from slots 0/7/8/15, a 34 % hue pull on the six chromatic
slots and a global chroma scale of 0.92.

**The theme never overrides a program's explicit truecolour** (`CellColor::Rgb` —
`term.rs:39` passes it through and must keep doing so). The 256-colour cube and
`term.dim_factor` are the only theme-controlled parts of terminal colour.

---

### 5.12 `elev.*` — the material ladder (9 levels × 24 tokens + 1 = 217)

**Elevation is a fixed enum, not a number a theme invents.** Widgets name a level; the
theme colours it. This enum is the widget contract.

| # | `Elev` | what lives there | glass? | default `fill` |
|---|---|---|---|---|
| 0 | `backdrop` | the backdrop plate / wallpaper / compositor view | never | `@surface.void` |
| 1 | `board` | the desktop field itself: family-A opaque field, family-B nothing | never | `@surface.base` |
| 2 | `panel` | a resting panel, card, taskbar, board fixture | yes | `@surface.panel` |
| 3 | `raised` | hovered panel, dragged card, active launcher tile | yes | `@surface.raised` |
| 4 | `focused` | the focused window, a modal, a dialog (image 7's magenta-framed panel) | yes | `@surface.panel` |
| 5 | `popover` | menu, tooltip, dropdown, context menu, drag ghost | yes | `@surface.raised` |
| 6 | `inset` | a well *pressed into* a panel: search fields, progress troughs, terminal body | **never** | `@surface.sunken` |
| 7 | `overlay` | the top plate: scanlines, vignette, noise | never | `none` |
| **8** | **`fixture`** | **a full-screen sheet riding over a still board: the APPGRID and SEARCH AND AI boards** | **yes** | `alpha(@surface.base, 0.0)` |

`inset` and `overlay` are outside the depth ladder because they are not surfaces.

**`fixture` is appended at index 8, never inserted**, because the enum is the widget
contract and renumbering it would silently re-material every existing call site.
`ELEV_COUNT` becomes 12 (9 used, 3 spare) — see §7.1.

**Why it has to exist.** `main.rs:1841-1855` and `1742-1755` already draw exactly this:
`ctx.dl.blur(0, 0, w, h, white)` followed by `ctx.dl.rect(0, 0, w, h,
theme.bg.alpha(frost_wash))` — a screen-filling two-quad glass sheet over the still
main board, which is the §5.12 recipe implemented verbatim before this specification
existed. It had nowhere to sit in the ladder: level 1 `board` is documented as never
glass, and level 2 `panel` is a panel. So the one place in the program where the glass
recipe is already correct was the one place a theme could not touch, and `frost_wash`
was wired to a user percentage for ever. `fixture` gets the full 24-token material set:
`cockpit` gave it rank 3 and a deep wash (§8); `default` gives it rank 0 and
`fill = alpha(@surface.base, 0.92)`, so a silent theme still gets a legible sheet with
no offscreen pass at all.

Z-order: the fixture sheet sits at **z 20**, immediately after the glass cut at 19 and
before panel shadows at 21, so the panels *of* the fixture board draw over it normally.

**Three hardware facts every token below is built on** (read from
`nacelle-renderer/src/gfx.rs` and `shaders.rs`, not assumed):

* `fs_blur` samples one **global snapshot per frame** and *multiplies* by the vertex
  colour. Multiplication can only darken and hue-shift; it can never brighten.
* Glass over glass does **not** double-blur — the second pane shows the same snapshot.
* If no run is tagged glass, the offscreen pass and the whole pyramid are skipped.
  How many passes that saves is **a range, not a constant**: `set_blur_radius()`
  produces `blur_depth ∈ {1, 2, 3}` from the user's slider and `render()` executes
  `1 → [(1,0)]`, `2 → [(1,0),(2,1)]`, `3 → [(1,0),(2,1),(3,2),(2,3)]`. With the base
  scene that is **2, 3 or 5 offscreen passes**. A theme with no glass costs **two to
  five render passes less per frame, depending on the user's blur setting** — which is
  the honest form of the claim, and the one a reader can check against `gfx.rs`.
* The base-scene target is cleared with `surface.void` at **alpha 1.0** and that alpha
  is not negotiable (§5.5): `fs_blur` emits `blurred.a * tint.a`, so a transparent
  base scene makes every glass quad transparent.

**DECISION M1 — `inset` is never glass.** A glass field inside a glass panel samples
the same snapshot as its parent and therefore shows the *wallpaper* straight through
the parent's own body — visibly wrong. `inset` is a dark wash plus a one-sided inner
shadow. This is the rule that keeps the one-shot snapshot honest.

**DECISION M2 — one glass cut, at z 19, between `board` and `panel`.** Separation
between stacked panels comes from wash alpha + edge + shadow, never from re-blurring.

**Glass is always TWO quads.** `fs_blur` multiplies, so a multiply tint must be paired
with an alpha-over wash:

```
glass(rect, level) = dl.blur(rect, glass.tint)   # multiply: darken/hue-shift behind
                     dl.rect(rect, glass.wash)   # alpha over: the theme's own light
out = mix(mix(dst, blurred * tint.rgb, tint.a), wash.rgb, wash.a)
```

A single "glass colour" token is a bug waiting to happen.

**One edge, one winner.** `edge.gradient` and the `edge.mode = gradient` + `edge.color2`
pair are not two competing features: **`color2` is sugar that bakes into an anonymous
two-stop gradient**, and if `edge.gradient` names a real `@grad.<name>`, *that wins* and
`color2` is ignored with a note. The sugar stays because a two-colour edge is the
common case and forcing a named `[grad.x]` block for it would be ceremony; the named
form stays because two stops is not enough for image 7's magenta→violet→blue frame.
Anonymous gradients live in the `Edge` struct and do not consume one of the eight
`GRAD_COUNT` named slots.

Per level, `<L>` ∈ the nine above:

| token | type | default in `default.theme` | note |
|---|---|---|---|
| `elev.<L>.glass.rank` | n | `0` | 0 = no glass (no `blur()` emitted); **1..3 = softness rank**, resolved to a pyramid target that is guaranteed to have been written this frame. Not a mip index. Old spelling `glass.level` aliases per §4.2. |
| `elev.<L>.glass.tint` | col | `#FFFFFF / 1.0` | multiply; alpha = frost coverage |
| `elev.<L>.glass.wash` | col | `none` | alpha over the frost; the only knob that can brighten |
| `elev.<L>.fill` | col | per table above | used **instead** of the pair when `glass.rank == 0` |
| `elev.<L>.corner` | enum | `round` (`chamfer` for `focused`/modal in family A) | `square \| round \| chamfer` |
| `elev.<L>.radius` | len | `@corner.md` | corner radius or chamfer cut |
| `elev.<L>.edge.color` | col | `@component.panel.border` → `@border.default` | the owner of the edge colour (§5.0) |
| `elev.<L>.edge.color2` | col | `same_as_parent` (= `edge.color`) | **sugar**: the far end of a two-stop edge |
| `elev.<L>.edge.mode` | enum | `solid` | `solid \| gradient` |
| `elev.<L>.edge.gradient` | grad | `none` | `@grad.<name>`, 2–8 stops; image 7's magenta focused frame |
| `elev.<L>.edge.axis` | enum | `x` | `x \| y \| diag_down \| diag_up \| angle(deg)` |
| `elev.<L>.edge.width` | str | `@stroke.hair` | |
| `elev.<L>.glow.inner.color` | col | `none` | |
| `elev.<L>.glow.inner.radius` | len | `0u` | |
| `elev.<L>.glow.inner.falloff` | enum | `gauss` | `linear \| quad \| gauss \| halo` |
| `elev.<L>.glow.inner.mode` | enum | `auto` | `off \| shell \| sprite \| auto` |
| `elev.<L>.glow.inner.boost` | f | `1.0` | > 1 survives only on the HDR path with no LUT |
| `elev.<L>.glow.outer.*` | — | same five | |
| `elev.<L>.shadow.color` | col | `none` | |
| `elev.<L>.shadow.radius` | len | `0u` | |
| `elev.<L>.shadow.dx` / `.dy` | len | `0u` | |
| `elev.<L>.shadow.spread` | len | `0u` | |
| `elev.<L>.reflect.height` | len | `0u` | 0 = off. Only `panel` and `focused` may set it. |
| `elev.<L>.reflect.alpha` | frac | `0.0` | |
| `elev.<L>.reflect.fade` | f | `2.0` | exponent of the alpha ramp |
| `elev.glass.generations` | n | `1` | max 2; > 1 costs +1 blit and +4 passes per generation (≈0.35 ms at 1440p) and is never on by default |

**`glass.rank` is a softness rank, not a mip index, and this is a correctness fix.**
`rebuild_blur_targets()` creates four targets at div 1/2/4/8, but only some of them are
*written* in a given frame: `blur_targets[0]` is the **unblurred** full-size base
scene, and targets 2 and 3 are entered only at higher `blur_depth`. A token that meant
"mip index" would let a theme ask for target 3 while the user's blur slider sits at
20 % (`blur_depth = 1`), sampling an image still in `UNDEFINED` layout with undefined
contents — invalid usage, not merely a wrong picture. Validation layers fire; some
drivers hand back last frame's pyramid and others hand back garbage.

The mapping is therefore live, in `record_runs`, against the actual `blur_depth`:

| rank | means | resolves to |
|---|---|---|
| 0 | no glass | no `blur()` run is emitted at all |
| 1 | lightly frosted | `blur_targets[1]` — always written whenever any glass exists |
| 2 | frosted | `blur_targets[2]` when `blur_depth >= 2`, else `blur_targets[1]` |
| 3 | deep | `blur_targets[3]` when `blur_depth == 3`, else the deepest target written |

`Gfx::glass_ranks() -> u8` reports how many ranks are distinguishable right now, and
the resolver clamps every `glass.rank` to it at bake, logging
`note  elev.popover.glass.rank 3 -> 2 (user blur setting allows 2 ranks)`. **The user's
blur slider is a ceiling on the theme's rank, not an independent control** — the same
shape as `performance.decor` and `Density=`, and it is written down in §9.3 rather than
left as two owners of one number.

**The other half of that setting.** `BlurOpacity=` (`config.rs:1169`, `main.rs:352`)
today *is* `frost_wash`, the alpha of the theme-background wash over every glass quad —
i.e. it is the user's version of `glass.wash.a`. It becomes a **multiplier on every
`elev.*.glass.wash` alpha**, applied after the theme in the same stage as
`performance.decor` and `Density=`. Neither key is deleted and neither silently loses:
one clamps a rank, one scales an alpha, both are stated, and §9.3 lists them.

**`reflect` mirrors the *material*, never the content** (image 8's desk reflection).
Below the reflecting rect: one `rect_grad` of its `glass.wash` colour with alpha
ramping to 0, plus one mirrored copy of the bottom edge line. **12 verts.** It reads
correctly because a reflection at that grazing angle is a smear of the surface's own
light, not a legible copy — which is exactly what the photograph shows. A true content
reflection needs a `fs_blur` UV transform and is not in v1.

**`reflect.*` is legal on `board`, `panel`, `focused` and `fixture`.** The earlier
restriction to `panel`/`focused` made image 8 unreproducible: the brief says "the whole
hologram sits on a desk and **reflects** in its surface" — one reflection of the entire
floating composition, not a dozen independent smears with desk showing through the gaps
between them. The surface that reflects is the **board**, so the board may reflect. Two
extra rules follow and both are cheap: a board's reflection is mirrored below the
board rect rather than below each child, and when a board sets `reflect.height > 0` its
panels' own `reflect` is suppressed for that frame (one flag, checked once) so the
composition does not smear twice. Cost is the same 12 verts already budgeted.

**Everything soft is one R8 mask × a vertex colour.** `FontSystem` gains
`mask(Mask) -> Option<Glyph>` producing `Radial(Falloff)`, `Box{r, f}` and
`Ramp(Falloff)` sprites into the existing shelf-packed atlas, cached and re-baked like
glyphs (~57 KB). Outer glow, inner glow, drop shadow and rounded-corner fills are all
one routine, `DrawList::soft_box()`, a 9-slice = 54 verts. **No renderer change.**
Because masks are stretched by the quad, glow radii are resolution-independent and
never re-bake on a DPI or scale change — which also answers "what radii are legal":
**all of them.** `glass.rank` is the only quantised material value.

**Masks live in a reserved atlas band that the shelf packer never allocates from and
`reset_atlas()` never clears.** This is not tidiness. `font.rs::reset_atlas()` is
called from `check_reset()` when shelf packing fails mid-frame and from
`set_mono_font` / `set_ui_font` on any font-settings change; it zeroes `atlas: Vec<u8>`
and clears the glyph cache. Mask UVs cached anywhere outside `FontSystem` would then
point at whatever glyph the packer subsequently placed there, and **every `soft_box()`
in the interface — every outer glow, inner glow, drop shadow and rounded-corner fill,
i.e. the entire material layer — would silently start sampling letterforms.** That is
exactly the class of failure this codebase's own comments call out: one that leaves no
trace. Two rules, both binding: (1) the top 64 rows of the 2048² atlas are the mask
band, allocated once at theme load, excluded from the shelf packer's free list and
skipped by `reset_atlas()`; (2) `mask()` UVs are **never** cached outside
`FontSystem` — a caller holds a `Mask` enum, not a UV rect. If the band ever has to
move, `reset_atlas()` re-bakes the mask set before returning, and a debug assertion
checks that no cached UV survives a reset.

Masks are dithered at bake with ±0.5/255 of hashed, position-seeded noise. An R8 mask
gives 256 alpha steps and a wide glow across 8-bit output bands visibly; the dither is
free at bake and invisible as grain.

**Z-order — one list, integers, no ambiguity:**

```
  0  backdrop plate                (1 image quad)
 10  board chrome
 12  board reflection              (when the board reflects — image 8)
 13  board ribbons                 (decor.ribbons with host = board — §5.15)
 19  ── GLASS SNAPSHOT CUT ──      (the first blur() run in the frame)
 20  fixture sheet                 (APPGRID / SEARCH AND AI: blur + wash + edge)
 21  panel shadow
 22  panel glass (blur + wash) / fill
 23  panel edge, inner glow, outer glow
 24  panel reflection
 25  panel title band              (band fill, title text, right text, rule, buttons)
 29  in-panel decoration (ribbons, panel grid, watermark)
 30  panel content (widgets)       ← the widget's rect starts HERE
 40  focused — the same 21..30 sub-order at its own level
 50  popover / menu / tooltip
 60  drag ghost
 70  overlay plate                 (1 image quad)
 80  cursor, IME, debug HUD        (never themed)
```

**Who draws z 21–25 is a question this specification must answer, and the answer is:
the host.** Today nothing draws a panel container at all — `nacelle-desktop/src/main.rs`
calls `wg.draw(&mut ctx, r, &host)` on a bare rectangle, and the only chrome on screen
is whatever each widget invents (`shell` draws a `chamfer`, the script widgets draw
`module_title`, `keyboard` draws nothing). Meanwhile `elev.panel`, `shape.panel` and
the 22-token `panel.*` block all describe a container with a border, a fill, a title
band and a body — the most repeated element in the entire brief. Fully tokenised and
undrawable: a theme could set all 22 and the screen would not change.

**Binding: in the panel loop, before `wg.draw()`, the host emits the container** —
`elev[panel.elev]` (shadow → glass/fill → edge → glows → reflection), then
`shape.panel`'s ring, then the title band from `panel.title.*`, then the panel's window
controls from `panel.button.*` if the panel declares any. `Ctx` then hands the widget
its **content box** — the panel rect deflated by the border, `panel.content_pad` and
the title block — and the widget's coordinate space starts there. This is §9.6 step 7a
and it is the single change that turns §5.25's `panel.*` block from documentation into
pixels.

**What happens to `module_title`.** It drew the title the container now owns, and it is
where `draw.rs:285`'s `px*1.75` lives — the very constant `panel.title.band_h` was
specified to replace. It becomes a thin wrapper that draws **nothing** and logs a
one-shot deprecation, kept only so the script API and the four plugins keep compiling
through the transition; the title text, its rule and its right-hand slot come from the
host's band. A widget that genuinely wants a second heading inside its body uses
`title.panel` through `text_role()` / §5.29's `title` element, which is a different
thing in a different place.

**Frame cost, counted, worst case, and against the real limit.** `MAX_VERTS` is
400 000 and `gfx.rs` does `let n = verts.len().min(MAX_VERTS)` — surplus geometry is
dropped **with no diagnostic** — while `blur_base` is gated on
`n + 6 <= MAX_VERTS`, so a frame that reaches the cap loses **all** glass at once.
Silent substitution is forbidden everywhere else in this document and it must be
forbidden here too. Two requirements: a **one-shot `eprintln!` plus a monotonic counter
on both overflow paths**, and a stated budget so a theme that blows it is a known
event rather than a mystery.

| layer, image-9 deck (26 panels + focused window + taskbar + 5 donuts + 2 line charts) | verts |
|---|---:|
| material (shadow, glass/fill, edge, glows, reflection, rounded rings) | 9 600 |
| panel title bands + controls (26 × ~180) | 4 700 |
| terminal, 200×60 cells, worst case ~12 verts/cell (bg quad + glyph + cursor) | 144 000 |
| ribbons, `max_hosts = 1` (§5.15) | 1 920 |
| reticle sweep ×1, hex dump, scale ticks | 2 400 |
| charts, donuts, gauges, lists, tables | 11 000 |
| **total** | **≈173 600 = 43 % of `MAX_VERTS`** |

Headroom is a factor of 2.3, and the terminal is 83 % of the frame — which is the
honest picture and the reason `MAX_VERTS` stays a memory-safety constant rather than
becoming a token (§5.28). A theme cannot reach the cap through material; a theme that
turns ribbons on for all 26 panels *can* (§5.15), which is why `decor.ribbons.max_hosts`
exists and defaults to 1.

---

### 5.13 `glow.*` — fifteen element classes (15 × 7 = 105 + 1)

Glow is not a global. `<C>` ∈ `panel_edge, focus_ring, text_display, text_alert,
icon_active, icon_idle, chart_line, gauge_arc, badge_critical, hexagon, node_dot,
leader_line, taskbar_active, cursor, separator` (`GLOW_CLASS_COUNT = 16`, 15 used).

| token | type | default | note |
|---|---|---|---|
| `glow.<C>.enabled` | bool | `false` | **everything off in `default`** |
| `glow.<C>.color` | col | `element` | the enum word `element` means "the colour of the thing being glowed". Baked as `Glow.inherit: u8 = 1` with `Glow.color` left at the class's own resolved colour — **never as a negative alpha** |
| `glow.<C>.radius` | len | `0u` | |
| `glow.<C>.falloff` | enum | per class | `linear \| quad \| gauss \| halo` |
| `glow.<C>.mode` | enum | `auto` | `off \| shell \| sprite \| text4 \| auto` |
| `glow.<C>.alpha` | frac | `0.0` | **the only 0..1 magnitude.** The state channel is `glow_alpha`; the high-contrast scalar is `glow.alpha_scale` |
| `glow.<C>.boost` | f | `1.0` | the only >1 magnitude; HDR path with no LUT only |
| `glow.text_max_chars` | n | `48` | over this, `text4` silently drops to no glow |
| `glow.alpha_scale` | f | `1.0` | global multiplier on every `glow.<C>.alpha`; what `[variant.hc]` turns down |

**Why `element` is an enum word with its own flag rather than a sentinel in the alpha
channel.** `Glow.color` is a `Color` that ends up in `Vertex.color`, and the blend
state is `SRC_ALPHA / ONE_MINUS_SRC_ALPHA`: a negative alpha reaching a vertex is
undefined output, not a wrong colour. The old encoding also collided outright —
§5.0's table assigns `a = -1.0` to `auto`, so a consumer doing the exact-value compare
the spec recommended would read `auto` where `element` was meant, and the `v < 0.0`
catch-all only hid it. One table, one value, and **no sentinel can reach a vertex**.

**Three techniques, each with a stated cost.**

* `sprite` — a 9-slice of `Mask::Box{r, feather}`, tinted. **54 verts.** The default
  and the one that looks right.
* `shell` — 2–3 concentric `rect_outline`s at a fixed geometric alpha series
  (`w=1 a=1.00`, `w=2 a=0.34`, `w=4 a=0.12`), so a theme supplies only radius and
  colour. **24–72 verts.** Cheaper for the 20–30 panels family A puts on screen;
  visibly steppy above ≈0.85u.
* `text4` — the string re-drawn 4× at ±1 px offsets, alpha/4 each. The only way to
  glow text without per-glyph blur. **5× the glyph quads of that string.**

**DECISION M4 — the mode is chosen by radius, not by mood.** `auto` resolves to
`shell` at `radius <= 0.85u` and `sprite` above. A theme may pin either.

**DECISION M5 — text glow is off by default and capped.** `text4` is allowed only on
roles `display.clock`, `display.hero`, `alert.banner` and `badge` (severity critical),
and only for strings ≤ `glow.text_max_chars`. Body text, labels, data dumps and the
terminal never glow. Image 3's screen looks like it glows everywhere; that is the
*monitor* blooming, not per-glyph work — the same look comes from the panel-edge glow
plus the overlay plate.

**Falloff per class is a real visual decision**, not decoration: `gauss` for panel
edges and focus rings (the wide soft halo of images 7/10), `quad` for chart lines and
gauge arcs (tight, keeps the data readable), `halo` (a plateau core then a Gaussian
shoulder) for icons and node dots so the sprite core stays saturated, `linear` for
separators and leader lines.

**Refused by name:** `glow.mode = bloom_pass`. It does not exist in the renderer; the
token space has room for it and `default` must not use it. A theme that sets it is
warned and gets `auto`. The bloom on the wall in image 2 is a photograph of a monitor,
not something the compositor draws. Do not attempt it.

**Additive blend** is a ~40-line renderer addition (`ADD_ATLAS = ImageId(u32::MAX-8)`,
one more pipeline, `SRC_ALPHA/ONE` colour, `ZERO/ONE` alpha). Alpha-blended glow over
a *dark* backdrop is indistinguishable from additive; over a *bright* backdrop —
exactly family B, panels over a lit planet — alpha glow reads as a milky film because
it *replaces* rather than *adds* light. Until it lands the resolver multiplies every
`glow.*.alpha` by 0.8 and the look degrades gracefully. Note the interaction with
`grade()`, which clamps to [0,1]: `boost > 1` survives only in the R16F swapchain with
no LUT loaded. Stated so nobody files it as a bug.

---

### 5.14 `grad.*` — gradients (8 named × 3)

| token | type | default | note |
|---|---|---|---|
| `grad.<name>.stops` | list | — | **2 to 8** `(position 0..1, colour)` pairs; alpha is a stop channel, so alpha gradients are free |
| `grad.<name>.axis` | enum | `x` | `x \| y \| diag_down \| diag_up \| angle(deg)` |
| `grad.<name>.space` | enum | `oklab` | `oklab \| srgb`; `srgb` exists only to reproduce an existing palette exactly. The shipped master uses `oklab`. |
| `grad.samples` | n | `8` | max 16; the resolver pre-resolves each gradient into this many evenly-spaced RGBA stops |

`GRAD_COUNT = 8` named slots. Reserved names, declared by the master (and used
by the original shipped themes): `grad.spectrum`, `grad.focus`, `grad.deck`,
`grad.hex`.

**Why this is free.** `Vertex` already carries `color: [f32;4]` and the rasteriser
already interpolates it; `DrawList` simply never exposed it. Adding `quad_c`,
`rect_grad` and `fan_c` costs no shader, no texture and no renderer change. A gradient
border is continuous around the frame at the same 24 verts a solid border costs (the
four side quads become four `quad_c`s whose eight corner colours are the gradient
evaluated at each corner's projection onto the axis) — that is exactly image 7's
magenta/violet focused frame. A gradient hexagon is a `fan_c`: 1 centre + 6 rim
colours, 18 verts (image 1). Interpolation is authored in OKLab and resolved at load;
across a 400 px panel with 8 samples the deviation from true OKLab is under 1/255,
measured on the magenta→violet→blue arc of image 7, the most hue-travelled gradient
in the brief.

**Refused, by name and with the reason:** radial gradients with more than two colours
(a two-colour radial is a flat quad plus a `Mask::Radial` quad; three stops needs two
mask quads and the middle stop is only approximate); conic/angular gradients as fills
(a swept donut gauge is drawn as N wedge quads with per-wedge vertex colours — at
N = 48 that is a smooth sweep for 288 verts); mesh and multi-axis gradients;
per-pixel dithering of a gradient. Gradients **on text** exist but are per-glyph:
`text_grad` evaluates the gradient at each glyph's own x, smooth for headings and
explicitly forbidden for body text and the terminal.

---

### 5.15 `backdrop.*` and `decor.*` — the layers behind and in front (61)

**The central family-B observation, made expressible:** images 7, 8, 10 prove that
*the background is not the theme's*. The theme supplies glass, edges, glow and text;
something else supplies what shows through. So `backdrop` is a **source**, and every
theme must be able to say "not mine".

| token | type | default | note |
|---|---|---|---|
| `backdrop.source` | enum | `solid` | `plate \| image \| solid \| passthrough` |
| `backdrop.solid` | col | `@surface.base` | |
| `backdrop.image` | text | `""` | relative to the theme directory, or absolute |
| `backdrop.fit` | enum | `cover` | `cover \| contain \| stretch \| centre` |
| `backdrop.treat.dim` | frac | `0.0` | toward black, applied at bake |
| `backdrop.treat.saturation` | f | `1.0` | 0..2 |
| `backdrop.treat.blur` | len | `0u` | CPU blur at bake, independent of glass |
| `backdrop.treat.tint` | col | `none` | |
| `backdrop.plate.layers` | list | `[]` | order of the baked layers; `source = plate` only |

`passthrough` needs `composite_alpha = PRE_MULTIPLIED` and an alpha-capable surface,
which the swapchain does not have (it is hard-coded `OPAQUE`). **Today it resolves to
`solid` with a load warning**, and `default.theme` says so in a comment so nobody
ships a theme that silently differs.

**DECISION M10 — all static decoration is CPU-baked, once, into TWO screen-sized RGBA
plates.** The sampler is a single shared `LINEAR / CLAMP_TO_EDGE` with `mip_levels = 1`:
**tiling is impossible.** Drawing scanlines or PCB traces as geometry would cost
hundreds to thousands of quads *every frame* for a picture that never changes. Instead:

* **backdrop plate** — z 0, one `dl.image()` quad *before* anything else and therefore
  inside the glass snapshot: wallpaper, solid, PCB traces, grid, starfield, backdrop
  vignette.
* **overlay plate** — z 70, one quad after everything: scanlines, noise, top vignette.

Cost: **2 quads (12 verts) per frame for arbitrarily complex decoration.**
Memory: 2 × w×h×4 = **33 MB at 1440p, 66 MB at 4K** — the price of not having a repeat
sampler, and it is stated so the trade can be re-weighed. Bake cost, single thread at
3840×2160: solid 6 ms, wallpaper resample + treat 30 ms, traces 12 ms, grid 4 ms,
vignette 9 ms, noise 14 ms, scanlines 3 ms. **DECISION M11 — baking runs on a worker
thread, resize is debounced 250 ms, and the previous plate is stretched over the new
size until the new one arrives.**

**DECISION M12 — how the pixels reach the GPU, and what that costs.** M10/M11 is the
largest visual feature in this document and it had no route from where the pixels are
produced (`plate.rs`, a worker thread in `libnacelle`) to where they are drawn
(`Gfx`, owned by `nacelle-desktop`): `dl.image()` records an `ImageId` and nothing
allocated one, owned it across a resize, or freed it on a theme swap.
`Gfx::create_texture` / `update_texture` / `destroy_texture` already exist and are all
`#[allow(dead_code)]` — "no widget consumes these yet". They are the backing; the
missing piece is a **host image registry** on `Ctx` / `HostApi` (§7.4):

```rust
image_register: extern "C" fn(ctx, w: u32, h: u32, fmt: u32) -> u32,  // -> ImageId
image_update:   extern "C" fn(ctx, id: u32, band_y: u32, band_h: u32,
                              px: *const u8, len: usize) -> u32,
image_retire:   extern "C" fn(ctx, id: u32),
```

**Ownership, stated once.** `nacelle-desktop` owns exactly two plate ids — one backdrop,
one overlay — for the life of the process; they are re-uploaded when a bake completes,
and **retired only on process exit**, never on resize and never on theme reload. A
theme reload re-uploads into the same ids; a resize allocates a *new* pair, uploads it,
swaps the ids, and retires the old pair one frame later.

**The upload cost is stated because M11's "no frame ever waits on a bake" is true of
the bake and was false of the upload.** `update_texture` today does
`tex.pending = Some(rgba.to_vec())` — a full 33 MB heap allocation and copy on the
calling thread at 4K — and `record_texture_uploads` then allocates a *second* staging
buffer and copies again at frame start; `destroy_texture` calls `device_wait_idle()`, a
full GPU stall. Two plates that way is ~130 MB of memcpy plus a device stall inside one
16 ms frame, landing exactly on the resize path the debounce was meant to smooth. The
mechanism is therefore specified, not assumed:

1. `plate.rs` bakes **directly into a persistently-mapped staging buffer owned per
   plate**. No `to_vec`, no second copy, no allocation on the calling thread.
2. `image_update` takes a **horizontal band**, and a swap uploads in bands across
   several frames (default 8 bands ⇒ ≤ 4 MB per frame at 4K, ~0.6 ms of PCIe).
3. The **old-size texture stays alive and drawn** until the new one is fully uploaded,
   so the resize path never calls `destroy_texture` and never stalls.
4. Retirement uses the discipline `Gfx::retired` already applies to staging buffers:
   an id is freed at least one full frame after its last use.

If (1)–(3) slip out of v1, the fallback is stated rather than discovered:
**`performance.decor` defaults to `static` above 2560×1440**, so the 4K path never
re-uploads a wallpaper plate mid-resize. No frame ever waits on a bake *or* on an
upload, and idle CPU does not regress: after load, decoration costs 12 verts.

| token | type | default | image |
|---|---|---|---|
| `decor.enabled` | bool | `false` | master switch; suppresses every layer and both plates |
| `decor.traces.enabled` | bool | `false` | 1, 4, 9 |
| `decor.traces.cell` | len | `6.7u` | walk grid |
| `decor.traces.density` | frac | `0.18` | fraction of cells carrying a trace |
| `decor.traces.width` | str | `@stroke.hair` | |
| `decor.traces.color` | col | `@accent.primary` | |
| `decor.traces.alpha` | frac | `0.10` | |
| `decor.traces.via_radius` | len | `0.28u` | |
| `decor.traces.via_alpha` | frac | `0.18` | |
| `decor.traces.seed` | n | `0` | 0 = derived from the theme name; a theme pins its own pattern |
| `decor.grid.enabled` | bool | `false` | 9 |
| `decor.grid.spacing` | len | `4.4u` | |
| `decor.grid.width` | str | `@stroke.hair` | |
| `decor.grid.alpha` | frac | `0.05` | |
| `decor.grid.major_every` | n | `4` | |
| `decor.grid.major_alpha` | frac | `0.09` | |
| `decor.starfield.enabled` | bool | `false` | 6 |
| `decor.starfield.count` | n | `400` | |
| `decor.starfield.size_min` / `_max` | len | `0.14u` / `0.28u` | |
| `decor.starfield.alpha_min` / `_max` | frac | `0.20` / `0.80` | |
| `decor.starfield.color` | col | `@text.primary` | |
| `decor.starfield.seed` | n | `0` | |
| `decor.vignette.enabled` | bool | `false` | 4 heavy, 6 none |
| `decor.vignette.layer` | enum | `overlay` | `backdrop \| overlay` |
| `decor.vignette.strength` | frac | `0.55` | alpha at the corners |
| `decor.vignette.radius` | frac | `0.75` | fraction of the half-diagonal where it starts |
| `decor.vignette.color` | col | `@palette.black` | |
| `decor.vignette.shape` | enum | `cos2` | `cos2 \| linear \| quad` |
| `decor.scanlines.enabled` | bool | `false` | |
| `decor.scanlines.period` | len | `0.42u` | |
| `decor.scanlines.duty` | frac | `0.34` | fraction of the period that is dark |
| `decor.scanlines.alpha` | frac | `0.05` | |
| `decor.scanlines.color` | col | `@palette.black` | |
| `decor.scanlines.drift` | f | `0.0` | u/s; the plate is baked one period taller and the UV window advances, wrapping at one period (needs `image_uv`, five lines). **Quantised to whole texels per frame** — see below. **Cyclic.** |
| `decor.noise.enabled` | bool | `false` | |
| `decor.noise.alpha` | frac | `0.02` | |
| `decor.noise.grain` | len | `0.14u` | |
| `decor.noise.chroma` | frac | `0.0` | 0 = monochrome, 1 = per-channel |
| `decor.noise.seed` | n | `0` | |
| `decor.ribbons.enabled` | bool | `false` | 1, 7, 9 — **the only animated decoration** |
| `decor.ribbons.count` | n | `5` | |
| `decor.ribbons.segments` | n | `64` | |
| `decor.ribbons.amplitude` | frac | `0.18` | of the host rect's height |
| `decor.ribbons.wavelength` | frac | `0.55` | of the host rect's width |
| `decor.ribbons.speed` | f | `0.06` | cycles per second |
| `decor.ribbons.phase_step` | f | `0.37` | phase offset between ribbons |
| `decor.ribbons.thickness` | len | `0.35u` | |
| `decor.ribbons.gradient` | grad | `@grad.spectrum` | hue travelled along the ribbon |
| `decor.ribbons.alpha` | frac | `0.55` | |
| `decor.ribbons.host` | enum | `panel` | `panel \| board`. **Cyclic.** `board` draws at **z 13** — above board chrome (10) and the reflection (12), below the glass cut (19), so every panel frosts it; 13 is the only value consistent with the z list and it is pinned here so no host has to guess. |
| `decor.ribbons.max_hosts` | n | `1` | how many hosts actually get ribbons; `0` = unlimited (and is reported as a cost decision) |
| `decor.ribbons.host_pick` | enum | `focused_else_largest` | `focused_else_largest \| largest \| largest_open \| first`; which panels win the `max_hosts` slots. `largest_open` = the largest panel that does **not** fill its content box with an `Elev::Inset` — on the default HOME the plain largest is the shell, whose opaque inset body would cover the ribbons completely (1 920 verts and two rebinds for nothing visible); `largest_open` resolves to `filesystem`, the image-8 card that reads well behind an icon grid (u1 §4.2). |
| `decor.dump.enabled` | bool | `false` | 9 — see `ornament.dump.*` for its appearance |
| `performance.decor` | enum | `all` | **user setting, applied over the theme**: `none` (no plates at all) `\| static` (plates but no ribbons) `\| all`. Defaults to `static` above 2560×1440 |

Ribbons are `segments` `quad_c` quads per ribbon with vertex colours sampled from the
gradient at the segment's normalised x and alpha tapering to 0 at both ends (free —
alpha is a vertex channel). They live at z 29: above the panel's glass and wash, below
its content, exactly as image 7's task panel shows. **They require per-run clip**
(`DrawRun.clip: Option<[f32;4]>` + dynamic scissor, ~30 lines), which charts, the
terminal and every scrolling list need too; without it the host must clip each sine
quad against the rect geometrically.

**The cost is per host, and `host` defaults to `panel`.** 5 × 64 × 6 = **1 920 verts
and 325 sine evaluations per host per frame**, plus **one clip push/pop per host** —
one `cmd_set_scissor` each — and, because ribbons sit at z 29 between the panel's own
atlas geometry and its content, **two extra pipeline/descriptor rebinds per host** in
`record_runs`. On the image-9 deck this specification uses for its own worst case
(26 panels) that is 49 920 verts, 8 450 sine evaluations, 26 scissor changes and ~52
rebinds every frame — a 26× understatement if quoted per screen, which an earlier draft
did. Hence `decor.ribbons.max_hosts = 1`: the focused panel, or the largest if none is
focused, gets ribbons and the rest get nothing. That is also what image 7 shows — one
panel with waves, not twelve. A theme that wants more sets the number and owns the
arithmetic, and `--check-theme` prints it: `note  decor.ribbons.max_hosts = 6 —
11 520 verts, 6 clips, 12 rebinds per frame`.

**Drift is quantised to whole texels per frame.** The sampler is a single shared
`LINEAR / CLAMP_TO_EDGE` with `mip_levels = 1`, and the scanline pattern is hard-edged
with a period of `0.42u` ≈ 2.3 px at 1080p, of which `duty 0.34` is a sub-pixel dark
band. Advancing the UV window by a fractional texel resamples that pattern with a
different sub-texel phase every frame, so it crawls and breathes rather than drifting —
worse than not drifting at all. The engine therefore accumulates `drift × dt` and
advances the window only when the accumulator crosses one texel, which also removes any
need for sub-texel precision in `image_uv`. The visible consequence is stated: **drift
is stepped, not smooth, and below ≈0.5 texel/frame (≈30 u/s at 1080p) the step is
invisible; above it the theme is asking for a strobe and gets one.**

**Opt-out at three levels:** per layer (`enabled = false`, and that *is* the value in
`default`, so a theme that says nothing gets nothing); per theme
(`decor.enabled = false`); per user (`performance.decor`).

---

### 5.16 `face.*` and `type.*` — typography (8 faces × 6 + 24 roles × 12 + 4 globals = 340)

**Faces — eight slots.** The first two keep their existing ids so no plugin and no
`CellC.font` value changes meaning.

```
FACE_UI = 0   FACE_MONO = 1   FACE_UI_MEDIUM = 2   FACE_UI_BOLD = 3
FACE_DISPLAY = 4   FACE_MONO_BOLD = 5   FACE_ICON = 6   FACE_RESERVED = 7
```

| token | type | default | note |
|---|---|---|---|
| `face.<f>.family` | list | see below | ordered candidate list, matched with the existing `find_font`/`load_variant_for` machinery |
| `face.<f>.weight` | n | `400` | maps to the filename word: 300 Light, 400 Regular, 500 Medium, 600 SemiBold, 700 Bold |
| `face.<f>.file` | text | `""` | path relative to the theme directory — the ONLY way a theme ships its own binary font, and the only path allowed to escape the system font dirs |
| `face.<f>.fallback` | enum | per face | another face id, or `builtin` for the icon face |
| `face.<f>.synthetic_bold` | em | `0.028em` | auto-enabled when a ≥600 request fell back to Regular |
| `face.icon.codepoints` | text | `"icons/icons.map"` | `name = U+XXXX` per line |

Defaults: `ui` = `["Rajdhani", "Saira", "Exo 2", "United Sans"]` @400 ·
`ui-medium` = same @500, fallback `ui` · `ui-bold` = same @700, fallback `ui-medium` ·
`mono` = `["JetBrains Mono", "Fira Mono"]` @400 · `mono-bold` = same @700, fallback
`mono` · `display` = `["Orbitron"]` @600, fallback `ui-bold` · `icon` = built-in.

**Resolution when a named face is not installed** — deterministic, five steps, once at
load, never at draw time: (1) each family at the requested weight; (2) each family at
Regular, and if the request was ≥600 set `synthetic_bold` so the weight difference
survives visually; (3) follow `fallback`, repeat, cycles broken at depth 8; (4) if the
chain ends unresolved, alias to `FACE_UI`, or to `FACE_MONO` for `mono*`; (5) if
`FACE_UI` itself is unresolved, alias it to `FACE_MONO`. **If `FACE_MONO` is
unresolvable the theme load fails and the previous theme stays live** — replacing
today's `panic!`. A theme must never be able to kill the process. Every substitution
records a warning; silent substitution is forbidden.

**Synthetic bold** = the run drawn twice, the second offset by `+px * synthetic_bold`
in x. It is the only weight synthesis possible with triangles; it is honest and cheap
because only heading roles use it.

**The 24 type roles.** `ROLE_COUNT = 24` (20 named, 4 spare), an append-only `u32`
enum indexing `[TypeRole; 24]`. **Sizes are authored in `u`** so type and space scale
together. Conversion from the old unit: `size_u = size_vh × 2.6`, i.e.
`type.body = 2.6u` is exactly today's `font_px(1.0)` = 14.04 px at 1080p.

| role | face | size | px@1080 | px@1440 | tracking | case | leading | tabular |
|---|---|---|---|---|---|---|---|---|
| `display.clock` | display | `8.32u` | 45 | 60 | 0.020em | none | 1.00 | **yes** |
| `display.date` | ui | `2.21u` | 12 | 16 | 0.160em | upper | 1.30 | yes |
| `display.hero` | display | `5.72u` | 31 | 41 | 0.040em | upper | 1.10 | no |
| `title.window` | ui-medium | `2.99u` | 16 | 22 | 0.120em | smallcaps | 1.40 | no |
| **`title.panel`** | **ui-medium** | **`2.47u`** | **13** | **18** | **0.140em** | **smallcaps** | **1.45** | no |
| `label.section` | ui | `2.03u` | 11 | 15 | **0.180em** | upper | 1.50 | no |
| `body` | ui | `2.47u` | 13 | 18 | 0.020em | none | 1.45 | no |
| `body.dim` | ui | `2.47u` | 13 | 18 | 0.020em | none | 1.45 | no |
| `value` | ui-medium | `3.25u` | 18 | 23 | 0.010em | none | 1.20 | **yes** |
| `value.large` | ui-medium | `4.68u` | 25 | 34 | 0.000em | none | 1.10 | **yes** |
| `caption` | ui | `1.77u` | 10 | 13 | **0.200em** | upper | 1.35 | no |
| `badge` | ui-medium | `1.61u` | 9 | 12 | 0.160em | upper | 1.00 | no |
| `button` | ui-medium | `2.21u` | 12 | 16 | 0.100em | upper | 1.00 | no |
| `field` | ui | `2.34u` | 13 | 17 | 0.020em | none | 1.30 | no |
| `alert.banner` | ui-medium | `2.34u` | 13 | 17 | 0.140em | upper | 1.20 | no |
| `tooltip` | ui | `1.87u` | 10 | 13 | 0.040em | none | 1.30 | no |
| `data` | mono | `1.87u` | 10 | 14 | 0.000em | none | 1.25 | **yes** |
| **`data.dump`** | **mono** | **`1.35u`** | **8\*** | **10** | **−0.020em** | **upper** | **1.05** | **yes** |
| `leader.label` | ui | `1.82u` | 10 | 13 | 0.150em | upper | 1.20 | no |
| `terminal` | mono | `2.9u` | 15.7 | 20.9 | **0 (locked)** | **none (locked)** | from font metrics | n/a |

`*` floored by `type.min_px`. Per-role `min_px` / `max_px` exist:
`data.dump { min_px: 8px; max_px: 13px }` stops the instrument decoration from
becoming readable body text on a 4K panel.

Per-role tokens (12 each): `face, size, min_px, max_px, tracking, case,
smallcaps_ratio, leading, tabular, fg, alpha, synthetic_bold`.

| global | type | default | note |
|---|---|---|---|
| `type.min_px` | px | `8px` | absolute floor |
| `type.smallcaps_ratio` | f | `0.78` | |
| `type.snap_px` | bool | `true` | **role px is rounded to whole pixels before rasterisation** |

`UI_FONT_BASE` (1.3) is **not** a token: it is folded into every `size` value
(`size_u = size_vh × 2.6 = 2 × 1.3`). The live global type knob is
`metric.density_type`; `metric.ui_scale` scales everything at once.

**`title.panel` quantified** (the brief demands it): tracking 0.140em, small caps at
0.78, leading 1.45. At 1440p that is **18 px caps, 14 px small caps, +2.5 px between
every pair of glyphs** — a title reads ≈40 % wider than the same string set solid.
`label.section` and `caption` push to 0.180 / 0.200em because they are shorter strings
that must read as instrument labels rather than words.

**`data.dump` quantified**: tracking −0.020em (glyphs touch), leading 1.05 (a mono
face's natural leading is ≈1.30, so this is a 19 % compression), case upper (hex
digits and labels sit on one cap height, which is what makes the block read as a solid
texture), alpha 0.45, column pitch 9 mono cells + 1 cell of gutter. At 1440p that is a
10 px face on a 10.5 px row pitch — the densest thing on screen, exactly image 9.

**Small caps** cannot use OpenType (`fontdue` exposes no features). `SmallCaps` =
uppercase the string and draw characters that were **lowercase in the source** at
`px * smallcaps_ratio` on the same baseline. At ratio 1.0 the role degrades to plain
`Upper` and the second atlas size is never rasterised. Most reference strings are
already uppercase in the source, so they render identically either way; the ratio
earns its keep on mixed-case strings with diacritics and parenthesised roles.

**Role px is snapped to whole pixels.** The glyph cache key rounds px to quarter
pixels, so a role whose px drifts with `panel_scale` can occupy four atlas entries for
one visual size. Snapping collapses that to one, makes `measure()` stable across
frames (no 1 px text jitter when a panel animates) and costs one `round()`. **The
terminal is exempt** because its cell grid is derived from sub-pixel px.

**The terminal's tracking and case are hard-pinned and not themeable.** A theme that
sets them is warned and ignored.

**Every one of these properties is reachable from every drawing surface, or none of it
is a system.** `case`, `tracking`, `smallcaps_ratio`, `synthetic_bold` and `tabular`
are implemented *inside* `FontSystem::text`/`measure`, which means the caller has to
name a **role** for them to apply. Three callers, three routes, all normative:

* statically linked Rust widgets: `ctx.ty(Role::X)` + `ctx.text_role(...)` (§7.3);
* dlopened plugins: `HostApi::text_role` / `measure_role` (§7.4) — **not** the raw
  `text(font, px, …)`, which cannot express any of the five and would otherwise leave
  every terminal tab label, key cap, file name and `SCROLL +n` indicator outside the
  type system;
* script widgets: `text(content, align, role:)` (§5.29) — **not** the bare size
  multiplier, which is why `clock.rhai`'s `21:57:30` could not reach `tabular` even
  though §5.17 uses that exact string as the argument for tabular figures.

A caller that reimplements case folding or the two-size small-caps run on its own side
allocates a `String` per label per frame behind an ABI whose stated rule is zero
per-draw allocation. That is the failure this rule exists to prevent.

**Atlas budget, redone against the real headcount.** The earlier estimate — "six live
faces × ~6 role sizes × 96 ASCII" — undercounted three ways: there are **24** type
roles, not ~6 sizes; `Type.px` is a function of the per-panel `panel_scale` (which is
the reason §5.0 separates `Ctx::u` from `font_px` at all), so roles snap to whole
pixels **independently per panel scale** rather than onto one shared set; and the
interface in the reference images is Polish, so the coverage is
Latin-2, not 96 ASCII.

| term | value | why |
|---|---:|---|
| distinct (face, px) sets | ~110 | 6 live faces × 24 roles collapsed by snapping, × ~5 distinct panel scales on a 26-panel deck |
| glyphs per set | 210 | Latin-2 coverage actually reachable in this UI (ASCII 96 + Polish/Czech/Hungarian accented forms + box-drawing + arrows) |
| average cell at 13 px, 1 px pad | 15 × 17 px | measured on the shipped faces |
| **text subtotal** | **≈5.9 M px** | 110 × 210 × 255 |
| icon face, 5 sizes × 64 glyphs | 0.25 M px | |
| mask band (§5.12) | 0.13 M px | reserved, never evicted |

**≈6.3 M px against 2048² = 4.2 M px.** A single 2048² page is therefore *not* enough
for the worst case, and pretending otherwise would mean `check_reset` firing in normal
use. Two changes, both cheap:

1. **A second atlas page**, allocated lazily on first overflow. `ImageId` already
   distinguishes atlases; `record_runs` gains one more descriptor binding and text
   batches split at a page boundary (in practice one extra bind per frame, because a
   panel's roles land on one page). 2 pages × 4 MB = 8 MB, against a process that
   already holds two 33 MB decoration plates.
2. **Dirty-rect upload.** Today `render(atlas: Some(..))` memcpys all
   `ATLAS_W*ATLAS_H` bytes and `record_atlas_upload` copies the *entire* image every
   time `atlas_dirty` is set — at 2048² that is **4 MB CPU + 4 MB PCIe in the frame
   where it happens**, four times today's cost, repeatedly. `FontSystem` tracks a
   dirty rect (shelf packing is monotonic, so it is the union of the rows touched
   since the last upload) and uploads only that; a full-page upload happens once, at
   theme load.

The existing deferred reset-when-full path stays as the safety net, and after a reset
the mask band is untouched (§5.12) — that band is excluded from both pages' free lists.

---

### 5.17 `num.*` and `type.suffix.*` — numerals, units, status suffixes (20)

| token | type | default | note |
|---|---|---|---|
| `num.tabular_set` | text | `"0123456789"` | |
| `num.tabular_punct` | bool | `true` | also fixes `. , : + - space %` |
| `num.align` | enum | `decimal` | `decimal \| right \| left` |
| `num.decimal_sep` | text | `"."` | **the theme decides, not a locale guess** — a theme must render identically on every machine |
| `num.group_sep` | text | `" "` (thin space) | validated at load; a face lacking the character falls back to `' '` with a warning |
| `num.group` | n | `3` | |
| `num.group_min` | n | `5` | `1234` is not grouped; `12 345` is |
| `num.unit.scale` | f | `0.72` | × the value's px |
| `num.unit.gap` | em | `0.18em` | |
| `num.unit.tracking` | em | `0.060em` | |
| `num.unit.case` | enum | `upper` | |
| `num.unit.color` | col | `@text.muted` | |
| `num.unit.baseline_shift` | em | `0.0em` | units sit on the baseline, never superscript |
| `num.unit.percent_attached` | bool | `true` | `85%` — no gap before `%` |
| `type.suffix.role` | role | `body` | the parenthesised status after a value |
| `type.suffix.brackets` | text | `"()"` | |
| `type.suffix.paren_alpha` | frac | `0.55` | the brackets read dimmer than the word — the detail that makes it look drawn rather than typed |
| `type.suffix.gap` | em | `0.35em` | |
| `type.suffix.case` | enum | `none` | |
| `type.suffix.face` | enum | `ui-medium` | |

**Tabular figures are implemented in the toolkit, not demanded of the font.**
`figure_advance(face, px)` = the widest advance among `'0'..'9'`, ten glyph lookups
once, cached beside the glyph cache. When `Type.tabular` is set, `text()` and
`measure()` advance every character in `num.tabular_set` by `figure_advance` and centre
the glyph inside it. Two consequences, both wanted: `21:57:30` stops shivering (image
1's clock ticks once a second and with proportional figures every tick reflows the
string); and **the width of an all-figure string becomes `len × figure_advance`,
computable without touching the atlas at all**, which makes right-aligned numeric
columns free. That is the performance argument for doing it in the toolkit rather than
demanding a `tnum` font.

**Parenthesised status suffixes are coloured by a severity index supplied by the
value's producer — never by keyword string matching.** A keyword table consulted per
row per frame is a string lookup on a hot path and it silently mis-colours in any
language the table does not know. Widgets pass a `u32` severity; the theme maps index
→ colour. `(Zakończone)` is `severity.ok`, `(W toku, 72%)` is `severity.info`,
`DAMAGE BREACH` is `severity.warning`.

**The channel that `u32` travels down is specified, in three places, because a rule
with no channel is decoration.** §5.10 names which existing call site becomes which
index; §7.4 adds `badge(ctx, r, severity, style, text, len)` and
`severity_style(ctx, i) -> SeverityStyleC` to `HostApi`; §5.29 adds a `severity:` key
to the script `rows`, `meter`, `text`, `table` cells and a `badge(text, severity)`
element. Without those three, §7.1 bakes `sev: [SeverityStyle; SEVERITY_COUNT]`,
§5.10 resolves 60 severity tokens, and **no widget in the tree can index any of it** —
which is the state this specification was in before this section said so. Image 4's
amber `CONTAINED` inside an all-red screen and image 1's yellow `ENERGIA` beside three
green rows are both exactly this mechanism and nothing else.

`num.align = decimal` uses `ui::column_numeric`, which splits at `decimal_sep`,
right-aligns the integer part to a common x computed as `digits × figure_advance` (no
measuring) and left-aligns the fraction. `rhythm.value_align = decimal` on
*pre-formatted* strings is a different thing and is **explicitly not offered** — it
would be a false promise.

---

### 5.18 `shape.*` — sixteen presets (16 × 18 + 5 preset-specific = 293)

`chamfer_frame`/`chamfer_fill` are a special case of one generator and are replaced by
it (the names survive as thin wrappers so `object/window.rs` and `object/winframe.rs`
keep compiling):

```rust
#[repr(u8)] pub enum CornerStyle { Square, Round, Chamfer }
pub struct Corner { pub style: CornerStyle, pub size: f32 }
pub fn ring(rect: Rect, c: &[Corner; 4], segments: u8, out: &mut Vec<[f32; 2]>);
```

Fill = a triangle fan from the centroid over the ring. **The existing
`chamfer_fill_stays_inside_the_frame` test becomes a ring test and must be kept** — a
fill poking past a cut corner is still the bug this shape exists to prevent. Sizes are
clamped to `min(w, h) * 0.5` inside `ring`, so bad data cannot invert the shape.

Per preset: `corners`, `corners_tl/tr/br/bl`, `border_style`, `border_width`,
`border_color`, `border_alpha`, `fill`, `fill_alpha`, `fill_to`, `fill_angle`,
`round_segments`, `glow` (a `@glow.<class>` reference), `dash`, `gap`, `phase`,
`bracket_len`, `bracket_inset`, plus the preset-specific keys below.

**`shape.*` and `elev.*` do not compete for the same pixel** (§5.0). Twelve of the
sixteen presets name something that has no elevation — badge, chip, key, tab, field,
tile, icon_tile, hex, taskbar, button, button_alt, spare — and for those `shape.*` is
the sole owner of fill, border colour and border width. The four that *do* name an
elevated container — `panel`, `card`, `window`, `modal` — take `same_as_parent` for
`fill`, `border_color` and `border_width` and read them from `elev.*`. Setting one
anyway is legal and useful (a card whose ring differs from its material) and produces a
**note**, never a silent second opinion: `shape.card.border_color overrides
elev.panel.edge.color for this preset`.

| preset | corners | border | fill | source |
|---|---|---|---|---|
| `shape.panel` | round `@corner.md` ×4 | solid 1× | `@surface.panel` | family A panels; 8 px at 1440p as measured in image 1 |
| `shape.card` | round `1.8u` ×4 | solid 1× | `@elev.panel.glass.wash` | family B rounded cards |
| `shape.window` | chamfer `@corner.lg` ×4 | solid 1× | `@surface.panel` | today's `winframe.rs` `cut = vh*1.1` |
| `shape.button` | chamfer `0.7u` ×4 | solid 1× | `@surface.raised` | image 9 taskbar buttons |
| `shape.button_alt` | chamfer `0.9u` tl+br, square tr+bl | solid 1× | `@surface.raised` | asymmetric variant |
| `shape.icon_tile` | round `0.7u` ×4 | solid 1× / 0.45α | `@surface.raised` | image 1 "each icon in its own bordered square" |
| `shape.badge` | round `pill` ×4 | none | `@component.badge.fill`; a badge **with** a severity takes `sev[i].fill` at draw time (§5.26) | `CRITICAL` / `CONTAINED` pills |
| `shape.chip` | round `0.5u` ×4 | solid 1× / 0.6α | none | image 8's `DFS` / `SDN` chips |
| `shape.field` | round `@corner.sm` ×4 | solid 1× / 0.55α | `@surface.sunken` | image 7's search field |
| `shape.tab` | chamfer `1.0u` tl+tr, square bl+br | solid 1×, bottom edge omitted | `@surface.raised` | terminal sessions |
| `shape.taskbar` | chevron | solid 1× | `@surface.raised` | image 7's stretched chevron |
| `shape.hex` | hexagon | solid 1× | gradient | image 1's five hexagons |
| `shape.tile` | round `@corner.md` ×4 | solid 1× | `@surface.inset` | launcher tiles |
| `shape.key` | round `@corner.sm` ×4 | solid 1× | `@surface.inset` | on-screen keyboard |
| `shape.modal` | chamfer `@corner.lg` ×4 | `@stroke.regular` | `@surface.panel` | settings |
| `shape.spare` | — | — | — | one reserved slot |

Preset-specific keys: `shape.tab.slant` (`0u`; px of horizontal inset at the top, per
side — turns the ring into a trapezoid before corners are applied) ·
`shape.tab.open` (`bottom`; `none \| bottom \| top \| left \| right`, which edge of the
frame is skipped so the tab fuses with its strip) · `shape.taskbar.chevron_depth`
(`50%` of the height, per end) · `shape.taskbar.chevron_dir` (`both`;
`left \| right \| both`) · `shape.hex.orientation` (`pointy`; `pointy \| flat`).

The chevron is a hexagon = the rect with one or both vertical ends collapsed to a
point at mid-height. Fill = 3 quads (left wedge, centre rect, right wedge), frame = a
closed 6-point polyline. At `chevron_depth = 100%` and `dir = right` it is also the
solid `>` paging arrow of image 9 — **geometry, not a font glyph**, deliberately, so
an arrow that must match the border colour and width exactly does so.

---

### 5.19 `icon.*` — iconography (25 controls + 64 icon slots)

**An icon FONT is the primary path**, entering through the existing R8 atlas. It is
the only option that satisfies requirement 1 of the brief at zero cost, because
tinting is a vertex colour. A baked RGBA sprite sheet cannot be re-skinned and would
make image 4 (all-red) impossible without a sheet per hue; SVG tessellation buys
resolution independence the atlas already provides and costs `usvg` + `lyon`.

**The RGBA `ImageId` path is retained and restricted to photographs and non-themeable
artwork** — `LIVE FEED` thumbnails, avatars, the video-call tile, app logos, wallpaper.
Image 4 explicitly keeps the `LIVE FEED` thumbnail in natural photographic colour,
which is exactly the boundary: **anything the theme must be able to recolour is a
glyph; anything that must keep its own colour is an image.**

| token | type | default | note |
|---|---|---|---|
| `icon.size.xs` | len | `1.8u` | |
| `icon.size.sm` | len | `2.2u` | |
| `icon.size.md` | len | `3.6u` | |
| `icon.size.lg` | len | `5.2u` | |
| `icon.size.launcher` | len | `7.2u` | |
| `icon.stroke` | em | `0.10em` | **built-in vector fallback ONLY** — see below |
| `icon.stroke_thin` | str | `@stroke.hair` | document rule lines etc. |
| `icon.baseline_shift` | em | `0.18em` | optical centring on body x-height |
| `icon.color` | col | `@accent.primary` | |
| `icon.duo1` | col | `@accent.primary` | image 8's yellow+blue folder |
| `icon.duo2` | col | `@accent.dim` | |
| `icon.alpha` | frac | `1.0` | |
| `icon.container` | enum | `none` | `none \| square \| hex \| circle \| tile` — **a style, not a length** |
| `icon.container_size` | len | `@icon.size.md` | the container box's side; **this is what `inset_ratio` is a ratio of** |
| `icon.container_shape` | shape | `@shape.icon_tile` | |
| `icon.container_pad` | em | `0.35em` | |
| `icon.container_fill` | col | `@surface.raised` | |
| `icon.container_border` | str | `@stroke.hair` | |
| `icon.active_color` | col | `@accent.warm` | image 8's ORANGE HOME |
| `icon.active_fill` | col | `alpha(@accent.warm, 0.15)` | |
| `icon.active_border` | col | `@accent.warm` | |
| `icon.disabled_alpha` | frac | `0.30` | |
| `icon.inset_ratio` | rat | `0.27x @icon.container_size` | unified from today's 0.26/0.28/0.26. The old right-hand side, `container`, was an **enum**, so the ratio had no length to be a ratio of |
| `icon.<name>.layers` | list | 1 layer | `[U+E210 @icon.duo1, U+E211 @icon.duo2]`; **hard cap 3** — beyond that an icon is artwork and belongs on the image path |

**An icon layer may name any colour token, including an indexed one.** `IconDef`
stores `layer_color: [u32; 3]` as `ColorToken` ids, and `ColorToken` covers the eight
data series and the sixteen ANSI slots as contiguous ranges (`DataSeries0..7`,
`TermAnsi0..15` — §7.1), so `@data.series[3]` and `@term.ansi[4]` are legal layer
colours and legal `type.<role>.fg` values. Without that, an icon layer and a type role
could reach about 150 of the ~174 colours the engine resolves and nobody could say
which 24 were missing.

`icon.duo1` / `icon.duo2` default to `@accent.primary` / `@accent.dim` — one hue in two
lightnesses — which cannot produce image 8's **yellow and blue** folder. `default.theme`
therefore ships the folder as the worked two-hue example, and it is the one every other
multi-layer icon is copied from:

```ini
[icon]
duo1 = @accent.primary          # layer 1 default: the theme's own hue
duo2 = @accent.dim              # layer 2 default: the same hue, dimmer
folder.layers = [ U+E210 @data.series[7],    # body — amber in every seed
                  U+E211 @data.series[2] ]   # tab  — blue  in every seed
folder_open.layers = [ U+E212 @data.series[7], U+E213 @data.series[2] ]
document.layers    = [ U+E214 @icon.duo1 ]   # one layer: follows the accent
```

Two independent hues in one icon, both still derived, both still re-skinning when
`palette.data` changes — which is what makes image 8 reproducible without a per-hue
sprite sheet.

**Names, not codepoints.** A widget names an icon; the name resolves to a codepoint
**once, at theme load**, into a sorted `[(u32 name_hash, u32 codepoint); N]` table. At
draw time the widget passes an `IconId(u32)` it obtained once. **No string reaches a
draw call.** Unmapped names fall back to the built-in vector set glyph-by-glyph, so a
partial icon font is legal and useful.

**The 40 required names** (`default.theme` must supply all of them; every one is
traceable to an image): `chip cpu gpu memory disk wifi network signal-bars bolt
battery phone video folder folder-open document archive lock unlock home search menu
terminal files star-map planet ship radar gear sliders equalizer globe user avatar
heart bell alert close minimize maximize chevron`. `ICON_COUNT = 64`.

**A compiled-in polyline fallback set** (the same 40 names as ~8–20-point recipes,
≈1.5 kB of `const` data) guarantees icons on a bare system. The project already
depends on system fonts for text and already degrades there, but the ICONS must not
vanish, because a panel of empty boxes reads as breakage.

**`icon.stroke` is a wart, and it says so.** With an icon font the stroke is baked into
the outlines; the token affects **only** the built-in vector fallback. A theme wanting
heavier icons ships a heavier icon font. The resolver **warns rather than silently
ignoring** when a theme sets `icon.stroke` while an icon font is live.

---

### 5.20 `ornament.*` — the decorative vocabulary (7 families, 62)

Classification: **T** = the theme can switch it on and the toolkit draws it with no
widget involvement · **B** = the theme owns appearance, the widget owns data/anchors.

**`ornament.leader.*` (B)** — image 1's lines pointing at features of the station
render, labelled `DAMAGE BREACH` (orange) and `UNIDENTIFIED UNIT (I45)` (dim mint).
`width @stroke.hair` · `color @accent.primary / 0.75` · `style elbow` (`straight |
elbow`) · `elbow_ratio 0.65` · `dot_radius 0.45u` · `dot_style filled` (`filled |
ring | none`) · `label_role leader.label` · `label_gap 0.45em` · `label_rule true` ·
`label_color @text.muted`. The widget supplies `(anchor_xy, side, text, severity)`;
colour comes from the severity index — that is how `DAMAGE BREACH` is orange in a
mint theme with no per-widget editing.

**`ornament.scale.*` (B)** — image 7's left-edge vertical measuring scale.
`orientation vertical` · `major_len 2.2u` · `minor_len 0.9u` · `minor_per_major 4` ·
`width @stroke.hair` · `color @accent.primary` · `major_alpha 0.85` ·
`minor_alpha 0.40` · `label_role data` · `label_every 1` · `label_gap 0.40em` ·
`side near` (`near | far | both`) · `pitch 2u`.

**`ornament.node.*` (B)** — image 1's `NETWORK STATUS`, image 8's `SYMULACJE AI`.
`radius 0.6u` · `shape square` (`square | round`) · `fill @accent.primary` ·
`fill_alpha 0.90` · `border 0` · `edge_width @stroke.hair` · `edge_color
@accent.primary` · `edge_alpha 0.35` · `pulse 0.0hz`. **`pulse` modulates alpha only,
never radius** — modulating radius would re-tessellate and would look like a bug on a
static graph.

**`ornament.hex.*` (B)** — image 1's five gradient hexagons, image 3's solid ones.
`size 5u` (circumradius) · `gap 1u` · `cols 5` · `fill @accent.primary / 0.85` ·
`fill_to @accent.primary / 0.15` (== `fill` ⇒ flat) · `fill_angle 90deg` ·
`empty_alpha 0.12` · `caption_role caption` · `caption_gap @space.1` ·
`glow @glow.hexagon`. Image 3's theme sets `fill_to = fill` and gets flat fills; the
gradient itself is free (per-vertex colour on a 6-triangle fan).

**`ornament.reticle.*` (T)** — image 1's `DANE NAWIGACYJNE` radar.
`rings 3` · `ring_width @stroke.hair` · `ring_color @accent.primary` ·
`ring_alpha 0.45` · `ring_segments 48` · `ticks 24` · `tick_len 0.7u` ·
`tick_every 6` · `crosshair true` · `crosshair_alpha 0.30` · `sweep true` ·
`sweep_arc 40deg` · `sweep_period 4.0s` · `sweep_color @accent.primary` ·
`sweep_alpha 0.55`. Cost: 144 ring quads + 24 ticks + an ~8-triangle sweep fan, drawn
at most twice per screen; the sweep's trailing fade is a per-vertex alpha ramp, free.

**`ornament.dump.*` (T)** — image 9's hexadecimal margins.
`role data.dump` · `source off` (`off | random | telemetry`) · `seed 0` ·
`alphabet "0123456789ABCDEF"` · `col_cells 9` · `gutter_cells 1` · `width 24u` ·
`color @text.instrument` · `alpha 0.45` · `refresh_hz 2.0hz` · `churn 0.06` ·
`label_role label.section` · `heading true`.

> `source = random` is deliberate: this is decoration a THEME turns on, and requiring
> a widget to exist for it would mean the layout has to change to change the look —
> which contradicts "the same layout re-skinned six times by changing the palette
> alone". The content is generated from a seeded PRNG in the toolkit; `churn` and
> `refresh_hz` keep the cost to a few dozen character replacements per second.
> **It is off in `default`, it must always carry its `heading`, and it must never be
> rendered in a colour role other than `text.instrument`** — which is contrast-exempt
> precisely because it is non-informational. `source = telemetry` renders real numbers
> when a widget supplies them.

**`ornament.bracket.*` (T)** — an alias family (`len`, `inset`, `width`, `color`) that
writes into the owning `ShapeSpec`, so a theme can say
`shape.panel.border_style = brackets` and nothing else.

**Explicitly not ornaments:** PCB traces, vignette, scanlines, noise, ribbon waves,
grid, glass, glow, reflection. Those are `decor.*` and `elev.*` (§5.12, §5.15).

**Three tokens in this section are cyclic animation sources** and are counted against
`motion.idle_cap` exactly as if they were motion ids (§5.22): `ornament.reticle.sweep`
+ `sweep_period`, `ornament.dump.refresh_hz` and `ornament.node.pulse`. They carry the
`is_cyclic` bit in the generated token table. A radar that sweeps forever costs the
same idle CPU as a glow that breathes forever, and the fact that it is spelled
`ornament.*` rather than `motion.*` does not change the electricity bill.

---

### 5.21 `state.*` and `focus.*` — interaction (7 states × 8 + 6 = 62)

**Seven mutually exclusive state slots**, resolved to a `u8` index:

```
State = Idle | Hover | Press | Selected | SelectedHover | Dragging | Disabled
```

Reasons for each: `Idle` baseline, every class must define it · `Hover` pointer inside
the hit rect, pointer only (keyboard never produces hover) · `Press` held, or the
150 ms decay after a click (today's `flash`); one slot, because a decay is a Press
whose weight is animated down · `Selected` this one of a set is the chosen one,
persistently — shell tabs, the current board tile, a latched modifier key, **image 8's
orange active launcher icon** · `SelectedHover` the eighth combination that actually
happens; without it a hovered selected tab loses its selection mark · `Dragging`
grabbed and moving, distinct from Press because a drag lasts and must read as detached
· `Disabled` present, visible, not actionable (`settings.rs:831` already needs it and
invents it locally).

**Precedence**, first hit wins:
`Disabled > Dragging > Press > SelectedHover > Hover > Selected > Idle`.
Disabled first: a disabled control must never light up under the pointer, and today
nothing stops it. `SelectedHover` defaults to channel-wise `max(Selected, Hover)` in
alpha with `Selected`'s hue.

**Focus is NOT a state slot.** It is an orthogonal `bool` on **containers only**
(window, panel, dialog, the focused board) and it does exactly three things:
swaps the container's edge role to `@accent.focus`; enables the container's focus
glow; and multiplies every descendant's resolved alpha by `focus.unfocused_dim` when
not focused. Image 7's magenta-framed `ZADANIA BIEŻĄCE` is exactly this — two tokens
and nothing else in the panel changes. A state slot would re-bake an entire `Surface`;
focus re-roles one channel of one element and dims a subtree. [CONFLICT 3]

**The default state ladder** — the global default, inherited by every class that does
not override. Values reconcile the 96-site inventory: the median of what the code
already does, with the outliers corrected. Channels are expressed against the class's
base colour.

**One syntax, eight channels.** A state cell is written as a **dotted key** —
`state.<state>.<channel>` for the global ladder, `<class>.state.<state>.<channel>` for
one class — because that is the form that composes with the mood and variant overlays,
which are also dotted keys against absolute paths (§3.2). The `[panel:hover]` section
header form is **cut**: it cannot express a global default, and two syntaxes for one
idea is exactly what §1.2(6) refuses. The channels are named **exactly as `ClassStyle`
spells them** (§7.1) — there are eight, and "glow (radius, strength)" was one column
holding two of them:

`fill` · `edge` · `text` · `glyph` · `edge_width` · `glow_radius` · `glow_alpha` ·
`elevation`

| state | fill | edge | text | glyph | edge_width | glow_radius | glow_alpha | elevation |
|---|---|---|---|---|---|---|---|---|
| `idle` | `alpha(base, 0.07)` | `alpha(base, 0.40)` | `alpha(base, 0.70)` | `alpha(base, 0.70)` | `@stroke.hair` | `0u` | `0.00` | `0` |
| `hover` | `alpha(base, 0.22)` | `alpha(base, 0.80)` | `base` | `base` | `@stroke.hair` | `1.2u` | `0.35` | `0` |
| `press` | `alpha(base, 0.45)` | `alpha(base, 0.95)` | `base` | `base` | `@stroke.hair` | `1.6u` | `0.60` | `0` |
| `selected` | `alpha(base, 0.14)` | `alpha(base, 0.95)` | `alpha(base, 0.95)` | `alpha(base, 0.95)` | `@stroke.thin` | `1.0u` | `0.25` | `0` |
| `selected_hover` | `alpha(base, 0.26)` | `base` | `base` | `base` | `@stroke.thin` | `1.4u` | `0.40` | `0` |
| `dragging` | `alpha(base, 0.18)` | `alpha(base, 0.90)` | `base` | `base` | `@stroke.thin` | `2.8u` | `0.50` | `2` |
| `disabled` | `alpha(base, 0.00)` | `alpha(base, 0.18)` | `alpha(base, 0.28)` | `alpha(base, 0.28)` | `@stroke.hair` | `0u` | `0.00` | `0` |

**`base` is a documented keyword** meaning "this class's own base colour", legal only
inside a state channel. It removes the quoted mini-language an earlier draft used
(`"alpha 0.88"`, an operator and an argument separated by a space, which is not the
grammar's `call` form), removes the pseudo-operator `same` (write `base`), and makes
`mix` well-formed by giving it somewhere to put its second operand:
`mix(base, @surface.panel, 0.3)`. The five legal operators over `base` are the ordinary
functions `alpha`, `shade`, `tint`, `mix`, `sat` — all of §6, evaluated in OKLab, so a
ladder built on a mint accent and the same ladder on a crimson accent read as the same
*amount* of change.

`state.disabled.text` is raised from the code's 0.25 to 0.28 because 0.25 on the
darkest backgrounds in the set falls under the disabled floor.

**Three corrections the ladder imposes on existing code**, which the implementation
applies rather than preserves:

1. Every button's idle fill rises from *nothing* to `alpha 0.07` (the `control`
   plugin's value). Today a button's idle interior is `theme.bg`, which makes a button
   on a glass panel **punch an opaque hole in the glass** — family B forbids it.
2. `menu.item` hover fill unifies at 0.22 (was 0.12 in `winframe.rs`, 0.25 in
   `dropdown.rs` — two menus, two behaviours, one interface).
3. `button` press fill unifies at 0.45 (was 0.35 in libnacelle, 0.55 in the `control`
   plugin).

**A state channel takes exactly one of three forms**, and all three are needed:

* **(a) explicit** — `#RRGGBB`, `#RRGGBBAA` or `@role`. Required when the state means
  something the base colour cannot express:
  `launcher.icon.state.selected.glyph = @accent.warm` is image 8, and *nothing derived
  from cyan produces orange*.
* **(b) derivation** from `base`, in ordinary call syntax, using `alpha`, `shade`,
  `tint`, `mix` or `sat`: `tab.state.selected.fill = mix(base, @surface.panel, 0.3)`.
  All in OKLab — sRGB `dim()` (what `term.rs:47` does) makes red vanish while green
  survives.
* **(c) material change** — the state changes substance, not colour:
  `panel.state.dragging.elevation = 2`; `window.state.selected.glow_radius = 2.8u` with
  `window.state.selected.glow_alpha = 0.6`. Image 7's floating glass panels differ from
  image 3's flat console panels in exactly this: the same token set, different material
  values.

Worked, in the form an author writes:

```ini
# global: every class inherits these unless it says otherwise
[state]
hover.fill = alpha(base, 0.22)
hover.edge = alpha(base, 0.80)

# one class, one state, one channel — the form 5.27's warning polices
[tab]
state.selected.edge       = @accent.primary
state.selected.glow_alpha = 0.30
```

| `focus.*` | type | default | note |
|---|---|---|---|
| `focus.unfocused_dim` | f | `0.62` | `winframe.rs:325`'s literal, now a token |
| `focus.ring.color` | col | `@accent.focus` | drawn around the focused *control*, separate from container focus |
| `focus.ring.width` | str | `@stroke.thin` | |
| `focus.ring.offset` | len | `0.4u` | |
| `focus.ring.style` | enum | `solid` | `solid \| dashed` |
| `focus.ring.enabled` | bool | `true` | **must survive `high_contrast` and reduced motion** |

---

### 5.22 `motion.*` — a closed catalogue (18 × 8 + 2 = 146)

The theme owns: whether an effect exists at all, its duration, its easing, its period
and amplitude, and a global scale. The theme does **not** own: the frame rate, the
epsilon at which a transition is finished, or the *geometry* of the motion (which way a
board rides, where a menu unfolds from). Geometry of motion is a layout fact — the
accordion unfolds from the anchor's bottom edge because that is where the anchor is —
and a theme that could change it could produce an unhittable menu.

| id | animates | duration / period | easing |
|---|---|---|---|
| `hover` | state crossfade into/out of Hover | 90 ms | `ease_out` |
| `press` | Press decay (today's `flash`, 0.15 s in four files) | 150 ms | `ease_out` |
| `select` | Selected crossfade | 120 ms | `ease_out` |
| `focus` | container focus edge + dim crossfade | 120 ms | `ease_out` |
| `disable` | into/out of Disabled | 160 ms | `ease_in_out` |
| `menu_unfold` | accordion / window-menu unfold | 150 ms | `ease_out` |
| `board_ride` | board transition | 300 ms | `ease_in_out` |
| `widget_grow` | editor tear-off growth | 250 ms | `ease_out` |
| `hold` | hold-to-confirm (`HOLD_SECS`) | 5000 ms, clamp 1500–8000 | `linear` |
| `window_open` | a window/dialog appearing | 180 ms | `ease_out` |
| `window_close` | a window/dialog leaving | 140 ms | `ease_in` |
| `mood_change` | the mood wash | 250 ms | `ease_in_out` |
| `glow_pulse` | cyclic glow breathing | 1600 ms, amplitude 0.25 | `sine` |
| `alarm_blink` | cyclic alarm emphasis | 1000 ms, duty 0.5, floor 0.35 | `step` |
| `caret_blink` | text caret | 1060 ms, duty 0.6 | `step` |
| `term_cursor_blink` | terminal cursor | 1060 ms, duty 0.5 | `step` |
| **`value_blink`** | **a separator or glyph inside a value that ticks — image 1's clock colon in `21:57:30`** | **1000 ms, duty 0.5** | **`step`** |
| `scroll_settle` | inertial settle after a scroll | 220 ms | `ease_out` |

Per entry (8): `duration_ms`, `period_ms`, `amplitude`, `floor`, `duty`, `easing`,
`easing_p[4]`, `enabled`. Globals: `motion.scale = 1.0` (multiplies every duration and
period; **0 = jump to the end state**, which is the reduced-motion implementation and
must be handled as "jump", never "run in 0 ms") and `motion.idle_cap = 1`.

**`value_blink` exists because the shipped clock already does it and nothing named
it.** `clock.rhai` writes `if host.t - floor(host.t) < 0.5` and blinks its colon — the
only animation in the whole script path, untokenised, and, read against prohibition 6
("an unknown effect id is reported and ignored"), illegal. It is now an entry with a
period, a duty and an `enabled` flag. §5.29 gives the script API `blink(id)` so a
script names the effect instead of reading the clock: **a script that reads `host.t`
directly bypasses `motion.scale = 0` entirely**, which means reduced motion cannot
reach it. `host.t` therefore becomes `host.t_motion` — the same clock multiplied by
`motion.scale`, frozen when reduced motion is on — and the raw `host.t` is retained,
deprecated, warned once per script at load, so an existing widget keeps working while
being told what to change.

**Six easing curves, closed set:** `linear` · `ease_out` `1-(1-t)²` ·
`ease_in` `t²` · `ease_in_out` `t·t·(3-2t)` (today's smoothstep) ·
`sine` `0.5-0.5·cos(2πt)` (cyclic only) · `step` `t < duty ? 1 : floor` ·
`custom` cubic-bezier with 4 Newton iterations. Five of the six are one or two FLOPs;
`custom` exists because an author will eventually want a specific overshoot, and it is
deliberately the awkward one so it is not the default.

**Six prohibitions, enforced by the grammar** (there is no token for them), each with
its reason:

1. **Per-cell terminal colour.** A 200×60 grid is 12 000 cells; animating fg or bg per
   cell means recomputing 12 000 colours and re-emitting 12 000 quads every frame.
   That alone is the frame. The only animated thing in the terminal is the cursor,
   which is one quad.
2. **Blur radius.** Changing it re-parameterises the downsample pyramid and forces the
   whole offscreen chain to be rebuilt.
3. **Anything that affects layout** — panel rectangles, font sizes, tracking, padding,
   grid columns. Motion may animate only colour, alpha, glow strength, elevation, a
   scalar fill fraction, and a rigid transform applied to already-solved rectangles.
4. **More than `motion.idle_cap` (= 1) always-on cyclic *sources*.** Idle CPU is ~5 %
   and must not regress; every never-idle animation pins the loop at 60 Hz forever.
   **The cap counts animation sources, not motion ids.** Every token that forces a
   redraw carries an `is_cyclic` bit in the generated token table, and `enforce.rs`
   counts the **union of enabled cyclic tokens** — which is how the five that live
   outside `motion.*` get caught. Named, so nobody has to infer the list:
   `decor.ribbons.speed` · `decor.scanlines.drift` · `ornament.reticle.sweep` (with
   `sweep_period`) · `ornament.dump.refresh_hz` · `ornament.node.pulse`, alongside
   `motion.glow_pulse`, `motion.alarm_blink`, `motion.caret_blink`,
   `motion.term_cursor_blink` and `motion.value_blink`. A theme that switched on
   ribbons, a radar sweep, a 2 Hz hex dump and drifting scanlines used to pin the event
   loop at 60 Hz forever and be entirely legal.

   **Precedence, and two exemptions that are not negotiable.** `caret_blink` and
   `term_cursor_blink` are **exempt from the cap by name**: they are one quad each,
   they cost nothing measurable, and freezing them at their mean produces a permanently
   half-lit block where the user is looking. It would also contradict this same
   section's reduced-motion paragraph, which goes out of its way to freeze them **ON**
   — the accessibility path protecting a caret while the performance path put it out
   is the kind of inconsistency that ships as a bug report. The remaining order is:
   `alarm_blink` (in an alert/lockdown mood) > `value_blink` > `glow_pulse` >
   `ornament.reticle.sweep` > `ornament.node.pulse` > `decor.ribbons` >
   `decor.scanlines.drift` > `ornament.dump.refresh_hz`. Losers are frozen at their
   mean, a warning is logged naming each one, and `--check-theme` prints the whole
   ranking so the author sees which effect actually survived.
5. **Per-glyph animation** — typewriter reveals, per-character jitter, wave text. Each
   glyph would need its own colour or transform, breaking the single batched R8-atlas
   draw that makes text cheap. There is no token and there will not be one.
6. **New effects.** An unknown id is reported and ignored. An effect the engine does
   not know has no draw-list representation.

*At the boundary, said out loud:* a shimmer sweeping along a panel edge **is**
expressible (an animated gradient quad clipped to the edge strip — triangles) and
could join the catalogue later as `edge_sweep`. Chromatic aberration is a per-pixel
filter and cannot exist. The downstream 3D LUT is a colour-space transform, not a
per-element effect, and must not be repurposed as one.

**Reduced motion** (`a11y.reduced_motion` ∈ `off | system | on`, default `system`):
`motion.scale` forced to 0 · `glow_pulse` freezes at its mean ·
**`alarm_blink` freezes at its HIGH value**, not its mean and not off — what reduced
motion removes is the flicker, not the alarm; in compensation the alarm bar's edge
width doubles and `severity.critical.badge_style` forces `solid` · `caret_blink`,
`term_cursor_blink` and `value_blink` freeze ON (a hidden caret is a usability failure,
not a motion preference; a clock whose colon vanishes for half a second reads as a
rendering fault) · `board_ride` becomes a cut · the mood wash is skipped entirely ·
every cyclic `decor.*` / `ornament.*` source freezes at its mean, which for
`reticle.sweep` means the sweep wedge is simply not drawn.

---

### 5.23 `a11y.*` — accessibility (14)

| token | type | default | note |
|---|---|---|---|
| `a11y.min_hit` | len | `4.8u` | the **hit** rect, never the drawn one |
| `a11y.min_hit_min_px` | px | `24px` | its floor. A companion token, not a `min` operator (§3.2) |
| `a11y.hit_pad_mode` | enum | `grow` | `grow \| none`; `grow` inflates symmetrically without changing a single drawn pixel, so the console stays dense and the controls stay reachable |
| `a11y.grab_pad` | len | `1.5u` | `editor.rs`'s `EDGE = 8.0`, generalised |
| `a11y.grab_pad_min_px` | px | `6px` | its floor |
| `a11y.reduced_motion` | enum | `system` | `off \| system \| on` |
| `a11y.contrast_floor` | bool | `false` | when `true` the lint **corrects** instead of reporting; `high_contrast` sets it |
| `a11y.lc_*` (7) | f | 90/75/60/45/45/25–40/25 | advisory APCA minima per role (§4.4 pass D) |

**This is a live defect, not a hypothetical.** `winframe.rs:129` sizes a title-bar
button at `title_h * 0.62` = 1.61 vh ≈ **17 px at 1080p** — under every hit-target
guideline there is, and the close button is one of them. `Metrics` gains `hit_pad` and
`Frame::hit` inflates `button_rect`/`menu_button_rect` before testing. Overlap between
grown neighbours is resolved by the existing slot order (outermost first), so close
never steals from maximize.

**The high-contrast variant is SIX SCALARS, not a colour table** — the best idea in
the libadwaita stack, adopted wholesale:

```ini
# Every key is an ABSOLUTE token path (3.2). Every value is unquoted (3.2).
# State channels use the eight ClassStyle names and the "base" keyword (5.21).
[variant.hc]
state.idle.text     = alpha(base, 0.88)   # was 0.70
state.idle.edge     = alpha(base, 0.72)   # was 0.40
state.idle.fill     = alpha(base, 0.00)   # no wash; edges do the work
state.disabled.text = alpha(base, 0.40)
glow.alpha_scale    = 0.50                # a real token (5.13); glow costs edge acuity
border.edge.width   = @stroke.regular
focus.ring.width    = @stroke.bold
focus.unfocused_dim = 0.85                # dimming an unfocused window must not hide it
elev.panel.glass.wash = @surface.panel    # opaque; no live backdrop behind text
elev.panel.glass.rank = 0                 # and therefore no blur pass at all
decor.enabled       = false
severity.mode       = hue                 # never rely on a brightness ladder here
a11y.contrast_floor = true
```

Three things this block used to get wrong and now does not: it wrote a quoted
mini-language (`"alpha 0.88"`) that the grammar has no production for; it named
`glow.strength_scale`, which appeared nowhere in §5.13's catalogue; and it named
`elev.panel.glass.wash_alpha`, which is not a token either — `wash` is a colour and its
alpha is a channel of that colour, so an opaque wash is written by naming an opaque
colour. Under §4.2 all three were unknown keys, warned and ignored, which means the
headline accessibility variant did **nothing** except emit warnings.

**This only works if `default.theme` writes every border, dim and disabled colour as
an expression over those scalars. That constraint is binding on whoever writes
`default.theme`.**

`a11y.contrast_floor = true` is the one place the engine changes an author's colour:
any text failing its role minimum is lifted in OKLab L at constant hue and chroma
until it passes or L saturates. **Hue is never touched, because hue is the theme's
identity and lightness is not.**

---

### 5.24 `mood.*` — global re-skins (per mood: sparse + 3 controls)

Image 4 is not a different theme from image 6 — it is the same layout, the same widget
set, the same geometry, with a handful of roles re-mapped. Without moods, "the station
is in alarm" would have to be expressed either by editing every widget (exactly what
the brief forbids) or by swapping the whole theme — which would be indistinguishable
from the user changing their theme and would lose their choice.

**Moods are pre-resolved sibling themes selected by index swap.** Cost per mood
≈47 KB; cost per frame of having moods at all: **zero**. (The rejected alternative —
resolving once and applying the mood as an overlay at lookup time — costs a branch and
a second indirection on every one of thousands of per-frame lookups to save a few
hundred KB. Wrong trade.)

| token | type | default | note |
|---|---|---|---|
| `mood.<m>.inherit` | text | `""` | another mood, depth 8 |
| `mood.<m>.wash` | col | `none` | the transition tint (§ below) |
| `mood.<m>.when` | text | `""` | one of four fixed predicate forms |

**Those three keys are the *only* ones that belong to the mood.** Every other key
inside `[mood.<m>]` is an **absolute token path** and is merged into the root tree
(§3.2): `palette.accent`, not `mood.alert.palette.accent`. `inherit`, `wash` and `when`
are reserved inside an overlay section and may not be shadowed. This is written in two
places on purpose — a mood is authored exclusively through this construct, and an
author following the plain-section concatenation rule literally writes a mood that
silently does nothing except emit unknown-key warnings.

**What a mood MAY change:** colour roles, severity roles and mode, glow and elevation
values, decoration on/off and strength, motion `enabled`/`duration`/`period`/
`amplitude`, glass wash strength, edge widths, and the visibility of elements the
layout has already reserved room for.

**What a mood MAY NOT change:** geometry (panel rects, padding, grid), typography
(family, size ladder, tracking, case), the motion catalogue's membership,
`image.photo.tint_strength`, `a11y.*` minima, or which widgets exist. The first two
would re-run layout and re-measure text on a mood change; the rest are safety or
honesty properties, and an alarm is the worst possible time to relax them.

**Triggers — three, in strict precedence, all HOST-side.** A theme never triggers its
own mood, because a theme cannot see telemetry and must not be able to decide the
machine is on fire.

1. **Explicit API** — `nacelle::theme::set_mood(MoodId)`, callable by the application,
   the settings panel and later the compositor. Latches until cleared. This is what
   image 5's `SYSTEM LOCKDOWN` launcher button calls.
2. **External signal** — a widget returning `Action::SetMood(id)`, or a compositor
   message. Does not latch over an explicit set.
3. **Declarative rule in the theme** — deliberately tiny. Four fixed predicate forms,
   parsed at load into an enum, **evaluated once per second** against the telemetry
   snapshot that already ticks at 1 Hz — never per frame:
   `"severity >= critical"` · `"count(severity == critical) >= 3"` ·
   `"battery < 10"` · `"temp > 90"`. No scripting, no expressions, no user functions.
   Edge-triggered with a **5-second hysteresis on the falling edge**, so a metric
   hovering at a threshold cannot strobe the entire interface.

Among moods: `lockdown > alert > normal`.

**The transition.** The theme swaps instantly; the *visual* transition is a single
full-screen quad in `mood.<m>.wash`, animated over `motion.mood_change.duration`
(250 ms, `ease_in_out`) from its declared alpha to 0, drawn last. One extra quad, one
extra colour. Crossfading two resolved themes per draw call would double every lookup
for the duration — exactly the overlay cost already rejected. An alarm arriving as a
wash of red over the screen and then settling is the right cinematography anyway.

**Shipped:** `normal` and `alert` in the engine, `high_contrast` as a variant,
`lockdown` as a *convention* — `default.theme` declares it, the engine needs no extra
code for it, because it is `alert` plus `palette.data` and one launcher entry. Naming
it in `default` documents the pattern, and the theme files are meant to be
documentation.

---

### 5.25 Component metrics — everything the geometry inventory found (≈425)

Every value below is a token that the owner cannot change today. Format:
**token · default · px@1080p · note.** `str` values are device pixels.

**panel** (23) — the base container, **drawn by the host** (§5.12). Parts: gutter (user setting, not the theme's) →
border → fill/glass → content pad → title band (left text, right text, rule) → body.

| token | default | px | note |
|---|---|---|---|
| `panel.content_pad` | `2.8u` | 15.1 | inset from the panel's border to its content. **Renamed from `panel.pad` to avoid colliding with the user's GridPadding.** [CONFLICT 4] |
| `panel.content_pad_x` / `_y` | `inherit` | 15.1 | may be split |
| `panel.corner` | `@corner.md` | 6.5 | |
| `panel.corner_mode` | `round` | — | family A may set `chamfer` |
| `panel.border` | `@stroke.hair` | 1 | the brief's "thin glowing 1px border" |
| `panel.border_focused` | `@stroke.regular` | 2 | image 7 |
| `panel.title.band_h` | `4.6u` | 24.8 | today `px*1.75` (`draw.rs:285`) |
| `panel.title.block_h` | `6.8u` | 36.7 | band + gap; today `px*2.6` / `title_px*2.8` |
| `panel.title.inset_x` | `1.6u` | 8.6 | today `px*0.6` |
| `panel.title.gap` | `4.7u` | 25.4 | min gap left text → right text; today `px*1.8` |
| `panel.title.rule` | `@stroke.hair` | 1 | `none` turns it off |
| `panel.title.rule_inset` | `@space.0` | 0 | |
| `panel.title.role` | `title.panel` | — | |
| `panel.glass.rect` | `border_box` | — | `border_box \| content_box` |
| `panel.glass.inset` | `@space.0` | 0 | shrink the glass quad inside the border |
| `panel.elev` | `panel` | — | which `Elev` a resting panel uses |
| `panel.button.size` | `0.62x @titlebar.h` | 17.4 | drawn size of a panel's own window control. Images 1 and 7 put `_ ✕` and `_ □ ✕` **on panels** |
| `panel.button.pad` | `0.30x @titlebar.h` | 8.4 | |
| `panel.button.gap` | `0.22x @titlebar.h` | 6.2 | |
| `panel.button.order` | `[minimise, maximise, close]` | — | right to left; a panel may carry a subset |
| `panel.button.icon_stroke` | `@stroke.thin` | 2 | |
| `panel.button.corner` | `@corner.none` | 0 | |

**`titlebar.h` is the shared root, and it is why the buttons are correct on both.**
`winframe.button.*` was expressed as a ratio of `winframe.title_h` — the frame around a
*foreign* window (`object/winframe.rs`) — while a panel's band is sized by
`panel.title.band_h`. Controls on a panel sized off the wrong root means changing
`panel.title.band_h` leaves the buttons where they were, and changing
`winframe.title_h` moves buttons on panels it never touched. One generic token now
feeds both:

| token | default | px | note |
|---|---|---|---|
| `titlebar.h` | `@size.md` | 28.1 | the generic title-bar height |
| `panel.title.band_h` | `@titlebar.h` | 28.1 | *(was `4.6u`; the change is 3.3 px and it makes one root real)* |
| `winframe.title_h` | `@titlebar.h` | 28.1 | exactly today's `vh(2.6)` |

A theme that wants chunky chrome still changes one number — it is now
`titlebar.h` — and a theme that wants panel bands and window bars to differ overrides
one of the two children, which is a decision it has made rather than a discrepancy it
has inherited.

**topbar** (16) — **new, and it was missing entirely.** The full-width status bar of
images 1, 2, 3, 4 and 6: three clusters — a product-and-timestamp line
left, a product-name-and-alert line centre with the alarm half
in amber, and a run of status glyphs plus `14.11.2TAE` right. It had colours
(`component.alarm_bar.*`) and one state and **no metrics at all**: no height, no
padding, no rule, no cluster gap, no glyph size, no divider, no per-cluster type role.
A theme could recolour it and could not lay out a single element of it, while image 6
defines itself partly by its *absence* ("no alarm, and the bar text is the ordinary
accent"). `alarm_bar` is not a separate object: **it is `topbar`'s alarmed state**, and
`component.alarm_bar.*` are the colours of that state.

| token | default | px | note |
|---|---|---|---|
| `topbar.h` | `@size.md` | 28.1 | the bar itself |
| `topbar.pad_x` | `@space.5` | 16.2 | left/right margin of the outer clusters |
| `topbar.pad_y` | `@space.1` | 2.7 | |
| `topbar.rule` | `@stroke.hair` | 1 | the line under the bar; `none` turns it off |
| `topbar.rule_color` | `@border.subtle` | — | |
| `topbar.elev` | `panel` | — | which `Elev` the bar's material comes from |
| `topbar.cluster_gap` | `@space.6` | 21.6 | minimum gap between the three clusters |
| `topbar.sep` | `pipe` | — | the `\|` divider inside a cluster: `pipe \| bullet \| slash \| space \| rule` — **geometry when `rule`**, a glyph otherwise, so a divider that must match the border width can |
| `topbar.sep_pad` | `@space.2` | 5.4 | around the divider |
| `topbar.glyph` | `@icon.size.sm` | 11.9 | the right-hand status glyph run |
| `topbar.glyph_gap` | `@space.2` | 5.4 | |
| `topbar.left.role` | `caption` | — | |
| `topbar.center.role` | `label.section` | — | |
| `topbar.right.role` | `caption` | — | |
| `topbar.alarm.role` | `alert.banner` | — | the amber half of images 1 and 4 |
| `topbar.alarm.pulse` | `true` | — | participates in `motion.alarm_blink`; image 6 sets `false` and gets the ordinary accent |

**render** (13) — **new, and it was missing entirely.** The central wireframe: the
station, the planet, the orbit lines and the ship silhouette that occupy the largest
panel of images 1, 2, 3, 4, 5 and 6. The catalogue offered `data.line` ("plot lines,
wireframes, gauge arcs, orbit lines") and one `tint_strength`, which is **one colour**
for something the brief describes in at least five separately coloured parts: image 3
"the station render is greyscale with green rim light", image 4 "grey with a red rim",
image 5 "the wireframe, the planet and the orbit lines are blue while the leader lines,
labels and the MISSION LOGS text stay red", image 1 "a cyan wireframe space station …
with a planet behind it" plus "a small ship silhouette at the right". A greyscale hull
with a coloured rim needs a body colour, a rim colour and a rim width. §6 even
describes the recipe — `sat(c, 0.0)` plus an accent rim — as its justification for
keeping `sat()`, and then provided nothing for the recipe to write into. Three of the
six console images were unreproducible.

| token | default | note |
|---|---|---|
| `render.wire` | `@data.line` | the wireframe itself |
| `render.wire_width` | `@stroke.chart` | |
| `render.hull` | `sat(@data.line, 0.0)` | the body between the wires. Images 4/5's greyscale hull is this default; image 1's cyan hull is `@data.line` |
| `render.hull_alpha` | `0.22` | |
| `render.rim` | `@accent.primary` | the rim light — image 3 green, image 4 red |
| `render.rim_width` | `@stroke.regular` | |
| `render.orbit` | `alpha(@data.line, 0.55)` | orbit lines |
| `render.orbit_width` | `@stroke.hair` | |
| `render.orbit_dash` | `none` | `none \| <len>`; dashed orbits are N short quads |
| `render.planet` | `alpha(@data.line, 0.35)` | the disc behind the station |
| `render.planet_terminator` | `alpha(@palette.black, 0.55)` | its night side |
| `render.silhouette` | `alpha(@data.line, 0.75)` | image 1's small ship at the right |
| `render.label_role` | `leader.label` | callouts on the render |

A lockdown-style theme therefore stays one line for the two-hue behaviour
(`palette.data`), and image 3/4's grey hull with a coloured rim costs **two** more.

**dot** (9) — **new.** The unread marker of `POWIADOMIENIA (4 NOWE)`, which appears in
images 7, 8, 9 and 10, and which image 7 names by colour ("a red dot"). `badge.*` is
the `CRITICAL`/`CONTAINED` pill, `list.glyph` is a row's leading chip and
`dock.active_indicator` is the taskbar; none of them is a dot attached to a title, an
icon or a dock item.

`dot.r 0.55u` (3.0) · `dot.offset_x 0.6u` · `dot.offset_y -0.6u` ·
`dot.anchor title_right` (`title_right \| icon_tr \| row_left \| dock_item_tr`) ·
`dot.fill @severity.critical.text` · `dot.border @stroke.hair` ·
`dot.count_role badge` · `dot.count_min_w 3.2u` · `dot.count_pad 0.6u`.

**table / columns roles** (4) — the shared primitives of `ui.rs` finally get theirs:
`table.head_role label.section` · `table.cell_role data` ·
`columns.label_role caption` · `columns.value_role value`. Their colours are
`component.table.*` and `component.columns.*` (§5.26).

**winframe** (20) — the frame around a window the toolkit does not own. Everything
below the title bar is a **ratio of `winframe.title_h`**, which is how the current code
already works and is the right shape: a theme that wants chunky window chrome changes
one number.

| token | default | px | note |
|---|---|---|---|
| `winframe.title_h` | `@titlebar.h` | 28.1 | exactly today's `vh(2.6)`; shared root above |
| `winframe.border` | `@stroke.regular` | 2 | today `(vh*0.18).max(1.5)` = 1.94 |
| `winframe.corner` | `@corner.lg` | 11.9 | |
| `winframe.corner_mode` | `chamfer` | — | family A's language; family B sets `round` |
| `winframe.grip` | `1.1x @titlebar.h` | 30.9 | **hit band only, nothing is drawn.** Today `(vh*0.55).max(6)` ≈ 6 px, which is not grabbable. |
| `winframe.grip_min_px` | `8px` | 8 | |
| `winframe.corner_zone` | `1.0x @titlebar.h` | 28.1 | edge length counting as a corner for resize |
| `winframe.button.size` | `0.62x @titlebar.h` | 17.4 | drawn size; the hit rect grows to `a11y.min_hit` |
| `winframe.button.pad` | `0.30x @titlebar.h` | 8.4 | |
| `winframe.button.gap` | `0.22x @titlebar.h` | 6.2 | |
| `winframe.button.order` | `[minimise, maximise, close]` | — | right to left; a theme may reorder or drop entries |
| `winframe.button.corner` | `@corner.none` | 0 | |
| `winframe.button.border` | `@stroke.hair` | 1 | |
| `winframe.icon.stroke` | `@stroke.thin` | 2 | today a flat `1.5` |
| `winframe.icon.inset` | `0.27x @winframe.button.size` | 4.7 | unified from today's 0.26/0.28/0.26 |
| `winframe.icon.menu_rows` | `3` | — | hamburger lines |
| `winframe.icon.menu_pitch` | `0.20x @winframe.button.size` | 3.5 | |
| `winframe.icon.minimise_y` | `0.68x @winframe.button.size` | 11.8 | |
| `winframe.title.role` | `title.window` | — | |
| `winframe.title.align` | `center` | — | `left \| center` |
| `winframe.title.room_pad` | `1.2x @type.title.window.size` | 16.8 | kept clear around a centred title |
| `winframe.rule` | `@stroke.hair` | 1 | the title-bar floor |

**menu** (10) — the window menu (`winframe.rs`) and the settings drop-down
(`dropdown.rs`) are the same object drawn twice with different numbers. **Unified.**

`menu.row_h 0.85x @titlebar.h` (23.9; the drop-down's was `btn_h*0.8` = 36.3 —
unified down) · `menu.pad 0.35x @menu.row_h` (8.4) · `menu.min_w 7.0x @titlebar.h` (196.6) ·
`menu.corner 0.5x @winframe.corner` (5.9) · `menu.border @stroke.hair` ·
`menu.item.role menu` (→ `body`) · `menu.item_inset 0.5x @menu.row_h` (12.0) ·
`menu.item_text_threshold 0.7x @menu.row_h` (16.7; below this an unfolding row draws no
text) · `menu.anchor_width anchor` (`anchor | min_w`) · `menu.rule @stroke.hair`.

**button** (10) — the parallelogram.

`button.h @size.xl` (45.4) · **`button.skew 0.55x @button.h`** (25.0 — **unified**: `button.rs`
used 0.7, the control plugin 0.55, for the same object; 0.7 eats 32 px of label room
on a 45 px button) · `button.corner @corner.none` (the parallelogram IS the shape) ·
`button.border @stroke.hair` · `button.border_hover @stroke.hair` (brightness carries
hover, not thickness) · `button.pad_x @space.5` (16.2) · `button.role button` ·
`button.icon_size 0.14x @button.h` (6.4; the BACK arrow) · `button.icon_size_min_px 4px` · `button.icon_gap
@space.2` · `button.min_w 14u` (75.6, so a two-letter label is still a button).

**iconbtn** (6) — image 1's row of five square icon buttons. *No such object exists in
code; it is built from `rect_outline` by hand in three places.*
`iconbtn.size @size.lg` (35.1) · `iconbtn.icon 0.5x @iconbtn.size` (17.6) ·
`iconbtn.corner @corner.sm` · `iconbtn.border @stroke.hair` · `iconbtn.gap @space.3` ·
`iconbtn.stroke @stroke.thin`.

**tile** (13) — launcher tile: icon over a wide-tracked caption (image 1).
`tile.size @size.2xl` (64.8) · `tile.shape round` (`round | hex | chamfer | square`) ·
`tile.corner @corner.md` · `tile.icon 0.5x @tile.size` (32.4) · `tile.border @stroke.hair` ·
`tile.border_active @stroke.bold` (4; image 5's larger, brighter LOCKDOWN) ·
`tile.caption_gap @space.2` · `tile.caption_role caption` · `tile.caption_lines 1` ·
`tile.gap_x @space.5` · `tile.gap_y @space.6` (a caption needs more room below than
beside) · `tile.cols 3` · `tile.scale_active 1.15` (f, not a ratio — image 5).

**checkbox** (7) — `checkbox.size 2.8u` (15.1; today `0.55x row_h`) ·
`checkbox.border @stroke.thin` (today a flat 1.5) · `checkbox.corner @corner.none` ·
`checkbox.tick_inset 0.22x @checkbox.size` (3.3) · `checkbox.tick_shape square`
(`square | check | cross`) · `checkbox.label_gap @space.3` (today `px*0.8`) ·
`checkbox.row_h @size.lg` (the hit target, larger than the box).

**slider** (10) — `slider.row_h @size.xl` · `slider.track_h @stroke.bold` (4; today a
flat 2.0) · `slider.track_corner @corner.pill` · `slider.fill_h same_as_parent` ·
`slider.knob_w 1u` (5.4; today `0.28x track.h`, which depended on the *row* height) ·
`slider.knob_h 3u` (16.2) · `slider.knob_corner @corner.none` ·
`slider.knob_border 0` · `slider.tick_count 0` · `slider.tick_h 1u`.

**cycler** (6) — `cycler.h @size.xl` · `cycler.chevron_w 3u` (16.2) ·
`cycler.chevron_stroke @stroke.thin` · `cycler.gap @space.3` ·
`cycler.value_align center` · `cycler.border @stroke.hair`.

**segmented** (7) — `segmented.h @size.xl` · `segmented.gap @space.5` ·
`segmented.corner @corner.sm` · `segmented.border @stroke.hair` ·
`segmented.border_active @stroke.regular` · `segmented.pad_x @space.3` ·
`segmented.min_cell_w 10u` (54).

**field** (10) — image 7's `browser` search box. *No such object exists.*
`field.h @size.lg` · `field.pad_x @space.4` · `field.corner @corner.sm` ·
`field.border @stroke.hair` · `field.border_focused @stroke.regular` ·
`field.icon 3u` · `field.icon_gap @space.2` · `field.caret_w @stroke.thin` ·
`field.caret_h 1.2x @type.body.size` (16.8) · `field.caret_style bar`
(`bar | block | underline`).

**list** (11) — the task list, notification list, crew list.
`list.row_h @rhythm.row` (27.0; today `px*1.9` = 26.7) · `list.pad_x @space.2` ·
`list.gap @space.0` · `list.rule none` · `list.rule_every 0` ·
`list.glyph 2.6u` (14.0; the leading coloured chip) · `list.glyph_gap @space.2` ·
`list.indent 3u` · `list.status_gap @space.3` (before the trailing `(Zakończone)`) ·
`list.bar_h @progress.h` · `list.bar_gap @space.2`.

**table** (12) — `table.head_role label.section` · `table.cell_role data` ·
`table.head_h 4.2u` (22.7; today `px*1.6`) · `table.row_h 4.2u` ·
`table.col_gap 2.4u` (13.0) · `table.col_min_w 2.6u` · `table.cell_pad 0.6u` ·
`table.rule @stroke.hair` · `table.head_gap 0.8x @table.row_h` (18.2) ·
`table.head_gap_below 0.2x @table.row_h` (4.5) · `table.zebra_every 0` · `table.min_rows 1`.

**tab** (9) — the terminal sessions. `tab.h @size.md` (28.1; today `vh(2.6)`) ·
`tab.skew 0.7x @tab.h` (19.7 — **kept at 0.7**; tabs read as slanted, buttons do not) ·
`tab.pad 1.5u` (8.1; today `vh(0.74)`) · `tab.gap @space.0` ·
`tab.rule @stroke.thin` · `tab.rule_gap 0.8x @tab.pad` (6.5) · `tab.role button` ·
`tab.underline_active @stroke.bold` · `tab.count 5` (**read-only for themes** —
mirrors `TAB_COUNT`; the session array is sized from it, so a theme may style the
strip but not resize it).

**badge** (9) — `badge.h @size.xs` (16.2) · `badge.pad_x 1.2u` ·
`badge.corner @corner.pill` · `badge.border @stroke.hair` · `badge.role badge` ·
`badge.glyph 2u` · `badge.glyph_gap @space.1` · `badge.gap @space.2` ·
`badge.style_from_severity true`.

**progress / meter** (10) — image 1's "thin progress bar".
**`progress.h 1.2u` (6.5)** — today's `ui::meter` draws a `line*0.6` ≈ 16 px bar,
which is not thin · `progress.border @stroke.hair` · `progress.inset @stroke.hair` ·
`progress.corner @corner.none` · `progress.track_style outline`
(`outline | fill | segments`) · `progress.segments 0` · `progress.segment_gap 0.3u` ·
`meter.label_gap 0.6u` · `meter.value_gap 0.6u` · `meter.row_h @rhythm.row` ·
`meter.bar_align middle`.

**gauge** (8) — `gauge.h @size.sm` · `gauge.gap @space.1` · `gauge.cols 2` ·
`gauge.border @stroke.hair` · `gauge.label_role caption` (the `C0` half) ·
`gauge.value_role value` (the `12%` half, the role every other numeric readout in
the master is bound to) · `gauge.value_inset 0.6u` ·
`gauge.value_clearance 1.2u` (fill closer than this and the number flips colour —
`ui.rs:138`'s real contrast decision, now named) · `gauge.min_h_for_value 1.4x @type.value.size`.

**dotmatrix** (5) — `dotmatrix.cell 1.6u` (8.6; today `vh(0.8)`) ·
`dotmatrix.cell_min_px 6px` · `dotmatrix.fill_ratio 0.6x @dotmatrix.cell` ·
`dotmatrix.fill_min_px 2px` · `dotmatrix.gap_min_px 2px` (guarantees rows never fuse
into bars).

**hex** (4 metrics; appearance in `ornament.hex.*`) — `hex.size 5u` (27.0
circumradius) · `hex.gap 1u` · `hex.cols 5` · `hex.stroke @stroke.hair`.

**chart** (13 shared + per type) — *none of these exist in code; all are quads and
lines.* `chart.pad @space.3` · `chart.gutter_left 6u` (32.4) ·
`chart.gutter_bottom 4u` (21.6) · `chart.axis.stroke @stroke.thin` ·
`chart.grid.stroke @stroke.hair` · `chart.grid.rows 4` · `chart.grid.cols 6` ·
`chart.grid.style solid` (`solid | dashed`; dashed = N short quads,
`chart.grid.dash 0.6u`, `chart.grid.gap 0.6u`) · `chart.tick_len 0.8u` ·
`chart.label_role caption` · `chart.legend.h @size.xs` · `chart.legend.swatch 1.6u` ·
`chart.legend.gap @space.3`.

*line*: `chart.line.stroke @stroke.chart` · `chart.line.point_r 0.5u` ·
`chart.line.point_every 0` · `chart.line.fill none` (`none | flat | ramp`;
`ramp` = N horizontal quad bands, `chart.line.fill_bands 8`) ·
`chart.line.smooth 0` (0 = polyline; `n` inserts n interpolated points per segment —
bounded, honest, triangles).
*bar*: `chart.bar.gap 0.5u` · `chart.bar.min_w 1u` · `chart.bar.corner @corner.none` ·
`chart.bar.baseline @stroke.thin` · `chart.bar.group_gap 1.5u`.
*donut* (image 9's 92/84/95 and the taskbar's 74/65): `donut.size 12u` (64.8 outer
diameter) · `donut.thickness 1.6u` (8.6) · `donut.segments 48` (96 triangles, smooth
at 65 px) · `donut.start_deg -90deg` · `donut.sweep_deg 360deg` (270 gives the open
gauge) · `donut.cap butt` (`butt | round`) · `donut.track on` ·
`donut.label_role value` · `donut.caption_role caption` · `donut.gap @space.5`.
*nodegraph*: `nodegraph.node_r 0.7u` · `nodegraph.node_shape square` ·
`nodegraph.edge @stroke.hair` · `nodegraph.pad @space.4` ·
`nodegraph.label_role caption` · `nodegraph.label_gap @space.1`.
*leader*: `leader.stroke @stroke.hair` · `leader.elbow 2u` · `leader.dot_r 0.5u` ·
`leader.label_gap @space.2` · `leader.label_role leader.label`.
*scale*: `scale.pitch 2u` · `scale.tick_minor 1u` · `scale.tick_major 2.4u` ·
`scale.major_every 5` · `scale.stroke @stroke.hair` · `scale.label_role data` ·
`scale.label_gap @space.2`.

**avatar / feed** (10) — `avatar.size 6u` (32.4) · `avatar.corner @corner.sm` ·
`avatar.border @stroke.hair` · `avatar.gap @space.3` · `avatar.row_h @list.row_h` ·
`avatar.name_role body` · `avatar.sub_role caption` · `avatar.sub_gap @space.hair` ·
`feed.aspect 0.5625` (frac, height/width) · `feed.corner @corner.md` · `feed.caption_h 4u` ·
`feed.caption_pad @space.2`.

**dock / taskbar** (14) — image 7's chevron, image 8's centred icons, image 9's tall
bordered buttons. `dock.h @size.2xl` (64.8) · `dock.pad @space.3` ·
`dock.shape rect` (`rect | chevron`) · `dock.chevron_skew 3u` ·
`dock.corner @corner.sm` · `dock.border @stroke.hair` · `dock.item_w 12u` ·
`dock.item_gap @space.3` · `dock.icon 5u` · `dock.caption_role caption` ·
`dock.caption_gap 0.6u` · `dock.chevron_w 4u` (the `<` `>` paging arrows) ·
`dock.align left` (`left | center | justify`; image 8 centres, image 9 justifies) ·
`dock.edge bottom` · `dock.active_indicator border`
(`border | underline | glow | none`).

**tooltip / chip** (8) — `tooltip.h 4.2u` (22.7) · `tooltip.pad_x 1.6u` ·
`tooltip.pad_y 0.6u` · `tooltip.corner @corner.sm` · `tooltip.border @stroke.hair` ·
`tooltip.offset @space.2` · `tooltip.role tooltip` · `tooltip.max_w 60u` (324).

**scrollbar** (9) — **new, required.** Nothing draws a scrollbar today; the filesystem
widget scrolls blind and the terminal has scrollback with no position indicator. A
dense instrument interface that scrolls without saying where it is is a defect.
`scrollbar.mode overlay` (`overlay | inset | none`; `overlay` costs no layout, which
is why it is the default) · `scrollbar.w 1.2u` · `scrollbar.w_hover 2u` ·
`scrollbar.margin @space.1` · `scrollbar.thumb_min 6u` ·
`scrollbar.corner @corner.pill` · `scrollbar.track off` ·
`scrollbar.auto_hide true` · `scrollbar.edge right`.

**terminal** (12) — `terminal.pad 1.5u` (8.1; today `vh(0.74)`) ·
`terminal.cell_font 2.9u` (15.7; today `vh(1.45)`, multiplied by `TermFontSize=`) ·
`terminal.min_px 8px` · `terminal.line_height 1.00` (f, a multiplier on the FONT's own metrics; **never a
synthetic 1.3**) · `terminal.cursor.underline_h @stroke.thin` (today a flat 1.0) ·
`terminal.cursor.underline_gap 0.3u` (today a flat 1.5) ·
`terminal.cursor.bar_w @stroke.thin` · `terminal.wheel_lines 3` ·
`terminal.indicator.role caption` · `terminal.indicator.inset @space.3` ·
`terminal.selection_pad 0` (selection quads follow the cell grid exactly) ·
`terminal.follow_density false`.

**keyboard** (10) — `keyboard.gap 0.9u` (4.9; today `vh(0.46)`) ·
`keyboard.pad same_as_parent` (= `@keyboard.gap`) · `keyboard.key_corner @corner.sm` ·
`keyboard.key_border @stroke.hair` · `keyboard.label_role body` (today
`font_px(1.05)`) · `keyboard.sub_role caption` · `keyboard.key_h @size.lg` · `keyboard.sub_inset_x 0.4x @type.caption.size` ·
`keyboard.sub_inset_y 0.3x @type.caption.size` · `keyboard.sub_corner top_right` ·
`keyboard.mod_dot 0.16x @keyboard.key_h` · `keyboard.mod_dot_min_px 4px`. **`keyboard.rows` and `keyboard.key_units`
are layout data and stay in the keyboard layout file.**

**editor overlay** (13) — `editor.edge 1.5u` · `editor.edge_min_px 8px` (today a raw `8.0 px`) ·
`editor.min_content 6u` · `editor.min_content_min_px 30px` (today a raw `30.0`) · `editor.handle 1.2u` · `editor.handle_min_px 6px` (today a raw `6.0`) · `editor.proxy.border @stroke.hair` ·
`editor.proxy.border_hot @stroke.regular` · `editor.hint.role tooltip` ·
`editor.hint.inset_x @space.6` (today `w*0.012` — a *width*-relative inset in a
height-scaled interface) · `editor.hint.inset_y 2x @type.tooltip.size` ·
`editor.list.head_h 4u` · `editor.list.pad 1.2u` · `editor.list.pad_min_px 6px` ·
`editor.list.title_gap 0.6x @type.body.size` · `editor.grid.stroke @stroke.hair`.
**`editor.grid.cols` / `.rows` are USER settings, not theme tokens.**

**boards / board switcher** (12) — `boards.tile.max_w_frac 26%` ·
`boards.tile.aspect window` · `boards.tile.gap @space.5` ·
`boards.tile.border @stroke.hair` · `boards.tile.border_current @stroke.regular` ·
`boards.tile.caption_h 2.6u` · `boards.tile.caption_gap 0.6u` ·
`boards.tile.close_size 2.2u` · `boards.tile.close_inset @space.1` ·
`boards.plus_size 2.6u` · `boards.plus_size_min_px 14px` · `boards.plus_stroke @stroke.thin` ·
`board.swipe_edge 4u` · `board.swipe_edge_min_px 20px` · `boardswitch.shade_min 0.35`.

**modal / settings** (16) — `modal.w_frac 40%` · `modal.h_frac 52%` ·
`modal.min_w 60u` · `modal.min_w_min_px 320px` · `modal.min_h 48u` · `modal.min_h_min_px 260px` · `modal.pad 2.8u` ·
`modal.corner @corner.lg` · `modal.corner_mode chamfer` ·
`modal.border @stroke.regular` · `modal.scrim_alpha 0.55` ·
`modal.title.role title.window` · `modal.title.band_h 7.4u` (40.0) ·
`modal.body_top 14.4u` (77.8 — one value, no arithmetic: it is `size.xl + 2 x space.5` in intent and the comment in `default.theme` says so) · `modal.row_gap @space.5` ·
`settings.back_w_frac 22%` · `settings.back_w_min 13u` · `settings.back_w_min_min_px 70px` ·
`settings.list_w_frac 60%` · `settings.grid_cols 3` · `settings.hint.role caption` ·
`settings.hint_inset 1.6x @type.caption.size` · `settings.note.role caption`.

**toast / dialog / boot / empty state / control / filetile** (28) —
`toast.h 16u` (86.4) · `toast.top 20u` · `toast.pad_x 8u` (today `vw(4.0)`, a
*width*-relative pad) · `toast.min_w_frac 50%` · `toast.max_w_frac 90%` ·
`toast.title_gap 0.12x @toast.h` · `toast.msg_gap 0.45x @toast.h` · `toast.corner @corner.lg` ·
`dialog.inset_x 4.4u` · `dialog.inset_y 8u` · `dialog.corner 8u` ·
`dialog.border @stroke.regular` · `dialog.title.role title.window` ·
`dialog.body.role body` · `dialog.button.w_frac 22%` · `dialog.button.h_frac 17%` ·
`dialog.button.y_frac 66%` · `boot.pad_top 6u` · `boot.pad_x 7u` (today `vw(2.0)`) ·
`boot.line_role body` · `boot.logo_role display.hero` · `boot.sub_role caption` ·
`emptystate.role value` · `emptystate.y_frac 40%` · `control.button.h @size.xl` ·
`control.button.gap 0.35x @control.button.h` · `control.button.w_frac 86%` ·
`control.button.align bottom` · `filetile.gap 2u` · `filetile.rows 3` ·
`filetile.cols auto` · `filetile.cell_min_px 20px` · `filetile.corner @corner.sm` ·
`filetile.icon.inset_x 22%` · `filetile.icon.inset_y 12%` · `filetile.icon.w 56%` ·
`filetile.icon.h 50%` · `filetile.icon.stroke @stroke.thin` ·
`filetile.caption_role tooltip` · `filetile.caption_gap 70%` ·
`filetile.wheel_px 8u`.

**The resolution dialog computes its own `u` from its own window height**, not the
desktop's — it is drawn in its own tiny window.

**rhythm** (12) — alignment and vertical rhythm.

| token | default | px | note |
|---|---|---|---|
| `rhythm.baseline` | `1u` | 5.4 | |
| `rhythm.snap_baseline` | `true` | — | `false` for family B, where cards float over a live background and the grid has nothing to lock to |
| `rhythm.snap_origin` | `panel_content_top` | — | |
| `rhythm.row` | `4.8u` → snapped | 27.0 | today `px*1.9` |
| `rhythm.row_compact` | `3.6u` → snapped | 21.6 | |
| `rhythm.label_col` | `auto` | — | `auto \| <n>u \| <n>%`; the widest label **in the block** |
| `rhythm.label_min` | `8u` | 43.2 | |
| `rhythm.label_max` | `45%` | — | |
| `rhythm.label_pad` | `@space.4` | 10.8 | |
| `rhythm.label_align` | `left` | — | |
| `rhythm.value_col` | `auto` | — | reserve a column so unit suffixes line up |
| `rhythm.value_gutter` | `@space.2` | 5.4 | |
| `rhythm.value_align` | `right` | — | **`decimal` is explicitly not offered** on pre-formatted strings |
| `rhythm.center_mode` | `optical` | — | `optical \| geometric` |
| `rhythm.cap_center_bias` | `-0.055` | — | fraction of px |

**Optical centring** replaces the 27 call sites doing `y + (h - px*1.3)/2.0`, which
centre a *synthetic* box (`1.3 × px`) rather than the font's real metrics; for an
all-caps run with no descenders that parks the text visibly low, because a third of
the assumed box is descender space no glyph occupies.
`baseline = box.y + (box.h + cap_height(font, px))/2 + bias*px`. `cap_height` is added
to `FontSystem::line_metrics` (fontdue exposes it; it is a per-(font, px) constant and
belongs in the existing glyph cache, so the cost is one hash lookup that already
happens). Optical mode applies when the run's case transform is `upper` or
`smallcaps` — nearly every label in this interface. Geometric mode stays for
mixed-case body text and for the terminal, where the cell box is authoritative.

**Baseline snapping**: text baselines round to the nearest `rhythm.baseline` from the
panel's content-box top — a `round()`, no layout change. Panel heights come out of a
flexbox solve that produces fractional pixels and changes every frame while a window
is resized or a board is swiped; without snapping a static label shimmers as its
baseline crosses pixel boundaries. Row heights (`rhythm.row`, `list.row_h`,
`table.row_h`, `menu.row_h`) are snapped **at bake time**, which is the only place the
baseline grid moves a rectangle, and it keeps stacked rows from accumulating drift.

**`rhythm.label_col = auto`** is the only proposal here that adds per-frame work: one
measurement pass per block, a few dozen `measure()` calls the cache already serves. It
degrades cleanly to a fixed `<n>u` if the frame budget is ever tight.

---

### 5.26 `component.*` — component colours (≈95)

Each has a default expression naming a semantic role. **This layer is what makes "one
palette re-skins everything" true — a widget never names a literal.**

```
component.panel.fill            = @surface.panel
component.panel.border          = @border.default
component.panel.title           = @text.title
component.panel.glow            = @accent.glow
component.panel.header_underline= alpha(@border.default, 0.30)     # draw.rs:302
component.titlebar.fill         = @surface.inset
component.titlebar.text         = @text.title
component.titlebar.rule         = alpha(@border.subtle, 0.30)      # winframe.rs:322
component.window_control.idle   = @text.muted
component.window_control.hover  = @accent.hover
component.window_control.close_hover = @severity.critical.text

component.hexcell.fill          = alpha(@accent.primary, 0.22)
component.hexcell.fill_top      = alpha(@accent.hover, 0.38)       # gradient end, image 1
component.hexcell.border        = @accent.border
component.hexcell.empty         = @surface.sunken

component.bar.track             = @surface.sunken
component.bar.fill              = @accent.primary
component.bar.fill_warn         = @severity.warning.text
component.bar.fill_crit         = @severity.critical.text
component.bar.text              = @text.primary
component.bar.text_on_fill      = @accent.on                       # ui.rs:138

component.chart.line            = @data.line
component.chart.fill            = @data.fill
component.chart.grid            = @data.grid
component.chart.axis            = @data.axis
component.chart.legend          = @text.muted
component.chart.bg              = @surface.sunken
component.chart.series[0..7]    = @data.series[0..7]

component.gauge.track           = @surface.sunken
component.gauge.value           = @data.line
component.gauge.text            = @text.primary
component.donut.track           = @surface.sunken
component.donut.arc             = @data.line

component.nodegraph.node        = @accent.primary
component.nodegraph.edge        = alpha(@accent.primary, 0.40)

# A badge with a severity takes its four colours from sev[i] AT DRAW TIME, indexed
# by the u32 the producer passed (5.10, 5.17, 7.4). There is no `<r>` metavariable
# in this language and there was never going to be one: the previous spelling,
# `component.badge.fill = @severity.<r>.fill`, is not an expression an author can
# write, and an author who copied it got an unknown-token warning. The five rows
# below are the NON-severity defaults — what a badge with no severity looks like.
component.badge.fill            = @surface.inset
component.badge.edge            = @border.default
component.badge.text            = @text.primary
component.badge.solid_fill      = @accent.primary
component.badge.solid_text      = @accent.on

component.launcher.tile         = @surface.inset
component.launcher.border       = @border.subtle
component.launcher.glyph        = @accent.primary
component.launcher.caption      = @text.muted
component.launcher.active_glyph = @accent.warm                     # image 8's orange HOME
component.launcher.active_border= @accent.warm

component.field.fill            = @surface.sunken
component.field.border          = @border.subtle
component.field.text            = @text.primary
component.field.placeholder     = @text.muted
component.field.caret           = @accent.primary

component.taskbar.fill          = @surface.raised
component.taskbar.border        = @border.default
component.avatar.ring           = @accent.border
component.leader_line           = alpha(@accent.primary, 0.70)
component.leader_label          = @text.muted
component.scale_tick            = @text.instrument
component.datadump              = @text.instrument
component.trace                 = alpha(@accent.primary, 0.08)     # PCB traces
component.vignette              = alpha(@palette.black, 0.55)
component.scanline              = alpha(@palette.black, 0.10)
component.modal.scrim           = @surface.scrim                   # window.rs:10 hard-coded BLACK today
component.editor.grid_line      = alpha(@border.subtle, 0.16)
component.editor.proxy_fill     = alpha(@accent.primary, 0.08)
component.nameplate.fill        = alpha(@surface.raised, 0.90)
# The alarm bar's TEXT is sev[highest_live].text, indexed at draw time by the host,
# which is the only thing that knows which severities are live. `@severity.<highest
# live>.fg` was a runtime query written as a load-time expression, in flat
# contradiction of 6's opening rule ("every one is pure, evaluable at load,
# dependent on no runtime state") and 7.1's "nothing symbolic survives into the
# resolved struct". The theme's knobs are the two below plus topbar.alarm.* (5.25).
component.alarm_bar.fill        = none
component.alarm_bar.text_alpha  = 1.00
component.alarm_bar.edge        = @severity.critical.edge

component.topbar.fill           = @surface.panel
component.topbar.text           = @accent.primary
component.topbar.sep            = alpha(@accent.primary, 0.45)
component.topbar.glyph          = @accent.dim
component.topbar.rule           = @border.subtle

component.render.wire           = @render.wire
component.render.hull           = @render.hull
component.render.rim            = @render.rim
component.render.orbit          = @render.orbit
component.render.planet         = @render.planet
component.render.silhouette     = @render.silhouette

component.dot.fill              = @severity.critical.text
component.dot.border            = @surface.panel
component.dot.count_text        = @severity.critical.on

component.table.head            = @text.secondary            # ui.rs:163, was base/0.5
component.table.rule            = alpha(@border.subtle, 0.70) # ui.rs:257, was base/0.35
component.table.row             = @text.primary              # was base/0.9
component.table.zebra           = alpha(@surface.inset, 0.35) # what zebra_every alternates
component.columns.label         = @text.secondary            # ui.rs:267, was base/0.5
component.columns.value         = @text.primary              # was base

component.script.title          = @text.title                # 5.29
component.script.label          = @text.secondary
component.script.value          = @text.primary
component.script.rule           = @border.subtle
component.script.meter_track    = @component.bar.track
component.script.meter_fill     = @component.bar.fill
component.script.dot_on         = @component.matrix.cell_on
component.script.dot_off        = @component.matrix.cell_off
component.file.glyph_dir        = @accent.primary
component.file.glyph_file       = alpha(@accent.primary, 0.75)
component.file.glyph_link       = @accent.secondary
component.log.line_current      = @text.primary
component.log.line_past         = alpha(@text.primary, 0.60)
component.matrix.cell_on        = @accent.primary
component.matrix.cell_off       = alpha(@accent.primary, 0.20)
component.image.photo.tint_strength = 0.0     # theme may raise to 0.35; NO MOOD may
component.image.photo.tint      = @accent.primary   # what it tints toward
component.image.photo.saturation = 1.0        # theme may lower to 0.35; NO MOOD may
component.image.render.tint_strength = 1.0    # a wireframe IS a drawing; themeable
```

**The photo pair, and why it is a ceiling rather than a lock.** Image 3's
`LIVE FEED: SECURITY B-16` is described in the brief as a photo of figures in a
corridor **"desaturated toward green"**, and §6 cites that exact image as the reason
`sat()` survives the function cull. A rule that forbids the effect while another
section justifies a function by it cannot both be right. So: a **theme** may set
`tint_strength` up to `0.35` and `saturation` down to `0.35` — enough for image 3's
green-graded feed and for image 4's deliberately *natural* thumbnail — and
a **mood** may set neither, at all, ever (§4.4 pass D 3). An alarm must not recolour
evidence; a theme grading its own security feed is a look, and the ceilings keep it
from becoming a claim. Values above the ceiling are clamped and logged with both
numbers. Reference: image 3's pure-green look used `tint_strength = 0.30`,
`saturation = 0.45` (§8).

---

### 5.27 The class × state matrix (49 classes × 7 states)

`•` = the class supports the state and `default` defines it, **and it is authorable as
`<class>.state.<state>.<channel>`** (§5.21). `–` = the class never enters that state; a
theme defining it is warned and the cell is ignored (the theme file is documentation and
a typo must be caught). **The `focus` column is an engine constant, not a token** — it
records which focus treatment the class receives (§5.21: focus is an orthogonal bool on
containers, not a state slot) and there is no `button.focus = ring` to write. The
`focus.*` family has six global tokens and no per-class member; that is a deliberate
scope decision and it is labelled here rather than left to look like a token family
with no names.

One worked cell, so the syntax the warning polices appears at least once:

```ini
[tab]
state.selected.edge       = @accent.primary   # a "•" cell: legal
state.dragging.fill       = alpha(base, 0.2)  # a "–" cell: warned and ignored
```

| class | Idle | Hover | Press | Sel | SelHov | Drag | Disab | focus |
|---|:-:|:-:|:-:|:-:|:-:|:-:|:-:|:-:|
| `button` | • | • | • | • | • | – | • | ring |
| `icon_button` | • | • | • | – | – | – | • | ring |
| `chip` | • | • | • | • | • | – | • | ring |
| `tab` | • | • | • | • | • | – | • | ring |
| `menu.item` | • | • | • | • | • | – | • | ring |
| `field` | • | • | – | – | – | – | • | ring |
| `checkbox` | • | • | • | • | • | – | • | ring |
| `slider.track` | • | • | – | – | – | • | • | ring |
| `slider.knob` | • | • | • | – | – | • | • | – |
| `list.item` | • | • | • | • | • | • | • | – |
| `tile` | • | • | • | • | • | • | • | – |
| `launcher.icon` | • | • | • | • | • | • | – | – |
| `key` | • | • | • | • | • | – | • | – |
| `window` | • | • | – | – | – | • | – | **container** |
| `panel` | • | • | – | • | • | • | – | **container** |
| `dialog` | • | – | – | – | – | – | – | **container** |
| `taskbar.button` | • | • | • | • | • | – | • | ring |
| `titlebar` | • | – | – | – | – | • | – | inherits window |
| `scrollbar.thumb` | • | • | – | – | – | • | • | – |
| `resize.grip` | • | • | – | – | – | • | – | – |
| `meter` | • | – | – | – | – | – | • | – |
| `gauge.donut` | • | – | – | – | – | – | • | – |
| `chart.line` `.bar` `.grid` `.axis` `.legend` | • | • | – | • | • | – | – | – |
| `hexcell` | • | • | – | • | • | – | – | – |
| `badge` | • | – | – | – | – | – | – | – |
| `node` | • | • | – | • | • | – | – | – |
| `avatar.tile` | • | • | – | • | • | – | – | – |
| `table.head` | • | – | – | – | – | – | – | – |
| `table.row` | • | • | – | • | • | – | – | – |
| `log.line` | • | • | – | • | • | – | – | – |
| `term.cell` | • | – | – | • (selection) | – | – | – | – |
| `leader.line` | • | – | – | – | – | – | – | – |
| `scale.tick` | • | – | – | – | – | – | – | – |
| `datadump` | • | – | – | – | – | – | – | – |
| `topbar` | • | – | – | – | – | – | – | – |
| `alarm_bar` | • | – | – | – | – | – | – | – |
| `render` | • | – | – | – | – | – | – | – |
| `dot` | • | – | – | – | – | – | – | – |
| `script.row` | • | • | – | • | • | – | – | – |
| `cycler` `segmented` `tooltip` `toast` `boot.line` `filetile` `boards.tile` `dock` `editor.proxy` `nameplate` | per analogy above | | | | | | | |

`CLASS_COUNT = 64` (49 used, 15 spare). The chart/hexcell/node hover and selected slots
exist because image 1's leader lines label features and image 9's gauges sit in a bar
the pointer travels over: a data element the user can point at must be able to answer.

**Two gaps the inventory exposed, which the class table closes:** `slider.rs` has no
hover, no drag and no disabled — a dragging slider looks identical to an idle one; and
`dropdown.rs:41` changes the fill on hover but not the edge.

---

### 5.28 Not tokens — and why

| constant | where | why it stays code |
|---|---|---|
| `GRID_MAX = 4096` | `plugin.rs:272` | memory safety across the ABI |
| `POLYLINE_MAX = 8192` | `plugin.rs:276` | memory safety across the ABI |
| `MAX_VERTS = 400_000` | renderer | memory safety — **but both overflow paths must report.** `let n = verts.len().min(MAX_VERTS)` drops surplus geometry silently and `blur_base`'s `n + 6 <= MAX_VERTS` gate drops **all** glass at once. Each gains a one-shot `eprintln!` and a monotonic counter surfaced in the debug HUD. Budget and headroom: §5.12 |
| the Rhai element **kinds** (`title`, `text`, `rows`, `meter`, `columns`, `dots`, `table`, `line`, `spacer`) | `script.rs` | the vocabulary is closed by the same argument as §5.22's motion catalogue: an element the toolkit cannot draw has no theme representation. Their **metrics and colours are tokens** (§5.29); their *membership* is not |
| the `share` distribution and the shrink-to-fit `scale = r.h / natural` | `script.rs` | layout arithmetic. The *policy* is a token (`script.overflow`); the arithmetic is not |
| `OFF_SPEC`, `Layout::empty` | `base.rs:267,349` | off-screen sentinels |
| `.round()` on glyph quads | `draw.rs:196` | text crispness |
| `cut.min(w*0.5).min(h*0.5)` | `draw.rs:177` | geometric safety, kept inside `ring()` |
| `item_h - 0.5` float tolerance | `dropdown.rs:34` | numerical |
| `FRAME = 1/60` | `main.rs:622` | engine constant |
| board-ride epsilon 0.05 / 0.001 | `main.rs:1675` | engine constant |
| initial window size 1600×900 | `main.rs:179` | application setting |
| `SESSIONS = 5` | `main.rs` | the application's session array is sized from it; the theme may style a strip drawn for them, not resize it |
| per-widget `ref_h_vh` / `min_h_vh` | `base.rs:241-254` | **LAYOUT file** |
| the default board's basis/grow/collapse/gap | `flex.rs:62-79` | **LAYOUT file** |
| keyboard per-key width units | `keyboard:119-188` | **LAYOUT file** |
| `SIDE_MIN` `SIDE_MAX` `CENTER_MIN`, board padding, portrait breakpoints, collapse floor, control-bar height | `flex.rs` | **[CONFLICT 4] — CUT.** These move panel rectangles. |
| `GridPadding`, snap, grid columns/rows | `main.rs:114`, settings | **USER settings.** A theme must not override them. |
| font-size slider ranges, grid-padding slider range | `settings.rs:261-283` | application data |

---

### 5.29 `script.*` — the Rhai element vocabulary (34)

**This is the largest single drawing surface in the program and it had no tokens at
all.** Eight of the twelve shipped widgets draw *exclusively* through
`libnacelle/src/script.rs` — `clock`, `cpu`, `hardware`, `memory`, `network`,
`processes`, `sysinfo`, `uptime`, i.e. every `nacelle-widgets/widgets/board/*/*.rhai`.
Its whole vocabulary was hard-coded: `title` block height `px*2.6`, `text` height
`px*1.5*size`, `line = px*1.9`, `meter` bar at `line*0.2` / `line*0.6`, `columns` at
`px*2.6`, `dots` cell `vh(0.8)*panel_scale`, every element drawn in `ctx.theme.base` at
a hand-picked alpha (0.5 label, 0.75, 0.9), and `text(content, align, size)` taking a
**raw size multiplier** — so `type.<role>.size`, `.tracking`, `.case`,
`.smallcaps_ratio` and `.tabular` were unreachable from two thirds of the interface,
including `clock.rhai`'s `21:57:30`, which §5.17 uses as its headline argument for
tabular figures. Tokenising the other two surfaces and not this one would have left
goal 1 ("nothing a widget draws may name a literal colour") and goal 2 ("cover
everything that can be drawn") false where they matter most.

**Every element kind maps to a colour role, a type role and an optional severity.**

| element | type role | colour | note |
|---|---|---|---|
| `title` | `title.panel` | `@component.script.title` | the in-body heading; the panel's own band is the host's (§5.12) |
| `text` | named by the call | `@component.script.value` | see the API below |
| `rows` label | `caption` | `@component.script.label` | the `PROCESOR:` half |
| `rows` value | `value` | `@component.script.value` | the `74%` half |
| `meter` track | — | `@component.script.meter_track` → `@component.bar.track` | |
| `meter` fill | — | `@component.script.meter_fill` → `@component.bar.fill` | severity-aware |
| `meter` label / value | `caption` / `value` | `@component.script.label` / `.value` | |
| `columns` label | `caption` | `@component.columns.label` | the label-over-value readout |
| `columns` value | `value` | `@component.columns.value` | |
| `dots` on / off | — | `@component.script.dot_on` / `.dot_off` | → `@component.matrix.cell_on/off` |
| `table` head / rule / cell / zebra | `label.section` / — / `data` / — | `@component.table.head` / `.rule` / `.row` / `.zebra` | |
| `line` | — | `@component.script.rule` | the horizontal rule |
| `badge` | `badge` | `sev[i]` at draw time | new element; §5.17 |

**Stack metrics — the numbers that were `px*k`.**

| token | default | px | replaces |
|---|---|---|---|
| `script.title_block` | `6.8u` | 36.7 | `px*2.6` |
| `script.text_leading` | `1.50` | — | the `*1.5` in `px*1.5*size` (f, a multiplier on the role's leading) |
| `script.row_h` | `@rhythm.row` | 27.0 | `line = px*1.9` |
| `script.element_gap` | `@space.2` | 5.4 | the implicit gap between stack elements |
| `script.pad_x` | `@space.0` | 0 | the host's `panel.content_pad` already inset the box |
| `script.meter_track_h` | `0.20x @script.row_h` | 5.4 | `line*0.2` |
| `script.meter_bar_h` | `@progress.h` | 6.5 | `line*0.6` ≈ 16 px, which §5.25 already calls "not thin" |
| `script.meter_gap` | `@space.1` | 2.7 | |
| `script.columns_block` | `6.8u` | 36.7 | `px*2.6` |
| `script.dots_cell` | `@dotmatrix.cell` | 8.6 | `vh(0.8)*panel_scale` |
| `script.dots_cell_min_px` | `6px` | 6 | companion floor (§3.2) |
| `script.rule_width` | `@stroke.hair` | 1 | |
| `script.spacer` | `@space.4` | 10.8 | |
| `script.overflow` | `scale` | — | **`scale \| clip \| scroll`** — the policy for an overfull panel |
| `script.overflow_min_scale` | `0.72` | — | the floor of the shrink-to-fit; below it, `clip` |

**`script.overflow` is the one that was invisible and mattered.** `script.rs` computes
`scale = r.h / natural` and silently rescales **every font in an overfull panel**,
which means a theme's carefully snapped role pixels (§5.16) quietly become fractional
and every text size in that panel drifts off the ladder. It is now a stated policy with
a floor: `scale` keeps today's behaviour but stops at `overflow_min_scale` and then
clips; `clip` never rescales; `scroll` clips and shows a `scrollbar.*` (§5.25). The
`share` distribution for flexible elements stays engine arithmetic (§5.28) — the policy
is themeable, the arithmetic is not.

**The script API gains a role and a severity, and loses the bare multiplier.**

```rhai
// before: a raw size multiplier and no way to name anything
text(content, align, size)

// after: a role name and an optional severity. Both optional; the old form
// still parses, warns once per script at load, and maps size -> nearest role.
text(content, align, #{ role: "value", severity: "warning" })
rows(items, #{ label_role: "caption", value_role: "value" })
meter(frac, #{ severity: "critical" })
table(cols, rows, #{ head_role: "label.section", cell_role: "data" })
badge(text, #{ severity: "offline" })       // new element
blink(id)                                   // -> motion.value_blink; NOT host.t
```

`severity:` takes a role name or a `u32`; anything unrecognised resolves to `unknown`,
never to `ok` (§5.10). `role:` takes one of the 24 names of §5.16; an unknown name
warns at load and falls back to `body`. **A script never names a colour and never names
a px size**, which is what makes `network.rhai`'s `"STATE", "OFFLINE"` row grey in
every theme and amber in an alarmed one without touching the script.

**`host.t` is replaced by `host.t_motion`.** A script reading the raw clock bypasses
`motion.scale = 0` entirely, so reduced motion could not reach `clock.rhai`'s blinking
colon (§5.22). `host.t_motion` is the same clock scaled by `motion.scale` and frozen
under reduced motion; `host.t` remains, deprecated, warned once per script at load.

---

## 6. DERIVATION FUNCTIONS

**Fourteen. Closed. No more.** Every one is pure `Color → Color` (or `Color → Color`
plus scalars), evaluable at load, and dependent on no runtime state. Two consequences
that the catalogue must respect: there is **no metavariable** (`@severity.<r>.fill` is
not an expression — §5.26) and there is **no runtime query** (`@severity.<highest
live>.text` is not one either). Anything indexed by something only the host knows at
draw time is indexed at draw time, from `sev[]`, and the theme's knob is a plain token.
`composite_as_rendered` (§4.4) is an internal engine routine, not a fifteenth function:
it is not authorable, does not appear in `fn-name`, and exists only so enforcement
measures the pixel the GPU produces. Argument types
are written `c: col`, `t: 0..1`, `k: f32`, `deg: degrees`, `n,i: usize`.

**Working spaces, and why each function is where it is.** `mix` and `over` evaluate in
**linear light** because they model *physical compositing*. `shade`, `tint`, `lum*`,
`sat`, `hue`, `ramp` and `ensure` evaluate in **OKLab / OKLCh** because they model
*perception*. `alpha`, `fade` and `contrast_on` touch no colour channel arithmetic at
all.

| fn | args | space | semantics | why it earns its place |
|---|---|---|---|---|
| `alpha(c, a)` | col, 0..1 | — | **sets** alpha to `a`; rgb untouched | "the accent at 60 %". Used ~40× in `default`; nothing else expresses it. Refuses GTK's multiply-surprise. |
| `fade(c, f)` | col, ≥0 | — | **multiplies** alpha by `f`, clamped to 0..1 | the honest name for GTK's `alpha()`. Needed by `opacity.*`-driven high-contrast expressions. |
| `mix(a, b, t)` | col, col, 0..1 | linear | premultiplied lerp, then un-premultiply; `out.a ≤ 0 ⇒ rgb = 0` | models physical light mixing. `mix(@palette.black, @palette.accent, 0.06)` is *how a tinted near-black background is made* — `#141E1A` is exactly black with a whisper of mint. Every console background in images 1–6 is this. |
| `over(a, b)` | col, col | linear | composites translucent `a` onto opaque `b`, returns **opaque** | **required, not convenient**: `surface.panel` is α 0.82, so the contrast of text on a panel is undefined until the panel can be resolved over the base. This is the **authoring** composite and it models physics. It is **not** what §4.4 measures — enforcement uses the internal `composite_as_rendered`, which mirrors the live blend equation in the live encoding (§2.2, §4.4). Two questions, two answers, both stated. |
| `shade(c, t)` | col, 0..1 | OKLab | perceptual mix toward `@palette.black` | distinct from `mix(c, black, t)`: it moves lightness evenly and drags chroma down smoothly instead of producing muddy midpoints. Every `severity.*.fill`. |
| `tint(c, t)` | col, 0..1 | OKLab | perceptual mix toward `@palette.white` | `accent.hover = tint(accent, 0.18)` lands the azure seed on `#4FC3F7`, the literal highlight of image 6. |
| `lum(c, k)` | col, ≥0 | OKLCh | **multiply** L; hue and chroma exactly preserved | the image-3 function: "severity by brightness because I only have one hue". Not `shade`, which pulls toward black's hue and desaturates. `lum(@accent.primary, 0.62)` stays *the same green*, just dimmer. |
| `lum_min(c, L)` | col, 0..1 | OKLCh | raise L to at least `L` | libadwaita's standalone-colour rule, ported: "readable on this background" as one constant per variant. |
| `lum_max(c, L)` | col, 0..1 | OKLCh | lower L to at most `L` | the light-surface counterpart; used by `text.base`-style expressions. |
| `sat(c, k)` | col, ≥0 | OKLCh | multiply chroma | image 3's photo "desaturated toward green" — which §4.4 pass D 3 and §5.26 now permit a **theme** to do (`component.image.photo.saturation`, ceiling 0.35) while still forbidding a **mood** from doing it; and images 4/5's greyscale hull with a coloured rim, which is `render.hull = sat(@data.line, 0.0)` + `render.rim` (§5.25) — real tokens the recipe writes into, rather than a recipe with nowhere to land. Hand-authoring desaturated variants defeats the one-palette premise. |
| `hue(c, deg)` | col, deg | OKLCh | rotate hue | generates `accent_alt`, the 8-series data ramp and the ANSI sixteen from one seed. Without it "the ANSI sixteen must feel like the theme" costs 16 hand-picked hexes per theme. |
| `ramp(c, n, i)` | col, usize, usize | OKLab | step `i` of an `n`-step L ladder centred on `c`'s L, span 0.62 | lets a theme opt *explicitly* into brightness-coded severity without switching `severity.mode`. Image 3 in one expression. |
| `contrast_on(bg, a, b)` | col, col, col | — | returns whichever of `a`/`b` has the greater WCAG contrast against `bg` | makes `text.inverse` and every `.on` token automatically correct: a crimson `#FF2A35` chip needs light text, an azure `#29B6F6` chip needs dark. Hand-authoring gets this wrong eventually, and unreadable badges in a terminal are a bug. libadwaita hardcodes `white` here and its own docs concede the failure. |
| `ensure(fg, bg, ratio)` | col, col, f32 | OKLCh | walks `fg`'s L away from `bg` until the WCAG ratio is met — 48 bounded steps, hue and chroma held, then clamp | turns §4.4 from an aspiration into a mechanism. |

### 6.1 Cut, and why

| cut | reason |
|---|---|
| `darken` / `lighten` | identical to `shade` / `tint` |
| `dim(c, f)` | today's `Color::dim` — an sRGB per-channel multiply. Makes red vanish while green survives. Replaced by `lum`. |
| GTK's `shade(c, n)` | HSL, and it scales *saturation* as well as lightness. Deprecated by GTK itself. |
| `lighter` / `darker` | HSV value scaling; degenerate at black and white (Qt's documented wart) |
| `hue_to(c, other)` | `hue` with a computed delta, and it invites cycles |
| any HSL/HSV variant | OKLab covers it and HSL's lightness is a lie |
| `lift(c, t)` | proposed as "toward `text.primary`", a *second* target beside black and white. Two targets are enough; `tint` and `lum` cover every use. **[CONFLICT 18]** |
| `gamut(c)` | gamut mapping is **automatic and mandatory** on every OKLCh→sRGB conversion (§6.2); an explicit call would be redundant and would imply it is optional. **[CONFLICT 25]** |
| `on(c)` | sugar for `contrast_on(c, black, white)`. One name per operation. **[CONFLICT 26]** |
| `linear(a, b[, deg])` | a **gradient**, not a colour. Gradients are `@grad.<name>` blocks with 2–8 stops (§5.14), which is strictly more expressive at the same draw cost. **[CONFLICT 13]** |
| `lerp` | `mix` |
| blur / glass parameters | scalars, not colours |

### 6.2 Gamut mapping — mandatory

**Every OKLCh→sRGB conversion gamut-maps by binary-searching chroma down (22
iterations) until the result is inside [0,1]³, holding L and hue fixed.**

Per-channel clamping is **forbidden**. This is not theory: the naive clamp collapsed
two of pure-green's eight data series onto nearly the same colour (ΔE 0.054) and
washed high-lightness accents toward white with the wrong hue. Chroma reduction fixed
it (ΔE 0.108) with no other change. libadwaita clips per channel and its own
documentation confesses the result, marking affected values with an asterisk.

Where the downstream scRGB/PQ pipeline can carry extended range, values are **not**
gamut-mapped until the encode stage; the clamp belongs to the output, not to the
derivation.

### 6.3 Evaluation, caching, cycles

Resolution is a **memoised DAG walk**, not top-to-bottom file evaluation. Order in the
file is irrelevant; **forward references are legal and used.**

1. Parse `default` → every token becomes `Literal(Color)` or `Expr(ast)`.
2. Overlay the loaded theme, then the mood/variant, then the user overlay, replacing
   whole nodes.
3. Resolve with an **explicit stack and three-colour marking** (White unvisited, Grey
   on-stack, Black done). Re-entering a Grey node is a cycle. Depth cap 32.
4. On a cycle: report the full path, then evaluate that token from `default`'s
   expression (§4.2). The compiled-in `default` is cycle-free by construction, which a
   unit test asserts by resolving it.
5. Bake (lengths → px) and **encode** (§6.3 below).
6. Run the enforcement passes (§4.4) **on the encoded values**, and emit.

Steps 5 and 6 are in that order deliberately and §2.2 gives the reason: enforcement
measures a composited pixel, and the GPU composites in the swapchain's encoding.

Total work is O(tokens) and is dominated by file I/O. Every node is evaluated **at
most once**; the memo is a `Vec<Option<Color>>` indexed by `TokenId`, not a map.
**Nothing symbolic survives into the resolved struct** — there is no string map, no
`Expr`, and no `Option` anywhere in `ResolvedTheme` after load. The strings live in
`ThemeDiagnostics`, a separate `Arc` (§7.1).

**Colour representation across the pipeline.** [CONFLICT 20]

* Authoring: sRGB hex, `rgb()`, or `oklch()`.
* Parse: decode to **linear light, straight (non-premultiplied) f32 RGBA**.
* Derive: linear for `mix`/`over`, OKLab/OKLCh for the rest, with automatic gamut
  mapping.
* **Encode (`encode.rs`, keyed on the live swapchain format):**
  `FormatKind::Unorm` (the 8/10-bit path, `_UNORM`, no hardware sRGB encode) →
  **sRGB-encode**, so an authored `#3FE3AE` lands on screen as `#3FE3AE`, matching
  today's behaviour exactly. `FormatKind::ScRgbLinear` (`R16_SFLOAT`, where the
  compositor reads linear light) → **leave linear**. Without this stage the HDR path
  is silently wrong and looks like a theme bug.
* Store: **straight alpha, never premultiplied.** The blend state is
  `SRC_ALPHA / ONE_MINUS_SRC_ALPHA` — straight-alpha blending — so the draw-list
  builder must not premultiply.

---

## 7. THE RESOLVED STRUCT

### 7.1 The Rust shape

```rust
// ---------- primitives ----------
#[repr(C)] #[derive(Clone, Copy, PartialEq)]
pub struct Color { pub r: f32, pub g: f32, pub b: f32, pub a: f32 }
// straight (non-premultiplied) alpha, in the live swapchain's encoding

#[repr(u8)] pub enum State { Idle=0, Hover=1, Press=2, Selected=3,
                             SelectedHover=4, Dragging=5, Disabled=6 }
pub const STATE_COUNT: usize = 7;

#[repr(u8)] pub enum Elev { Backdrop=0, Board=1, Panel=2, Raised=3,
                            Focused=4, Popover=5, Inset=6, Overlay=7,
                            Fixture=8 }          // APPENDED, never inserted
pub const ELEV_COUNT: usize = 12;      // 9 used, 3 spare

#[repr(u8)] pub enum Severity { Ok=0, Info=1, Warning=2, Critical=3,
                                Contained=4, Offline=5, Unknown=6 }
pub const SEVERITY_COUNT: usize = 7;

#[repr(u8)] pub enum CornerStyle { Square=0, Round=1, Chamfer=2 }
#[repr(u8)] pub enum Falloff { Linear=0, Quad=1, Gauss=2, Halo=3 }
#[repr(u8)] pub enum GlowMode { Off=0, Shell=1, Sprite=2, Text4=3 }
#[repr(u8)] pub enum Case    { None=0, Upper=1, Lower=2, Title=3, SmallCaps=4 }
#[repr(u8)] pub enum GradAxis{ X=0, Y=1, DiagDown=2, DiagUp=3, Angle=4 }

// ---------- generated token ids (append-only, NEVER renumbered) ----------
#[repr(u32)] pub enum ColorToken  { SurfaceVoid=0, SurfaceSunken=1, /* ... */
    // The indexed families are CONTIGUOUS RANGES of ColorToken, not separate
    // arrays. This is what makes @data.series[3] and @term.ansi[4] legal as an
    // icon layer colour, a type role's fg, or any component default. Without it
    // an icon layer could reach ~150 of the ~174 colours the engine resolves and
    // nobody could say which 24 were missing.
    DataSeries0 = 200, /* ..= DataSeries7 = 207 */
    TermAnsi0   = 208, /* ..= TermAnsi15  = 223 */ }
#[repr(u32)] pub enum ScalarToken { MetricUnitPx=0, SpaceHair=1,   /* ... */ }
pub const COLOR_COUNT:  usize = 256;   // ~174 used, incl. the two ranges above
pub const SCALAR_COUNT: usize = 512;   // ~470 used
pub const FLAG_WORDS:   usize = 8;

// ---------- material ----------
#[repr(C)] #[derive(Clone, Copy)]
pub struct Edge   { pub color: Color, pub color2: Color, pub gradient: u8,
                    pub axis: GradAxis, pub mode: u8, pub width: f32 }
#[repr(C)] #[derive(Clone, Copy)]
pub struct Glow   { pub color: Color, pub radius: f32, pub falloff: Falloff,
                    pub mode: GlowMode, pub enabled: u8,
                    pub inherit: u8,        // 1 = "element": use the drawn thing's
                    pub _pad: u8,           //     own colour. NEVER a negative alpha.
                    pub alpha: f32, pub boost: f32 }
#[repr(C)] #[derive(Clone, Copy)]
pub struct Shadow { pub color: Color, pub radius: f32, pub dx: f32,
                    pub dy: f32, pub spread: f32 }
#[repr(C)] #[derive(Clone, Copy)]
pub struct Reflect{ pub height: f32, pub alpha: f32, pub fade: f32 }
#[repr(C)] #[derive(Clone, Copy)]
pub struct Surface {                     // ELEV_COUNT of these — MATERIAL
    pub glass_rank: u8, pub corner: CornerStyle, pub _pad: u16,
    pub glass_tint: Color, pub glass_wash: Color, pub fill: Color,
    pub radius: f32,
    pub edge: Edge, pub inner_glow: Glow, pub outer_glow: Glow,
    pub shadow: Shadow, pub reflect: Reflect,
}

// ---------- interaction ----------
#[repr(C)] #[derive(Clone, Copy)]
pub struct ClassStyle {                  // CLASS_COUNT x STATE_COUNT — COLOUR + WEIGHT
    pub fill: Color, pub edge: Color, pub text: Color, pub glyph: Color,
    pub edge_width: f32, pub glow_radius: f32, pub glow_strength: f32,
    pub elevation: f32,
}                                        // 80 bytes
pub const CLASS_COUNT: usize = 64;       // 49 used

#[repr(C)] #[derive(Clone, Copy)]
pub struct SeverityStyle {                // member names match 5.0's naming law
    pub text: Color, pub fill: Color, pub edge: Color,
    pub glyph: Color, pub on: Color,
    pub glyph_id: u16, pub badge_style: u8, pub pulse: u8, pub rank: f32,
}

#[repr(C)] #[derive(Clone, Copy)]
pub struct Motion {
    pub duration_ms: f32, pub period_ms: f32, pub amplitude: f32,
    pub floor: f32, pub duty: f32, pub easing: u8, pub enabled: u8,
    pub _pad: u16, pub easing_p: [f32; 4],
}
pub const MOTION_COUNT: usize = 24;      // 18 used (value_blink added, 5.22)

// ---------- typography ----------
#[repr(C)] #[derive(Clone, Copy)]
pub struct TypeRole {                    // authored; px still relative
    pub face: u8, pub case: Case, pub tabular: u8, pub _pad: u8,
    pub size_u: f32, pub min_px: f32, pub max_px: f32,
    pub tracking_em: f32, pub leading: f32,
    pub smallcaps_ratio: f32, pub synthetic_bold: f32,
    pub fg: u32 /* ColorToken */, pub alpha: f32,
}
#[repr(C)] #[derive(Clone, Copy)]
pub struct Type {                        // BAKED; what a draw call needs, absolute px
    pub face: u8, pub case: Case, pub tabular: u8, pub _pad: u8,
    pub px: f32,            // already .round()ed, >= type.min_px
    pub tracking_px: f32, pub leading_px: f32,
    pub smallcaps_px: f32,  // 0.0 when not small caps
    pub bold_px: f32,       // 0.0 when no synthetic bold
    pub fg: Color,
}
pub const ROLE_COUNT: usize = 24;
pub const FACE_COUNT: usize = 8;

// ---------- shapes, gradients, icons ----------
#[repr(C)] #[derive(Clone, Copy)]
pub struct ShapeSpec {
    pub corners: [Corner; 4], pub segments: u8, pub border_style: u8,
    pub tab_open: u8, pub chevron_dir: u8,
    pub border_width: f32, pub border_color: Color,
    pub fill: Color, pub fill_to: Color, pub fill_angle: f32,
    pub dash: f32, pub gap: f32, pub phase: f32,
    pub bracket_len: f32, pub bracket_inset: f32,
    pub tab_slant: f32, pub chevron_depth: f32, pub glow_class: u32,
}
pub const SHAPE_COUNT: usize = 16;

#[repr(C)] #[derive(Clone, Copy)]
pub struct Gradient { pub stops: [Color; 16], pub count: u8,
                      pub axis: GradAxis, pub _pad: u16, pub angle_deg: f32 }
pub const GRAD_COUNT: usize = 8;

#[repr(C)] #[derive(Clone, Copy)]
pub struct IconDef { pub cp: [u32; 3], pub layer_color: [u32; 3], pub layers: u8,
                     pub _pad: [u8; 3] }
pub const ICON_COUNT: usize = 64;
pub const GLOW_CLASS_COUNT: usize = 16;

// ---------- the whole thing ----------
#[repr(C)]
pub struct ResolvedTheme {
    pub abi_version: u32,
    pub struct_size: u32,        // sizeof at build time
    pub epoch: u32,
    pub format_kind: u32,        // which encode stage produced this

    // baked scale
    pub unit_px: f32,            // u
    pub panel_scale_min: f32, pub panel_scale_max: f32,
    pub density_space: f32, pub density_type: f32,
    pub hit_pad: f32, pub grab_pad: f32,

    pub colors:  [Color; COLOR_COUNT],
    pub scalars: [f32;   SCALAR_COUNT],   // every metric, already in px
    pub flags:   [u32;   FLAG_WORDS],

    pub elev:    [Surface; ELEV_COUNT],
    pub classes: [[ClassStyle; STATE_COUNT]; CLASS_COUNT],
    pub sev:     [SeverityStyle; SEVERITY_COUNT],
    pub motions: [Motion; MOTION_COUNT],
    pub glows:   [Glow; GLOW_CLASS_COUNT],
    pub roles:   [TypeRole; ROLE_COUNT],
    pub shapes:  [ShapeSpec; SHAPE_COUNT],
    pub grads:   [Gradient; GRAD_COUNT],
    pub icons:   [IconDef; ICON_COUNT],
    pub faces:   [u8; FACE_COUNT],        // resolved alias table
    // NOTE: there are no separate `ansi` / `series` arrays. Both live inside
    // `colors[]` at the contiguous ColorToken ranges declared above, and any
    // accessor named `ansi(i)` / `series(i)` is a VIEW: `colors[TermAnsi0 + i]`.
    // A second storage would make them unaddressable from IconDef.layer_color
    // and TypeRole.fg, which are ColorToken indices.
}

// ---------- diagnostics: a SEPARATE Arc, published beside the POD ----------
pub struct ThemeDiagnostics {                 // NOT #[repr(C)], NOT POD, NOT on
    pub name:        Vec<(LangTag, String)>,  // any draw path, NOT in ThemeC
    pub description: Vec<(LangTag, String)>,
    pub author: String, pub family: u8, pub schema: u32,
    pub warnings: Vec<String>,
}

const _: () = assert!(std::mem::size_of::<ResolvedTheme>() <= 64 * 1024);
```

**`meta` is out of `ResolvedTheme`, and that is a correctness fix rather than
tidiness.** §7.2 asserts `assert_impl_all!(ResolvedTheme: Send + Sync +
Copy-able-by-memcpy)` and the whole justification for the layout — and for `ThemeC`
being a *projection* rather than a rebuild — is that the struct is POD. A
`Vec<String>` is not `Copy`; memcpy-ing one produces two owners of one heap allocation
and a double free. The localised `name[<lang>]` / `description[<lang>]` tables had the
same problem *and* made `size_of::<ResolvedTheme>()` depend on how many locales a theme
declares, so the `<= 64 KB` const assertion could not compile as written either. The
load path now publishes **two** `Arc`s (§2.2): `Arc<ResolvedTheme>`, pure POD, and
`Arc<ThemeDiagnostics>`, which owns every string the theme has. `ThemeC` becomes a
straight memcpy of a prefix.

`Ctx` additionally carries `type_cache: [Type; ROLE_COUNT]` plus the `panel_scale` it
was built for. Panels already set and reset `panel_scale` on entry and exit; the cache
is rebuilt there — **once per panel per frame**, not per draw call. Rebuild is 24 ×
(two multiplies + a `round`). `ctx.ty(Role::TitlePanel)` is an array index.

### 7.2 Memory, against the per-frame budget

| member | bytes |
|---|---|
| `colors` 256 × 16 | 4 096 |
| `scalars` 512 × 4 | 2 048 |
| `classes` 64 × 7 × 80 | 35 840 |
| `elev` 12 × 192 | 2 304 |
| `shapes` 16 × ~120 | 1 920 |
| `roles`, `motions`, `glows`, `sev`, `grads`, `icons`, `ansi`, `series` | ~4 300 |
| **total** | **≈ 50 KB** |

Eight resolved variants (moods × contrast) is ≈ 400 KB — a rounding error against a
process that already holds a 4 MB glyph atlas and two 33 MB decoration plates.

**Why this layout.** Draw-path access is `theme.classes[class as usize][state as
usize]` or `theme.scalars[Tok::PanelContentPad as usize]` — a field offset and a bounds
check the compiler elides. A class's seven states are contiguous, so a widget that
crossfades hover touches one cache line. This is GTK's own hot-path design
(`GtkCssStyle` is an array indexed by a property-id enum, never a map) and QPalette's
(`br[group][role]`), and it is the *only* shape that survives thousands of lookups per
frame. The alternative — a `HashMap<String, Color>` — costs a hash, a probe and a
cache miss per lookup, which at 12 000 terminal cells is the frame.

**Banned anywhere in `ResolvedTheme`:** `HashMap`, `String`/`&str`, `Cow`, `RefCell`,
`Box<dyn>`, `Vec`, any `Option` requiring a branch, any allocation. Enforced by the
const size assertion, an `assert_impl_all!(ResolvedTheme: Send + Sync +
Copy-able-by-memcpy)` test, and a review rule. The assertion is now *true*: with `meta`
moved into `Arc<ThemeDiagnostics>` (§7.1) there is no `Vec<String>` inside the struct
and no locale count that can change its size.

**Growth is append-only.** New tokens are appended to the end of their sub-array's
used range (the arrays are already oversized), new sub-structs are appended at the end
of `ResolvedTheme`, and `ColorToken`/`ScalarToken` ids are **never renumbered and never
reused**. `struct_size` lets a plugin built against an older header know where its
knowledge ends. `abi_version` bumps only on a breaking layout change.

### 7.3 How a Rust widget reads it

```rust
let t = ctx.theme;                                   // &ResolvedTheme
let s = &t.classes[Class::Button as usize][st as usize];
ctx.dl.rect(r.x, r.y, r.w, r.h, s.fill);
ctx.dl.rect_outline(r.x, r.y, r.w, r.h, s.edge_width, s.edge);
// text goes through the ROLE, so case, tracking, small caps, synthetic bold and
// tabular advance are applied by FontSystem rather than by the caller (5.16)
ctx.text_role(Role::Button, x, y, label, s.text, Align::Left);
// a severity-coloured element indexes sev[], it never names a colour (5.10)
let sv = ctx.severity(sev_idx);                      // &SeverityStyle
ctx.badge(badge_rect, sev_idx, BadgeStyle::FromSeverity, "CONTAINED");
```

`ctx.ty(Role::Button)` still exists and returns the baked `Type` for callers that need
the metrics themselves (measuring, laying out a column); `ctx.text_role` is what
actually draws, because a caller that has `px` and a `Color` has already lost `case`
and `smallcaps_ratio`.

### 7.4 How a widget behind the C ABI reads it

`ABI_VERSION` 4 → **5**. **Nineteen** entries **appended** to `HostApi`; nothing is
reordered or removed.

```rust
// --- theme, appended to HostApi -------------------------------------------
/// The whole resolved theme for THIS frame. Valid until the next epoch change;
/// a widget must not cache the POINTER across frames, only values.
pub theme_snapshot: extern "C" fn(ctx: *mut c_void) -> *const ThemeC,
/// sizeof(ThemeC) in the HOST's build. A plugin reads min(host_size, its own
/// sizeof) bytes and no more.
pub theme_size:     extern "C" fn() -> u32,
/// Increments whenever the host swaps the resolved theme (reload, mood, variant,
/// resize, format change). A plugin caching derived geometry invalidates on change.
pub theme_epoch:    extern "C" fn(ctx: *mut c_void) -> u32,
/// The slow path, for tokens added after this plugin was built. Documented as
/// "call at widget init, cache, invalidate on epoch"; NEVER inside a draw loop.
/// An unknown id returns default's value plus a one-shot warning / NaN.
pub theme_color:    extern "C" fn(ctx: *mut c_void, token: u32) -> ColorC,
pub theme_scalar:   extern "C" fn(ctx: *mut c_void, token: u32) -> f32,
/// The font scale of the panel being drawn.
pub panel_scale:    extern "C" fn(ctx: *mut c_void) -> f32,
/// The BAKED type role for the panel currently being drawn: absolute px, already
/// rounded, tracking and leading in px, `fg` resolved to a Color. Contract:
/// "call once per panel, after panel_scale is known; never per element."
/// ThemeC does NOT carry [TypeRoleC; 24] — see below.
pub theme_type:     extern "C" fn(ctx: *mut c_void, role: u32) -> TypeC,
/// One severity, fully resolved. The only way a plugin colours by severity.
pub severity_style: extern "C" fn(ctx: *mut c_void, i: u32) -> SeverityStyleC,
/// Resolve an icon NAME to an IconId, once. Documented "call at widget init,
/// cache the u32, invalidate on epoch". This is the ONE place a string crosses
/// the ABI, at init only, never in a draw loop — without it "no strings cross
/// the ABI" would make icons unreachable rather than cheap.
pub icon_id:        extern "C" fn(ctx: *mut c_void, name: *const u8, len: u32) -> u32,

// --- recipes: the host draws, the widget says WHAT and WHERE ---------------
/// Role-aware text. The host applies case, tracking, small caps, synthetic bold
/// and tabular advance from roles[role]; the plugin supplies bytes and a tint.
/// `tint` may be the sentinel COLOR_ROLE_DEFAULT to take the role's own fg.
pub text_role:    extern "C" fn(ctx: *mut c_void, role: u32, r: RectC,
                                text: *const u8, len: u32, tint: ColorC, align: u32),
/// Its measure twin — same rules, so a plugin's layout matches its drawing.
pub measure_role: extern "C" fn(ctx: *mut c_void, role: u32,
                                text: *const u8, len: u32) -> f32,
/// A severity pill: fill, edge, glyph, label, all from sev[severity]. `style`
/// is FromSeverity | Outlined | Solid.
pub badge:        extern "C" fn(ctx: *mut c_void, r: RectC, severity: u32,
                                style: u32, text: *const u8, len: u32),
/// The decoration-plate / wallpaper image registry (5.15, M12). Allocate once,
/// update in horizontal bands, retire at process exit.
pub image_register: extern "C" fn(ctx: *mut c_void, w: u32, h: u32, fmt: u32) -> u32,
pub image_update:   extern "C" fn(ctx: *mut c_void, id: u32, band_y: u32,
                                  band_h: u32, px: *const u8, len: usize) -> u32,
pub image_retire:   extern "C" fn(ctx: *mut c_void, id: u32),
/// Emits a whole elevation recipe: shadow -> glass/fill -> edge -> glows ->
/// reflection. A widget draws a themed panel in one call and cannot get it wrong;
/// the recipe may change in any release without an ABI break.
pub surface:  extern "C" fn(ctx: *mut c_void, r: RectC, elev: u32, state: u32),
/// `inherit` is used when the class's colour is the inherit sentinel.
pub glow:     extern "C" fn(ctx: *mut c_void, r: RectC, class: u32, inherit: ColorC),
/// One shape preset: corner ring, border style, fill (flat or gradient).
pub shape:    extern "C" fn(ctx: *mut c_void, r: RectC, shape: u32, state: u32),
/// One icon, already resolved (layers, colours, container) by the host.
pub icon:     extern "C" fn(ctx: *mut c_void, id: u32, r: RectC, state: u32, tint: ColorC),
```

`ThemeC` is `#[repr(C)]` with a leading `{version: u32, size: u32, epoch: u32,
_pad: u32}` header and is a **curated flat projection** of `ResolvedTheme` — not the
whole tree. Exposing the host's internal layout would couple every plugin to it and
make any refactor an ABI break. It carries: the semantic colours, the seven
severities, eight data series, the sixteen ANSI colours, `[[ClassStyleC; 7]; 64]`,
`[SurfaceC; 12]`, `[MotionC; 24]`, the metric scalars, and `u32` font handles (never
paths, never names).

**It does NOT carry `[TypeRoleC; 24]`.** `TypeRole` is the *authored* struct —
`size_u`, `min_px`, `max_px`, `smallcaps_ratio`, `synthetic_bold`, `fg: u32` — and §7.1
is explicit that `Type` is "BAKED; what a draw call needs". Handing a plugin
`TypeRoleC` would make it redo bake step 5 itself, per role, per frame: multiply
`size_u` by `unit_px * density_type * panel_scale`, clamp, `round()`, then index the
colour array. That moves theme resolution back onto the per-frame path across the ABI,
which goal 10 forbids, and it *guarantees* drift — the host's rounding and the plugin's
rounding disagree by a pixel and plugin text stops sitting on the same baseline as
toolkit text in the same panel. `theme_type(ctx, role) -> TypeC` returns the baked
thing, with the contract "call once per panel, after `panel_scale` is known". Engine
state stays in the engine.

**Two lifetime rules, because the mood path is designed to change the theme while the
program is running.** (1) **`ThemeC` is materialised once per epoch**, not per frame —
it carries `[[ClassStyleC; 7]; 64]`, so a per-frame rebuild would be ~40 KB of copying
per frame for nothing — and **the previous projection is retained for at least one full
frame after a swap**, the same retirement discipline `Gfx::retired` already applies to
staging buffers. (2) **A theme swap is applied at a frame boundary only, never
mid-frame.** Without those two, a mood trigger (evaluated once per second, host-side),
a resize or a `set_color_depth` can swap the `Arc` between two `PluginApi::draw` calls
**in the same frame** and free a `ThemeC` a widget has already dereferenced —
use-after-free across the C ABI, on the one path built to fire while the user is
looking at it.

**Rules that must not be lost.**

* **No strings cross the ABI on a draw path.** `class`, `state`, `elev`, `severity`,
  `motion`, `shape`, `role`, `icon` and `token` are small integer enums frozen by
  `ABI_VERSION`. An out-of-range value returns the `Unknown`/`Idle`/disabled fallback —
  never a panic, never a wrong colour. **`icon_id(name)` is the single exception and it
  is an init-time call**, documented as "call at widget init, cache the `u32`,
  invalidate on epoch". Without it §5.19's "a widget names an icon; the name resolves
  to a codepoint once, at theme load" is unimplementable from a plugin: there was no
  way to obtain an `IconId` at all, and the obvious workaround is the thing the rule
  forbids.
* **Case, tracking, small caps and tabular figures are the host's job, always.** A
  plugin that reimplements the two-size small-caps run allocates a `String` per label
  per frame behind an ABI whose stated rule is zero per-draw allocation. `text_role` /
  `measure_role` exist so it never has to; the raw `text` / `measure` / `module_title`
  entries stay at their offsets, deprecated, and are removed from every first-party
  widget.
* **The host resolves state before the widget sees anything.** A widget never asks
  "am I hovered, which colour do I use"; it indexes with the state it already knows,
  or calls `surface()`/`shape()` with it.
* **One call fills a whole struct**, never one call per channel: a channel-wise
  accessor would be four function-pointer crossings per drawn element.
* **A widget caches at the top of its draw, not per element.** The shell widget
  drawing 12 000 terminal cells makes ONE snapshot read, not 12 000.
* **`theme_base`, `theme_bg`, `text`, `measure` and `module_title` REMAIN as fields**,
  deprecated, at their existing offsets — `theme_base`/`theme_bg` resolving to
  `accent.primary` and `surface.base`, `text`/`measure` behaving exactly as today, and
  `module_title` drawing nothing now that the host owns the panel's title band (§5.12).
  Removing any of them would shift every subsequent field offset and break every ABI-4
  plugin, which `runtime::attach`'s version check cannot save (it refuses an *older
  host*, not an older plugin). Each logs a one-shot deprecation naming its replacement,
  and each is removed from every first-party widget. **[CONFLICT 16]**
* `ABI_VERSION` 4 → 5 is a **coordinated rebuild**: existing plugins in
  `nacelle-widgets/plugins` must be rebuilt. The attach check makes this loud, not
  silent.

---

## 8. THEMES TO SHIP

*(Amended 2026-08-16: the owner removed the eight variant themes from the
binary. `default` is now the only compiled-in look; every other theme is a
user file on the search path, written by hand or by the editor. This section
stays as the record of the original seeds and their derivations — nothing
below except `default` ships anymore.)*

Nine looks, as originally shipped. `default` is the master document; the other
eight were seeds plus deviations. Every "five defining colours" row is
**accent · surface.base · text.primary · severity.critical · data.line**.

| # | theme | one-line identity | images | the five |
|---|---|---|---|---|
| 0 | **`default`** | The master. Every token declared with a comment; **flat and cheap** — no glass, no glow, no decoration, no shadow, no reflection, so a silent theme renders correctly and skips five render passes. Mint seed. | — (the substrate of all ten) | `#3FE3AE` · `#0B1310` · `#D2E5DC` · `#ED5F1E` · `#3FE3AE` |
| 1 | **`aurora`** | Mint console with a cyan secondary: gradient hexagons, PCB traces, ribbon waves, the full image-1 dressing. | 1 | `#3FE3AE` · `#0B1310` · `#D2E5DC` · `#ED5F1E` · `#3FE3AE` |
| 2 | **`spring`** | Near-monochrome spring green on black. **One accent hex and nothing else** — the proof of the cascade. | 2 | `#4FE07A` · `#0B140D` · `#D3E5D6` · `#ED5F1E` · `#4FE07A` |
| 3 | **`pure`** | The most terminal-like: one saturated green does everything. The only theme that flips `severity.mode = mono`, so `CRITICAL` is bright green and `CONTAINED` dim olive. Hexagons solid, not gradient. | 3 | `#33DD44` · `#0B140B` · `#D2E6D1` · `#5AFA63` · `#33DD44` |
| 4 | **`crimson`** | Alert console: everything red, with amber `CONTAINED` surviving and the LIVE FEED thumbnail keeping natural photographic colour. Black background, red traces, heavy vignette. | 4 | `#FF2A35` · `#1B0D0C` · `#F7D7D3` · `#F0574E` · `#FF2A35` |
| 5 | **`lockdown`** | `crimson`'s chrome with `palette.data = #35A7FF`. Panel borders, titles and leader lines stay red; the station wireframe, planet, orbit lines and every plot go blue. **The two-hue behaviour is one line.** | 5 | `#FF2A35` · `#1B0D0C` · `#F7D7D3` · `#F0574E` · `#35A7FF` |
| 6 | **`azure`** | Calm blue console: thinner glows, more empty space reading as depth, a starfield instead of traces, no alarm in the top bar. | 6 | `#29B6F6` · `#0B1217` · `#D2E2EC` · `#EE5470` · `#29B6F6` |
| 7 | **`cockpit`** | Family B, night: deep glass over a wallpaper, generous Gaussian glow, strong depth, a magenta→violet gradient frame on the focused window, spectrum ribbons. Glass rank rises 1→3 up the ladder (clamped by `Gfx::glass_ranks()`, §5.12); edge alpha 0.85→1.00; shadow 3.9u→6.4u. | 7, 10 | `#4FE8FF` · `#0A2230` · `#DCEEF6` · `#FF4A4A` · `#4FE8FF` |
| 8 | **`instrument`** | Family B, flat: the anti-cockpit. Less depth, more density, a screen rather than a hologram. Navy plate with a grid and traces, blur pinned at level 1 for every elevation, short shadows, half the glow, chamfered focused frame, nearly opaque popovers. Density `instrument`. | 8, 9 | `#23C9E8` · `#060B18` · `#D6E6F2` · `#FF5A48` · `#23C9E8` |

**Common structure of the six console palettes.** Only the five seeds are authored;
everything else derives. Surfaces sit at fixed OKLab lightness
(`void 0.115 / sunken 0.152 / base 0.178 / panel 0.232 α0.82 / inset 0.283 /
raised 0.330`) with the accent hue at low chroma, and text at
(`title 0.870 / primary 0.905 / secondary 0.755 / muted 0.590 / disabled 0.435 /
instrument 0.372`). **One derivation, applied six times** — that is the mechanism
behind "the same layout re-skinned six times by changing the palette alone", and
every one of the six passed every contrast and separation floor with zero failures
(worst values: `text.primary` 12.86 vs floor 7.0; severity fg 4.52 vs 4.5; min ansi
4.56 vs 4.5; min severity ΔE 0.117 vs 0.115).

**Authored deviations, all of them, all traceable:**

* `aurora` — `palette.accent_alt = #43D9E8` (the default `hue(accent, 32)` gives a
  greener cyan than image 1 shows).
* `pure` — `severity.mode = mono`, `severity.chroma = 0.92`,
  `ornament.hex.fill_to = @ornament.hex.fill` (solid, not gradient),
  `component.image.photo.tint_strength = 0.30` + `.saturation = 0.45` +
  `.tint = @accent.primary` (image 3's `LIVE FEED: SECURITY B-16` is explicitly
  "desaturated toward green"; §5.26 caps this at 0.35 / 0.35 and no mood may touch it),
  and `render.rim = @accent.primary` over the default greyscale `render.hull` — image
  3's "greyscale with green rim light" in one more line.
* `crimson` and `lockdown` — `severity.contained.text = #E8B33A` (image 4 states it)
  and `severity.warning.text = #FF7A00`. Pinning contained collides with the derived
  amber warning at ΔE 0.096 — too close to tell apart — so warning is moved to the hot
  orange image 1 already uses for `DAMAGE BREACH`. Resulting ΔE 0.127.
* `lockdown` — `palette.data = #35A7FF`. That is the entire two-hue feature.
* `azure` — `accent.hover = #4FC3F7` (the default `tint(accent, 0.18)` gives
  `#56C6F4`, near-identical, but the image names a value and the theme should say it).
* `cockpit` / `instrument` — roughly 80 of the ~190 material keys each, plus their
  gradients and decoration. Everything else is inherited.

**`default` turns everything off.** `glass.rank = 0` on every elevation,
`edge.width = @stroke.hair`, every `glow.<class>.enabled = false`,
`shadow.radius = 0u`, `reflect.height = 0u`, `decor.enabled = false`,
`backdrop.source = solid`. That is the inheritance proof the owner asked for: a theme
that says nothing renders flat, correct and fast.

---

## 9. MIGRATION

### 9.1 Deleted outright

| what | where |
|---|---|
| `parse_css`, `strip_css_comments` | `nacelle-desktop/src/config.rs:1752-1831` |
| `resolve()`, `load_theme`, `load_theme_from`, `default_theme_config`, `canonicalize_components` (the theme half) | `config.rs` |
| the `Look=` / `Style=` key logic and its "a look wins and clears the components" rule | `config.rs` |
| `list_themes()`'s `look/` directory + metafile `Name=` scan | `config.rs:132-150` |
| `struct Theme`, `Theme::tron`, `Theme::from_edex_json`, `Theme::load`, `default_ansi` | `libnacelle/src/theme.rs` (all 203 lines) |
| `Color::dim` (sRGB per-channel multiply) | `theme.rs:42` — replaced by `lum` |
| `nacelle-themes/style/` — all 10 `*.css` | the whole directory |
| `nacelle-themes/look/` — all 10 theme directories, their metafiles and symlinks | the whole directory |
| `NACELLE_THEME` (eDEX JSON) | `theme.rs:137` |
| `winframe::Metrics::new(screen_h)` | replaced by the baked struct |
| `geometry::control::button_rects`, `geometry::shell::tab_rects` | plugins read the baked struct; the duplication is admitted in a comment at `shell/src/lib.rs:102-107` |
| the duplicate `panel_font_scale` | `main.rs:1627-1631` duplicates `base.rs:445-449` — delete the copy, do not re-tokenise it twice |

`Color` **survives** (it is the draw list's currency) but moves to
`libnacelle/src/theme/color.rs` and gains linear/OKLab conversion. `xterm_256` survives
unchanged.

### 9.2 Kept, untouched

`nacelle-themes/layauts/` and `sounds/`, the `.layaut` format, `Layaut=`, `Sounds=`,
`asset_dirs`, `find_asset`, `data_dirs`, `safe_component`, the whole of `flex.rs`,
`LayoutSpec`/`LayoutDef`/`FlexLayaut`, `outer_layout`, `padded()`, `def.pick(screen)`,
the board model, the per-panel intrinsic sizing pass and `panel_scale`.

### 9.3 What replaces the `.conf` keys

| old | new | note |
|---|---|---|
| `Look=<name>` | `Theme=<name>` | matches `[meta] name`, or the file stem |
| `Style=<name>` | **deleted** | the theme *is* the style |
| `Layaut=<name>` | `Layaut=<name>` | unchanged |
| `Sounds=<set>` | `Sounds=<set>` | unchanged |
| — | `Mood=<name>` | default `normal`; the settings panel and the API also set it |
| — | `Contrast=<normal\|high>` | selects the `hc` variant |
| — | `Density=<airy\|comfortable\|compact\|dense\|instrument>` | user override of `metric.density` |
| — | `Decor=<none\|static\|all>` | user override of `performance.decor` |
| — | `ReducedMotion=<off\|system\|on>` | user override of `a11y.reduced_motion` |
| `BlurRadius=` | **kept, redefined as a ceiling** | it sets `gfx.set_blur_radius()` and therefore `blur_depth ∈ {1,2,3}`, which is how many pyramid targets get written. It becomes a **clamp on the theme's `glass.rank`**: `rank = min(theme_rank, Gfx::glass_ranks())`, applied after the theme in the same stage as `Density=` and `Decor=`, and logged per clamped elevation. |
| `BlurOpacity=` | **kept, redefined as a multiplier** | it already *is* `frost_wash`, the alpha of the wash over every glass quad. It becomes a **multiplier on every `elev.*.glass.wash` alpha**, same stage. |
| `UIFontSize=`, `TermFontSize=`, `GridPadding=` | unchanged | they are the user's, not the theme's |

`BlurRadius=` and `BlurOpacity=` are listed because both describe quantities the theme
also describes, and two owners with no stated precedence is the defect. The rule is the
one `performance.decor` already set: **the user sits over the theme, and the direction
is always toward cheaper and calmer** — a ceiling on a rank, a scale on an alpha, never
a raise. A user who has turned blur down to 20 % does not get a theme quietly asking
the GPU for a pyramid level that was never rendered.

A `Look=` still present in an existing `.conf` is read once, mapped to `Theme=` if a
theme of that name exists, reported, and then ignored. There is **no migration of the
old CSS themes** — the owner's instruction is that every old theme is deleted and the
new ones written from scratch from the images.

### 9.4 On-disk layout

```
~/.local/share/nacelle-desktop/themes/<name>.theme          # a bare file, or
~/.local/share/nacelle-desktop/themes/<name>/theme.theme    # a directory, when the
                                                            # theme ships assets:
                                                            #   fonts/*.otf
                                                            #   icons/*.otf, icons.map
                                                            #   wallpaper/*.jpg
~/.config/nacelle-desktop/theme.local                       # the settings-UI overlay
```

`nacelle-themes/Makefile` installs `themes/ layauts/ sounds/`; `look/` and `style/`
die with the old engine and the installer must follow.

### 9.5 What the settings UI must now offer

1. **Theme** — a list from `list_themes()` (now scanning `themes/`), showing
   `[meta] name` in the user's language, `description`, `family`, and **the load
   warnings for the selected theme**, so a broken theme explains itself in the UI
   rather than only in the terminal.
2. **Mood** — `normal / alert / lockdown` (only those the theme declares), with a
   "follow telemetry" toggle for the declarative predicate.
3. **Contrast** — `normal / high`.
4. **Density** — the five levels, with a live preview panel.
5. **Reduced motion** — `off / system / on`.
6. **Decoration** — `none / static / all`, with the plate memory cost shown
   (33 MB at 1440p, 66 MB at 4K) so the trade is visible.
7. **Reload theme** — re-runs the load path and swaps the `Arc`.
8. The settings toggles above write to `theme.local`. The THEME EDITOR does not:
   its SAVE writes the theme being edited, SAVE AS writes a new one.
   *(Amended 2026-08-16 with rule 5 and question 12.)*
   The split is deliberate. A toggle like reduced-motion is a property of the
   PERSON and should follow them across themes, which is what an overlay does.
   A border colour is a property of the THEME, and an overlay would carry it
   onto every other theme the owner switched to — including one it was never
   designed for. The overlay also lands after the variant layer, so a colour
   written there would quietly defeat `[variant.hc]` and switch high contrast
   off as a side effect of picking a colour.

The existing per-widget settings (`UIFontSize`, `TermFontSize`, `GridPadding`, grid
snap/columns/rows) stay exactly where they are and keep their current meaning.

**And what the command line must offer, because the settings panel is the wrong tool
for the persona this format targets.** Everything else in this document is built for a
person editing a file by hand, and the loop that person currently has is: edit, restart
the desktop, look at the screen, guess. `lum(@accent.primary, 0.62)`,
`ensure(@palette.data, @surface.panel, 3.0)` and the eight-entry generated
`data.series` ramp are unguessable by inspection, and §4.4 can move an authored colour
with the note visible only on a running process's stderr. The load path is already a
pure function from bytes to a resolved struct; exposing it as a command is two hundred
lines and the largest ergonomics win available.

```
nacelle-desktop --check-theme <path> [--screen 1920x1080] [--density compact]
    Runs steps 1–6 of §2.2 and prints the full diagnostic list (§4.3).
    Exits 0 with warnings; exits non-zero only when [meta] strict = true and an
    error was reported. This is the thing a Makefile and a CI job call.

nacelle-desktop --dump-theme <path> [glob]
    Prints  token = authored-expression -> resolved value  for every matching
    token, one per line, with the file:line of the winning declaration and the
    whole default chain (§5.0) for anything that came from a chain:

      $ nacelle-desktop --dump-theme lockdown 'severity.*'
      severity.critical.text  = #FF2A35            lockdown.theme:12
      severity.warning.text   = #FF7A00 -> #FF8F48 lockdown.theme:34 (pass B)
      severity.ok.text        = hue(@palette.accent, 142) -> #35C07A  (derived)
      severity.ok.fill        = alpha(shade(@severity.ok.text,0.78),0.88) -> #0F2C1E
```

Both are the existing load path plus a printer. `[meta] watch` stays `false` as the
shipped default — a theme in a user's directory must not hold an inotify watch — but
its comment in `default.theme` tells an author to set it `true` while editing, which is
the other half of the same loop.

### 9.6 Order of work

1. `theme/color.rs` + `theme/expr.rs` + the 14 functions + gamut mapping. Port the
   verification harness (`palette.py` / `themes.py`) function-for-function and make
   its output a **golden test**: resolving the six console seeds must reproduce §8's
   hexes exactly.
2. `tokens.rs` generation + `ResolvedTheme` + `FALLBACK`.
3. `parse.rs` + `cascade.rs` + `resolve.rs` + diagnostics.
4. `enforce.rs` + `bake.rs` + `encode.rs`.
5. `default.theme` — every token, every comment. This is the long pole and it is the
   documentation.
6. `draw.rs` additions (`ring`, `soft_box`, `quad_c`, `rect_grad`, `fan_c`,
   `image_uv`, `push_clip`); `font.rs` additions (8 faces, 2048² atlas **with the
   reserved mask band and a second page**, dirty-rect upload, `mask`,
   `figure_advance`, `cap_height`, no panic).
7. `Ctx::u/gu/stroke/ty/text_role/measure_role/severity` and the conversion of all 62
   `vh`/`font_px` call sites.
   **7a. The host draws the panel container** (§5.12): the panel loop in
   `nacelle-desktop/src/main.rs` emits `elev[panel.elev]` + `shape.panel` + the title
   band + `panel.button.*` before `wg.draw()`, and `Ctx` hands the widget its content
   box. `module_title` becomes a deprecated no-op. **Nothing in §5.25's `panel.*` block
   draws a pixel until this step lands**, so it is not optional and it is not last.
   **7b. `Elev::Fixture`** replaces the hard-wired `blur` + `rect(bg, frost_wash)` at
   `main.rs:1841` and `main.rs:1742`.
8. `HostApi` + `ABI_VERSION` 5 — all nineteen appended entries, including
   `text_role`/`measure_role`, `icon_id`, `theme_type`, `badge`/`severity_style` and
   the image registry; rebuild the four plugins.
   **8a. `script.rs`** (§5.29): the element vocabulary reads `script.*` and
   `component.script.*`, `text()` takes a role, `rows`/`meter`/`text`/`table`/`badge`
   take `severity:`, `host.t` becomes `host.t_motion`. **This is two thirds of the
   shipped widgets and it must not trail the four plugins.**
   **8b. `ui.rs`** `table` and `columns` take their roles and colours from
   `component.table.*` / `component.columns.*`.
9. The eight shipped themes. *(Done, then removed 2026-08-16 — only `default`
   remains compiled in; see §8's amendment.)*
10. Renderer: additive blend (R1), per-run clip (R2), per-run glass **rank** (R3)
    including `Gfx::glass_ranks()`, the host image registry behind
    `create_texture`/`update_texture`, and the two `MAX_VERTS` overflow diagnostics.
11. `plate.rs` + the worker thread.

**Verification duty (from `scope-boundary.md`): prove the panels did not move.**
Render the same layout at the same screen size before and after, and diff the panel
rectangles. A screenshot diff of the OLD default theme's geometry against the NEW
default theme's must show colour and material differences only — never a panel edge in
a different place. This is a test, not an assertion.

---

## 10. OPEN QUESTIONS

Each is phrased as a question with a **recommended answer**. An implementer who does
not hear otherwise takes the recommendation; none of them blocks the work.

1. **Is the hexagon strip in image 1 pointy-top or flat-top?**
   The two agents who touched it disagreed and only one gave a reason.
   *Recommended:* **pointy-top** (`shape.hex.orientation = pointy`), because pointy-top
   hexes have flat vertical sides and therefore tile a single horizontal row with no
   vertical offset, which is what "five hexagons in a strip" needs. **Please confirm
   against the image — it is one token to flip.**

2. **Should `corner.mode = round` really be the panel default?**
   It is what all ten images show, but it is a visible change from today's
   chamfer-everywhere eDEX language. **Recomputed from `ring()`'s actual point count**,
   not from the earlier estimate: a ring with four round corners at
   `corner.segments = 6` has **28 perimeter points**, giving a 28-triangle centroid fan
   for the fill plus a 28-quad (56-triangle) border strip — **≈84 triangles per panel
   against the 12 a rect costs, so ≈2 900 extra at 40 panels for the panels alone**.
   Every preset that defaults to `round` pays it too: badges, chips, tiles, fields,
   icon tiles and the 60-key on-screen keyboard take the total to **≈7 400 extra
   triangles** on the image-9 deck. Against §5.12's ≈173 600-vert budget that is still
   ~4 % of one frame's geometry and ~0.6 % of `MAX_VERTS`.
   *Recommended:* **yes, `round` for `panel`/`card`, `chamfer` retained for `winframe`
   and `modal`** — but on the recomputed number, not the old one. The mixed default
   keeps the eDEX character where the window chrome is, and a theme that disagrees sets
   `corner.segments = 3` and gets most of it back.

3. **Does the accessible severity mode ship, and is it user-selectable?**
   `severity.mode = mono_strict` scores ≈0.080–0.086 under all three dichromacies
   versus 0.019–0.067 for hue mode, at the cost of dropping to ≈0.085 for normal
   vision — and on a red accent it still degrades under protanopia (0.038).
   *Recommended:* **ship it as a mode, do not ship it as a theme, and expose it in the
   settings UI as "colour-blind severity" alongside the contrast toggle.** It is one
   token; hiding it would be the wrong call for an accessibility feature, and making
   it a whole theme would double the palette maintenance.

4. **Is 66 MB of decoration plates at 4K acceptable?**
   The alternative is a second `REPEAT` sampler plus tiled decoration textures — a
   renderer change — or hundreds of quads per frame.
   *Recommended:* **accept it, and default `performance.decor = all` with `none`
   one click away.** Revisit only if the project starts targeting low-memory GPUs.

5. **Should `motion.hold.duration` (`HOLD_SECS`, the 5 s tear-off) be a theme token
   at all?** It is interaction timing rather than aesthetics.
   *Recommended:* **keep it in the catalogue, clamped 1.5–8 s, but move the user-facing
   control to the settings UI**, so a theme can style the hold progress bar without
   being able to make the gesture unusable.

6. **`ornament.dump.source = random` invents content.**
   It is decoration a *theme* can enable with no widget, which is required by "the same
   layout re-skinned by the palette alone", and it is seeded and reproducible.
   *Recommended:* **keep it, off by default, always with its heading, and always in
   `text.instrument`** — the one role that is contrast-exempt precisely because it is
   non-informational. If that still reads as the toolkit inventing data, delete the
   `random` value and keep `telemetry`; nothing else changes.

7. **`data.dump` cannot reach image 9's 3–4 px density at 1080p.**
   The global 8 px font floor stops it; capped at 13 px so it does not become readable
   body text on 4K.
   *Recommended:* **accept the 1080p compromise.** The alternative is dropping
   `type.min_px` below 8, which would let any role become unreadable.

8. **Should `metric.unit_max_px = 10px` stay the default given the planned VR
   cockpit?** In VR the angular size of a panel is set by its placement in 3D, not by
   texture resolution, so the right ceiling there is much higher.
   *Recommended:* **keep 10 px as the desktop default, keep the token per-theme, and
   make it per-output when the VR path lands.** A VR theme writes one line.

9. **Do per-surface severity overrides (`surface.glass.severity.<r>`) need to exist for
   family B?** Glass over a bright planet may need lifted severities.
   *Recommended:* **not in v1.** Ship the global `[severity.*]` and see whether
   a real family-B theme actually needs it against a real wallpaper; if it does, add exactly one
   override section rather than reinstating KDE's per-set repetition (80 of 96 colour
   lines in `BreezeDark.colors` are duplicates).

10. **Which family-B theme owns image 10?** Image 10 is "image 8 at night" in
    composition but carries the cockpit palette.
    *Recommended:* **`cockpit` owns 7 and 10, `instrument` owns 8 and 9**, following
    the agent who owned family B. If the owner reads image 10 as image 8 re-lit, the
    fix is to move two gradient definitions between two files.
    *(Moot since 2026-08-16: both family-B themes left the binary with the other
    variants; the split stays only as guidance for whoever writes them as user
    themes.)*

11. **`icon.stroke` is live but only affects the built-in vector fallback.**
    *Recommended:* **keep the token and keep the warning** rather than deleting it — a
    theme on a bare system genuinely needs it, and a silently ignored token is worse
    than a documented narrow one.

12. **Should the settings UI be able to edit a theme, or only override it?**
    **DECIDED 2026-08-16 by the owner: edit it.** SAVE writes the theme being
    edited; SAVE AS writes a new one; the built-in `default` — since 2026-08-16
    the only one — is materialised into the user's theme directory on its first
    save, so the embedded master is never touched.
    This overrides the recommendation that used to stand here ("override only,
    plus an export button"), and rules 5 and 8 above were amended with it.

    What the recommendation was protecting is kept as a constraint on the write
    rather than a ban on writing: a save patches the bytes of the value spans it
    changes and leaves every comment and every other byte alone. Regenerating a
    file from the AST is forbidden — `strip_comment` (`parse.rs`) drops comments
    before parsing and `Document` has nowhere to hold them, so a round-trip
    through the AST would silently delete the reasoning this project keeps in its
    theme files.

    Two things this decision leaves open, and they are not small:
    - **Values that span more than one line** would break span patching, because
      `value_span.len` measures the joined text while `.line` remembers only the
      first. The embedded `default.theme` — the only shipped file since
      2026-08-16 — has none today. A save must refuse rather than corrupt.
    - **Where a theme lives is not answerable from outside the engine.**
      `FsThemes` is private and `available_themes()` returns names only, so the
      editor cannot ask which file backs a theme or where it may write. That API
      has to exist before SAVE can.

13. **`rhythm.label_col = auto` is the only per-frame measurement pass in the spec.**
    *Recommended:* **ship it as `auto`.** It is a few dozen `measure()` calls per panel
    that the glyph cache already serves; if a profile ever shows it, change one token
    to a fixed `<n>u` and nothing else moves.

---

## Appendix A — the conflict register

Every place the seven agents disagreed, the decision, and the reason.

| # | conflict | decision | reason |
|---|---|---|---|
| 1 | Reference sigil: `$palette.x` + `@role.x` (d1) vs `@` everywhere (s1, s2) | **`@` for everything** | The layer is already visible in the path (`palette.` prefix). Two sigils is two ways to say "look this up". |
| 2 | Metric unit: `u` (u1) vs `ux` = px@1440p (d2) vs `vh`/`em` (d3) | **`u`**; `ux` accepted as a deprecated alias (1 ux = 0.13889u); `vh`/`vw` deprecated | A type ladder in vh and a spacing ladder in u drift apart the moment the screen changes size. All of d2's material lengths are converted in §5.12. |
| 3 | States: 7 slots + orthogonal focus + alarmed-as-mood (u2) vs 6 slots including Focused and Alarmed (d2, d3, s1, s2, and the brief's wording) | **7 slots + focus flag + alarmed mood** | u2's model is the only one derived from an exhaustive inventory of the real code (41 genuine state sites). Focus re-roles one channel and dims a subtree; a state slot re-bakes a whole `Surface`. Alarmed re-skins the *whole screen*, which is a mood by construction. The brief's list is still satisfied: its `active` = `Selected`, its `focused` = the flag, its `alarmed` = the mood. |
| 4 | `layout.*` tokens: board padding, column min/max, portrait breakpoints, control-bar height, `panel.gutter`, density scaling column widths (u1) | **CUT, all of it** | `scope-boundary.md` is the owner's line and overrides the spec: a theme may change what a panel looks like inside its rectangle, never the rectangle. `panel.pad` is renamed `panel.content_pad` because `padded()`/GridPadding is the user's. |
| 5 | Six *surface* levels (d1) vs six *elevations* + Inset + Overlay (d2) | **Both, mapped one-to-one** | They are different axes: `surface.*` is the colour ladder, `elev.*` the material ladder. Each `Elev.fill` defaults to a `@surface.*` reference (§5.12), so nothing is duplicated. |
| 6 | Contrast metric: WCAG 2.x with verified floors (d1) vs APCA Lc (u2) | **WCAG enforced, APCA computed and reported** | u2's technical argument is right, but d1 measured six complete palettes against WCAG floors and they pass with margin. Switching the enforced metric would require re-deriving every palette against numbers nobody has verified. Both are computed; one is binding. |
| 7 | Default severity mode: `hue` (d1) vs `mono_plus_warning` (u2) vs literal roots never derived from accent (s1) | **`hue` by default; explicit per-role overrides always win** | d1's hue-pull-with-a-±14°-clamp was verified numerically across six palettes with zero failures. s1's requirement is still met: amber survives an all-red theme because `crimson` wrote `severity.contained.text` explicitly. `mono_plus_warning` ships as a mode because image 4 is literally it. |
| 8 | Colour storage: linear (d1) vs "whatever the format wants" (d2's M9) vs today's sRGB-encoded u8/255 | **Derive in linear/OKLab; store in the swapchain's encoding** | The only self-consistent reading. `Unorm` → sRGB-encode (matching today exactly), `ScRgbLinear` → leave linear. Without the encode stage the HDR path is silently wrong. |
| 9 | Cycle: reject the whole theme, loud (d1, s1, s2) | **Per-token fallback to `default`'s expression, loud** | "This program must never fail to start because a theme file is wrong" outranks. It is not silently black — it is `default`'s value, reported with the full cycle path in the log and the settings panel. |
| 10 | Unknown key: load error (s1) vs warn+ignore (d1, u1, s2). Unknown function: reject (d1) | **Warn + fall back, always; `[meta] strict` raises the log level but never refuses to start** | Same rule as 9. A theme written for a newer engine must degrade, not refuse. |
| 11 | Alpha sugar: `accent 0.85` (d3) vs `#RRGGBB @ 0.85` (d2) vs `alpha()` only | **`x / 0.85`**, matching CSS Color 4 and `oklch(L C H / a)` | `@` already means "reference"; overloading it is a parser trap. `#4FE8FF / 0.85` stays far more readable than `#4FE8FFD9`, which is why the suffix survives at all. |
| 12 | Image 8's orange active icon has no token in d1's closed set | **Add `accent.warm`**, default `@severity.warning.text` | Nothing derived from cyan produces orange, and the colour is used by more than one component, so d1's "put it in `component.*`" rule would duplicate it. One token, one line, one image. |
| 13 | Gradients: `linear(a,b)` as a *colour function*, 2 stops (s1, s2) vs `grad.<name>` blocks with 2–8 OKLab stops resolved to 8 samples (d2) | **d2's** | An 8-stop gradient is exactly as cheap at draw time as a 2-stop one once it is pre-resolved, and a gradient is not a colour — making it one puts a non-colour in every colour slot. This also closes s2's "BLOCKER". |
| 14 | Decoration: per-frame quads / registered images (u1, s1, s2) vs two CPU-baked screen-sized plates (d2) | **Plates** | The sampler is `CLAMP_TO_EDGE` with `mip_levels = 1`: tiling is *impossible*. 2 quads/frame beats 720 triangles/frame for scanlines alone. The 33–66 MB cost is stated, not hidden, and has a kill switch. |
| 15 | Hexagon orientation: `pointy` (u1) vs `flat-top` (d3) | **`pointy`** | Only u1 gave a reason, and it is correct: pointy-top hexes have flat vertical sides and tile a horizontal strip with no offset. Flagged as open question 1. |
| 16 | Delete `theme_base` / `theme_bg` (s2) | **Retain as deprecated fields** | Removing them shifts every subsequent `HostApi` field offset. `runtime::attach` refuses an *older host*, not an older plugin, so removal would silently corrupt ABI-4 plugins. They are removed from all first-party widgets instead. |
| 17 | Glow modes: `none/rings/sprite/bloom_pass` (u1) vs `Off/Shell/Sprite` chosen by radius + `Text4` (d2) | **d2's four modes + `auto`**, `bloom_pass` refused | `bloom_pass` does not exist in the renderer and `default` must not name a mode it cannot draw. `rings` == `shell`. Mode by radius is a mechanical rule, not a mood. |
| 18 | `lift(c, t)` toward `text.primary` (u2) | **Cut; use `tint`/`lum`** | Two derivation targets (black, white) are enough, and both are literals by rule, which is what keeps `shade`/`tint` structurally cycle-free. u2's severity ladder is re-expressed in `lum`. |
| 19 | Type ladder: 24 semantic roles in `size_vh` (d3) vs a 9-step generic ladder in `u` (u1) | **d3's 24 roles, authored in `u`** (`size_u = size_vh × 2.6`) | u1 marked its ladder provisional and its binding demand was only "express it in u" — which is now satisfied exactly: `type.body = 2.6u` is today's `font_px(1.0)`. Every `*_size` token in u1's component tables names a role instead. |
| 20 | Premultiplied storage (d1) | **Straight alpha** | The blend state is `SRC_ALPHA / ONE_MINUS_SRC_ALPHA` — straight-alpha blending. Premultiplying in the draw-list builder would double-apply alpha. |
| 21 | Theme name `mint` (d1) vs `aurora` (s2) | **`aurora`**, and `default` is a separate flat master | Image 1 is titled "Aurora / mint console", and the task requires `default` to be the master carrying every setting — which must be the *cheap* one, or "a silent theme is a fast theme" is false. |
| 22 | Severity count: 7 (d1, u2) vs 6 (s1, s2) vs 8 statuses incl. `active` (d3) | **7** | `unknown` is the ABI's safety valve: a widget compiled against a later enum sending severity 9 to an older host must resolve to `unknown`, never to `ok`. d3's `active` is not a severity — it is `State::Selected`; its "in progress" use maps to `info`. |
| 23 | Corner tessellation: 4 segments (u1) vs 6 (d3) | **6** | d3 gave the measurement: 0.4 px chord error at an 8 px radius. |
| 24 | Density scales column min/max widths (u1 §4.4) | **Cut** | It changes how many columns survive, which moves panel rectangles — [CONFLICT 4]. Density now acts strictly inside panels. |
| 25 | `gamut(c)` as a callable function (s1) | **Cut; mapping is automatic** | Making it optional invites a theme to skip it, and skipping it is the observed bug that collapsed two data series. |
| 26 | `on(c)` (s1) vs `contrast_on(bg, a, b)` (d1) | **`contrast_on`** | Explicit, already used throughout d1's verified default expressions, and it lets a theme choose the two candidates rather than assuming black/white. |

---

## Appendix B — renderer work this specification depends on

Ordered by ratio of unlocked design to lines of code. None of it blocks the theme
engine landing; each degrades as stated.

| # | change | size | without it |
|---|---|---|---|
| **R1** | **Additive blend pipeline** — `ADD_ATLAS = ImageId(u32::MAX-8)`, one more pipeline (`fs_main` again, `SRC_ALPHA/ONE` colour, `ZERO/ONE` alpha), `Bound::Add` in `record_runs` | ~40 lines, 0 extra passes, 0 extra memory | Glow over a bright backdrop reads as a milky film. The resolver multiplies every `glow.*.alpha` by 0.8 and a holographic theme degrades gracefully. |
| **R2** | **Per-run clip** — `DrawRun.clip: Option<[f32;4]>` + `cmd_set_scissor` (already dynamic state), `DrawList::push_clip/pop_clip` maintaining a stack intersected into each new run | ~30 lines | Ribbons must be clipped geometrically per sine quad. Charts, the terminal and every scrolling list want this too — it unblocks four other areas. |
| **R3** | **Per-run glass RANK** — reserve a handle band `glass_rank(k) = ImageId(u32::MAX-1-k)`; the base-scene scan treats any id `>= u32::MAX-4` as glass; `record_runs` maps rank → **a pyramid target that this frame's `blur_depth` actually wrote** (rank 1 → target 1 always; rank 2 → target 2 only when `blur_depth >= 2`, else 1; rank 3 → target 3 only when `blur_depth == 3`, else the deepest written), plus `Gfx::glass_ranks() -> u8` so the resolver clamps at bake. **Prerequisite: the base-scene target is cleared at alpha 1.0** (§5.5). | ~25 lines, 0 extra passes | Blur radius stays one global scalar. A holographic theme that specifies per-level blur degrades to one rank; the elevation ladder loses its clearest depth cue. **Naively mapping `blur_targets[l.clamp(0,3)]` is not an option**: target 0 is the *unblurred* base scene and targets 2–3 are unwritten at low `blur_depth`, so a legal token value would sample an image in `UNDEFINED` layout. |
| **R4** | `DrawList` colour/UV API: `quad_c`, `rect_grad`, `fan_c`, `image_uv` | libnacelle only | No gradients (five of ten images need them), no drifting scanlines. |
| **R5** | `FontSystem::mask()` + atlas 1024²→2048² | libnacelle only, 4 MB | No soft glow, no soft shadow, no round-corner fills; glow falls back to `shell` everywhere. |
| **R6** | Second glass generation (`elev.glass.generations = 2`) | ~60 lines, +1 blit and +4 passes per generation ≈ +0.35 ms at 1440p | Popover-over-focused-over-panel does not genuinely re-blur. **Default 1; the ladder does not need it.** |
| **R7** | `fs_blur` UV transform (push-constant `mat2x3`; the block is 16 bytes of a guaranteed 128) | ~20 lines | `reflect` stays the honest fake (§5.12). No parallax glass. |
| **R8** | `composite_alpha = PRE_MULTIPLIED` + alpha-capable surface + swapchain `clear` to `[0,0,0,0]`. **Prerequisite, not optional: the OFFSCREEN base-scene clear stays at alpha 1.0** — `fs_blur` emits `blurred.a * tint.a`, so clearing the base scene transparent deletes the entire glass layer of any family-B theme (§5.5, §5.12). R8 changes the *swapchain* clear only. | swapchain change | `backdrop.source = passthrough` resolves to `solid` with a load warning. Declared, not scheduled. |

---

## REVIEW RESPONSES

Three adversarial reviews were run against the previous revision of this document.
Every **blocker** and every **major** finding is resolved in place, in the section it
belongs to, and is listed in the index below with where the fix landed. This section
exists for the remainder: **minor findings that were resolved differently from the way
the reviewer proposed, or refused outright**, each named with its reason. A finding is
never dropped silently — that is the same rule §4.2 imposes on the engine.

### Resolved, with where

| finding | resolved in |
|---|---|
| The Rhai script path has no tokens (blocker) | **§5.29** (new), §2.1, §5 preamble, §5.26, §9.6 step 8a |
| Nothing draws the panel container (blocker) | §5.12 z-order + "who draws z 21–25", §5.25 `panel` block, §2.1, §9.6 step 7a |
| No role-aware text/measure across the C ABI (blocker) | §2.3, §5.16, §7.3, §7.4 (`text_role`, `measure_role`) |
| Severity has no channel from producer to pixel (blocker) | §4.5, §5.10 call-site table, §5.17, §5.29, §7.4 (`badge`, `severity_style`) |
| R3 samples pyramid targets that were never rendered (blocker) | §5.12 `glass.rank`, §9.3, Appendix B R3 |
| No route from `plate.rs` to the GPU (blocker) | §5.15 DECISION M12, §7.4 image registry, §2.1 |
| Quoting is undefined; the worked example does not parse (blocker) | §3.2 quoting rule, §3.3, §3.4, §5.23, all examples normalised |
| No scoping rule for `[mood.*]` / `[variant.*]` (blocker) | §3.2 `overlay-section`, §3.4, §5.23, §5.24 |
| Five families own the same drawn property (blocker) | §5.0 "One owner per drawn property", §5.18, §5.12 |
| `default.theme` is never shown (blocker) | **§5.0b** (new), §5 preamble's source/resolved columns |
| Top status bar has no metrics (major) | §5.25 `topbar` (16 tokens) |
| Photo tint lock contradicts image 3 (major) | §4.4 pass D 3, §5.26, §6 `sat`, §8 `pure` |
| Central render has no tokens (major) | §5.25 `render` (13 tokens), §3.4 |
| Fixture boards have no elevation (major) | §5.12 `Elev::Fixture`, §7.1, §2.1 |
| `BlurRadius=` / `BlurOpacity=` have two owners (major) | §5.12, §9.3 |
| `idle_cap` freezes the terminal cursor (major) | §5.22 prohibition 4 + exemptions, `value_blink` |
| Panel window controls sized off the wrong root (major) | §5.25 `titlebar.h`, `panel.button.*` |
| Plate upload cost hidden (major) | §5.15 DECISION M12 |
| Five cyclic sources escape the cap (major) | §5.22 prohibition 4, §5.15, §5.20 |
| Enforcement composites in the wrong space (major) | §2.2, §4.4, §5.5, §6, §6.3 |
| `ThemeC` exposes authored, not baked, type (major) | §7.4 `theme_type` |
| No `icon_id`, no themed text for plugins (major) | §7.4 |
| Mask atlas can be reset under the material layer (major) | §5.12, §2.1 |
| Atlas budget undercounted (major) | §5.16 |
| `ResolvedTheme` cannot be POD with `meta` (major) | §7.1 `ThemeDiagnostics`, §2.2, §4.2, §7.2 |
| `theme_snapshot` has no lifetime rule (major) | §7.4 |
| Ribbon cost quoted per screen (major) | §5.15 `max_hosts` |
| R8 deletes the glass layer (major) | §5.5, §5.12, Appendix B R8 |
| No vertex budget; both overflow paths silent (major) | §5.12 frame-cost table, §5.28 |
| Three state-authoring syntaxes (major) | §5.21 |
| No production for indexed / localised keys (major) | §3.2 |
| `ratio` under-specified (major) | §3.2, §5.25 (all ratios rewritten `@`-qualified) |
| `<length> min <n>px` has no grammar (major) | §3.2, §5.23, §5.25 (companion `_min_px` tokens) |
| `<r>` metavariable and a runtime query in `component.*` (major) | §5.26, §6 |
| `density` vs `density_space` precedence and naming (major) | §5.3, global rename |
| Unknown-key suggestion misses renames (major) | §4.2 alias table |
| No offline validator (major) | §9.5 `--check-theme` / `--dump-theme` |
| Reflection forbidden on the board (minor) | §5.12 |
| Notification dot has no tokens (minor) | §5.25 `dot` |
| `ui.rs` table/columns colours (minor) | §5.25, §5.26, §5.27, §2.1 |
| Series and ANSI unaddressable from icons and roles (minor) | §7.1 `ColorToken` ranges, §5.19 folder example |
| Sentinel table contradicts itself (minor) | §5.0, §5.13, §7.1 |
| Scanline drift will alias (minor) | §5.15 |
| Two frame-cost figures wrong (minor) | §5.12, §10 q2 |
| Five inheritance mechanisms uncollected (minor) | §4.1 |
| Diagnostics discard the span (minor) | §4.3 |
| §5.27's `focus` column looks authorable (minor) | §5.27 |
| Heading counts wrong in four sections (minor) | §5.7, §5.9, §5.16, §5.18 |

### Refused, or resolved differently, with the reason

**1. "Rename `severity.<r>.fg → .text`, `.bg → .fill`, `.border → .edge`" — adopted,
but the old spellings are kept as aliases rather than deleted.**
The rename is right and it is done, everywhere, including `SeverityStyle`. What is
refused is the *removal*: `severity.critical.fg` still resolves, through §4.2's alias
table, with a note naming the new key. Deleting a spelling that appears in every
existing draft, in the shipped verification harness (`palette.py`, `themes.py`) and in
the reviewer's own findings would make a correct old theme silently render as `default`
— which is the exact failure mode §4.2 exists to forbid. Growing the catalogue by five
aliases costs five rows in a static table and nothing at runtime.

**2. "Split `border.*` into `border.<role>` and `border.edge.*`" — adopted, with the
flat spellings kept as aliases.** Same reason. `border.width` is written by
`[variant.hc]`, was written by every original shipped theme, and appears in half this
document's own prose; a rename with no alias would silently drop the high-contrast
variant's border weight.

**3. "Delete `shape.<p>.border_color` / `.fill` / `.border_width` where they duplicate
`elev.*`" — refused; they are defined as `same_as_parent` instead.**
Deleting them removes a real capability: a card whose ring differs from its material is
a thing image 8 shows. The defect was never the existence of two tokens, it was that
nothing said which won. §5.0 and §5.18 now say: for the four elevated presets they
default to `same_as_parent` and read from `elev.*`; setting one is legal and emits a
**note**, not a warning. One owner, one documented override, no deletion.

**4. "Delete `edge.color2` and `edge.mode`; express two-colour edges as a 2-stop
`@grad`" — refused; `color2` is defined as sugar that bakes into an anonymous two-stop
gradient (§5.12).** `GRAD_COUNT = 8` is a small budget and a two-colour panel edge is
the common case; forcing every one of them through a named `[grad.x]` block would spend
named slots on things that do not deserve names, and would make the most ordinary
material edit in the format require two sections instead of one line. The redundancy
the reviewer objected to — *two ways with no stated winner* — is gone: `edge.gradient`
wins when set, `color2` is ignored with a note, and anonymous gradients never consume a
named slot. The `gradient-ref` grammar production, which genuinely was a second way to
say the same thing, **is** deleted (§3.2).

**5. "Rename `metric.unit_vh → metric.unit_pct_h` **or** let it take a `%` value" —
the rename is adopted; the `%` spelling is refused.**
`%` is defined as "a fraction of the token's host rect on the axis named by its suffix"
(§3.2). `metric.unit_pct_h` has no host rect — it is the root of every length in the
system and its parent is the *screen*. Letting it take `%` would make the one unit
whose meaning must be unambiguous the one place `%` means something else.

**6. "Allow `%` as an accepted spelling for `frac` tokens" — adopted. "State the parent
for `%` in every row that uses it, in a dedicated column" — resolved with a general
rule plus per-row exceptions instead.**
A dedicated column on ≈425 rows to carry information that is identical for all but six
of them is worse documentation, not better: the reader learns nothing from 419
identical cells and stops reading the column. §3.2 states the suffix rule once, and the
six tokens whose parent is *not* what the suffix implies — `border.edge.bracket_len`
(shorter side), `filetile.caption_gap` (caption block height), `filetile.icon.w/.h`
(tile box), `shape.taskbar.chevron_depth` (height, per end), `boardswitch.shade_min`
(a fraction of nothing, it is an alpha) — say so in their own `note`.

**7. "Rename `type.suffix.*` members to match a type role exactly" — partially
refused.** The family moved under `type.*` as asked. Its members did **not** become
`face / size / tracking / case / …` clones of a `TypeRole`, because
`type.suffix.paren_alpha`, `.brackets` and `.gap` have no `TypeRole` counterpart and
`type.suffix.role` *points at* a role rather than being one. Making the suffix a 24th
role would spend an append-only enum slot on a decoration that only ever varies in
three ways.

**8. "Unify all five inheritance depth caps at 8" — adopted for four of the five;
`@include` stays at 4.** The reason is stated in §4.1: `@include` is a *file* splice,
not a value mechanism, and a theme that needs five levels of file nesting has a
directory-layout problem that a deeper cap would hide rather than solve. The other four
are now 8, and the mood chain in particular went from 4 to 8 to match `[meta] base`.

**9. "`ornament.dump.source = random` invents content" (open question 6) — unchanged.**
It was not raised as a finding in this round and the previous decision stands, with its
three constraints intact: off in `default`, always carrying its heading, always in
`text.instrument`.

**10. On the two counts this document still states as approximations.**
`≈425` component metrics and `≈95` component colours are approximations *on purpose*:
the exact figure is generated by `tokens.rs` and asserted by
`cargo test theme_token_count`, and a hand-maintained exact number in prose is a number
that will be wrong within one commit. The four *formula* counts the reviewer caught
(§5.7, §5.9, §5.16, §5.18) were a different thing — arithmetic that did not add up —
and those are fixed.
