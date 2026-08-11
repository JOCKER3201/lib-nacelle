//! Widgets written as scripts.
//!
//! A widget is a directory holding a `<name>.rhai` script and whatever
//! assets it needs. The script defines one function:
//!
//! ```text
//! fn draw() {
//!     [
//!         title("UPTIME"),
//!         rows([
//!             ["UP",   uptime(host.uptime)],
//!             ["HOST", upper(host.name)],
//!         ]),
//!     ]
//! }
//! ```
//!
//! It RETURNS a list of elements rather than drawing them. That is what
//! keeps this fast enough to run every frame: one call per widget, no
//! crossing back and forth for each primitive. It is also what keeps the
//! interface stable — the elements are a small vocabulary that can grow
//! without invalidating scripts, unlike a binary interface where moving
//! a field breaks every widget silently.
//!
//! Scripts are sandboxed by what they are given: `host` and the element
//! and formatting functions, and nothing else. A widget cannot read a
//! file, open a socket or run a program, because no such function
//! exists in its world.

use crate::ui::{self, Align};
use crate::widget::Sizing;
use crate::{Host, Widget};
use crate::font::FONT_UI;
use crate::telemetry::{fmt_bytes, fmt_rate, fmt_uptime};
use crate::theme::{self, Color, TokenId};
use crate::{Ctx, Rect};
use rhai::{Array, Dynamic, Engine, Map, Scope, AST};
use std::path::Path;
use std::sync::OnceLock;

/// Token id resolved once by name; MISSING degrades through the engine's
/// per-kind fallback rather than panicking.
fn tok(cell: &'static OnceLock<TokenId>, name: &'static str) -> TokenId {
    *cell.get_or_init(|| theme::id(name).unwrap_or(TokenId::MISSING))
}

/// A colour token, delivered in the `Color` the draw calls take.
fn col(cell: &'static OnceLock<TokenId>, name: &'static str) -> Color {
    let c = theme::resolved().color(tok(cell, name));
    Color { r: c.r, g: c.g, b: c.b, a: c.a }
}

/// Compiled widget script.
pub struct Script {
    engine: Engine,
    ast: AST,
    /// Reported once: a script that keeps failing must not flood the
    /// terminal with the same message sixty times a second.
    failed: bool,
}

/// Builds the engine every script runs in. Everything a script can do is
/// registered here; there is no other way in.
fn engine() -> Engine {
    let mut engine = Engine::new();
    // A runaway script must not take the frame with it.
    engine.set_max_operations(200_000);
    engine.set_max_expr_depths(64, 64);
    engine.set_max_string_size(10_000);
    engine.set_max_array_size(4_000);

    // --- elements -------------------------------------------------
    let el = |kind: &str| {
        let mut m = Map::new();
        m.insert("kind".into(), kind.into());
        m
    };

    // The header is the script author's choice, piece by piece:
    //   title("CPU")               — text with the underline
    //   title("CPU", false)        — text alone
    //   title("", true) / title("") — the underline alone
    //   (no title element)         — neither
    fn title_map(left: &str, right: &str, line: bool) -> Map {
        let mut m = Map::new();
        m.insert("kind".into(), "title".into());
        m.insert("left".into(), left.into());
        m.insert("right".into(), right.into());
        m.insert("line".into(), line.into());
        m
    }
    engine.register_fn("title", move |text: &str| title_map(text, "", true));
    engine.register_fn("title", move |text: &str, line: bool| title_map(text, "", line));
    engine.register_fn("title", move |left: &str, right: &str| title_map(left, right, true));
    engine.register_fn("title", move |left: &str, right: &str, line: bool| {
        title_map(left, right, line)
    });
    // Copies a call's option map into the element, keys the element has
    // not already claimed. Unknown options ride along unread — a script
    // written against a NEWER vocabulary still parses here.
    fn merge_opts(m: &mut Map, opts: Map) {
        for (k, v) in opts {
            m.entry(k).or_insert(v);
        }
    }
    engine.register_fn("rows", move |rows: Array| {
        let mut m = Map::new();
        m.insert("kind".into(), "rows".into());
        m.insert("rows".into(), Dynamic::from_array(rows));
        m
    });
    // rows(items, #{ label_role, value_role, columns, label_width, align,
    // density }) — u2 §3.1 #4. An item may be [label, value] or
    // [label, value, severity].
    engine.register_fn("rows", move |rows: Array, opts: Map| {
        let mut m = Map::new();
        m.insert("kind".into(), "rows".into());
        m.insert("rows".into(), Dynamic::from_array(rows));
        merge_opts(&mut m, opts);
        m
    });
    engine.register_fn("text", move |content: &str| {
        let mut m = Map::new();
        m.insert("kind".into(), "text".into());
        m.insert("content".into(), content.into());
        // No alignment stored: a script that names none defers to the
        // theme's `script.text_align`, which the renderer reads.
        m.insert("size".into(), Dynamic::from_float(1.0));
        m
    });
    engine.register_fn("text", move |content: &str, align: &str, size: f64| {
        let mut m = Map::new();
        m.insert("kind".into(), "text".into());
        m.insert("content".into(), content.into());
        m.insert("align".into(), align.into());
        m.insert("size".into(), Dynamic::from_float(size));
        m
    });
    // text(content, align, #{ role, severity }) — u2 §3.1 #2. The free
    // size becomes a role name; the deprecated size form is mapped to the
    // nearest role at draw time.
    engine.register_fn("text", move |content: &str, align: &str, opts: Map| {
        let mut m = Map::new();
        m.insert("kind".into(), "text".into());
        m.insert("content".into(), content.into());
        m.insert("align".into(), align.into());
        merge_opts(&mut m, opts);
        m
    });
    // runs(items, align) — u2 §3.1 #3, NEW: one line of styled runs
    // sharing a baseline, aligned as a unit. Each item is
    // #{ t, role, severity, blink, align }; blink names a motion.* effect
    // and drives the run's ALPHA, never its glyph (I13). An item's
    // align: "right" pins it to the line's right end — u2 §2.5's
    // temperature run — while the rest align as one unit.
    engine.register_fn("runs", move |items: Array| {
        let mut m = Map::new();
        m.insert("kind".into(), "runs".into());
        m.insert("items".into(), Dynamic::from_array(items));
        m
    });
    engine.register_fn("runs", move |items: Array, align: &str| {
        let mut m = Map::new();
        m.insert("kind".into(), "runs".into());
        m.insert("items".into(), Dynamic::from_array(items));
        m.insert("align".into(), align.into());
        m
    });
    // rule() — u2 §3.1 #12, NEW: a horizontal hairline as a stack element
    // in its own right. Until now the only rule was welded to `title`.
    engine.register_fn("rule", move || el("rule"));
    // group(label, elements) — u2 §3.1 #13, NEW: a labelled sub-block —
    // a section caption, an optional rule, and a nested element list
    // measured as one unit.
    engine.register_fn("group", move |label: &str, elements: Array| {
        let mut m = Map::new();
        m.insert("kind".into(), "group".into());
        m.insert("label".into(), label.into());
        m.insert("elements".into(), Dynamic::from_array(elements));
        m
    });
    // badge(text, #{ severity, style }) — u2 §3.1 #11, NEW: the status
    // pill of images 1, 3 and 4. The string is content; the severity is
    // the script's judgement of it; every colour is the theme's.
    engine.register_fn("badge", move |text: &str| {
        let mut m = Map::new();
        m.insert("kind".into(), "badge".into());
        m.insert("text".into(), text.into());
        m
    });
    engine.register_fn("badge", move |text: &str, opts: Map| {
        let mut m = Map::new();
        m.insert("kind".into(), "badge".into());
        m.insert("text".into(), text.into());
        merge_opts(&mut m, opts);
        m
    });
    engine.register_fn("meter", move |label: &str, frac: f64, value: &str| {
        let mut m = Map::new();
        m.insert("kind".into(), "meter".into());
        m.insert("label".into(), label.into());
        m.insert("fraction".into(), Dynamic::from_float(frac));
        m.insert("value".into(), value.into());
        m
    });
    // meter(label, frac, value, #{ severity, track }) — u2 §3.1 #6.
    engine.register_fn("meter", move |label: &str, frac: f64, value: &str, opts: Map| {
        let mut m = Map::new();
        m.insert("kind".into(), "meter".into());
        m.insert("label".into(), label.into());
        m.insert("fraction".into(), Dynamic::from_float(frac));
        m.insert("value".into(), value.into());
        merge_opts(&mut m, opts);
        m
    });
    engine.register_fn("gauges", move |values: Array, columns: i64| {
        let mut m = Map::new();
        m.insert("kind".into(), "gauges".into());
        m.insert("values".into(), Dynamic::from_array(values));
        m.insert("columns".into(), Dynamic::from_int(columns));
        m
    });
    // gauges(values, #{ columns, style, label, value_fmt }) — u2 §3.1 #7,
    // style ∈ { row, cell, bar, donut }.
    engine.register_fn("gauges", move |values: Array, opts: Map| {
        let mut m = Map::new();
        m.insert("kind".into(), "gauges".into());
        m.insert("values".into(), Dynamic::from_array(values));
        merge_opts(&mut m, opts);
        m
    });
    engine.register_fn("dots", move |frac: f64| {
        let mut m = Map::new();
        m.insert("kind".into(), "dots".into());
        m.insert("fraction".into(), Dynamic::from_float(frac));
        m
    });
    engine.register_fn("table", move |headings: Array, rows: Array, elastic: i64| {
        let mut m = Map::new();
        m.insert("kind".into(), "table".into());
        m.insert("headings".into(), Dynamic::from_array(headings));
        m.insert("rows".into(), Dynamic::from_array(rows));
        m.insert("elastic".into(), Dynamic::from_int(elastic));
        m
    });
    // table(headings, rows, elastic, #{ zebra, severity_col }) — u2 §3.1
    // #10. A heading may be [name, align] or [name, align, #{ kind,
    // width, of }], kind ∈ { text, bar, badge }.
    engine.register_fn(
        "table",
        move |headings: Array, rows: Array, elastic: i64, opts: Map| {
            let mut m = Map::new();
            m.insert("kind".into(), "table".into());
            m.insert("headings".into(), Dynamic::from_array(headings));
            m.insert("rows".into(), Dynamic::from_array(rows));
            m.insert("elastic".into(), Dynamic::from_int(elastic));
            merge_opts(&mut m, opts);
            m
        },
    );
    engine.register_fn("columns", move |cells: Array| {
        let mut m = Map::new();
        m.insert("kind".into(), "columns".into());
        m.insert("cells".into(), Dynamic::from_array(cells));
        m
    });
    // columns(cells, #{ label_role, value_role, align, dividers }) — u2
    // §3.1 #5. A cell may be [label, value] or [label, value, severity].
    engine.register_fn("columns", move |cells: Array, opts: Map| {
        let mut m = Map::new();
        m.insert("kind".into(), "columns".into());
        m.insert("cells".into(), Dynamic::from_array(cells));
        merge_opts(&mut m, opts);
        m
    });
    engine.register_fn("space", move |size: f64| {
        let mut m = el("space");
        m.insert("size".into(), Dynamic::from_float(size));
        m
    });

    // --- formatting -----------------------------------------------
    engine.register_fn("bytes", |n: f64| fmt_bytes(n.max(0.0) as u64));
    engine.register_fn("bytes", |n: i64| fmt_bytes(n.max(0) as u64));
    engine.register_fn("rate", |n: f64| fmt_rate(n));
    engine.register_fn("uptime", |n: f64| fmt_uptime(n.max(0.0) as u64));
    engine.register_fn("uptime", |n: i64| fmt_uptime(n.max(0) as u64));
    engine.register_fn("upper", |s: &str| s.to_uppercase());
    engine.register_fn("lower", |s: &str| s.to_lowercase());
    engine.register_fn("round", |n: f64, places: i64| {
        let p = places.clamp(0, 6) as usize;
        format!("{n:.p$}")
    });
    engine
}

