//! Theming.
//!
//! One engine: a `.theme` file format, a cascade over a master
//! `default.theme`, and a resolved struct with no strings and no per-frame
//! lookups. The seven-field eDEX-shaped `Theme` the program was built on is
//! gone; `theme::Color` — the four-`f32` colour every draw call takes — is
//! [`color::Color`], exactly as [`color`]'s docs promised when the two
//! engines still shared this module.
//!
//! # `default.theme` is the schema
//!
//! §2.1 of the specification asks for a **generated `tokens.rs`** holding enums
//! for ~2 190 tokens, and §7 for a `ResolvedTheme` with one named field per
//! token. That table would have to be kept byte-identical with `default.theme`
//! by hand, forever, against the owner's own requirement that `default.theme`
//! carry absolutely every setting. It does not exist here. Instead:
//!
//! * `default.theme` is embedded with `include_str!` and parsed at startup.
//!   **The set of tokens that exist is exactly the set of keys it declares.**
//! * A token's *type* is inferred from the form of its default value.
//! * Its [`TokenId`] is its index in `default.theme`'s declaration order,
//!   interned into a name -> id map at load.
//! * A key in a user theme that `default.theme` does not declare is an unknown
//!   key and warns, exactly as §4.2 requires — the check falls out of the
//!   design instead of needing a second table to fall out of sync with it.
//!
//! [`ResolvedTheme`] is therefore four parallel arrays indexed by `TokenId`
//! (`colors`, `scalars`, `flags`, `enums`) with no strings, no `Vec`, no
//! `HashMap` and no allocation on any draw path; the strings live in
//! [`ThemeDiagnostics`], published as a separate `Arc` beside it. Hot draw paths
//! hold their ids in a `static OnceLock<TokenId>` resolved once by name at load
//! (see [`ids`]), so a per-frame read is one bounds-checked slice index — the
//! same cost as a struct field, and with none of the maintenance. Every promise
//! §7.2 makes about the per-frame budget is kept: no hashing, no strings, no
//! allocation while drawing.
//!
//! # Deliberately not in this stage
//!
//! Each has a comment where it belongs, in the module that will call it:
//!
//! | module | why later | noted in |
//! |---|---|---|
//! | `encode.rs` | keyed on the live swapchain format (§6.3) | [`bake`] |
//! | `enforce.rs` | must run *after* encode, on the pixels the GPU blends (§2.2, §4.4) | [`resolve`] |
//! | `abi.rs` | `ThemeC` + the 19 appended `HostApi` entries (§7.4) | here |
//! | `mask.rs` | procedural R8 masks in the glyph atlas (M0, Appendix B) | here |
//!
//! Everything the enforcement passes need already exists — [`color::Color`]'s
//! `wcag_contrast`, `apca_lc`, `delta_e_ok` and `composite_as_rendered`, and
//! §6's `ensure()` — so `enforce.rs` is a pass over baked values and nothing
//! here has to move for it. The engine is complete and useful without all five.

pub mod bake;
pub mod cascade;
pub mod color;
pub mod expr;
pub mod parse;
pub mod plate;
pub mod resolve;

pub use bake::{BakeInput, ResolvedTheme, Viewport};
pub use plate::Plate;
pub use cascade::{Schema, ThemeSpec, TokenId};
pub use color::Color;
pub use color::Color as ThemeColor;
pub use expr::{Expr, Kind, Value};
pub use parse::{Diagnostic, Level, Span};

use cascade::ThemeSource;
use parse::{Document, LangTag, SectionKind, Sources};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicPtr, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

/// The master theme. It is the documentation as much as the configuration
/// (§1.1(8), §5.0b), and it is the schema (see the module docs).
const DEFAULT_THEME: &str = include_str!("default.theme");

// --------------------------------------------------------------- diagnostics

/// Everything about a loaded theme that is a **string**, published as its own
/// `Arc` beside the POD [`ResolvedTheme`] (§7.1).
///
/// It is a separate type rather than a few extra fields because the POD
/// guarantee has to be a property of the type: a `Vec<String>` is not `Copy`,
/// memcpy-ing one produces two owners of one heap allocation, and a locale count
/// would make `size_of::<ResolvedTheme>()` depend on how many languages a theme
/// declares. Nothing on a draw path can reach this.
#[derive(Default)]
pub struct ThemeDiagnostics {
    pub name: Vec<(LangTag, String)>,
    pub description: Vec<(LangTag, String)>,
    pub author: String,
    pub family: String,
    pub schema: u32,
    /// Where the selected theme came from, for the settings panel.
    pub path: Option<PathBuf>,
    /// §4.2's report list, already rendered with `file:line:col` and a caret.
    pub warnings: Vec<String>,
    /// The cold-path text tokens — font families, file names, separators. Off
    /// every draw path by construction: they are not in `ResolvedTheme`.
    pub texts: Vec<(String, String)>,
    /// The moods and variants this theme resolved into, in selection order.
    /// Index 0 is always the plain theme.
    pub siblings: Vec<String>,
}

impl ThemeDiagnostics {
    /// The theme's name in the user's language, falling back to the untagged
    /// one and then to the file stem.
    pub fn localised_name(&self, lang: &str) -> &str {
        self.name
            .iter()
            .find(|(l, _)| l == lang)
            .or_else(|| self.name.iter().find(|(l, _)| l.is_empty()))
            .map(|(_, v)| v.as_str())
            .unwrap_or("default")
    }

    pub fn text(&self, token: &str) -> Option<&str> {
        self.texts.iter().find(|(k, _)| k == token).map(|(_, v)| v.as_str())
    }
}

// ------------------------------------------------------------------- engine

struct Sibling {
    label: String,
    mood: Option<String>,
    variant: Option<String>,
    spec: ThemeSpec,
    explicit_density: (bool, bool),
}

struct Engine {
    schema: Schema,
    sources: Sources,
    siblings: Vec<Sibling>,
    active: usize,
    viewport: Viewport,
    /// One leaked `ResolvedTheme` per (sibling, quantised `u`). Bounded by the
    /// handful of distinct unit sizes a session ever sees, which is what makes
    /// handing out `&'static` from [`resolved`] affordable: a resize storm
    /// re-uses a bake instead of leaking one per event.
    cache: HashMap<(usize, u32), &'static ResolvedTheme>,
    diagnostics: Arc<ThemeDiagnostics>,
}

static ENGINE: OnceLock<Mutex<Engine>> = OnceLock::new();
static ACTIVE: AtomicPtr<ResolvedTheme> = AtomicPtr::new(std::ptr::null_mut());
static EPOCH: AtomicU32 = AtomicU32::new(0);
static DIAGS: OnceLock<Mutex<Arc<ThemeDiagnostics>>> = OnceLock::new();

fn diags_slot() -> &'static Mutex<Arc<ThemeDiagnostics>> {
    DIAGS.get_or_init(|| Mutex::new(Arc::new(ThemeDiagnostics::default())))
}

