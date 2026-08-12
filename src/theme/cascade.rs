//! The token schema and the five ordered sparse overlays of §4.1.
//!
//! ### `default.theme` is the schema
//!
//! §2.1 asks for a generated `tokens.rs` holding enums for ~2 190 tokens and §7
//! for a `ResolvedTheme` with one named field per token. That is a 2 190-entry
//! Rust table that has to be kept byte-identical with `default.theme` by hand,
//! forever, against an owner's requirement that `default.theme` carry absolutely
//! every setting. So it does not exist. **The set of tokens that exist is exactly
//! the set of keys `default.theme` declares**: it is embedded with `include_str!`,
//! parsed at startup, and interned here into a name -> [`TokenId`] map in
//! declaration order.
//!
//! A token's *type* is inferred from the form of its default value — hex / `rgb`
//! / `oklch` / call -> colour, number+unit -> length, bare number -> scalar,
//! `true`/`false` -> flag, bare identifier -> enum (or a §5.0 sentinel, which is
//! a scalar), quoted -> text, `[..]` -> an indexed family. A reference defers to
//! whatever it points at, which is why [`Schema::adopt_kinds`] runs after
//! `default` has been resolved once: `border.width = @stroke.hair` is a length,
//! and no syntactic rule can know that.
//!
//! A key a user theme declares that `default.theme` does not is an **unknown
//! key** and warns, exactly as §4.2 requires — the check falls out of the design
//! instead of needing a second table to fall out of sync with the first.
//!
//! ### The cascade proper
//!
//! Five stages, later replaces earlier, and **an override replaces a whole node
//! — there is no partial merge of a value.** Because `default`'s entries are
//! *expressions*, a theme that sets only `palette.accent` re-derives every
//! dependent token; that is the entire mechanism behind one layout in four hues,
//! and it is the fix for the CSS-variable footgun where `--accent-color` "must be
//! manually overridden as well".

use super::expr::{Expr, Kind, Value};
use super::mood::{self, MoodRule};
use super::parse::{Diagnostic, Document, KeyVal, LangTag, Level, Section, SectionKind, Sources, Span, self as parse};
use std::collections::HashMap;

// ------------------------------------------------------------------ TokenId

/// A token's index in `default.theme`'s declaration order.
///
/// A `u16` newtype: hot draw paths hold theirs in a `static OnceLock<TokenId>`
/// resolved once by name at load, so a per-frame read is one bounds-checked
/// slice index — the same cost as a struct field, with none of the maintenance.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TokenId(pub u16);

impl TokenId {
    /// The id an `ids::` helper degrades to when `default.theme` does not
    /// declare the name. Every accessor tolerates it; none panics on it.
    pub const MISSING: TokenId = TokenId(u16::MAX);

    pub fn index(self) -> usize {
        self.0 as usize
    }

    pub fn is_missing(self) -> bool {
        self == TokenId::MISSING
    }
}

/// §4.1's caps. All eight except `@include`, which is four.
pub const MAX_BASE_DEPTH: u32 = 8;
pub const MAX_MOOD_DEPTH: u32 = 8;
/// `{normal, alert, lockdown, one spare} x {plain, high_contrast}`, and there is
/// no ninth thing a mood should be.
pub const MAX_SIBLINGS: usize = 8;

/// The three keys that bind to the mood itself (§5.24). Reserved inside an
/// overlay section; every *other* key there is an absolute token path.
pub const MOOD_KEYS: [&str; 3] = ["inherit", "wash", "when"];

// ------------------------------------------------------------------- schema

#[derive(Clone, Debug)]
struct Token {
    name: String,
    kind: Kind,
    expr: Expr,
    span: Span,
    /// The token's expression mentions the `base` keyword of a `[state]` block,
    /// so its real value is materialised per class by the class x state bake.
    deferred: bool,
    /// Words this token has been seen to take, default's own at index 0.
    words: Vec<String>,
}

/// The token catalogue, interned from `default.theme`.
pub struct Schema {
    tokens: Vec<Token>,
    index: HashMap<String, TokenId>,
    /// `term.ansi` -> [(0, id), (1, id), ...] so `@term.ansi` gathers its slots
    /// and a whole-array override can check its length.
    families: HashMap<String, Vec<(u32, TokenId)>>,
    /// `meta.name` + `pl` -> "Zorza". Off every draw path; §7.1's
    /// `ThemeDiagnostics` owns these.
    pub localised: Vec<(String, LangTag, String)>,
}

impl Schema {
    /// Intern every key `default.theme` declares, in declaration order.
    pub fn from_default(doc: &Document, out: &mut Vec<Diagnostic>) -> Schema {
        let mut s = Schema {
            tokens: Vec::with_capacity(2400),
            index: HashMap::with_capacity(2400),
            families: HashMap::new(),
            localised: Vec::new(),
        };
        for kv in &doc.keys {
            // A `[mood.<m>]` / `[variant.<v>]` block inside the master is stage
            // 4, not stage 2: it re-declares tokens that already exist and must
            // not contribute to the dense default spec. `default.theme` ships
            // `lockdown` as a convention (§5.24), so without this the master's
            // own alarm colours would silently become everyone's defaults.
            if doc.sections.get(kv.section as usize).map(Section::is_overlay).unwrap_or(false) {
                continue;
            }
            if let Some(lang) = &kv.locale {
                if let Expr::Text(t) = &kv.value {
                    s.localised.push((kv.key.clone(), lang.clone(), t.clone()));
                } else if let Expr::Word(t) = &kv.value {
                    s.localised.push((kv.key.clone(), lang.clone(), t.clone()));
                }
                continue;
            }
            // An array declares an indexed family: `term.ansi = [..]` in the
            // master declares term.ansi[0..n-1], which is what makes both
            // `@term.ansi[4]` and a whole-row override addressable (§3.2).
            if let (Expr::Array(items), None) = (&kv.value, kv.index) {
                // An EMPTY row still declares the family. `[]` is a real
                // default — "no layers", "no candidate families", "the
                // engine's own order" — and a family that exists with zero
                // slots is how a theme is told the token is real but takes
                // no per-slot pin. Declaring nothing here made the master's
                // own key unknown to the master.
                s.families.entry(kv.key.clone()).or_default();
                for (i, item) in items.iter().enumerate() {
                    s.declare(&format!("{}[{}]", kv.key, i), item.clone(), kv.value_span, out);
                    s.families
                        .entry(kv.key.clone())
                        .or_default()
                        .push((i as u32, TokenId((s.tokens.len() - 1) as u16)));
                }
                continue;
            }
            let name = kv.token();
            s.declare(&name, kv.value.clone(), kv.value_span, out);
            // The master's comment may declare the token's enum list
            // (`enum: a | b | c`). That list, in declared order, IS the
            // word numbering: `enum_of` indexes into it, the ABI's
            // `theme_enum` promises it, and a compiled plugin hardcodes
            // against it. The default's own word joins the list if the
            // master mis-spells it outside its own declaration, so a
            // baked value always has an index.
            if !kv.declared_words.is_empty() {
                let id = s.index[&name];
                s.tokens[id.index()].words = kv.declared_words.clone();
                if let Expr::Word(w) = &kv.value {
                    if !kv.declared_words.iter().any(|d| d == w) {
                        out.push(Diagnostic::warn(
                            kv.value_span,
                            format!(
                                "\"{name}\" is set to \"{w}\", which its own comment's \
                                 declared list does not contain"
                            ),
                        ));
                        s.tokens[id.index()].words.push(w.clone());
                    }
                }
            }
            if let Some(i) = kv.index {
                let id = s.index[&name];
                let fam = s.families.entry(kv.key.clone()).or_default();
                if !fam.iter().any(|(j, _)| *j == i) {
                    fam.push((i, id));
                }
            }
        }
        for fam in s.families.values_mut() {
            fam.sort_unstable();
        }
        s
    }