/// The host data every script on this frame reads, built once and
/// handed out as a shared value.
///
/// It used to be built per widget, and it is not small: the process
/// table alone is a map per process with its name copied into it. Eight
/// scripted widgets meant eight copies of the whole machine's state
/// sixty times a second — more than half of everything the program did,
/// measured — while only the process list widget looked at the heaviest
/// part of it. The data is the same for all of them within a frame, so
/// now it is made once; time comes from the host, so a new frame is a
/// new map.
fn host_shared(host: &Host) -> Dynamic {
    thread_local! {
        static CACHE: std::cell::RefCell<Option<(f64, Dynamic)>> =
            const { std::cell::RefCell::new(None) };
    }
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if let Some((t, d)) = c.as_ref() {
            if *t == host.t {
                return d.clone();
            }
        }
        // Shared, so handing it to each script is a reference count and
        // not another copy of the process table.
        let d = Dynamic::from_map(host_map(host)).into_shared();
        *c = Some((host.t, d.clone()));
        d
    })
}

/// The two parts of the host data that are lists rather than numbers,
/// kept until the collector replaces the snapshot they came from.
///
/// The clock in the map has to be rebuilt every frame, but the process
/// table does not: it is rewritten once a second and copying it sixty
/// times a second was the single most expensive thing the program did.
fn host_lists(host: &Host) -> (Dynamic, Dynamic) {
    thread_local! {
        static CACHE: std::cell::RefCell<Option<(u64, Dynamic, Dynamic)>> =
            const { std::cell::RefCell::new(None) };
    }
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        let s = host.snap;
        if let Some((g, procs, each)) = c.as_ref() {
            if *g == s.generation {
                return (procs.clone(), each.clone());
            }
        }
        let procs = Dynamic::from_array(
            s.top
                .iter()
                .map(|p| {
                    let mut e = Map::new();
                    e.insert("pid".into(), Dynamic::from_int(p.pid as i64));
                    e.insert("name".into(), p.name.clone().into());
                    e.insert("cpu".into(), Dynamic::from_float(p.cpu as f64));
                    e.insert("mem".into(), Dynamic::from_float(p.mem_pct as f64));
                    Dynamic::from_map(e)
                })
                .collect(),
        )
        .into_shared();
        let each = Dynamic::from_array(
            s.cpu_per_core
                .iter()
                .map(|v| Dynamic::from_float(*v as f64))
                .collect(),
        )
        .into_shared();
        *c = Some((s.generation, procs.clone(), each.clone()));
        (procs, each)
    })
}

/// The host data a script can read, as a plain map. Rebuilt per frame:
/// a script sees a snapshot, never a live handle it could hold on to.
fn host_map(host: &Host) -> Map {
    let s = host.snap;
    let mut m = Map::new();
    let mut put = |k: &str, v: Dynamic| {
        m.insert(k.into(), v);
    };
    put("cpu_name", s.cpu_name.clone().into());
    let (procs, cpu_each) = host_lists(host);
    put("cpu_each", cpu_each);
    put("cpu_cores", Dynamic::from_int(s.cpu_per_core.len() as i64));
    put("load1", Dynamic::from_float(s.load_avg[0]));
    put("load5", Dynamic::from_float(s.load_avg[1]));
    put("load15", Dynamic::from_float(s.load_avg[2]));
    put(
        "temp",
        s.temp_c
            .map(|t| Dynamic::from_float(t as f64))
            .unwrap_or(Dynamic::UNIT),
    );
    put("mem_used", Dynamic::from_float(s.mem_used as f64));
    put("mem_total", Dynamic::from_float(s.mem_total as f64));
    put("mem_fraction", Dynamic::from_float(frac(s.mem_used, s.mem_total)));
    put("swap_used", Dynamic::from_float(s.swap_used as f64));
    put("swap_total", Dynamic::from_float(s.swap_total as f64));
    put(
        "swap_fraction",
        Dynamic::from_float(frac(s.swap_used, s.swap_total)),
    );
    put("uptime", Dynamic::from_float(s.uptime as f64));
    put("iface", s.iface.clone().into());
    put(
        "ipv4",
        s.ipv4.clone().map(Dynamic::from).unwrap_or(Dynamic::UNIT),
    );
    put(
        "ping",
        s.ping_ms
            .map(|p| Dynamic::from_int(p as i64))
            .unwrap_or(Dynamic::UNIT),
    );
    put("online", s.online.into());
    put("net_up", Dynamic::from_float(s.net_up_rate));
    put("net_down", Dynamic::from_float(s.net_down_rate));
    put("manufacturer", s.manufacturer.clone().into());
    put("model", s.model.clone().into());
    put("chassis", s.chassis.clone().into());
    put("name", s.hostname.clone().into());
    put("user", s.username.clone().into());
    put("os", s.os_name.clone().into());
    put("kernel", s.kernel.clone().into());
    put(
        "battery",
        s.battery
            .map(|(p, _)| Dynamic::from_int(p as i64))
            .unwrap_or(Dynamic::UNIT),
    );
    put(
        "charging",
        s.battery.map(|(_, c)| Dynamic::from(c)).unwrap_or(Dynamic::UNIT),
    );
    put("proc_count", Dynamic::from_int(s.proc_count as i64));
    // The wall clock and the animation clock: widgets that show the
    // time or blink need them, and nothing else in the host data does.
    let now = chrono::Local::now();
    use chrono::{Datelike, Timelike};
    put("hour", Dynamic::from_int(now.hour() as i64));
    put("minute", Dynamic::from_int(now.minute() as i64));
    put("second", Dynamic::from_int(now.second() as i64));
    put("day", Dynamic::from_int(now.day() as i64));
    put("date", now.format("%a %b %d").to_string().into());
    put("date_long", now.format("%A %d %B %Y").to_string().into());
    put("t", Dynamic::from_float(host.t));
    put("processes", procs);
    m
}