/// The theme this frame is drawn from (§2.3 tier 1: statically linked widgets
/// index Rust structs directly — no call, no copy, no marshalling).
///
/// The first call loads the theme. The reference is valid for the life of the
/// process; a widget may cache *values* across frames but should re-read after
/// [`epoch`] changes.
pub fn resolved() -> &'static ResolvedTheme {
    let p = ACTIVE.load(Ordering::Acquire);
    if !p.is_null() {
        // SAFETY: every value stored in ACTIVE is a `Box::leak`ed
        // `ResolvedTheme` that is never freed and never mutated after publish.
        return unsafe { &*p };
    }
    load();
    let p = ACTIVE.load(Ordering::Acquire);
    if p.is_null() {
        return empty_theme();
    }
    unsafe { &*p }
}

/// The last-resort theme: no tokens at all, so every accessor returns its kind's
/// fallback. Reached only if `default.theme` itself declares nothing.
fn empty_theme() -> &'static ResolvedTheme {
    static E: OnceLock<&'static ResolvedTheme> = OnceLock::new();
    E.get_or_init(|| {
        let mut out = Vec::new();
        let mut src = Sources::new();
        let f = src.add("<empty>", "");
        let doc = parse::parse(&mut src, f, None, &mut out);
        let schema = Schema::from_default(&doc, &mut out);
        let r = resolve::resolve_default(&schema, &mut out);
        Box::leak(Box::new(bake::bake(&schema, &r, &BakeInput::default(), &mut out)))
    })
}

/// Increments whenever the host swaps the resolved theme: reload, mood, variant,
/// resize, format change (§7.4).
pub fn epoch() -> u32 {
    EPOCH.load(Ordering::Acquire)
}

/// The strings that came with the loaded theme.
pub fn diagnostics() -> Arc<ThemeDiagnostics> {
    diags_slot().lock().map(|g| g.clone()).unwrap_or_default()
}

/// The token id for a name, or `None` if `default.theme` does not declare it.
///
/// **Call this at init and cache the id**, never inside a draw loop — that is
/// the whole reason [`TokenId`] exists. [`ids`] does exactly that for the hot
/// set.
/// The index of an interaction class — `class_id("button")`, `class_id
/// ("slider.knob")` — for [`ResolvedTheme::class_state`]. Resolved once at
/// init, exactly like a [`TokenId`]; the order is the master's declaration
/// order of its `class.*` tokens.
pub fn class_id(name: &str) -> Option<u16> {
    let engine = ENGINE.get()?.lock().ok()?;
    let want = format!("class.{name}");
    let mut i = 0u16;
    for n in engine.schema.names() {
        if n == want {
            return Some(i);
        }
        if n.starts_with("class.") {
            i += 1;
        }
    }
    None
}

pub fn id(name: &str) -> Option<TokenId> {
    let e = ENGINE.get()?;
    let g = e.lock().ok()?;
    g.schema.id(name)
}

/// The word behind an enum token's baked index, for diagnostics and for a
/// caller that wants to compare by name once at init.
pub fn enum_index(token: TokenId, word: &str) -> Option<u16> {
    let e = ENGINE.get()?;
    let g = e.lock().ok()?;
    g.schema.enum_index(token, word)
}

/// The word an enum token currently resolves to. This is how OPEN word sets
/// are read — a type-role binding (`script.rows_label_role = caption`) names a
/// role, not a member of a closed enum, so the consumer wants the word itself
/// rather than an index to compare. The resolved index is taken before the
/// engine lock: [`resolved`] may itself load on first use.
pub fn enum_word_of(token: TokenId) -> Option<String> {
    let i = resolved().enum_of(token);
    let e = ENGINE.get()?;
    let g = e.lock().ok()?;
    g.schema.enum_word(token, i).map(|s| s.to_string())
}

impl ResolvedTheme {
    /// The cold path: resolve a token by name. `Some` for every key
    /// `default.theme` declares. Call at widget init, cache the id, invalidate
    /// on [`epoch`].
    pub fn id(&self, name: &str) -> Option<TokenId> {
        crate::theme::id(name)
    }
}

// --------------------------------------------------------------------- load

/// Which theme to load, and where from.
#[derive(Clone, Debug, Default)]
pub struct LoadRequest {
    /// A theme name (`aurora`), looked up on the search path. `None` uses
    /// `NACELLE_THEME_NAME`, then `default`.
    pub name: Option<String>,
    /// A path to a `.theme` file, which wins over `name`.
    pub path: Option<PathBuf>,
    /// `None` keeps the viewport the running engine already learned from
    /// [`set_viewport`] — the right choice for every reload whose window
    /// did not change, which is all of them but the host's resize path.
    /// `Viewport::default()` is a real 1080-line request, not a sentinel:
    /// passing it here re-bakes every u-derived length at reference size.
    pub viewport: Option<Viewport>,
}

/// Parse, cascade, resolve and bake. **Always succeeds** (§4.2): a missing or
/// broken theme degrades to `default`, and a broken `default` degrades to the
/// per-kind fallback of `resolve::fallback`.
pub fn load() -> Arc<ThemeDiagnostics> {
    load_with(LoadRequest::default())
}