    fn declare(&mut self, name: &str, expr: Expr, span: Span, out: &mut Vec<Diagnostic>) {
        let deferred = expr.mentions_base();
        let (kind, words) = syntactic_kind(&expr);
        if let Some(id) = self.index.get(name).copied() {
            // Within a stage the last declaration of a token wins (§4.1).
            let t = &mut self.tokens[id.index()];
            t.expr = expr;
            t.kind = kind;
            t.span = span;
            t.deferred = deferred;
            t.words = words;
            return;
        }
        if self.tokens.len() >= u16::MAX as usize - 1 {
            out.push(Diagnostic::warn(
                span,
                format!("more than {} tokens declared; \"{name}\" is ignored", u16::MAX - 1),
            ));
            return;
        }
        self.index.insert(name.to_string(), TokenId(self.tokens.len() as u16));
        self.tokens.push(Token {
            name: name.to_string(),
            kind,
            expr,
            span,
            deferred,
            words,
        });
    }

    pub fn len(&self) -> usize {
        self.tokens.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tokens.is_empty()
    }

    pub fn id(&self, name: &str) -> Option<TokenId> {
        self.index.get(name).copied()
    }

    pub fn name(&self, id: TokenId) -> &str {
        self.tokens.get(id.index()).map(|t| t.name.as_str()).unwrap_or("<missing>")
    }

    pub fn kind(&self, id: TokenId) -> Kind {
        self.tokens.get(id.index()).map(|t| t.kind).unwrap_or(Kind::Scalar)
    }

    pub fn default_expr(&self, id: TokenId) -> Option<&Expr> {
        self.tokens.get(id.index()).map(|t| &t.expr)
    }

    pub fn default_span(&self, id: TokenId) -> Span {
        self.tokens.get(id.index()).map(|t| t.span).unwrap_or_default()
    }

    /// The token is a `[state]` template: its expression names `base`, "the
    /// class's own base colour", which only the class x state bake can supply.
    pub fn deferred(&self, id: TokenId) -> bool {
        self.tokens.get(id.index()).map(|t| t.deferred).unwrap_or(false)
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.tokens.iter().map(|t| t.name.as_str())
    }

    /// The slots of an indexed family, in index order.
    pub fn family(&self, base: &str) -> Option<&[(u32, TokenId)]> {
        self.families.get(base).map(|v| v.as_slice())
    }

    /// The word list a `Kind::Enum` token takes. When the master's comment
    /// declares one (`enum: a | b | c`) the list is that declaration, in
    /// declared order — the numbering `enum_of` answers and the ABI's
    /// `theme_enum` promises. Without a declaration it grows from the words
    /// the cascade has seen, `default`'s own at index 0. `enum_of(id)` on a
    /// resolved theme indexes into this either way.
    pub fn enum_words(&self, id: TokenId) -> &[String] {
        self.tokens.get(id.index()).map(|t| t.words.as_slice()).unwrap_or(&[])
    }

    pub fn enum_index(&self, id: TokenId, word: &str) -> Option<u16> {
        self.enum_words(id).iter().position(|w| w == word).map(|i| i as u16)
    }

    pub fn enum_word(&self, id: TokenId, i: u16) -> Option<&str> {
        self.enum_words(id).get(i as usize).map(|s| s.as_str())
    }

    /// Register a word this token may take, returning its index. Words arrive
    /// from themes as well as from `default`, so the list grows at load and is
    /// stable for the life of the process.
    pub fn intern_word(&mut self, id: TokenId, word: &str) -> u16 {
        let Some(t) = self.tokens.get_mut(id.index()) else { return 0 };
        if let Some(i) = t.words.iter().position(|w| w == word) {
            return i as u16;
        }
        t.words.push(word.to_string());
        (t.words.len() - 1) as u16
    }

    /// Adopt the kinds that only a resolution can settle: a token whose default
    /// is `@stroke.hair` is a length, and no syntactic rule can know that.
    /// Called once, with `default`'s own resolved values.
    pub fn adopt_kinds(&mut self, values: &[Value]) {
        for (i, v) in values.iter().enumerate() {
            if i >= self.tokens.len() {
                break;
            }
            if matches!(self.tokens[i].expr, Expr::Ref(..) | Expr::Ratio(..)) {
                self.tokens[i].kind = v.kind();
                if let (Kind::Enum, Value::Word(w)) = (v.kind(), v) {
                    if self.tokens[i].words.is_empty() {
                        self.tokens[i].words.push(w.clone());
                    }
                }
            }
        }
    }

    /// `default`'s dense starting point: every token has an expression, which is
    /// what makes stage 2 of §4.1 dense and every later stage sparse.
    pub fn base_spec(&self) -> ThemeSpec {
        ThemeSpec {
            label: "default".into(),
            exprs: self.tokens.iter().map(|t| t.expr.clone()).collect(),
            spans: self.tokens.iter().map(|t| t.span).collect(),
            wash: None,
        }
    }
}