fn frac(part: u64, whole: u64) -> f64 {
    if whole == 0 {
        0.0
    } else {
        part as f64 / whole as f64
    }
}

impl Script {
    /// Compiles a widget script. A script that will not compile is
    /// reported once and the widget stays blank; a broken widget must
    /// not take the program down with it.
    pub fn load(path: &Path) -> Option<Script> {
        let src = std::fs::read_to_string(path).ok()?;
        let engine = engine();
        match engine.compile(&src) {
            Ok(ast) => Some(Script { engine, ast, failed: false }),
            Err(e) => {
                eprintln!("nacelle-desktop: {}: {e}", path.display());
                None
            }
        }
    }
}

fn str_of(m: &Map, key: &str) -> String {
    m.get(key)
        .map(|v| v.clone().into_string().unwrap_or_default())
        .unwrap_or_default()
}

fn f32_of(m: &Map, key: &str, def: f32) -> f32 {
    m.get(key)
        .and_then(|v| v.as_float().ok())
        .map(|f| f as f32)
        .filter(|f| f.is_finite())
        .unwrap_or(def)
}

fn align_of(s: &str) -> Align {
    match s {
        "right" => Align::Right,
        "center" => Align::Center,
        _ => Align::Left,
    }
}

fn bool_of(m: &Map, key: &str, def: bool) -> bool {
    m.get(key).and_then(|v| v.as_bool().ok()).unwrap_or(def)
}

fn int_of(m: &Map, key: &str, def: i64) -> i64 {
    m.get(key).and_then(|v| v.as_int().ok()).unwrap_or(def)
}

/// The severity an element or item carries, if it names one. The word is
/// from the closed set; an unknown word resolves through
/// `script.severity_fallback` — to `unknown`, NEVER to `ok` (§5.10).
fn sev_opt(m: &Map, key: &str) -> Option<ui::Sev> {
    let word = str_of(m, key);
    if word.is_empty() {
        return None;
    }
    Some(ui::sev_of(&word).unwrap_or_else(|| {
        ui::warn_once(
            &format!("sev:{word}"),
            &format!("unknown severity \"{word}\" — resolving to the fallback, never ok"),
        );
        ui::sev_fallback()
    }))
}

/// The role an element names for itself, if any. An unknown role name
/// warns once and falls back to `body` inside [`ui::role`].
fn role_opt(m: &Map, key: &str) -> Option<ui::Role> {
    let word = str_of(m, key);
    if word.is_empty() {
        None
    } else {
        Some(ui::role(&word))
    }
}

/// The theme's default text alignment (`script.text_align`), for a call
/// that names none.
fn theme_text_align() -> Align {
    static ALIGN: OnceLock<TokenId> = OnceLock::new();
    static CENTER: OnceLock<Option<u16>> = OnceLock::new();
    static RIGHT: OnceLock<Option<u16>> = OnceLock::new();
    let id = tok(&ALIGN, "script.text_align");
    let cur = theme::resolved().enum_of(id);
    if *CENTER.get_or_init(|| theme::enum_index(id, "center")) == Some(cur) {
        Align::Center
    } else if *RIGHT.get_or_init(|| theme::enum_index(id, "right")) == Some(cur) {
        Align::Right
    } else {
        Align::Left
    }
}

/// The role a `text` element draws in. Three forms, in vocabulary order
/// (u2 §3.1 #2): a named role; the deprecated free size, mapped to the
/// NEAREST role on the type ladder (warned once); no styling at all,
/// which defers to the `script.text_role` binding.
fn text_role_of(ctx: &Ctx, m: &Map) -> ui::Role {
    static TEXT_ROLE: OnceLock<TokenId> = OnceLock::new();
    if let Some(role) = role_opt(m, "role") {
        return role;
    }
    // The deprecated form is recognisable by carrying BOTH an alignment
    // and a size: the one-argument form stores only its default size.
    if m.contains_key("size") && m.contains_key("align") {
        ui::warn_once(
            "text.size",
            "text(content, align, size) is deprecated — the size maps to the \
             nearest type role; name a role instead: text(content, align, #{ role: \"…\" })",
        );
        return nearest_role(ctx, ctx.font_px(1.0) * f32_of(m, "size", 1.0));
    }
    ui::bound_role(&TEXT_ROLE, "script.text_role")
}

/// The content role whose resolved px sits closest to a deprecated free
/// size. The ladder is §5.16's content roles — chrome roles (titles,
/// buttons, badges) are not candidates: a `text` element is body copy or
/// a display value, never chrome.
fn nearest_role(ctx: &Ctx, target: f32) -> ui::Role {
    const LADDER: [&str; 8] = [
        "caption",
        "data",
        "display.date",
        "body",
        "value",
        "value.large",
        "display.hero",
        "display.clock",
    ];
    let mut best = ui::role("body");
    let mut best_d = f32::MAX;
    for name in LADDER {
        let r = ui::role(name);
        let d = (r.px(ctx, 1.0) - target).abs();
        if d < best_d {
            best_d = d;
            best = r;
        }
    }
    best
}

/// A widget drawn by its script.
pub struct ScriptWidget {
    script: Script,
    /// What the script last answered, and the moment it answered.
    /// A frame asks twice — once to measure, once to draw — and the
    /// script is not cheap: running it again would double the cost and
    /// let the two answers disagree, which is worse. Time comes from
    /// the host, so a new frame is a new answer.
    cached: Option<(f64, Array)>,
}

impl ScriptWidget {
    pub fn new(script: Script) -> Self {
        ScriptWidget { script, cached: None }
    }
}

impl ScriptWidget {
    /// Runs the script's `draw` and hands back the elements it asked
    /// for. None when the script has failed — said once, then the
    /// widget goes quiet, because sixty identical lines a second would
    /// bury everything else.
    fn elements(&mut self, host: &Host) -> Option<Array> {
        if self.script.failed {
            return None;
        }
        if let Some((t, elements)) = &self.cached {
            if *t == host.t {
                return Some(elements.clone());
            }
        }
        let mut scope = Scope::new();
        scope.push_constant("host", host_shared(host));
        let result: Result<Array, _> =
            self.script
                .engine
                .call_fn(&mut scope, &self.script.ast, "draw", ());
        match result {
            Ok(a) => {
                self.cached = Some((host.t, a.clone()));
                Some(a)
            }
            Err(e) => {
                eprintln!("nacelle-desktop: widget script failed: {e}");
                self.script.failed = true;
                None
            }
        }
    }
}

impl Widget for ScriptWidget {
    fn draw(&mut self, ctx: &mut Ctx, r: Rect, host: &Host) {
        let Some(elements) = self.elements(host) else { return };
        render(ctx, r, &elements);
    }

    /// The script's `title` element, read as a chrome declaration: the
    /// host's title band shows the same two strings, from the same host
    /// data (u2 §3.1 #1, §4). The element list is cached per frame, so
    /// asking here and drawing later runs the script once, and the two
    /// answers cannot disagree.
    fn chrome(&mut self, _ctx: &mut Ctx, host: &Host) -> crate::widget::Chrome {
        match self.elements(host) {
            Some(elements) => chrome_of(&elements),
            None => crate::widget::Chrome::none(),
        }
    }