pub fn load_with(req: LoadRequest) -> Arc<ThemeDiagnostics> {
    let mut out: Vec<Diagnostic> = Vec::new();
    let mut src = Sources::new();

    // ---- stage 2: default.theme, dense --------------------------------
    // NACELLE_THEME_MASTER substitutes the embedded master with a file — the
    // governing principle's own test facility: run with a [meta]-only master
    // and the program must come up RAW (grey ink, kind defaults) rather than
    // in anybody's design. A theme file is data, so this opens no door a
    // theme could not already walk through.
    let master_text: String = std::env::var_os("NACELLE_THEME_MASTER")
        .and_then(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_else(|| DEFAULT_THEME.to_string());
    let f = src.add("default.theme", master_text);
    let default_doc = parse::parse(&mut src, f, None, &mut out);
    let mut schema = Schema::from_default(&default_doc, &mut out);
    {
        // Settle the kinds a reference cannot declare syntactically, by
        // resolving `default` once. §6.3 asserts this walk is cycle-free.
        let r = resolve::resolve_default(&schema, &mut out);
        schema.adopt_kinds(&r.values);
    }

    // ---- stage 3: the selected theme, and its [meta] base chain -------
    let mut fs = FsThemes::new();
    let chosen = req
        .path
        .clone()
        .or_else(|| std::env::var_os("NACELLE_THEME_PATH").map(PathBuf::from));
    let name = req
        .name
        .clone()
        .or_else(|| std::env::var("NACELLE_THEME_NAME").ok())
        .unwrap_or_else(|| "default".into());

    let theme_doc: Option<Document> = match &chosen {
        Some(p) => {
            let d = parse::parse_file(&mut src, p, &mut out);
            if d.is_none() {
                out.push(Diagnostic::warn(
                    Span::default(),
                    format!("theme file {} could not be read — using default", p.display()),
                ));
            }
            d
        }
        None if name != "default" => {
            let d = fs.open(&name, &mut src, &mut out);
            if d.is_none() {
                out.push(Diagnostic::warn(
                    Span::default(),
                    format!("theme \"{name}\" was not found on the search path — using default"),
                ));
            }
            d
        }
        None => None,
    };

    let chain: Vec<Document> = match &theme_doc {
        Some(d) => cascade::base_chain(d, &mut fs, &mut src, &mut out),
        None => Vec::new(),
    };

    // ---- stage 5: the user overlay ------------------------------------
    let user_doc = user_overlay_path().and_then(|p| parse::parse_file(&mut src, &p, &mut out));

    let strict = theme_doc.as_ref().and_then(|d| d.meta_bool("meta.strict")).unwrap_or(false);
    let opts = cascade::Options { strict };

    // ---- the plain sibling, and one per mood / variant ----------------
    let mut stages: Vec<cascade::Stage> = Vec::new();
    for c in &chain {
        stages.push(cascade::Stage::Document(c));
    }
    if let Some(d) = &theme_doc {
        stages.push(cascade::Stage::Document(d));
    }
    if let Some(d) = &user_doc {
        stages.push(cascade::Stage::Document(d));
    }

    let mut siblings: Vec<Sibling> = Vec::new();
    let plain = cascade::cascade(&mut schema, &stages, opts, &mut out);
    let explicit = explicit_density(&schema, &plain);
    siblings.push(Sibling {
        label: "plain".into(),
        mood: None,
        variant: None,
        spec: plain,
        explicit_density: explicit,
    });

    if let Some(doc) = &theme_doc {
        let declared = cascade::sibling_names(doc, &mut out);
        let moods: Vec<String> = declared
            .iter()
            .filter(|(k, _)| *k == SectionKind::Mood)
            .map(|(_, n)| n.clone())
            .collect();
        let variants: Vec<String> = declared
            .iter()
            .filter(|(k, _)| *k == SectionKind::Variant)
            .map(|(_, n)| n.clone())
            .collect();
        // §4.1: `[mood.<m>]` applies BEFORE `[variant.hc]`, so high contrast
        // always wins over an alarm's decoration.
        let mut combos: Vec<(Option<String>, Option<String>)> = Vec::new();
        for m in &moods {
            combos.push((Some(m.clone()), None));
        }
        for v in &variants {
            combos.push((None, Some(v.clone())));
        }
        for m in &moods {
            for v in &variants {
                combos.push((Some(m.clone()), Some(v.clone())));
            }
        }
        for (m, v) in combos {
            if siblings.len() >= cascade::MAX_SIBLINGS {
                out.push(Diagnostic::warn(
                    Span::default(),
                    format!(
                        "more than {} resolved siblings; the rest are dropped (not a load failure)",
                        cascade::MAX_SIBLINGS
                    ),
                ));
                break;
            }
            let mut st: Vec<cascade::Stage> = Vec::new();
            for c in &chain {
                st.push(cascade::Stage::Document(c));
            }
            st.push(cascade::Stage::Document(doc));
            if let Some(n) = &m {
                st.push(cascade::Stage::Overlay {
                    doc,
                    kind: SectionKind::Mood,
                    name: n.clone(),
                });
            }
            if let Some(n) = &v {
                st.push(cascade::Stage::Overlay {
                    doc,
                    kind: SectionKind::Variant,
                    name: n.clone(),
                });
            }
            if let Some(d) = &user_doc {
                st.push(cascade::Stage::Document(d));
            }
            let spec = cascade::cascade(&mut schema, &st, opts, &mut out);
            let explicit = explicit_density(&schema, &spec);
            siblings.push(Sibling {
                label: label_of(&m, &v),
                mood: m,
                variant: v,
                spec,
                explicit_density: explicit,
            });
        }
    }

    // ---- diagnostics ---------------------------------------------------
    let mut meta = ThemeDiagnostics {
        siblings: siblings.iter().map(|s| s.label.clone()).collect(),
        path: chosen,
        ..Default::default()
    };
    collect_meta(&mut meta, &schema, &default_doc, theme_doc.as_ref());

    // ---- publish -------------------------------------------------------
    // A request that names no viewport keeps the one the running engine
    // already learned from [`set_viewport`]: a theme switch happens in a
    // window whose height did not change with it, and resetting to the
    // 1080-line default here is how every u-derived length used to snap
    // back to reference size on a settings click. `None` is the sentinel —
    // `Viewport::default()` is 1080.0, a value a numeric guard cannot tell
    // apart from an explicit request, which is exactly how the snap-back
    // came back for every window that was not 1080 lines tall.
    let viewport = req.viewport.unwrap_or_else(|| {
        ENGINE
            .get()
            .and_then(|slot| slot.lock().ok().map(|g| g.viewport))
            .unwrap_or_default()
    });
    let mut engine = Engine {
        schema,
        sources: src,
        siblings,
        active: 0,
        viewport,
        cache: HashMap::new(),
        diagnostics: Arc::new(ThemeDiagnostics::default()),
    };

    let theme = engine.bake_active(&mut out);
    for d in &out {
        meta.warnings.push(d.render(&engine.sources));
    }
    collect_texts(&mut meta, &engine);
    report(&meta);

    let diags = Arc::new(meta);
    engine.diagnostics = diags.clone();
    publish(theme);
    *diags_slot().lock().unwrap() = diags.clone();

    match ENGINE.get() {
        Some(slot) => {
            if let Ok(mut g) = slot.lock() {
                *g = engine;
            }
        }
        None => {
            let _ = ENGINE.set(Mutex::new(engine));
        }
    }
    diags
}

fn label_of(m: &Option<String>, v: &Option<String>) -> String {
    match (m, v) {
        (Some(m), Some(v)) => format!("{m}+{v}"),
        (Some(m), None) => m.clone(),
        (None, Some(v)) => v.clone(),
        (None, None) => "plain".into(),
    }
}

/// §5.3's precedence rule: an *explicit* `density_space` / `density_type` — one
/// appearing in any stage after `default` — replaces the enum-supplied value for
/// that axis only. Detectable because a stage that set it replaced the whole
/// node, so the spec's expression is no longer `default`'s.
fn explicit_density(schema: &Schema, spec: &ThemeSpec) -> (bool, bool) {
    let differs = |name: &str| {
        schema
            .id(name)
            .map(|id| spec.get(id) != schema.default_expr(id))
            .unwrap_or(false)
    };
    (differs("metric.density_space"), differs("metric.density_type"))
}

impl Engine {
    fn bake_active(&mut self, out: &mut Vec<Diagnostic>) -> &'static ResolvedTheme {
        let i = self.active.min(self.siblings.len().saturating_sub(1));
        let s = &self.siblings[i];
        let r = resolve::resolve(&self.schema, &s.spec, out);
        let input = BakeInput {
            viewport: self.viewport,
            epoch: EPOCH.load(Ordering::Acquire).wrapping_add(1),
            explicit_density: s.explicit_density,
        };
        let probe = bake::metrics(&self.schema, &r, &input, &mut Vec::new());
        let key = (i, probe.u.to_bits());
        if let Some(t) = self.cache.get(&key) {
            return t;
        }
        let baked: &'static ResolvedTheme = Box::leak(Box::new(bake::bake(
            &self.schema,
            &r,
            &input,
            out,
        )));
        self.cache.insert(key, baked);
        baked
    }
}