/// The kind a value form declares on sight. `Expr::Ref` cannot be settled here
/// and is provisionally a colour until [`Schema::adopt_kinds`] corrects it.
fn syntactic_kind(e: &Expr) -> (Kind, Vec<String>) {
    match e {
        Expr::Color(_) | Expr::Rgb(..) | Expr::Oklch(..) | Expr::Call(..) => (Kind::Color, Vec::new()),
        Expr::Num(_) | Expr::Len(..) | Expr::Codepoint(_) | Expr::Ratio(..) => (Kind::Scalar, Vec::new()),
        Expr::Bool(_) => (Kind::Flag, Vec::new()),
        Expr::Text(_) => (Kind::Text, Vec::new()),
        Expr::Word(w) => {
            if super::expr::sentinel(w).is_some() {
                (Kind::Scalar, Vec::new())
            } else {
                (Kind::Enum, vec![w.clone()])
            }
        }
        Expr::Array(items) => items.first().map(syntactic_kind).unwrap_or((Kind::Scalar, Vec::new())),
        Expr::Ref(..) => (Kind::Color, Vec::new()),
        Expr::Bad(_) => (Kind::Scalar, Vec::new()),
    }
}

// ---------------------------------------------------------------- theme spec

/// The merged tree: one expression per token, plus where the winning
/// declaration came from so §4.3's enforcement notes can name it.
#[derive(Clone)]
pub struct ThemeSpec {
    pub label: String,
    pub exprs: Vec<Expr>,
    pub spans: Vec<Span>,
    /// `mood.<m>.wash`, the one-quad transition tint (§5.24). Not a token: it
    /// belongs to the mood, not to the tree the mood overlays.
    pub wash: Option<Expr>,
}

impl ThemeSpec {
    pub fn get(&self, id: TokenId) -> Option<&Expr> {
        self.exprs.get(id.index())
    }

    pub fn span(&self, id: TokenId) -> Span {
        self.spans.get(id.index()).copied().unwrap_or_default()
    }
}

/// One stage of §4.1. Stage 1 (the compiled-in fallback) is not a document: it
/// is the per-kind fallback in `resolve.rs`, because with `default.theme` as the
/// schema a `const FALLBACK: ResolvedTheme` could not be written down.
pub enum Stage<'a> {
    /// Stages 2, 3, 3a and 5: a whole document, applied in order.
    Document(&'a Document),
    /// Stage 4: one `[mood.<m>]` or `[variant.<v>]` block of a document.
    Overlay { doc: &'a Document, kind: SectionKind, name: String },
}

#[derive(Clone, Copy, Default)]
pub struct Options {
    /// `[meta] strict = true`: unknown keys are logged at error level. **The
    /// theme still loads** ([CONFLICT 10]).
    pub strict: bool,
}

/// Apply the stages in order onto `default`'s dense spec.
pub fn cascade(
    schema: &mut Schema,
    stages: &[Stage<'_>],
    opts: Options,
    out: &mut Vec<Diagnostic>,
) -> ThemeSpec {
    let mut spec = schema.base_spec();
    for st in stages {
        match st {
            Stage::Document(doc) => apply_document(&mut spec, schema, doc, None, opts, out),
            Stage::Overlay { doc, kind, name } => {
                apply_overlay(&mut spec, schema, doc, *kind, name, opts, out)
            }
        }
    }
    spec
}

fn apply_document(
    spec: &mut ThemeSpec,
    schema: &mut Schema,
    doc: &Document,
    only_overlay: Option<(SectionKind, &str)>,
    opts: Options,
    out: &mut Vec<Diagnostic>,
) {
    for kv in &doc.keys {
        let section = doc.sections.get(kv.section as usize);
        let overlay = section.map(Section::is_overlay).unwrap_or(false);
        match only_overlay {
            Some((kind, name)) => {
                let Some(s) = section else { continue };
                if s.kind != kind || s.path != name {
                    continue;
                }
            }
            // A plain pass never applies overlay sections: a mood is resolved
            // into its own complete sibling theme, not folded into the root.
            None if overlay => continue,
            None => {}
        }
        apply_key(spec, schema, kv, overlay, opts, out);
    }
}

fn apply_overlay(
    spec: &mut ThemeSpec,
    schema: &mut Schema,
    doc: &Document,
    kind: SectionKind,
    name: &str,
    opts: Options,
    out: &mut Vec<Diagnostic>,
) {
    // `inherit` chains one whole overlay node into another, depth 8 (§4.1).
    let mut chain: Vec<String> = vec![name.to_string()];
    let mut cur = name.to_string();
    loop {
        let Some(parent) = overlay_control(doc, kind, &cur, "inherit").and_then(text_of) else {
            break;
        };
        if chain.contains(&parent) {
            out.push(Diagnostic::warn(
                overlay_span(doc, kind, &cur),
                format!(
                    "mood chain cycle: {} -> {parent} (inherit is dropped)",
                    chain.join(" -> ")
                ),
            ));
            break;
        }
        if chain.len() as u32 >= MAX_MOOD_DEPTH {
            out.push(Diagnostic::warn(
                overlay_span(doc, kind, &cur),
                format!("mood chain depth > {MAX_MOOD_DEPTH}: {} (inherit is dropped)", chain.join(" -> ")),
            ));
            break;
        }
        chain.push(parent.clone());
        cur = parent;
    }
    // Parents first, so the named overlay wins.
    for anc in chain.iter().rev() {
        apply_document(spec, schema, doc, Some((kind, anc)), opts, out);
    }
    if let Some(w) = overlay_control(doc, kind, name, "wash") {
        spec.wash = Some(w.clone());
    }
    spec.label = format!("{}+{name}", spec.label);
}

fn overlay_control<'a>(doc: &'a Document, kind: SectionKind, name: &str, key: &str) -> Option<&'a Expr> {
    doc.keys.iter().find_map(|kv| {
        let s = doc.sections.get(kv.section as usize)?;
        (s.kind == kind && s.path == name && kv.key == key).then_some(&kv.value)
    })
}

fn overlay_span(doc: &Document, kind: SectionKind, name: &str) -> Span {
    doc.sections
        .iter()
        .find(|s| s.kind == kind && s.path == name)
        .map(|s| s.span)
        .unwrap_or_default()
}

fn text_of(e: &Expr) -> Option<String> {
    match e {
        Expr::Text(s) | Expr::Word(s) => Some(s.clone()),
        _ => None,
    }
}