    fn sizing(&mut self, ctx: &mut Ctx, host: &Host) -> Sizing {
        let Some(elements) = self.elements(host) else { return Sizing::Rows };
        let maps: Vec<Map> = elements
            .iter()
            .filter_map(|e| e.clone().try_cast::<Map>())
            .collect();
        let (fixed, flexible) = measure(ctx, &maps, &metrics());
        // One growing element and the widget has no height of its own:
        // a table takes as many rows as it is given, and giving it a
        // fixed height would be inventing a limit.
        if flexible > 0 {
            Sizing::Rows
        } else {
            Sizing::Content(fixed)
        }
    }
}

/// The first `title` element in a script's answer, as the host's chrome
/// declaration. `title("")` — the underline alone — declares nothing: a
/// rule is not a heading, and an empty band would take height from a
/// panel that asked for a line.
fn chrome_of(elements: &Array) -> crate::widget::Chrome {
    for e in elements.iter() {
        let Some(m) = e.read_lock::<Map>() else { continue };
        if str_of(&m, "kind") != "title" {
            continue;
        }
        let left = str_of(&m, "left");
        let right = str_of(&m, "right");
        if left.is_empty() && right.is_empty() {
            continue;
        }
        return crate::widget::Chrome {
            title: (!left.is_empty()).then_some(left),
            right: (!right.is_empty()).then_some(right),
            ..crate::widget::Chrome::none()
        };
    }
    crate::widget::Chrome::none()
}

/// The stack metrics every element height comes from, read from the
/// theme once per pass. Measure and draw walk the same numbers, so they
/// are gathered here rather than looked up twice and allowed to drift.
struct Metrics {
    row_h: f32,
    row_compact: f32,
    title_block: f32,
    columns_block: f32,
    spacer: f32,
    rule_block: f32,
    group_gap: f32,
    /// A multiplier on the type size, not a length — never scaled.
    text_leading: f32,
    min_flex_h: f32,
}

fn metrics() -> Metrics {
    static ROW_H: OnceLock<TokenId> = OnceLock::new();
    static ROW_COMPACT: OnceLock<TokenId> = OnceLock::new();
    static TITLE_BLOCK: OnceLock<TokenId> = OnceLock::new();
    static COLUMNS_BLOCK: OnceLock<TokenId> = OnceLock::new();
    static SPACER: OnceLock<TokenId> = OnceLock::new();
    static RULE_BLOCK: OnceLock<TokenId> = OnceLock::new();
    static GROUP_GAP: OnceLock<TokenId> = OnceLock::new();
    static TEXT_LEADING: OnceLock<TokenId> = OnceLock::new();
    static MIN_FLEX_H: OnceLock<TokenId> = OnceLock::new();
    static MIN_FLEX_H_MIN: OnceLock<TokenId> = OnceLock::new();
    let t = theme::resolved();
    Metrics {
        row_h: t.px(tok(&ROW_H, "script.row_h")),
        row_compact: t.px(tok(&ROW_COMPACT, "rhythm.row_compact")),
        title_block: t.px(tok(&TITLE_BLOCK, "script.title_block")),
        columns_block: t.px(tok(&COLUMNS_BLOCK, "script.columns_block")),
        spacer: t.px(tok(&SPACER, "script.spacer")),
        rule_block: t.px(tok(&RULE_BLOCK, "script.rule_block")),
        group_gap: t.px(tok(&GROUP_GAP, "script.group_gap")),
        text_leading: t.px(tok(&TEXT_LEADING, "script.text_leading")),
        min_flex_h: t
            .px(tok(&MIN_FLEX_H, "script.min_flex_h"))
            .max(t.px(tok(&MIN_FLEX_H_MIN, "script.min_flex_h_min_px"))),
    }
}

impl Metrics {
    /// The shrink-to-fit pass scales lengths, not ratios.
    fn scaled(&self, k: f32) -> Metrics {
        Metrics {
            row_h: self.row_h * k,
            row_compact: self.row_compact * k,
            title_block: self.title_block * k,
            columns_block: self.columns_block * k,
            spacer: self.spacer * k,
            rule_block: self.rule_block * k,
            group_gap: self.group_gap * k,
            text_leading: self.text_leading,
            min_flex_h: self.min_flex_h * k,
        }
    }

    /// One `rows` line at the element's declared density.
    fn rows_line_h(&self, m: &Map) -> f32 {
        if str_of(m, "density") == "compact" {
            self.row_compact
        } else {
            self.row_h
        }
    }
}

/// Lines a `rows` element occupies: its items flowed row-major into its
/// grid columns (u2 §2.3).
fn rows_lines(m: &Map) -> usize {
    let n = m
        .get("rows")
        .and_then(|v| v.read_lock::<Array>().map(|a| a.len()))
        .unwrap_or(0);
    let cols = int_of(m, "columns", 1).max(1) as usize;
    n.div_ceil(cols)
}

/// The tallest role on a `runs` line, at shrink 1 — the line's height is
/// that px under the stack's text leading.
fn runs_px(ctx: &Ctx, m: &Map) -> f32 {
    static TEXT_ROLE: OnceLock<TokenId> = OnceLock::new();
    m.get("items")
        .and_then(|v| v.read_lock::<Array>())
        .map(|items| {
            items
                .iter()
                .map(|it| {
                    let role = it
                        .read_lock::<Map>()
                        .and_then(|m| role_opt(&m, "role"))
                        .unwrap_or_else(|| ui::bound_role(&TEXT_ROLE, "script.text_role"));
                    role.px(ctx, 1.0)
                })
                .fold(0.0, f32::max)
        })
        .unwrap_or(0.0)
}

/// Height the fixed elements need, and how many elements grow into
/// whatever is left. Walked before drawing, and again a frame earlier
/// by [`ScriptWidget::sizing`] — a widget with nothing growing has
/// a height of its own, and the layout gives it exactly that.
/// Recursive: a `group`'s children are measured as one unit (§3.1 #13).
fn measure(ctx: &Ctx, maps: &[Map], met: &Metrics) -> (f32, usize) {
    let mut fixed = 0.0;
    let mut flexible = 0usize;
    for m in maps {
        match str_of(m, "kind").as_str() {
            // A `title` is a chrome declaration, consumed by the host's
            // band (u2 §3.1 #1, §4): it takes no body height — the band's
            // block is what `chrome_extra` adds around the content box.
            "title" => {}
            "rows" => fixed += met.rows_line_h(m) * rows_lines(m) as f32,
            "text" => {
                // The role decides the height; the deprecated free size
                // reaches the same role the draw will (text_role_of), so
                // measure and draw cannot disagree.
                fixed += text_role_of(ctx, m).px(ctx, 1.0) * met.text_leading;
            }
            "runs" => fixed += runs_px(ctx, m) * met.text_leading,
            "columns" => fixed += met.columns_block,
            "meter" => fixed += met.row_h,
            "badge" => fixed += met.row_h,
            "rule" => fixed += met.rule_block,
            "group" => {
                fixed += met.group_gap + met.row_h;
                let children: Vec<Map> = m
                    .get("elements")
                    .and_then(|v| v.read_lock::<Array>())
                    .map(|a| a.iter().filter_map(|e| e.clone().try_cast::<Map>()).collect())
                    .unwrap_or_default();
                let (f, fl) = measure(ctx, &children, met);
                fixed += f;
                flexible += fl;
            }
            "space" => fixed += met.spacer * f32_of(m, "size", 1.0),
            _ => flexible += 1,
        }
    }
    (fixed, flexible)
}

/// The stack-fit arithmetic, pure so a test can hold it still: the
/// height each flexible element receives, the shrink factor, and whether
/// the panel must still clip. The ladder, in order:
/// 1. flexible elements keep `min_flex_h` and the WHOLE stack shrinks
///    toward the `floor` — type included, so nothing is dropped;
/// 2. at the floor the `min_flex_h` guarantee is the next thing to
///    yield: the flexible elements give height back, down to nothing,
///    before a FIXED element — memory's SWAP meter, cpu's LOAD line,
///    exactly the last rows u1 §5.5 check 4 names — is pushed past the
///    bottom edge;
/// 3. only when the fixed rows ALONE overrun the panel at the floor
///    does the panel clip — and [`render`] says so on stderr, once,
///    because a silently dropped row reads as missing data.
fn stack_fit(
    h: f32,
    fixed: f32,
    flexible: usize,
    min_flex: f32,
    floor: f32,
    scales: bool,
) -> (f32, f32, bool) {
    let mut share = if flexible > 0 {
        ((h - fixed) / flexible as f32).max(min_flex)
    } else {
        0.0
    };
    let natural = fixed + share * flexible as f32;
    let raw = if natural > h && natural > 0.0 { h / natural } else { 1.0 };
    let scale = if scales { raw.max(floor) } else { 1.0 };
    if natural * scale > h + 0.5 && flexible > 0 {
        share = ((h / scale - fixed) / flexible as f32).max(0.0);
    }
    let clipped = (fixed + share * flexible as f32) * scale > h + 0.5;
    (share, scale, clipped)
}