fn publish(t: &'static ResolvedTheme) {
    ACTIVE.store(t as *const _ as *mut ResolvedTheme, Ordering::Release);
    EPOCH.store(t.epoch, Ordering::Release);
}

// ------------------------------------------------- viewport, mood, variant

/// Re-bake for a new window height or ui scale. **Runs on resize, never per
/// frame** (§2.2 step 4). A height that produces the same `u` re-uses the
/// existing bake and does not bump the epoch.
pub fn set_viewport(screen_h: f32, ui_scale: f32) {
    let Some(slot) = ENGINE.get() else { return };
    let Ok(mut g) = slot.lock() else { return };
    let next = Viewport { screen_h, ui_scale };
    if (g.viewport.screen_h - next.screen_h).abs() < f32::EPSILON
        && (g.viewport.ui_scale - next.ui_scale).abs() < f32::EPSILON
    {
        return;
    }
    g.viewport = next;
    let mut out = Vec::new();
    let t = g.bake_active(&mut out);
    let cur = ACTIVE.load(Ordering::Acquire);
    if cur != t as *const _ as *mut ResolvedTheme {
        publish(t);
    }
}

/// Every resolved sibling, in selection order. Index 0 is the plain theme.
pub fn siblings() -> Vec<String> {
    ENGINE
        .get()
        .and_then(|s| s.lock().ok())
        .map(|g| g.siblings.iter().map(|s| s.label.clone()).collect())
        .unwrap_or_default()
}

/// Select a sibling by index. Switching is one store: no recomputation, no
/// per-draw branch (§5.24). Returns `false` for an index that does not exist.
pub fn set_sibling(i: usize) -> bool {
    let Some(slot) = ENGINE.get() else { return false };
    let Ok(mut g) = slot.lock() else { return false };
    if i >= g.siblings.len() {
        return false;
    }
    if g.active == i {
        return true;
    }
    g.active = i;
    let mut out = Vec::new();
    let t = g.bake_active(&mut out);
    publish(t);
    true
}

/// §5.24's explicit API. `None` clears the mood, keeping the current variant.
/// A mood the theme does not declare is refused rather than guessed at.
pub fn set_mood(name: Option<&str>) -> bool {
    select(name, current_variant().as_deref())
}

/// The contrast variant, `high_contrast` being the one the engine ships.
pub fn set_variant(name: Option<&str>) -> bool {
    select(current_mood().as_deref(), name)
}

pub fn current_mood() -> Option<String> {
    ENGINE
        .get()
        .and_then(|s| s.lock().ok())
        .and_then(|g| g.siblings.get(g.active).and_then(|s| s.mood.clone()))
}

pub fn current_variant() -> Option<String> {
    ENGINE
        .get()
        .and_then(|s| s.lock().ok())
        .and_then(|g| g.siblings.get(g.active).and_then(|s| s.variant.clone()))
}

fn select(mood: Option<&str>, variant: Option<&str>) -> bool {
    let want = {
        let Some(slot) = ENGINE.get() else { return false };
        let Ok(g) = slot.lock() else { return false };
        g.siblings.iter().position(|s| {
            s.mood.as_deref() == mood && s.variant.as_deref() == variant
        })
    };
    match want {
        Some(i) => set_sibling(i),
        None => false,
    }
}

/// The mood's transition tint (§5.24): one full-screen quad animated from its
/// declared alpha to zero over `motion.mood_change.duration`, drawn last.
pub fn mood_wash() -> Option<color::Color> {
    let slot = ENGINE.get()?;
    let g = slot.lock().ok()?;
    let s = g.siblings.get(g.active)?;
    let mut out = Vec::new();
    let r = resolve::resolve(&g.schema, &s.spec, &mut out);
    r.wash.map(|c| c.to_srgb())
}

// ------------------------------------------------------------ the search path

struct FsThemes {
    dirs: Vec<PathBuf>,
}

impl FsThemes {
    fn new() -> FsThemes {
        let mut dirs = Vec::new();
        if let Some(d) = std::env::var_os("NACELLE_THEME_DIR") {
            dirs.push(PathBuf::from(d));
        }
        if let Some(home) = home_dir() {
            if let Some(data) = std::env::var_os("XDG_DATA_HOME") {
                dirs.push(PathBuf::from(data).join("nacelle-desktop/themes"));
            } else {
                dirs.push(home.join(".local/share/nacelle-desktop/themes"));
            }
            dirs.push(home.join(".config/nacelle-desktop/themes"));
        }
        dirs.push(PathBuf::from("/usr/share/nacelle-desktop/themes"));
        FsThemes { dirs }
    }
}

impl cascade::ThemeSource for FsThemes {
    fn open(
        &mut self,
        name: &str,
        src: &mut Sources,
        out: &mut Vec<Diagnostic>,
    ) -> Option<Document> {
        // A theme name is a bare identifier, never a path: a `[meta] base` that
        // could name `../../etc/passwd` would be a file-read primitive.
        if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-') {
            out.push(Diagnostic::warn(
                Span::default(),
                format!("\"{name}\" is not a theme name (letters, digits, _ and - only)"),
            ));
            return None;
        }
        for d in &self.dirs {
            let p = d.join(format!("{name}.theme"));
            if p.is_file() {
                return parse::parse_file(src, &p, out);
            }
        }
        // The shipped themes are in the binary, and a file of the same name
        // anywhere on the search path shadows one — the same rule the widget
        // registry follows. A program with nothing installed still has all
        // nine looks; a user who wants to edit `aurora` drops a file in his
        // own directory and the built-in steps aside.
        builtin(name).and_then(|text| {
            parse::parse_text(src, &format!("<built-in {name}>"), text, out)
        })
    }
}

/// Every theme this program can load, by name: the eight compiled in, plus
/// every `<name>.theme` on the search path, with a file shadowing a built-in of
/// the same name. Sorted, `default` first — it is the master, and it is always
/// selectable even though it is not in [`BUILTIN_THEMES`].
///
/// For the settings panel. It touches the filesystem, so it is not for a draw
/// path.
pub fn available_themes() -> Vec<String> {
    let mut out: Vec<String> = vec!["default".to_string()];
    for (n, _) in BUILTIN_THEMES {
        out.push(n.to_string());
    }
    for d in FsThemes::new().dirs {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) != Some("theme") {
                continue;
            }
            if let Some(stem) = p.file_stem().and_then(|x| x.to_str()) {
                if !out.iter().any(|n| n == stem) {
                    out.push(stem.to_string());
                }
            }
        }
    }
    out[1..].sort();
    out
}