fn apply_key(
    spec: &mut ThemeSpec,
    schema: &mut Schema,
    kv: &KeyVal,
    overlay: bool,
    opts: Options,
    out: &mut Vec<Diagnostic>,
) {
    // The three keys that belong to the mood rather than to the tree (§5.24).
    if overlay && MOOD_KEYS.contains(&kv.key.as_str()) && kv.index.is_none() {
        return;
    }
    if kv.locale.is_some() {
        // Localised text never reaches `ResolvedTheme`; it is collected into
        // `ThemeDiagnostics` by the loader.
        return;
    }

    let name = kv.token();

    // A whole-array override replaces a generated row entirely (§3.2): the
    // author now owns all sixteen. Per-slot pins leave the rest generated.
    if let (Expr::Array(items), None) = (&kv.value, kv.index) {
        let Some(fam) = schema.family(&kv.key).map(|f| f.to_vec()) else {
            unknown_key(&name, kv, schema, opts, out);
            return;
        };
        if items.is_empty() && fam.is_empty() {
            // Restating an empty row is a no-op, not a length mismatch.
            return;
        }
        if items.len() != fam.len() {
            out.push(Diagnostic::warn(
                kv.value_span,
                format!(
                    "\"{}\" takes {} values, found {} (whole row ignored; \
                     write \"{}[i] = ...\" to pin one slot)",
                    kv.key,
                    fam.len(),
                    items.len(),
                    kv.key
                ),
            ));
            return;
        }
        for ((_, id), item) in fam.iter().zip(items) {
            set(spec, schema, *id, item.clone(), kv.value_span, out);
        }
        return;
    }

    let Some(id) = resolve_name(&name, kv, schema, opts, out) else { return };
    set(spec, schema, id, kv.value.clone(), kv.value_span, out);
}

/// Whole-node replacement, with the type check §4.2 calls "type mismatch" and
/// the re-lex §3.2 calls a note.
fn set(
    spec: &mut ThemeSpec,
    schema: &mut Schema,
    id: TokenId,
    value: Expr,
    span: Span,
    out: &mut Vec<Diagnostic>,
) {
    let want = schema.kind(id);
    let name = schema.name(id).to_string();

    // Quotes are optional everywhere and mandatory nowhere except `text`: a
    // quoted value lands on a non-text token re-lexed, with a note (§3.2).
    let value = match (&value, want) {
        (Expr::Text(t), k) if k != Kind::Text => {
            let mut sink = Vec::new();
            let re = parse::relex(t, span, &mut sink);
            match re {
                Expr::Bad(_) => {
                    out.extend(sink);
                    out.push(Diagnostic::warn(
                        span,
                        format!("\"{t}\" is not a legal {} for \"{name}\" (using default)", kind_word(want)),
                    ));
                    return;
                }
                good => {
                    out.push(Diagnostic::note(
                        span,
                        format!("value quoted where a {} is expected — quotes ignored", kind_word(want)),
                    ));
                    good
                }
            }
        }
        _ => value,
    };

    if let Expr::Bad(reason) = &value {
        out.push(Diagnostic::warn(
            span,
            format!("{reason} (using default for \"{name}\")"),
        ));
        return;
    }

    // The type check. A colour expression assigned to a scalar token is §4.2's
    // "type mismatch" and behaves exactly like a malformed value.
    let (got, _) = syntactic_kind(&value);
    let compatible = match (want, got) {
        (a, b) if a == b => true,
        // A reference or a ratio is only settled by evaluation; resolve.rs
        // catches the mismatch there and reports it against the same span.
        _ if matches!(value, Expr::Ref(..) | Expr::Ratio(..)) => true,
        // An enum word may be a sentinel on a scalar token, and vice versa.
        (Kind::Scalar, Kind::Enum) | (Kind::Enum, Kind::Scalar) => true,
        // A token whose OWN default is a sentinel (`none`, `auto`) declares
        // no type at all: "colour or none" and "len or none" are written the
        // same way, and the master says which in its comment, where no parser
        // can read it. Such a token adopts whatever the first theme assigns.
        // This is the one place the schema-from-defaults design cannot infer,
        // so it declines to guess rather than refusing a legal value.
        _ if matches!(
            schema.default_expr(id),
            Some(Expr::Word(w)) if super::expr::sentinel(w).is_some()
        ) =>
        {
            true
        }
        // Nor can a default that is itself a reference or a ratio say what the
        // token is: `dock.h = @size.2xl` is a length, `panel.fill = @surface.panel`
        // a colour, and both look identical here. resolve.rs settles them by
        // evaluation and reports a genuine mismatch against this same span.
        _ if matches!(schema.default_expr(id), Some(Expr::Ref(..) | Expr::Ratio(..))) => true,
        _ => false,
    };
    if !compatible {
        out.push(Diagnostic::warn(
            span,
            format!(
                "expected a {} for \"{name}\", found a {} (using default)",
                kind_word(want),
                kind_word(got)
            ),
        ));
        return;
    }
    if want == Kind::Scalar && matches!(value, Expr::Num(_)) && matches!(schema.default_expr(id), Some(Expr::Len(..))) {
        // §4.3's second worked diagnostic, word for word.
        out.push(Diagnostic::warn(
            span,
            format!("expected a length with a unit, found a bare number (using default for \"{name}\")"),
        ));
        return;
    }
    if want == Kind::Enum {
        if let Expr::Word(w) = &value {
            schema.intern_word(id, w);
        }
    }
    if let Some(slot) = spec.exprs.get_mut(id.index()) {
        *slot = value;
        spec.spans[id.index()] = span;
    }
}

fn kind_word(k: Kind) -> &'static str {
    match k {
        Kind::Color => "colour",
        Kind::Scalar => "length or number",
        Kind::Flag => "true/false",
        Kind::Enum => "enum word",
        Kind::Text => "quoted text",
    }
}

fn resolve_name(
    name: &str,
    kv: &KeyVal,
    schema: &Schema,
    opts: Options,
    out: &mut Vec<Diagnostic>,
) -> Option<TokenId> {
    if let Some(id) = schema.id(name) {
        return Some(id);
    }
    // §4.2's static rename/alias table, consulted BEFORE edit distance:
    // Levenshtein <= 3 cannot find `panel.content_pad` from `panel.pad`.
    if let Some((target, why)) = alias(name) {
        match target.and_then(|t| schema.id(&t).map(|id| (t, id))) {
            Some((t, id)) => {
                out.push(Diagnostic::warn(kv.key_span, format!("\"{name}\" -> \"{t}\": {why}")));
                return Some(id);
            }
            None => {
                out.push(Diagnostic::warn(kv.key_span, format!("\"{name}\": {why}")));
                return None;
            }
        }
    }
    unknown_key(name, kv, schema, opts, out);
    None
}