/// Draws the element list a script returned.
fn render(ctx: &mut Ctx, r: Rect, elements: &Array) {
    let t = theme::resolved();
    let px = ctx.font_px(1.0);
    let met = metrics();

    // Fixed-height elements take what they need; the rest share what is
    // left, and the whole stack is fitted to the panel so a widget can
    // never spill onto its neighbours.
    // Cast out of the Dynamics once: read_lock hands back a guard, and
    // the whole list is walked twice (measure, then draw).
    let maps: Vec<Map> = elements
        .iter()
        .filter_map(|e| e.clone().try_cast::<Map>())
        .collect();
    let (fixed, flexible) = measure(ctx, &maps, &met);
    // The overflow policy: `scale` shrinks the stack to fit but no
    // further than the floor — type shrunk past it stops being legible,
    // so from there the flexible elements yield and, at the very end,
    // the panel clips (see `stack_fit`). Any other policy keeps full
    // size (`scroll` needs scroll state that does not exist yet and
    // degrades the same way).
    static OVERFLOW: OnceLock<TokenId> = OnceLock::new();
    static OVERFLOW_SCALE: OnceLock<Option<u16>> = OnceLock::new();
    static MIN_SCALE: OnceLock<TokenId> = OnceLock::new();
    let ov = tok(&OVERFLOW, "script.overflow");
    let scales = OVERFLOW_SCALE
        .get_or_init(|| theme::enum_index(ov, "scale"))
        .is_none_or(|i| t.enum_of(ov) == i);
    // u2 §6.4: the master pins `script.overflow_min_scale` at 0.62 for
    // now — the clamp `panel_font_scale` already applies and the one
    // `uptime` and `hardware` sit on. The specification's 0.72 floor
    // would CLIP those two panels under the default theme; raise the
    // master's value to 0.72 only after the compact arrangements of
    // u2 §2.3/§2.4 land and take them off the clamp.
    let floor = t.px(tok(&MIN_SCALE, "script.overflow_min_scale"));
    let (share, scale, clipped) =
        stack_fit(r.h, fixed, flexible, met.min_flex_h, floor, scales);
    if clipped {
        // Clipping here DROPS content — the tail is one of u1 §5.5's
        // last rows — so it is never silent: one line per widget, the
        // way the panel ladder's report_step announces its steps.
        let who = chrome_of(elements)
            .title
            .unwrap_or_else(|| "(untitled)".into());
        ui::warn_once(
            &format!("script.clip.{who}"),
            &format!(
                "script widget {who}: fixed rows overrun the panel even at \
                 the overflow floor — clipping the tail"
            ),
        );
        ctx.dl.push_clip(r.x, r.y, r.w, r.h);
    }
    let pass = Pass {
        px: px * scale,
        share: share * scale,
        scale,
        met: met.scaled(scale),
    };
    let y = ui::block_top(&r, (fixed + share * flexible as f32) * scale);
    draw_stack(ctx, &r, y, &maps, &pass);
    if clipped {
        ctx.dl.pop_clip();
    }
}

/// What one drawing pass carries down the stack — the measured share for
/// flexible elements and the shrink factor everything scales by. One
/// struct, because `group` recurses (u2 §3.1 #13) and its children draw
/// under exactly the numbers their parent measured with.
struct Pass {
    /// The legacy base type size, already shrunk.
    px: f32,
    /// The height each flexible element receives, already shrunk.
    share: f32,
    /// The shrink factor itself, for role sizes and paddings.
    scale: f32,
    /// The stack metrics, already shrunk.
    met: Metrics,
}