/// The themes compiled into the toolkit. `default` is not here — it is the
/// master, embedded separately and always loaded first (§4.1 stage 2).
pub const BUILTIN_THEMES: [(&str, &str); 8] = [
    ("aurora", include_str!("themes/aurora.theme")),
    ("spring", include_str!("themes/spring.theme")),
    ("pure", include_str!("themes/pure.theme")),
    ("crimson", include_str!("themes/crimson.theme")),
    ("lockdown", include_str!("themes/lockdown.theme")),
    ("azure", include_str!("themes/azure.theme")),
    ("cockpit", include_str!("themes/cockpit.theme")),
    ("instrument", include_str!("themes/instrument.theme")),
];

fn builtin(name: &str) -> Option<&'static str> {
    BUILTIN_THEMES
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, text)| *text)
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// §4.1 stage 5. One file, always the last word.
fn user_overlay_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("NACELLE_THEME_LOCAL") {
        return Some(PathBuf::from(p));
    }
    let base = match std::env::var_os("XDG_CONFIG_HOME") {
        Some(c) => PathBuf::from(c),
        None => home_dir()?.join(".config"),
    };
    let p = base.join("nacelle-desktop/theme.local");
    p.is_file().then_some(p)
}

// ------------------------------------------------------------------- reports

fn collect_meta(
    meta: &mut ThemeDiagnostics,
    schema: &Schema,
    default_doc: &Document,
    theme_doc: Option<&Document>,
) {
    let mut take = |doc: &Document| {
        if let Some(v) = doc.meta_text("meta.name") {
            meta.name.retain(|(l, _)| !l.is_empty());
            meta.name.insert(0, (String::new(), v));
        }
        if let Some(v) = doc.meta_text("meta.description") {
            meta.description.retain(|(l, _)| !l.is_empty());
            meta.description.insert(0, (String::new(), v));
        }
        if let Some(v) = doc.meta_text("meta.author") {
            meta.author = v;
        }
        if let Some(v) = doc.meta_text("meta.family") {
            meta.family = v;
        }
        if let Some(Expr::Num(v)) = doc.meta(&"meta.schema".to_string()) {
            meta.schema = *v as u32;
        }
        for kv in &doc.keys {
            let (Some(lang), Expr::Text(t)) = (&kv.locale, &kv.value) else { continue };
            match kv.key.as_str() {
                "meta.name" => meta.name.push((lang.clone(), t.clone())),
                "meta.description" => meta.description.push((lang.clone(), t.clone())),
                _ => {}
            }
        }
    };
    take(default_doc);
    if let Some(d) = theme_doc {
        take(d);
    }
    for (k, lang, v) in &schema.localised {
        if k == "meta.name" && !meta.name.iter().any(|(l, _)| l == lang) {
            meta.name.push((lang.clone(), v.clone()));
        }
    }
}

fn collect_texts(meta: &mut ThemeDiagnostics, engine: &Engine) {
    let s = &engine.siblings[engine.active.min(engine.siblings.len().saturating_sub(1))];
    let mut out = Vec::new();
    let r = resolve::resolve(&engine.schema, &s.spec, &mut out);
    for (i, v) in r.values.iter().enumerate() {
        if let Value::Text(t) = v {
            meta.texts.push((engine.schema.name(TokenId(i as u16)).to_string(), t.clone()));
        }
    }
}

/// §4.2: reports go to four places, and stderr at load is the first. Printed
/// once, in §4.3's shape.
fn report(meta: &ThemeDiagnostics) {
    if meta.warnings.is_empty() {
        return;
    }
    let name = meta.localised_name("");
    eprintln!("theme \"{name}\"");
    for w in &meta.warnings {
        eprint!("{w}");
    }
}

// ----------------------------------------------------------------- hot ids

/// The hot set: the tokens a draw path reads every frame.
///
/// Each helper resolves **by name at load** and caches the id in a
/// `static OnceLock<TokenId>`, so a per-frame read is
/// `theme.color(ids::text_primary())` — one atomic load of an already-set
/// `OnceLock` plus one bounds-checked slice index. A name `default.theme` does
/// not declare degrades to [`TokenId::MISSING`], which every accessor tolerates,
/// and warns once.
///
/// Everything outside this list goes through [`ResolvedTheme::id`] at widget
/// init and is cached by the caller. Nothing here is a hard-coded value — only
/// a hard-coded *question*.
pub mod ids {
    use super::TokenId;
    use std::sync::OnceLock;

    macro_rules! hot {
        ($($fname:ident => $token:literal),* $(,)?) => {
            $(
                #[doc = concat!("`", $token, "`")]
                #[inline]
                pub fn $fname() -> TokenId {
                    static ID: OnceLock<TokenId> = OnceLock::new();
                    *ID.get_or_init(|| super::hot_id($token))
                }
            )*
            /// Every name in the hot set, for the startup check.
            pub const HOT_SET: &[&str] = &[$($token),*];
        };
    }

    hot! {
        // the five seeds (§5.2)
        palette_black   => "palette.black",
        palette_white   => "palette.white",
        palette_accent  => "palette.accent",
        // surfaces (§5.5)
        surface_base    => "surface.base",
        surface_panel   => "surface.panel",
        surface_scrim   => "surface.scrim",
        // text roles (§5.6)
        text_title      => "text.title",
        text_primary    => "text.primary",
        text_secondary  => "text.secondary",
        text_muted      => "text.muted",
        text_disabled   => "text.disabled",
        text_inverse    => "text.inverse",
        // chrome (§5.7, §5.8)
        accent_primary  => "accent.primary",
        accent_hover    => "accent.hover",
        border_default  => "border.default",
        border_width    => "border.edge.width",
        focus_ring_width => "focus.ring.width",
        // the terminal (§5.11) — the 12 000-cell inner loop
        term_fg         => "term.fg",
        term_bg         => "term.bg",
        term_cursor     => "term.cursor",
        // the ladders a widget reaches for constantly (§5.4)
        space_2         => "space.2",
        space_4         => "space.4",
        size_md         => "size.md",
        stroke_hair     => "stroke.hair",
        corner_md       => "corner.md",
    }

    /// `term.ansi[i]`, resolved once per slot. The sixteen live in the same
    /// token space as everything else, so they are addressable from an icon
    /// layer or a type role's `fg` exactly like any other colour (§7.1).
    pub fn term_ansi(i: usize) -> TokenId {
        static IDS: OnceLock<[TokenId; 16]> = OnceLock::new();
        let all = IDS.get_or_init(|| {
            let mut v = [TokenId::MISSING; 16];
            for (k, slot) in v.iter_mut().enumerate() {
                *slot = super::id(&format!("term.ansi[{k}]")).unwrap_or(TokenId::MISSING);
            }
            v
        });
        all.get(i).copied().unwrap_or(TokenId::MISSING)
    }