fn unknown_key(name: &str, kv: &KeyVal, schema: &Schema, opts: Options, out: &mut Vec<Diagnostic>) {
    // "names the key, the nearest known token by Levenshtein distance <= 3,
    // and that token's resolved value" (§4.2). The value is filled in by the
    // loader, which is the only stage that has one.
    let msg = match parse::suggest(name, schema.names()) {
        Some(near) => format!("unknown key \"{name}\" — did you mean \"{near}\"? (ignored)"),
        None => format!("unknown key \"{name}\" (ignored)"),
    };
    let level = if opts.strict { Level::Error } else { Level::Warn };
    out.push(Diagnostic::new(level, kv.key_span, msg));
}

/// §4.2's static rename/alias table. `None` as a target means the key was
/// **cut**: there is no token and the message says why.
///
/// Every rename this engine performs is here. Adding one to the catalogue
/// without adding it here is a review failure, because the two together are the
/// only reason an author's muscle memory degrades into a message rather than
/// into silence.
pub fn alias(key: &str) -> Option<(Option<String>, &'static str)> {
    const CUT: &str = "cut: a theme may not move a panel rectangle (scope-boundary.md, CONFLICT 4)";
    if key == "panel.pad" {
        return Some((
            Some("panel.content_pad".into()),
            "renamed in schema 1 — the user's GridPadding owns \"pad\"",
        ));
    }
    if let Some(rest) = key.strip_prefix("alarm_bar.") {
        return Some((
            Some(format!("component.alarm_bar.{rest}")),
            "alarm bar colours live in the component layer",
        ));
    }
    if let Some(rest) = key.strip_prefix("severity.") {
        for (old, new) in [("fg", "text"), ("bg", "fill"), ("border", "edge")] {
            if let Some(role) = rest.strip_suffix(&format!(".{old}")) {
                return Some((
                    Some(format!("severity.{role}.{new}")),
                    "colour members are fill / edge / text / glyph / on (5.0)",
                ));
            }
        }
    }
    if let Some(rest) = key.strip_prefix("icon.size_") {
        return Some((Some(format!("icon.size.{rest}")), "ladders are dotted (5.0)"));
    }
    if key == "shape.button.alt" {
        return Some((Some("shape.button_alt".into()), "preset variants are underscored (5.0)"));
    }
    if let Some(rest) = key.strip_prefix("suffix.") {
        return Some((
            Some(format!("type.suffix.{rest}")),
            "the status suffix is a type role, not a family",
        ));
    }
    if key == "glow.strength_scale" {
        return Some((
            Some("glow.alpha_scale".into()),
            "glow has alpha (0..1) and boost (>1); \"strength\" was neither",
        ));
    }
    if key == "metric.unit_vh" {
        return Some((
            Some("metric.unit_pct_h".into()),
            "\"vh\" is a deprecated unit; this token is a percentage",
        ));
    }
    for axis in ["space", "type"] {
        if key == format!("density.{axis}") {
            return Some((Some(format!("metric.density_{axis}")), "the family is metric.*"));
        }
    }
    for cut in ["panel.gutter", "column.min_w", "column.max_w", "controlbar.h"] {
        if key == cut {
            return Some((None, CUT));
        }
    }
    for pre in ["layout.", "board.pad", "breakpoint."] {
        if key.starts_with(pre) {
            return Some((None, CUT));
        }
    }
    None
}

// ------------------------------------------------------- the [meta] base chain

/// Something that can open a theme by name — the filesystem search path in
/// practice, a map in the tests.
pub trait ThemeSource {
    fn open(&mut self, name: &str, src: &mut Sources, out: &mut Vec<Diagnostic>) -> Option<Document>;
}

/// Resolve `[meta] base = "aurora"` depth-first, cap 8 (§4.1). Returns the
/// ancestor documents **oldest first**, ready to be applied before the theme.
///
/// A missing parent, a cycle or an overflow all warn and **restart the chain at
/// `default`** — which, since `default` is stage 2 and always applied, means
/// returning the ancestors resolved so far and no more.
pub fn base_chain(
    doc: &Document,
    themes: &mut dyn ThemeSource,
    src: &mut Sources,
    out: &mut Vec<Diagnostic>,
) -> Vec<Document> {
    let mut chain: Vec<Document> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    let mut cur = doc.meta_text("meta.base");
    let mut span = doc
        .keys
        .iter()
        .find(|k| k.key == "meta.base")
        .map(|k| k.value_span)
        .unwrap_or_default();
    while let Some(name) = cur {
        if name.is_empty() || name == "default" {
            break;
        }
        if seen.contains(&name) {
            out.push(Diagnostic::warn(
                span,
                format!(
                    "[meta] base chain cycle: {} -> {name} (the chain restarts at default)",
                    seen.join(" -> ")
                ),
            ));
            break;
        }
        if seen.len() as u32 >= MAX_BASE_DEPTH {
            out.push(Diagnostic::warn(
                span,
                format!(
                    "[meta] base chain depth > {MAX_BASE_DEPTH}: {} (the chain restarts at default)",
                    seen.join(" -> ")
                ),
            ));
            break;
        }
        let Some(parent) = themes.open(&name, src, out) else {
            out.push(Diagnostic::warn(
                span,
                format!("[meta] base = \"{name}\" was not found (the chain restarts at default)"),
            ));
            break;
        };
        seen.push(name);
        cur = parent.meta_text("meta.base");
        span = parent
            .keys
            .iter()
            .find(|k| k.key == "meta.base")
            .map(|k| k.value_span)
            .unwrap_or(span);
        chain.push(parent);
    }
    chain.reverse(); // oldest ancestor first
    chain
}

/// Every mood and variant a document declares, capped at [`MAX_SIBLINGS`].
/// Each produces its **own complete sibling spec** (§2.2, §5.24): switching is
/// `self.active = i` — one store, no recomputation, no per-draw branch.
pub fn sibling_names(doc: &Document, out: &mut Vec<Diagnostic>) -> Vec<(SectionKind, String)> {
    let mut all: Vec<(SectionKind, String)> = Vec::new();
    for m in doc.overlays(SectionKind::Mood) {
        all.push((SectionKind::Mood, m));
    }
    for v in doc.overlays(SectionKind::Variant) {
        all.push((SectionKind::Variant, v));
    }
    if all.len() > MAX_SIBLINGS {
        let dropped: Vec<String> = all[MAX_SIBLINGS..].iter().map(|(_, n)| n.clone()).collect();
        out.push(Diagnostic::warn(
            Span::default(),
            format!(
                "more than {MAX_SIBLINGS} variants/moods declared; dropping {} (not a load failure)",
                dropped.join(", ")
            ),
        ));
        all.truncate(MAX_SIBLINGS);
    }
    all
}