/// Draws one element list downwards from `y`; returns the y below the
/// last element. `group` re-enters with its children.
fn draw_stack(ctx: &mut Ctx, r: &Rect, mut y: f32, maps: &[Map], p: &Pass) -> f32 {
    let t = theme::resolved();
    let (px, share, scale) = (p.px, p.share, p.scale);
    let met = &p.met;
    for m in maps {
        match str_of(m, "kind").as_str() {
            "title" => {
                // Re-homed (u2 §3.1 #1): the element is the chrome
                // declaration the host's title band draws — same strings,
                // same data — and draws NOTHING in the body. The widgets
                // stopped drawing their own titles when the band arrived;
                // drawing here again would show every heading twice.
            }
            "rows" => {
                static LABEL_ROLE: OnceLock<TokenId> = OnceLock::new();
                static VALUE_ROLE: OnceLock<TokenId> = OnceLock::new();
                let items: Vec<ui::RowItem> = m
                    .get("rows")
                    .and_then(|v| v.read_lock::<Array>())
                    .map(|a| {
                        a.iter()
                            .map(|row| {
                                let entry = row.read_lock::<Array>();
                                let get = |i: usize| {
                                    entry
                                        .as_ref()
                                        .and_then(|p| p.get(i))
                                        .map(|v| v.to_string())
                                        .unwrap_or_default()
                                };
                                let sev = entry
                                    .as_ref()
                                    .and_then(|p| p.get(2))
                                    .map(|v| v.to_string())
                                    .filter(|w| !w.is_empty())
                                    .map(|w| {
                                        ui::sev_of(&w).unwrap_or_else(ui::sev_fallback)
                                    });
                                ui::RowItem { label: get(0), value: get(1), sev }
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let row_h = met.rows_line_h(m);
                let h = row_h * rows_lines(m) as f32;
                let st = ui::RowsStyle {
                    label_role: role_opt(m, "label_role")
                        .unwrap_or_else(|| ui::bound_role(&LABEL_ROLE, "script.rows_label_role")),
                    value_role: role_opt(m, "value_role")
                        .unwrap_or_else(|| ui::bound_role(&VALUE_ROLE, "script.rows_value_role")),
                    columns: int_of(m, "columns", 1).max(1) as usize,
                    label_width: if str_of(m, "label_width") == "max" {
                        ui::LabelWidth::Max
                    } else {
                        ui::LabelWidth::Auto
                    },
                    row_h,
                    shrink: scale,
                };
                ui::rows_label_value(ctx, Rect::new(r.x, y, r.w, h), &items, &st);
                y += h;
            }
            "text" => {
                let content = str_of(m, "content");
                let role = text_role_of(ctx, m);
                let fpx = role.px(ctx, scale);
                let spacing = role.tracking_px(fpx);
                let color = match sev_opt(m, "severity") {
                    Some(s) => ui::sev_text(s),
                    // A named role writes in its own ink; the older forms
                    // keep the component colour they always had.
                    None if m.contains_key("role") => role.color(),
                    None => {
                        static VALUE: OnceLock<TokenId> = OnceLock::new();
                        col(&VALUE, "component.script.value")
                    }
                };
                let align = match str_of(m, "align").as_str() {
                    "right" => Align::Right,
                    "center" => Align::Center,
                    "left" => Align::Left,
                    // A script that names no alignment gets the theme's.
                    _ => theme_text_align(),
                };
                match align {
                    Align::Left => {
                        ctx.dl.text(ctx.fonts, FONT_UI, fpx, r.x, y, &content, color, spacing);
                    }
                    Align::Right => {
                        ctx.dl.text_right(
                            ctx.fonts, FONT_UI, fpx, r.right(), y, &content, color, spacing,
                        );
                    }
                    Align::Center => {
                        ctx.dl.text_center(
                            ctx.fonts, FONT_UI, fpx, r.cx(), y, &content, color, spacing,
                        );
                    }
                }
                y += role.px(ctx, 1.0) * met.text_leading * scale;
            }
            "runs" => {
                static TEXT_ROLE: OnceLock<TokenId> = OnceLock::new();
                let items: Vec<ui::Run> = m
                    .get("items")
                    .and_then(|v| v.read_lock::<Array>())
                    .map(|a| {
                        a.iter()
                            .filter_map(|it| {
                                let im = it.read_lock::<Map>()?;
                                Some(ui::Run {
                                    text: str_of(&im, "t"),
                                    role: role_opt(&im, "role").unwrap_or_else(|| {
                                        ui::bound_role(&TEXT_ROLE, "script.text_role")
                                    }),
                                    sev: sev_opt(&im, "severity"),
                                    blink: Some(str_of(&im, "blink"))
                                        .filter(|b| !b.is_empty()),
                                    end: str_of(&im, "align") == "right",
                                })
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let h = runs_px(ctx, m) * met.text_leading * scale;
                let align = match str_of(m, "align").as_str() {
                    "right" => Align::Right,
                    "center" => Align::Center,
                    "left" => Align::Left,
                    _ => theme_text_align(),
                };
                ui::runs(ctx, Rect::new(r.x, y, r.w, h), &items, align, scale);
                y += h;
            }
            "rule" => {
                ui::rule(ctx, Rect::new(r.x, y, r.w, met.rule_block));
                y += met.rule_block;
            }
            "badge" => {
                // u2 §2.8's STATE line: an optional `label` opt puts a
                // rows-style label on the badge's line and the pill at the
                // right edge, so a key:value line may carry a pill as its
                // value — `STATE   [ ONLINE ]`, both strings exactly what
                // the rows line showed. Without a label the pill stands
                // alone, aligned by the theme, as before.
                let label = str_of(m, "label");
                let align = if label.is_empty() {
                    theme_text_align()
                } else {
                    static LABEL_ROLE: OnceLock<TokenId> = OnceLock::new();
                    static LABEL_C: OnceLock<TokenId> = OnceLock::new();
                    let role = ui::bound_role(&LABEL_ROLE, "script.rows_label_role");
                    let lpx = role.px(ctx, scale);
                    let lsp = role.tracking_px(lpx);
                    // The 1.3 cap-height guess is F021, as in `meter`.
                    let ty = y + (met.row_h - lpx * 1.3) / 2.0;
                    ctx.dl.text(
                        ctx.fonts, FONT_UI, lpx, r.x, ty, &label,
                        col(&LABEL_C, "component.script.label"), lsp,
                    );
                    Align::Right
                };
                ui::badge(
                    ctx,
                    Rect::new(r.x, y, r.w, met.row_h),
                    &str_of(m, "text"),
                    sev_opt(m, "severity"),
                    match str_of(m, "style").as_str() {
                        "solid" => ui::BadgeStyle::Solid,
                        "hollow" | "outlined" => ui::BadgeStyle::Hollow,
                        _ => ui::BadgeStyle::FromTheme,
                    },
                    align,
                    scale,
                );
                y += met.row_h;
            }
            "group" => {
                y += met.group_gap;
                ui::group_header(
                    ctx,
                    Rect::new(r.x, y, r.w, met.row_h),
                    &str_of(m, "label"),
                    scale,
                );
                y += met.row_h;
                let children: Vec<Map> = m
                    .get("elements")
                    .and_then(|v| v.read_lock::<Array>())
                    .map(|a| a.iter().filter_map(|e| e.clone().try_cast::<Map>()).collect())
                    .unwrap_or_default();
                y = draw_stack(ctx, r, y, &children, p);
            }
            "meter" => {
                static LABEL: OnceLock<TokenId> = OnceLock::new();
                static VALUE: OnceLock<TokenId> = OnceLock::new();
                static LABEL_GAP: OnceLock<TokenId> = OnceLock::new();
                static VALUE_GAP: OnceLock<TokenId> = OnceLock::new();
                static TRACK_OFF: OnceLock<TokenId> = OnceLock::new();
                static BAR_H: OnceLock<TokenId> = OnceLock::new();
                static LABEL_TRACKING: OnceLock<TokenId> = OnceLock::new();
                static VALUE_TRACKING: OnceLock<TokenId> = OnceLock::new();
                let label = str_of(m, "label");
                let value = str_of(m, "value");
                let f = f32_of(m, "fraction", 0.0);
                let lsp = px * t.px(tok(&LABEL_TRACKING, "type.caption.tracking"));
                let vsp = px * t.px(tok(&VALUE_TRACKING, "type.value.tracking"));
                let lw = ctx.fonts.measure(FONT_UI, px, &label, lsp)
                    + t.px(tok(&LABEL_GAP, "meter.label_gap")) * scale;
                let vw = ctx.fonts.measure(FONT_UI, px, &value, vsp)
                    + t.px(tok(&VALUE_GAP, "meter.value_gap")) * scale;
                // The 1.3 cap-height guess is F021: it waits for the shared
                // optical-centring primitive, not a per-site token.
                let ty = y + (met.row_h - px * 1.3) / 2.0;
                ctx.dl.text(
                    ctx.fonts, FONT_UI, px, r.x, ty, &label,
                    col(&LABEL, "component.script.label"), lsp,
                );
                let bar = Rect::new(
                    r.x + lw,
                    y + t.px(tok(&TRACK_OFF, "script.meter_track_h")) * scale,
                    (r.w - lw - vw).max(1.0),
                    t.px(tok(&BAR_H, "script.meter_bar_h")) * scale,
                );
                // ui::meter reads its own track and fill; the element
                // only says where the bar sits, how full it is, and — the
                // script's judgement — how it stands (u2 §3.1 #6).
                ui::meter(ctx, bar, f, sev_opt(m, "severity"), bool_of(m, "track", true));
                ctx.dl.text_right(
                    ctx.fonts, FONT_UI, px, r.right(), ty, &value,
                    col(&VALUE, "component.script.value"), vsp,
                );
                y += met.row_h;
            }
            "gauges" => {
                let values: Vec<f32> = m
                    .get("values")
                    .and_then(|v| v.read_lock::<Array>())
                    .map(|a| {
                        a.iter()
                            .map(|v| v.as_float().unwrap_or(0.0) as f32)
                            .collect()
                    })
                    .unwrap_or_default();
                static COLS: OnceLock<TokenId> = OnceLock::new();
                static STYLE: OnceLock<TokenId> = OnceLock::new();
                let cols = m
                    .get("columns")
                    .and_then(|v| v.as_int().ok())
                    .unwrap_or_else(|| t.px(tok(&COLS, "gauge.cols")) as i64)
                    .clamp(1, 16) as usize;
                // The form: the script's arrangement choice, defaulting to
                // the theme's `gauge.style`. `bar` and `donut` cannot yet
                // carry the per-core number they owe, so they degrade to
                // `row` with one warning — a stated fallback, never a
                // silent content drop (u2 §2.5).
                let style_word = {
                    let w = str_of(m, "style");
                    if w.is_empty() {
                        ui::theme_word(tok(&STYLE, "gauge.style"))
                    } else {
                        w
                    }
                };
                let kind = match style_word.as_str() {
                    "row" => ui::GaugeKind::Row,
                    "cell" | "" => ui::GaugeKind::Cell,
                    "bar" | "donut" => {
                        ui::warn_once(
                            "gauges.style",
                            &format!(
                                "gauge style \"{style_word}\" cannot carry its value \
                                 labels yet — drawing rows instead"
                            ),
                        );
                        ui::GaugeKind::Row
                    }
                    other => {
                        ui::warn_once(
                            "gauges.style",
                            &format!("unknown gauge style \"{other}\" — drawing cells"),
                        );
                        ui::GaugeKind::Cell
                    }
                };
                let labels = match m.get("label") {
                    Some(v) if v.is_array() => ui::GaugeLabels::Text(
                        v.read_lock::<Array>()
                            .map(|a| a.iter().map(|x| x.to_string()).collect())
                            .unwrap_or_default(),
                    ),
                    Some(v) => {
                        let w = v.to_string();
                        if w.is_empty() {
                            ui::GaugeLabels::None
                        } else {
                            ui::GaugeLabels::Index(w)
                        }
                    }
                    None => ui::GaugeLabels::None,
                };
                let st = ui::GaugeStyle {
                    cols,
                    kind,
                    labels,
                    value_fmt: if str_of(m, "value_fmt") == "raw" {
                        ui::GaugeValueFmt::Raw
                    } else {
                        ui::GaugeValueFmt::Percent
                    },
                    shrink: scale,
                };
                // The gauges are data, not chrome — that is why [data]
                // exists; gauge_grid reads its own colours and metrics.
                ui::gauge_grid(ctx, Rect::new(r.x, y, r.w, share), &values, &st);
                y += share;
            }
            "dots" => {
                // ui::dot_matrix reads its own pitch and cell colours;
                // only the stack's shrink factor travels with the call,
                // so the pitch shrinks in step with everything else.
                ui::dot_matrix(
                    ctx,
                    Rect::new(r.x, y, r.w, share),
                    f32_of(m, "fraction", 0.0),
                    scale,
                );
                y += share;
            }
            "table" => {
                let cols: Vec<ui::Column> = m
                    .get("headings")
                    .and_then(|v| v.read_lock::<Array>())
                    .map(|a| {
                        a.iter()
                            .map(|h| {
                                let entry = h.read_lock::<Array>();
                                let name = entry
                                    .as_ref()
                                    .and_then(|p| p.first())
                                    .map(|v| v.to_string())
                                    .unwrap_or_default();
                                let al = entry
                                    .as_ref()
                                    .and_then(|p| p.get(1))
                                    .map(|v| v.to_string())
                                    .unwrap_or_default();
                                let opts = entry
                                    .as_ref()
                                    .and_then(|p| p.get(2))
                                    .and_then(|v| v.clone().try_cast::<Map>());
                                let kind = match opts.as_ref().map(|o| str_of(o, "kind")) {
                                    Some(k) if k == "bar" => ui::CellKind::Bar {
                                        of: opts
                                            .as_ref()
                                            .map(|o| f32_of(o, "of", 100.0))
                                            .unwrap_or(100.0),
                                    },
                                    Some(k) if k == "badge" => ui::CellKind::Badge,
                                    _ => ui::CellKind::Text,
                                };
                                // Content-measured widths are the default
                                // (u2 §2.7); `heading` keeps the old rule.
                                let width = match opts.as_ref().map(|o| str_of(o, "width")) {
                                    Some(w) if w == "heading" => ui::ColWidth::Heading,
                                    _ => ui::ColWidth::Content,
                                };
                                ui::Column { title: name, align: align_of(&al), kind, width }
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let rows: Vec<Vec<String>> = m
                    .get("rows")
                    .and_then(|v| v.read_lock::<Array>())
                    .map(|a| {
                        a.iter()
                            .map(|row| {
                                row.read_lock::<Array>()
                                    .map(|c| c.iter().map(|v| v.to_string()).collect())
                                    .unwrap_or_default()
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let st = ui::TableStyle {
                    elastic: int_of(m, "elastic", 0).max(0) as usize,
                    zebra: bool_of(m, "zebra", false),
                    severity_col: m
                        .get("severity_col")
                        .and_then(|v| v.as_int().ok())
                        .filter(|i| *i >= 0)
                        .map(|i| i as usize),
                    shrink: scale,
                };
                ui::table(ctx, Rect::new(r.x, y, r.w, share), &cols, &rows, &st);
                y += share;
            }
            "columns" => {
                static LABEL_ROLE: OnceLock<TokenId> = OnceLock::new();
                static VALUE_ROLE: OnceLock<TokenId> = OnceLock::new();
                let cells: Vec<ui::ColumnCell> = m
                    .get("cells")
                    .and_then(|v| v.read_lock::<Array>())
                    .map(|a| {
                        a.iter()
                            .map(|c| {
                                let entry = c.read_lock::<Array>();
                                let get = |i: usize| {
                                    entry
                                        .as_ref()
                                        .and_then(|p| p.get(i))
                                        .map(|v| v.to_string())
                                        .unwrap_or_default()
                                };
                                let sev = entry
                                    .as_ref()
                                    .and_then(|p| p.get(2))
                                    .map(|v| v.to_string())
                                    .filter(|w| !w.is_empty())
                                    .map(|w| {
                                        ui::sev_of(&w).unwrap_or_else(ui::sev_fallback)
                                    });
                                ui::ColumnCell { label: get(0), value: get(1), sev }
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let st = ui::ColumnsStyle {
                    label_role: role_opt(m, "label_role").unwrap_or_else(|| {
                        ui::bound_role(&LABEL_ROLE, "script.columns_label_role")
                    }),
                    value_role: role_opt(m, "value_role").unwrap_or_else(|| {
                        ui::bound_role(&VALUE_ROLE, "script.columns_value_role")
                    }),
                    align: match str_of(m, "align").as_str() {
                        "" => None,
                        w => Some(align_of(w)),
                    },
                    dividers: bool_of(m, "dividers", false),
                    shrink: scale,
                };
                ui::columns(ctx, Rect::new(r.x, y, r.w, met.columns_block), &cells, &st);
                y += met.columns_block;
            }
            "space" => y += met.spacer * f32_of(m, "size", 1.0),
            _ => {}
        }
    }
    y
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot() -> crate::telemetry::Snapshot {
        crate::telemetry::Snapshot {
            hostname: "desktop".into(),
            uptime: 3661,
            mem_used: 2 * 1024 * 1024 * 1024,
            mem_total: 8 * 1024 * 1024 * 1024,
            cpu_per_core: vec![10.0, 20.0],
            ..Default::default()
        }
    }

    fn run(src: &str) -> Result<Array, String> {
        let engine = engine();
        let ast = engine.compile(src).map_err(|e| e.to_string())?;
        let snap = snapshot();
        let host = Host {
            snap: &snap,
            term: None,
            tabs: &[],
            tab_active: 0,
            shell_cwd: None,
            t: 0.0,
            window: (1280.0, 720.0),
        };
        let mut scope = Scope::new();
        scope.push_constant("host", host_shared(&host));
        engine
            .call_fn::<Array>(&mut scope, &ast, "draw", ())
            .map_err(|e| e.to_string())
    }

    #[test]
    fn a_script_builds_elements_from_host_data() {
        let out = run(r#"
            fn draw() {
                [
                    title("UPTIME", upper(host.name)),
                    rows([["UP", uptime(host.uptime)]]),
                    meter("MEM", host.mem_fraction, bytes(host.mem_used)),
                    gauges(host.cpu_each, 2),
                ]
            }
        "#)
        .unwrap();
        assert_eq!(out.len(), 4);
        let m = out[0].read_lock::<Map>().unwrap();
        assert_eq!(str_of(&m, "left"), "UPTIME");
        assert_eq!(str_of(&m, "right"), "DESKTOP");
        let rows = out[1].read_lock::<Map>().unwrap();
        let r = rows.get("rows").unwrap().read_lock::<Array>().unwrap();
        let first = r[0].read_lock::<Array>().unwrap();
        assert_eq!(first[1].to_string(), "01:01:01");
        let meter = out[2].read_lock::<Map>().unwrap();
        assert_eq!(str_of(&meter, "value"), "2.00 GiB");
        assert!((f32_of(&meter, "fraction", 0.0) - 0.25).abs() < 0.001);
    }

    #[test]
    fn scripts_cannot_reach_the_system_and_cannot_hang() {
        // No file, network or process functions exist in a script's world.
        for forbidden in [
            r#"fn draw() { open_file("/etc/passwd") }"#,
            r#"fn draw() { import "std::fs" as fs; [] }"#,
        ] {
            assert!(run(forbidden).is_err(), "{forbidden} should not run");
        }
        // A runaway loop is cut off rather than freezing the frame.
        let out = run(r#"fn draw() { let i = 0; while true { i += 1; } [] }"#);
        assert!(out.is_err(), "an endless loop must be stopped");
    }

    /// The six title items that MOVE to the host's band (u2 §6.1) come
    /// out of the same element the script has always answered with —
    /// same strings, same data. A script with no title, or with only
    /// the underline, declares no band.
    #[test]
    fn a_title_element_is_the_chrome_declaration() {
        let out = run(r#"
            fn draw() {
                [ title("UPTIME", "CHARGING"), rows([["UP", "01:01:01"]]) ]
            }
        "#)
        .unwrap();
        let c = chrome_of(&out);
        assert_eq!(c.title.as_deref(), Some("UPTIME"));
        assert_eq!(c.right.as_deref(), Some("CHARGING"));

        let no_right = run(r#"fn draw() { [ title("HARDWARE") ] }"#).unwrap();
        let c = chrome_of(&no_right);
        assert_eq!(c.title.as_deref(), Some("HARDWARE"));
        assert_eq!(c.right, None);

        let untitled = run(r#"fn draw() { [ text("21:57:30", "center", 2.4) ] }"#).unwrap();
        assert_eq!(chrome_of(&untitled).title, None);

        // The underline alone is a rule, not a band.
        let rule_only = run(r#"fn draw() { [ title("") ] }"#).unwrap();
        assert_eq!(chrome_of(&rule_only).title, None);
    }

    #[test]
    fn a_broken_script_is_an_error_not_a_crash() {
        assert!(run("fn draw() { this is not rhai }").is_err());
        // A script without draw() is an error, not a panic.
        assert!(run("fn other() { [] }").is_err());
    }

    /// The four NEW elements of u2 §3.1 — runs, rule, group, badge —
    /// build the maps the renderer walks, from the same host data the
    /// old vocabulary reads.
    #[test]
    fn the_four_new_elements_parse() {
        let out = run(r#"
            fn draw() {
                [
                    runs([
                        #{ t: "LOAD", role: "caption" },
                        #{ t: ":", role: "display.clock", blink: "value_blink" },
                        #{ t: "42", role: "value", severity: "warning" },
                        #{ t: "47°C", role: "data", align: "right" },
                    ], "center"),
                    rule(),
                    group("SWAP", [
                        rows([["USED", "128 MiB"]]),
                    ]),
                    badge("ONLINE", #{ severity: "ok" }),
                    badge("OFFLINE"),
                ]
            }
        "#)
        .unwrap();
        assert_eq!(out.len(), 5);
        let runs = out[0].read_lock::<Map>().unwrap();
        assert_eq!(str_of(&runs, "kind"), "runs");
        assert_eq!(str_of(&runs, "align"), "center");
        let items = runs.get("items").unwrap().read_lock::<Array>().unwrap();
        assert_eq!(items.len(), 4);
        let colon = items[1].read_lock::<Map>().unwrap();
        assert_eq!(str_of(&colon, "blink"), "value_blink");
        // u2 §2.5's right-aligned temperature run: the item pins itself
        // to the line's right end.
        let temp = items[3].read_lock::<Map>().unwrap();
        assert_eq!(str_of(&temp, "align"), "right");
        assert_eq!(str_of(&out[1].read_lock::<Map>().unwrap(), "kind"), "rule");
        let group = out[2].read_lock::<Map>().unwrap();
        assert_eq!(str_of(&group, "kind"), "group");
        assert_eq!(str_of(&group, "label"), "SWAP");
        let children = group.get("elements").unwrap().read_lock::<Array>().unwrap();
        assert_eq!(children.len(), 1);
        let badge = out[3].read_lock::<Map>().unwrap();
        assert_eq!(str_of(&badge, "kind"), "badge");
        assert_eq!(str_of(&badge, "text"), "ONLINE");
        assert_eq!(str_of(&badge, "severity"), "ok");
        // The one-argument badge carries no severity at all.
        assert_eq!(str_of(&out[4].read_lock::<Map>().unwrap(), "severity"), "");
    }

    /// u2 §2.8's STATE line: a badge may carry the row's label as an
    /// option, so a key:value line can have a pill for its value. The
    /// two strings are exactly the two the old rows line showed — the
    /// pill is presentation, never new content.
    #[test]
    fn a_badge_may_carry_its_rows_label() {
        let out = run(
            r#"fn draw() { [ badge("ONLINE", #{ label: "STATE", severity: "ok" }) ] }"#,
        )
        .unwrap();
        let b = out[0].read_lock::<Map>().unwrap();
        assert_eq!(str_of(&b, "label"), "STATE");
        assert_eq!(str_of(&b, "text"), "ONLINE");
        assert_eq!(str_of(&b, "severity"), "ok");
    }

    /// The EXTENDED forms of u2 §3.1 are added overloads: the options
    /// ride on the element map without displacing what the old form
    /// stored, so both generations of script read back the same way.
    #[test]
    fn the_extended_forms_parse_beside_the_old_ones() {
        let out = run(r#"
            fn draw() {
                [
                    rows([["UP", "01:01:01", "ok"], ["HOST", "ORION"]],
                         #{ columns: 2, label_width: "max", density: "compact" }),
                    columns([["POWER", "87% +", "warning"]], #{ dividers: true }),
                    meter("SWAP", 0.5, "128 MiB", #{ severity: "critical", track: false }),
                    gauges(host.cpu_each, #{ columns: 2, style: "row", label: "C" }),
                    table([["PID", "right"], ["CPU", "right", #{ kind: "bar", of: 100.0 }]],
                          [["1", "41.2%", "warning"]], 0,
                          #{ zebra: true, severity_col: 2 }),
                    text("21:57:30", "center", #{ role: "display.clock" }),
                ]
            }
        "#)
        .unwrap();
        assert_eq!(out.len(), 6);
        let rows = out[0].read_lock::<Map>().unwrap();
        assert_eq!(str_of(&rows, "label_width"), "max");
        assert_eq!(str_of(&rows, "density"), "compact");
        let first = rows.get("rows").unwrap().read_lock::<Array>().unwrap()[0]
            .read_lock::<Array>()
            .unwrap()
            .get(2)
            .unwrap()
            .to_string();
        assert_eq!(first, "ok");
        let cols = out[1].read_lock::<Map>().unwrap();
        assert!(cols.get("dividers").unwrap().as_bool().unwrap());
        let meter = out[2].read_lock::<Map>().unwrap();
        assert_eq!(str_of(&meter, "severity"), "critical");
        assert!(!meter.get("track").unwrap().as_bool().unwrap());
        let gauges = out[3].read_lock::<Map>().unwrap();
        assert_eq!(str_of(&gauges, "style"), "row");
        assert_eq!(str_of(&gauges, "label"), "C");
        let table = out[4].read_lock::<Map>().unwrap();
        assert!(table.get("zebra").unwrap().as_bool().unwrap());
        assert_eq!(table.get("severity_col").unwrap().as_int().unwrap(), 2);
        let text = out[5].read_lock::<Map>().unwrap();
        assert_eq!(str_of(&text, "role"), "display.clock");
        // The named role replaces the free size entirely.
        assert!(!text.contains_key("size"));
    }

    /// The severity words a script may use are the closed set of §5.10,
    /// and an unknown word resolves to the fallback — never to ok.
    #[test]
    fn severity_is_a_closed_set_with_a_safe_fallback() {
        for (i, name) in ui::SEVERITY_ROLES.iter().enumerate() {
            assert_eq!(ui::sev_of(name), Some(ui::Sev(i as u16)));
        }
        assert_eq!(ui::sev_of("fine"), None);
        assert_ne!(ui::sev_fallback(), ui::Sev(0), "the fallback must never be ok");
    }

    /// memory at 1280×800: two fixed rows and one flexible matrix in a
    /// panel shorter than fixed + min_flex even at the 0.62 floor. The
    /// flexible share must yield below its minimum so the SWAP meter —
    /// the last fixed element, the exact row u1 §5.5 check 4 protects —
    /// stays inside the panel instead of past the clip.
    #[test]
    fn stack_fit_yields_the_flexible_share_before_the_fixed_tail() {
        let (share, scale, clipped) = stack_fit(40.0, 45.4, 1, 28.1, 0.62, true);
        assert!(!clipped, "the fixed tail fits once the flexible yields");
        assert!(share >= 0.0 && share < 28.1, "the min_flex_h guarantee gave way");
        assert!((45.4 + share) * scale <= 40.5, "the whole stack sits inside the panel");
    }

    /// Only when the fixed rows ALONE overrun the panel at the floor may
    /// the panel clip — and then the flexible elements have nothing left.
    #[test]
    fn stack_fit_clips_only_when_the_fixed_rows_cannot_fit() {
        let (share, scale, clipped) = stack_fit(20.0, 45.4, 1, 28.1, 0.62, true);
        assert_eq!(share, 0.0);
        assert_eq!(scale, 0.62);
        assert!(clipped);
    }

    /// A panel with room keeps today's arithmetic: the flexible share is
    /// the leftover, nothing shrinks, nothing clips.
    #[test]
    fn stack_fit_leaves_a_roomy_panel_alone() {
        let (share, scale, clipped) = stack_fit(100.0, 45.4, 1, 28.1, 0.62, true);
        assert_eq!(share, 100.0 - 45.4);
        assert_eq!(scale, 1.0);
        assert!(!clipped);
    }
}