    /// `data.series[i]`, the eight-colour plot ramp.
    pub fn data_series(i: usize) -> TokenId {
        static IDS: OnceLock<[TokenId; 8]> = OnceLock::new();
        let all = IDS.get_or_init(|| {
            let mut v = [TokenId::MISSING; 8];
            for (k, slot) in v.iter_mut().enumerate() {
                *slot = super::id(&format!("data.series[{k}]")).unwrap_or(TokenId::MISSING);
            }
            v
        });
        all.get(i).copied().unwrap_or(TokenId::MISSING)
    }
}

fn hot_id(name: &str) -> TokenId {
    match id(name) {
        Some(t) => t,
        None => {
            eprintln!(
                "nacelle::theme: default.theme does not declare \"{name}\" — \
                 drawing falls back to this token's kind default"
            );
            TokenId::MISSING
        }
    }
}

/// Report every hot-set name `default.theme` does not declare. Called once at
/// startup by the application so the omission is a line in the log rather than
/// a colour nobody can explain.
pub fn check_hot_set() -> Vec<String> {
    ids::HOT_SET
        .iter()
        .filter(|n| id(n).is_none())
        .map(|n| format!("hot token \"{n}\" is not declared by default.theme"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every name in the hot set must be a token `default.theme` declares.
    ///
    /// A hot id that resolves to MISSING does not crash and does not warn on a
    /// draw path — it silently falls back, which is how `border.width` was
    /// wrong for a whole release without anybody seeing it: the borders simply
    /// kept the hard-coded thickness and the theme looked like it only changed
    /// colour. The check belongs in the test suite, not in the log.
    #[test]
    fn every_hot_token_is_a_token_the_master_declares() {
        let mut out = Vec::new();
        let mut src = Sources::new();
        let f = src.add("default.theme", DEFAULT_THEME);
        let doc = parse::parse(&mut src, f, None, &mut out);
        let schema = Schema::from_default(&doc, &mut out);
        let missing: Vec<&str> = ids::HOT_SET
            .iter()
            .copied()
            .filter(|n| schema.id(n).is_none())
            .collect();
        assert!(missing.is_empty(), "hot names default.theme does not declare: {missing:?}");
    }

    /// Every shipped theme, over the master, with nothing else. A theme is a
    /// sparse overlay, so the only way it can be wrong is by naming a token
    /// the master does not declare, spelling a value the grammar refuses, or
    /// writing a cycle — and each of those is a diagnostic, which is what
    /// this asserts the absence of. It also proves a theme resolves and bakes
    /// to a complete table: no token is left without a value because an
    /// override replaced a derivation with something unresolvable.
    #[test]
    fn every_shipped_theme_loads_over_the_master_without_a_word_of_complaint() {
        for (name, text) in BUILTIN_THEMES {
            let mut out = Vec::new();
            let mut src = Sources::new();

            let f = src.add("default.theme", DEFAULT_THEME);
            let master = parse::parse(&mut src, f, None, &mut out);
            let g = src.add(name, text);
            let overlay = parse::parse(&mut src, g, None, &mut out);

            let mut schema = Schema::from_default(&master, &mut out);
            let spec = cascade::cascade(
                &mut schema,
                &[
                    cascade::Stage::Document(&master),
                    cascade::Stage::Document(&overlay),
                ],
                cascade::Options::default(),
                &mut out,
            );
            let r = resolve::resolve(&schema, &spec, &mut out);
            schema.adopt_kinds(&r.values);
            let t = bake::bake(&schema, &r, &BakeInput::default(), &mut out);

            let rendered: String = out.iter().map(|d| d.render(&src)).collect();
            assert!(out.is_empty(), "theme \"{name}\" is not clean:\n{rendered}");
            assert_eq!(t.len(), schema.len(), "theme \"{name}\" baked short");
        }
    }

    /// The class x state matrix is real: a button's hover rung derives from
    /// the ACCENT (its class base) while a panel's derives from the BORDER
    /// colour — two different classes, two different ladders, from one
    /// [state] section. Before the [class] block existed every rung baked
    /// against white, which is exactly what this asserts never returns.
    #[test]
    fn the_state_ladder_bakes_per_class_not_against_white() {
        let mut out = Vec::new();
        let mut src = Sources::new();
        let f = src.add("default.theme", DEFAULT_THEME);
        let doc = parse::parse(&mut src, f, None, &mut out);
        let mut schema = Schema::from_default(&doc, &mut out);
        let spec = schema.base_spec();
        let r = {
            let rr = resolve::resolve_default(&schema, &mut out);
            schema.adopt_kinds(&rr.values);
            let _ = spec;
            resolve::resolve(&schema, &schema.base_spec(), &mut out)
        };
        let t = bake::bake(&schema, &r, &BakeInput::default(), &mut out);
        assert!(t.class_count() >= 20, "class matrix missing: {}", t.class_count());

        let button = r
            .class_ids
            .iter()
            .position(|&id| schema.name(id) == "class.button")
            .unwrap() as u16;
        let panel = r
            .class_ids
            .iter()
            .position(|&id| schema.name(id) == "class.panel")
            .unwrap() as u16;

        let bh = t.class_state(button, parse::State::Hover);
        let ph = t.class_state(panel, parse::State::Hover);
        // hover.text = base — so it must BE each class's base, not white.
        assert!(bh.text.r < 0.99 || bh.text.g < 0.99 || bh.text.b < 0.99,
            "button hover text baked against white: {:?}", bh.text);
        // The two bases share a hue by design (@border.default IS the accent
        // at 0.55), so the distance must include alpha or it proves nothing.
        let d = (bh.text.r - ph.text.r).abs()
            + (bh.text.g - ph.text.g).abs()
            + (bh.text.b - ph.text.b).abs()
            + (bh.text.a - ph.text.a).abs();
        assert!(d > 0.05, "two classes share one ladder: {:?} vs {:?}", bh.text, ph.text);
        // And the ladder's own arithmetic survives: press fills stronger than idle.
        let bi = t.class_state(button, parse::State::Idle);
        let bp = t.class_state(button, parse::State::Press);
        assert!(bp.fill.a > bi.fill.a, "press ({}) not above idle ({})", bp.fill.a, bi.fill.a);
    }

    /// The governing principle's own acceptance test: a [meta]-only master
    /// still parses, resolves and bakes — into an EMPTY table, whose every
    /// lookup answers the per-kind raw default. The program with no design
    /// anywhere must run and look unstyled, never crash and never look like
    /// yesterday's theme.
    #[test]
    fn an_empty_master_bakes_raw_and_nothing_panics() {
        let mut out = Vec::new();
        let mut src = Sources::new();
        let f = src.add("default.theme", "[meta]\nschema = 1\nname = \"raw\"\n");
        let doc = parse::parse(&mut src, f, None, &mut out);
        let mut schema = Schema::from_default(&doc, &mut out);
        let r = resolve::resolve_default(&schema, &mut out);
        schema.adopt_kinds(&r.values);
        let spec = schema.base_spec();
        let rr = resolve::resolve(&schema, &spec, &mut out);
        let t = bake::bake(&schema, &rr, &BakeInput::default(), &mut out);
        // The two [meta] keys intern like any others; nothing else exists.
        assert!(t.len() <= 2, "{} tokens from an empty master", t.len());
        assert_eq!(t.class_count(), 0);
        // Every read degrades to the kind default — grey ink, zero lengths.
        assert_eq!(t.color(TokenId::MISSING), bake::StateStyle::RAW.text);
        let st = t.class_state(0, parse::State::Hover);
        assert_eq!(st.edge_width, 1.0);
    }

    #[test]
    fn the_embedded_master_parses_resolves_and_bakes() {
        let mut out = Vec::new();
        let mut src = Sources::new();
        let f = src.add("default.theme", DEFAULT_THEME);
        let doc = parse::parse(&mut src, f, None, &mut out);
        let rendered: String = out.iter().map(|d| d.render(&src)).collect();
        assert!(out.is_empty(), "default.theme must parse clean:\n{rendered}");

        let mut schema = Schema::from_default(&doc, &mut out);
        let r = resolve::resolve_default(&schema, &mut out);
        schema.adopt_kinds(&r.values);
        let rendered: String = out.iter().map(|d| d.render(&src)).collect();
        // §6.3 step 4: "The compiled-in `default` is cycle-free by
        // construction, which a unit test asserts by resolving it."
        assert!(
            !rendered.contains("reference cycle"),
            "default.theme must be cycle-free:\n{rendered}"
        );
        let t = bake::bake(&schema, &r, &BakeInput::default(), &mut out);
        assert_eq!(t.len(), schema.len());
        assert!(t.unit_px > 0.0);
    }

    #[test]
    fn every_token_declared_by_the_master_is_addressable_by_name() {
        let mut out = Vec::new();
        let mut src = Sources::new();
        let f = src.add("default.theme", DEFAULT_THEME);
        let doc = parse::parse(&mut src, f, None, &mut out);
        let schema = Schema::from_default(&doc, &mut out);
        for n in schema.names() {
            assert!(schema.id(n).is_some(), "{n} interned but not addressable");
        }
    }

    #[test]
    fn resolved_never_returns_null_and_never_panics() {
        let t = resolved();
        // Whatever default.theme currently holds, the accessors are total.
        let _ = t.color(TokenId::MISSING);
        let _ = t.px(TokenId::MISSING);
        let _ = t.flag(TokenId::MISSING);
        let _ = t.enum_of(TokenId::MISSING);
        assert!(epoch() >= 1);
    }

    #[test]
    fn the_hot_set_degrades_rather_than_panicking() {
        let _ = resolved();
        // Absent tokens come back MISSING and read as their kind's fallback;
        // present ones index the arrays.
        let idx = ids::palette_accent();
        let t = resolved();
        let _ = t.color(idx);
        let _ = ids::term_ansi(99);
        let _ = ids::data_series(99);
        // check_hot_set names what is missing rather than hiding it
        for line in check_hot_set() {
            assert!(line.contains("is not declared"));
        }
    }

    #[test]
    fn a_theme_name_may_not_be_a_path() {
        let mut fs = FsThemes::new();
        let mut src = Sources::new();
        let mut out = Vec::new();
        assert!(fs.open("../../etc/passwd", &mut src, &mut out).is_none());
        assert!(out[0].message.contains("is not a theme name"));
    }

    #[test]
    fn selecting_a_sibling_that_does_not_exist_is_refused_not_guessed() {
        let _ = resolved();
        assert!(!set_mood(Some("no-such-mood")));
        assert!(!set_sibling(9999));
        // the plain theme is always index 0 and always selectable
        assert!(set_sibling(0));
    }

    /// A realistic master, in the shape §5.0b prescribes. It stands in for the
    /// embedded `default.theme` so the whole pipeline is exercised end to end
    /// even while the real master is still being written.
    const MASTER: &str = r#"
[meta]
schema = 1
name = "Aurora"
name[pl] = "Zorza"
description = "Mint console, reference image 1."
family = console
strict = false

[palette]
black   = #0A100E
white   = #EAF6F1
accent  = #3FE3AE
data    = #35A7FF
neutral = #74707E

[metric]
unit_pct_h  = 0.5
unit_min_px = 4px
unit_max_px = 10px
ui_scale    = 1.0
density     = compact
density_space = 1.00
density_type  = 1.00

[surface]
base  = mix(@palette.black, @palette.accent, 0.06)
panel = alpha(mix(@palette.black, @palette.accent, 0.10), 0.82)
scrim = alpha(@palette.black, 0.66)

[text]
title     = lum_min(@palette.accent, 0.87)
primary   = tint(@palette.accent, 0.55)
secondary = alpha(@text.primary, 0.78)
muted     = alpha(@text.primary, 0.52)
disabled  = alpha(@text.primary, 0.30)
inverse   = contrast_on(@accent.primary, @palette.black, @palette.white)

[accent]
primary = @palette.accent
hover   = tint(@accent.primary, 0.18)
on      = contrast_on(@accent.primary, @palette.black, @palette.white)

[border]
default = alpha(@accent.primary, 0.55)
width   = @stroke.hair

[space]
0 = 0u
2 = 1u
4 = 2u
6 = 4u

[size]
md = 5.2u
xl = 8.4u

[stroke]
hair    = 0.2u
regular = 0.4u
bold    = 0.7u

[corner]
md   = 1.2u
pill = pill

[focus]
ring.width = @stroke.thin
ring.enabled = true

[panel]
content_pad   = 2.8u
content_pad_x = same_as_parent
title_h       = 5.2u
title_pad     = 0.35x @panel.title_h

[a11y]
min_hit        = 4.8u
min_hit_min_px = 24px

[state]
idle.fill  = alpha(base, 0.07)
idle.edge  = alpha(base, 0.40)
hover.fill = alpha(base, 0.22)

[glow]
alpha_scale = 1.0

[decor]
enabled = true
vignette.strength = 55%

[term]
bg     = @surface.base
fg     = @text.primary
cursor = @accent.primary
ansi = [
  #0A100E, #CD3131, #0DBC79, #E5E510,
  #2472C8, #BC3FBC, #11A8CD, #E5E5E5,
  #666666, #F14C4C, #23D18B, #F5F543,
  #3B8EEA, #D670D6, #29B8DB, #FFFFFF,
]

[data]
line = @palette.data
series = [ #3FE3AE, #35A7FF, #E8B33A, #FF7A00,
           #BC3FBC, #11A8CD, #74707E, #FF2A35 ]

[mood.alert]
palette.accent = #FF2A35
decor.enabled  = false
wash = #FF2A35 / 0.22

[variant.hc]
state.idle.edge  = alpha(base, 0.72)
glow.alpha_scale = 0.50
border.width     = @stroke.regular
decor.enabled    = false
"#;

    /// The whole pipeline on a realistic master: parse, intern, resolve,
    /// cascade a theme over it, resolve a mood sibling, and bake two screens.
    #[test]
    fn end_to_end_over_a_realistic_master() {
        let mut out = Vec::new();
        let mut src = Sources::new();
        let f = src.add("default.theme", MASTER);
        let doc = parse::parse(&mut src, f, None, &mut out);
        let show = |o: &Vec<Diagnostic>, s: &Sources| -> String {
            o.iter().map(|d| d.render(s)).collect()
        };
        assert!(out.is_empty(), "master must parse clean:\n{}", show(&out, &src));

        let mut schema = Schema::from_default(&doc, &mut out);
        let r0 = resolve::resolve_default(&schema, &mut out);
        schema.adopt_kinds(&r0.values);

        // `focus.ring.width = @stroke.thin` is a dangling reference on purpose.
        // §4.2 says warn and fall back; §4.3 says print the source line with a
        // caret under the offending span. Both, or the diagnostic is a rumour.
        assert!(schema.id("stroke.thin").is_none());
        let printed = show(&out, &src);
        assert!(printed.contains("unknown token \"stroke.thin\""), "{printed}");
        assert!(printed.contains("ring.width = @stroke.thin"), "{printed}");
        assert!(printed.contains('^'), "{printed}");
        assert!(printed.contains("default.theme:68:14"), "{printed}");
        assert!(
            out.iter().all(|d| d.message.contains("stroke.thin")),
            "the master must be clean apart from the deliberate dangler:\n{printed}"
        );
        // and it still produced a usable theme
        assert_eq!(r0.values.len(), schema.len());

        // The indexed families are contiguous, addressable per slot, and typed.
        assert_eq!(schema.family("term.ansi").map(|f| f.len()), Some(16));
        assert_eq!(schema.family("data.series").map(|f| f.len()), Some(8));
        assert_eq!(schema.kind(schema.id("term.ansi[4]").unwrap()), Kind::Color);
        // References take the kind of what they point at, after adopt_kinds.
        assert_eq!(schema.kind(schema.id("border.width").unwrap()), Kind::Scalar);
        assert_eq!(schema.kind(schema.id("term.fg").unwrap()), Kind::Color);
        // The state ladder is a template, not a value.
        assert!(schema.deferred(schema.id("state.idle.fill").unwrap()));

        // A theme that says one thing re-derives everything downstream of it.
        let g = src.add("crimson.theme", "[palette]\naccent = #FF2A35\n");
        let theme = parse::parse(&mut src, g, None, &mut out);
        let spec = cascade::cascade(
            &mut schema,
            &[cascade::Stage::Document(&theme)],
            cascade::Options::default(),
            &mut out,
        );
        let r = resolve::resolve(&schema, &spec, &mut out);
        let red = ThemeColor::from_hex("#FF2A35").unwrap().to_linear().to_oklch().h;
        for tok in ["accent.hover", "text.title", "border.default", "term.cursor"] {
            let c = r.get(schema.id(tok).unwrap()).unwrap().as_color().unwrap();
            assert!(
                (c.to_oklch().h - red).abs() < 30.0,
                "{tok} did not follow the seed: hue {} vs {red}",
                c.to_oklch().h
            );
        }
        // `.on` picked the readable side of the new chip by measurement.
        let on = r.get(schema.id("accent.on").unwrap()).unwrap().as_color().unwrap();
        let chip = r.get(schema.id("accent.primary").unwrap()).unwrap().as_color().unwrap();
        assert!(ThemeColor::wcag_contrast(on, chip) > 4.0);

        // The mood is a complete sibling, resolved separately.
        let mspec = cascade::cascade(
            &mut schema,
            &[
                cascade::Stage::Document(&theme),
                cascade::Stage::Overlay {
                    doc: &doc,
                    kind: SectionKind::Mood,
                    name: "alert".into(),
                },
            ],
            cascade::Options::default(),
            &mut out,
        );
        let mr = resolve::resolve(&schema, &mspec, &mut out);
        assert_eq!(mr.get(schema.id("decor.enabled").unwrap()), Some(&Value::Bool(false)));
        assert!(mr.wash.is_some());
        assert_eq!(r.get(schema.id("decor.enabled").unwrap()), Some(&Value::Bool(true)));

        // And two screen heights bake to two whole themes.
        let at = |h: f32, out: &mut Vec<Diagnostic>| {
            bake::bake(
                &schema,
                &r,
                &BakeInput {
                    viewport: Viewport { screen_h: h, ui_scale: 1.0 },
                    ..Default::default()
                },
                out,
            )
        };
        let lo = at(720.0, &mut out);
        let hi = at(2160.0, &mut out);
        let md = schema.id("size.md").unwrap();
        assert!((lo.px(md) - 20.8).abs() < 1e-3, "{}", lo.px(md));
        assert!((hi.px(md) - 52.0).abs() < 1e-3, "{}", hi.px(md));
        // strokes are whole physical pixels at both
        assert_eq!(lo.px(schema.id("stroke.hair").unwrap()), 1.0);
        assert_eq!(hi.px(schema.id("stroke.hair").unwrap()), 2.0);
        // the min-hit floor bites at 720p and not at 4K
        assert_eq!(lo.px(schema.id("a11y.min_hit").unwrap()), 24.0);
        assert_eq!(hi.px(schema.id("a11y.min_hit").unwrap()), 48.0);
        // sentinels folded, colours encoded, nothing NaN
        assert_eq!(lo.px(schema.id("panel.content_pad_x").unwrap()), -3.0);
        assert_eq!(lo.px(schema.id("corner.pill").unwrap()), -2.0);
        assert_eq!(
            lo.color(schema.id("palette.accent").unwrap()).to_hex(),
            "#FF2A35"
        );
        for i in 0..lo.len() {
            let id = TokenId(i as u16);
            assert!(lo.px(id).is_finite() && lo.color(id).is_finite());
        }

        // The only diagnostics are the deliberate dangling reference, once per
        // resolution, and nothing else.
        let msgs: Vec<&str> = out.iter().map(|d| d.message.as_str()).collect();
        assert!(
            msgs.iter().all(|m| m.contains("stroke.thin")),
            "unexpected diagnostics: {msgs:#?}"
        );
        assert!(!msgs.is_empty(), "the dangling reference must be reported");
    }

    #[test]
    fn the_draw_colour_api_still_works_because_the_program_calls_it() {
        // `theme::Color` is [`color::Color`] now, and the five methods the
        // draw calls were built on are unchanged.
        let c = Color::rgb8(170, 207, 209);
        assert_eq!(c.to_array()[3], 1.0);
        assert!(Color::from_hex("#05080d").is_some());
        assert_eq!(c.alpha(0.5).a, 0.5);
        let _ = c.dim(0.5);
    }
}