/// Every mood a document declares, with its `when` predicate parsed (§5.24),
/// in declaration order.
///
/// A mood's trigger is its **own**. `inherit` chains the token overlay, not
/// the rule — exactly as [`apply_overlay`] does not chain `wash` — because
/// `[mood.lockdown]` inherits every colour `alert` sets and still says
/// `when = ""`, and a trigger that came down the chain would make that
/// sentence impossible to write.
pub fn mood_rules(doc: &Document, out: &mut Vec<Diagnostic>) -> Vec<MoodRule> {
    doc.overlays(SectionKind::Mood)
        .into_iter()
        .map(|name| {
            let declared = doc.keys.iter().find(|kv| {
                let s = doc.sections.get(kv.section as usize);
                kv.key == "when"
                    && s.is_some_and(|s| s.kind == SectionKind::Mood && s.path == name)
            });
            // The caret belongs under the VALUE where there is one, and under
            // the section header where the mood simply never wrote a `when`.
            let (text, span) = match declared {
                Some(kv) => (text_of(&kv.value).unwrap_or_default(), kv.value_span),
                None => (String::new(), overlay_span(doc, SectionKind::Mood, &name)),
            };
            let when = mood::parse_when(&name, &text, span, out);
            MoodRule { name, when }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::color::Color;
    use crate::theme::expr::Unit;
    use crate::theme::parse::{parse, Sources};

    fn doc(text: &str, src: &mut Sources, out: &mut Vec<Diagnostic>) -> Document {
        let f = src.add("t.theme", text);
        parse(src, f, None, out)
    }

    const DEFAULT: &str = "\
[meta]
schema = 1
name = \"default\"
[palette]
black = #0A100E
white = #EAF6F1
accent = #3FE3AE
[stroke]
hair = 0.2
[border]
width = @stroke.hair
[panel]
content_pad = 2.8u
corner_mode = round     # how the radius is cut · images 1,7 · enum: round | chamfer · -
[text]
primary = oklch(0.905, 0.06, 165)
[decor]
enabled = true
[term]
ansi = [ #000000, #CD3131, #0DBC79, #E5E510 ]
";

    fn schema_of(text: &str) -> (Schema, Sources, Vec<Diagnostic>) {
        let mut src = Sources::new();
        let mut out = Vec::new();
        let d = doc(text, &mut src, &mut out);
        let s = Schema::from_default(&d, &mut out);
        (s, src, out)
    }

    #[test]
    fn default_theme_is_the_schema() {
        let (s, _, out) = schema_of(DEFAULT);
        assert!(out.is_empty(), "{out:?}");
        // ids are declaration order
        assert_eq!(s.id("meta.schema"), Some(TokenId(0)));
        assert!(s.id("palette.accent").is_some());
        // an array declares an indexed family, addressable per slot
        assert_eq!(s.family("term.ansi").map(|f| f.len()), Some(4));
        assert!(s.id("term.ansi[2]").is_some());
        assert!(s.id("term.ansi[9]").is_none());
        // and nothing else exists
        assert!(s.id("panel.boarder").is_none());
    }

    #[test]
    fn a_mood_inside_the_master_does_not_become_everyones_default() {
        let (s, _, _) = schema_of(
            "[palette]\naccent = #3FE3AE\n[decor]\nenabled = true\n\
             [mood.alert]\npalette.accent = #FF2A35\ndecor.enabled = false\n\
             [variant.hc]\ndecor.enabled = false\n",
        );
        // stage 2 is the plain tree only
        assert_eq!(
            s.default_expr(s.id("palette.accent").unwrap()),
            Some(&Expr::Color(Color::from_hex("#3FE3AE").unwrap().to_linear()))
        );
        assert_eq!(s.default_expr(s.id("decor.enabled").unwrap()), Some(&Expr::Bool(true)));
        // and an overlay declares no token of its own
        assert!(s.id("wash").is_none());
        assert!(s.id("inherit").is_none());
    }

    #[test]
    fn kinds_come_from_the_form_of_the_default_value() {
        let (s, _, _) = schema_of(DEFAULT);
        assert_eq!(s.kind(s.id("palette.accent").unwrap()), Kind::Color);
        assert_eq!(s.kind(s.id("text.primary").unwrap()), Kind::Color);
        assert_eq!(s.kind(s.id("panel.content_pad").unwrap()), Kind::Scalar);
        assert_eq!(s.kind(s.id("stroke.hair").unwrap()), Kind::Scalar);
        assert_eq!(s.kind(s.id("decor.enabled").unwrap()), Kind::Flag);
        assert_eq!(s.kind(s.id("panel.corner_mode").unwrap()), Kind::Enum);
        assert_eq!(s.kind(s.id("meta.name").unwrap()), Kind::Text);
        // The comment's `enum: round | chamfer` is the declaration: both
        // words exist before any theme has used them, in declared order.
        let cm = s.id("panel.corner_mode").unwrap();
        assert_eq!(s.enum_words(cm), ["round", "chamfer"]);
        assert_eq!(s.enum_index(cm, "chamfer"), Some(1));
    }

    #[test]
    fn a_reference_takes_the_kind_of_what_it_points_at() {
        let (mut s, _, _) = schema_of(DEFAULT);
        // syntactically a Ref looks like a colour...
        assert_eq!(s.kind(s.id("border.width").unwrap()), Kind::Color);
        // ...until default has been resolved once.
        let mut values = vec![Value::Num(0.0); s.len()];
        values[s.id("border.width").unwrap().index()] = Value::Num(0.2);
        s.adopt_kinds(&values);
        assert_eq!(s.kind(s.id("border.width").unwrap()), Kind::Scalar);
    }

    #[test]
    fn three_levels_of_override_and_the_last_stage_wins() {
        let (mut s, mut src, mut out) = schema_of(DEFAULT);
        let mid = doc("[palette]\naccent = #FF2A35\n[panel]\ncontent_pad = 3u\n", &mut src, &mut out);
        let top = doc("[panel]\ncontent_pad = 4u\n", &mut src, &mut out);
        let user = doc("[palette]\naccent = #29B6F6\n", &mut src, &mut out);
        let spec = cascade(
            &mut s,
            &[Stage::Document(&mid), Stage::Document(&top), Stage::Document(&user)],
            Options::default(),
            &mut out,
        );
        assert!(out.is_empty(), "{out:?}");
        let acc = s.id("palette.accent").unwrap();
        assert_eq!(
            spec.get(acc),
            Some(&Expr::Color(Color::from_hex("#29B6F6").unwrap().to_linear()))
        );
        assert_eq!(spec.get(s.id("panel.content_pad").unwrap()), Some(&Expr::Len(4.0, Unit::U)));
        // untouched tokens still hold DEFAULT's expression, not its value
        assert_eq!(spec.get(s.id("border.width").unwrap()), Some(&Expr::Ref("stroke.hair".into(), None)));
    }

    #[test]
    fn an_override_replaces_a_whole_node_never_partially() {
        let (mut s, mut src, mut out) = schema_of(DEFAULT);
        let t = doc("[text]\nprimary = #FFFFFF\n", &mut src, &mut out);
        let spec = cascade(&mut s, &[Stage::Document(&t)], Options::default(), &mut out);
        // the oklch() call is gone entirely; there is no merge of L with the
        // literal's channels.
        assert!(matches!(spec.get(s.id("text.primary").unwrap()), Some(Expr::Color(_))));
    }

    #[test]
    fn an_unknown_key_warns_with_a_suggestion_and_is_ignored() {
        let (mut s, mut src, mut out) = schema_of(DEFAULT);
        let t = doc("[panel]\nboarder = #FFFFFF\n", &mut src, &mut out);
        let _ = cascade(&mut s, &[Stage::Document(&t)], Options::default(), &mut out);
        let m = &out.last().unwrap().message;
        assert!(m.contains("unknown key \"panel.boarder\""), "{m}");
    }

    #[test]
    fn strict_raises_the_level_and_still_loads() {
        let (mut s, mut src, mut out) = schema_of(DEFAULT);
        let t = doc("[panel]\nboarder = #FFFFFF\n", &mut src, &mut out);
        let spec = cascade(&mut s, &[Stage::Document(&t)], Options { strict: true }, &mut out);
        assert_eq!(out.last().unwrap().level, Level::Error);
        assert_eq!(spec.exprs.len(), s.len(), "the theme still loads");
    }

    #[test]
    fn the_alias_table_is_consulted_before_edit_distance() {
        let (mut s, mut src, mut out) = schema_of(DEFAULT);
        // panel.pad -> panel.content_pad is distance 8: only the table finds it
        let t = doc("[panel]\npad = 3u\n", &mut src, &mut out);
        let spec = cascade(&mut s, &[Stage::Document(&t)], Options::default(), &mut out);
        assert!(out.iter().any(|d| d.message.contains("GridPadding owns")), "{out:?}");
        assert_eq!(spec.get(s.id("panel.content_pad").unwrap()), Some(&Expr::Len(3.0, Unit::U)));
        // and a cut key names the reason instead of guessing
        assert!(alias("layout.columns").unwrap().0.is_none());
        assert!(alias("severity.warning.fg").unwrap().0.unwrap() == "severity.warning.text");
    }

    #[test]
    fn a_type_mismatch_falls_back_to_default() {
        let (mut s, mut src, mut out) = schema_of(DEFAULT);
        let t = doc("[panel]\ncontent_pad = #FFFFFF\n", &mut src, &mut out);
        let spec = cascade(&mut s, &[Stage::Document(&t)], Options::default(), &mut out);
        assert!(out.iter().any(|d| d.message.contains("expected a length or number")), "{out:?}");
        assert_eq!(spec.get(s.id("panel.content_pad").unwrap()), Some(&Expr::Len(2.8, Unit::U)));
    }

    #[test]
    fn a_bare_number_where_a_length_is_wanted_is_reported() {
        let (mut s, mut src, mut out) = schema_of(DEFAULT);
        let t = doc("[panel]\ncontent_pad = 8\n", &mut src, &mut out);
        let spec = cascade(&mut s, &[Stage::Document(&t)], Options::default(), &mut out);
        assert!(
            out.iter().any(|d| d.message.contains("expected a length with a unit")),
            "{out:?}"
        );
        assert_eq!(spec.get(s.id("panel.content_pad").unwrap()), Some(&Expr::Len(2.8, Unit::U)));
    }

    #[test]
    fn a_quoted_value_on_a_colour_token_is_a_note_and_works() {
        let (mut s, mut src, mut out) = schema_of(DEFAULT);
        let t = doc("[palette]\naccent = \"#FF2A35\"\n", &mut src, &mut out);
        let spec = cascade(&mut s, &[Stage::Document(&t)], Options::default(), &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].level, Level::Note);
        assert_eq!(
            spec.get(s.id("palette.accent").unwrap()),
            Some(&Expr::Color(Color::from_hex("#FF2A35").unwrap().to_linear()))
        );
    }

    #[test]
    fn a_whole_array_replaces_the_row_and_a_wrong_length_does_not() {
        let (mut s, mut src, mut out) = schema_of(DEFAULT);
        let ok = doc("[term]\nansi = [ #111111, #222222, #333333, #444444 ]\n", &mut src, &mut out);
        let spec = cascade(&mut s, &[Stage::Document(&ok)], Options::default(), &mut out);
        assert!(out.is_empty(), "{out:?}");
        assert_eq!(
            spec.get(s.id("term.ansi[3]").unwrap()),
            Some(&Expr::Color(Color::from_hex("#444444").unwrap().to_linear()))
        );
        let bad = doc("[term]\nansi = [ #111111, #222222 ]\n", &mut src, &mut out);
        let spec2 = cascade(&mut s, &[Stage::Document(&bad)], Options::default(), &mut out);
        assert!(out.iter().any(|d| d.message.contains("takes 4 values, found 2")), "{out:?}");
        assert_eq!(
            spec2.get(s.id("term.ansi[0]").unwrap()),
            Some(&Expr::Color(Color::from_hex("#000000").unwrap().to_linear()))
        );
    }

    #[test]
    fn one_slot_pins_and_leaves_the_rest() {
        let (mut s, mut src, mut out) = schema_of(DEFAULT);
        let t = doc("[term]\nansi[1] = #2A7FE0\n", &mut src, &mut out);
        let spec = cascade(&mut s, &[Stage::Document(&t)], Options::default(), &mut out);
        assert!(out.is_empty(), "{out:?}");
        assert_eq!(
            spec.get(s.id("term.ansi[1]").unwrap()),
            Some(&Expr::Color(Color::from_hex("#2A7FE0").unwrap().to_linear()))
        );
        assert_eq!(
            spec.get(s.id("term.ansi[0]").unwrap()),
            Some(&Expr::Color(Color::from_hex("#000000").unwrap().to_linear()))
        );
        // an index past the end is an unknown key
        let past = doc("[term]\nansi[9] = #FFFFFF\n", &mut src, &mut out);
        let _ = cascade(&mut s, &[Stage::Document(&past)], Options::default(), &mut out);
        assert!(out.iter().any(|d| d.message.contains("unknown key \"term.ansi[9]\"")), "{out:?}");
    }

    #[test]
    fn a_mood_is_a_sibling_not_a_fold_into_the_root() {
        let (mut s, mut src, mut out) = schema_of(DEFAULT);
        let t = doc(
            "[palette]\naccent = #FF2A35\n[mood.alert]\npalette.accent = #FFFFFF\nwash = #FF2A35 / 0.22\n",
            &mut src,
            &mut out,
        );
        let plain = cascade(&mut s, &[Stage::Document(&t)], Options::default(), &mut out);
        let acc = s.id("palette.accent").unwrap();
        assert_eq!(plain.get(acc), Some(&Expr::Color(Color::from_hex("#FF2A35").unwrap().to_linear())));
        assert!(plain.wash.is_none());

        let alert = cascade(
            &mut s,
            &[
                Stage::Document(&t),
                Stage::Overlay { doc: &t, kind: SectionKind::Mood, name: "alert".into() },
            ],
            Options::default(),
            &mut out,
        );
        assert_eq!(alert.get(acc), Some(&Expr::Color(Color::WHITE.to_linear())));
        assert!(alert.wash.is_some(), "the mood's wash belongs to the mood");
        assert!(alert.label.ends_with("+alert"));
    }

    #[test]
    fn a_mood_inherits_another_mood_depth_capped() {
        let (mut s, mut src, mut out) = schema_of(DEFAULT);
        let t = doc(
            "[mood.normal]\npanel.content_pad = 3u\n\
             [mood.alert]\ninherit = \"normal\"\npalette.accent = #FF2A35\n",
            &mut src,
            &mut out,
        );
        let spec = cascade(
            &mut s,
            &[Stage::Overlay { doc: &t, kind: SectionKind::Mood, name: "alert".into() }],
            Options::default(),
            &mut out,
        );
        assert_eq!(spec.get(s.id("panel.content_pad").unwrap()), Some(&Expr::Len(3.0, Unit::U)));
        assert_eq!(
            spec.get(s.id("palette.accent").unwrap()),
            Some(&Expr::Color(Color::from_hex("#FF2A35").unwrap().to_linear()))
        );
    }

    #[test]
    fn a_mood_inherit_cycle_warns_and_drops_the_inherit() {
        let (mut s, mut src, mut out) = schema_of(DEFAULT);
        let t = doc(
            "[mood.a]\ninherit = \"b\"\n[mood.b]\ninherit = \"a\"\npalette.accent = #FF2A35\n",
            &mut src,
            &mut out,
        );
        let _ = cascade(
            &mut s,
            &[Stage::Overlay { doc: &t, kind: SectionKind::Mood, name: "a".into() }],
            Options::default(),
            &mut out,
        );
        assert!(out.iter().any(|d| d.message.contains("mood chain cycle")), "{out:?}");
    }

    #[test]
    fn more_than_eight_siblings_is_reported_and_the_excess_dropped() {
        let mut src = Sources::new();
        let mut out = Vec::new();
        let mut text = String::new();
        for i in 0..11 {
            text.push_str(&format!("[mood.m{i}]\npalette.accent = #FF2A35\n"));
        }
        let d = doc(&text, &mut src, &mut out);
        let sibs = sibling_names(&d, &mut out);
        assert_eq!(sibs.len(), MAX_SIBLINGS);
        assert!(out.iter().any(|x| x.message.contains("more than 8 variants/moods")), "{out:?}");
    }

    #[test]
    fn the_base_chain_is_depth_first_capped_and_recovers() {
        struct Fs(HashMap<String, String>);
        impl ThemeSource for Fs {
            fn open(&mut self, name: &str, src: &mut Sources, out: &mut Vec<Diagnostic>) -> Option<Document> {
                let text = self.0.get(name)?.clone();
                let f = src.add(format!("{name}.theme"), text);
                Some(parse(src, f, None, out))
            }
        }
        let mut fs = Fs(HashMap::new());
        fs.0.insert("aurora".into(), "[meta]\nbase = \"spring\"\n[palette]\naccent = #3FE3AE\n".into());
        fs.0.insert("spring".into(), "[palette]\nwhite = #FFFFFF\n".into());
        let mut src = Sources::new();
        let mut out = Vec::new();
        let leaf = doc("[meta]\nbase = \"aurora\"\n", &mut src, &mut out);
        let chain = base_chain(&leaf, &mut fs, &mut src, &mut out);
        assert!(out.is_empty(), "{out:?}");
        // oldest ancestor first: spring, then aurora
        assert_eq!(chain.len(), 2);
        assert!(chain[0].keys.iter().any(|k| k.key == "palette.white"));

        // a missing parent warns and restarts at default
        let orphan = doc("[meta]\nbase = \"nowhere\"\n", &mut src, &mut out);
        let c2 = base_chain(&orphan, &mut fs, &mut src, &mut out);
        assert!(c2.is_empty());
        assert!(out.iter().any(|d| d.message.contains("was not found")), "{out:?}");
    }

    #[test]
    fn a_base_chain_cycle_warns_naming_the_chain() {
        struct Fs(HashMap<String, String>);
        impl ThemeSource for Fs {
            fn open(&mut self, name: &str, src: &mut Sources, out: &mut Vec<Diagnostic>) -> Option<Document> {
                let text = self.0.get(name)?.clone();
                let f = src.add(format!("{name}.theme"), text);
                Some(parse(src, f, None, out))
            }
        }
        let mut fs = Fs(HashMap::new());
        fs.0.insert("a".into(), "[meta]\nbase = \"b\"\n".into());
        fs.0.insert("b".into(), "[meta]\nbase = \"a\"\n".into());
        let mut src = Sources::new();
        let mut out = Vec::new();
        let leaf = doc("[meta]\nbase = \"a\"\n", &mut src, &mut out);
        let _ = base_chain(&leaf, &mut fs, &mut src, &mut out);
        assert!(out.iter().any(|d| d.message.contains("base chain cycle")), "{out:?}");
    }

    #[test]
    fn localised_names_never_become_tokens() {
        let (s, _, _) = schema_of("[meta]\nname = \"default\"\nname[pl] = \"Domyślny\"\n");
        assert!(s.id("meta.name").is_some());
        assert!(s.id("meta.name[pl]").is_none());
        assert_eq!(s.localised.len(), 1);
        assert_eq!(s.localised[0].2, "Domyślny");
    }
}
