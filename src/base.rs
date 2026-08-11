//! Core widget framework: geometry, the panel/layout model and the
//! drawing context shared by every nacelle widget.

use crate::draw::DrawList;
use crate::focus::FocusCtl;
use crate::font::{FontSystem, FONT_UI};
use std::sync::{OnceLock, RwLock};

#[derive(Clone, Copy, Debug)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Rect { x, y, w, h }
    }
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h
    }
    pub fn right(&self) -> f32 {
        self.x + self.w
    }
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }
    pub fn cx(&self) -> f32 {
        self.x + self.w / 2.0
    }
}

/// How large the interface type is before any setting touches it.
///
/// Every size a widget asks for is a multiple of this, so it moves the
/// whole interface at once rather than one label at a time — and it is
/// separate from UIFontSize= so that setting still means what it says:
/// 100% is the size the interface was designed at, not a correction.
pub const UI_FONT_BASE: f32 = 1.3;

/// Panel position and size in vw/vh units (percent of the window).
#[derive(Clone, Copy)]
pub struct PanelSpec {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// An individually placeable widget (panel) of the interface.
///
/// This is only an index into the widget registry, which is built at
/// startup by scanning the widgets directory. Everything a widget is —
/// its name, label, default sizes and how it draws — comes from that
/// registry, so adding a widget never means touching this type.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Panel(pub u16);

impl Panel {
    pub fn idx(self) -> usize {
        self.0 as usize
    }

    /// Every registered widget, in registry order.
    pub fn all() -> Vec<Panel> {
        (0..panel_count() as u16).map(Panel).collect()
    }

    fn def(self) -> Option<&'static WidgetDef> {
        registry().get(self.idx())
    }

    /// Name used in .layaut files.
    pub fn name(self) -> &'static str {
        self.def().map(|d| d.name.as_str()).unwrap_or("?")
    }

    /// Label shown in the layout editor.
    pub fn label(self) -> &'static str {
        self.def().map(|d| d.label.as_str()).unwrap_or("?")
    }

    /// Which kind of board this widget may be placed on.
    pub fn category(self) -> WidgetCategory {
        self.def().map(|d| d.category).unwrap_or_default()
    }

    pub fn from_name(name: &str) -> Option<Panel> {
        registry()
            .iter()
            .position(|d| d.name.eq_ignore_ascii_case(name))
            .map(|i| Panel(i as u16))
    }

    /// Reference height (vh) at which the widget renders at 100% scale.
    /// Enlarging a panel past its reference box scales the whole widget,
    /// fonts included. This is a LAYOUT property, not a widget one — a
    /// layout may give the same widget a different reference — so it
    /// comes from the size table the current layout installed.
    pub fn ref_h_vh(self) -> f32 {
        sizes()
            .read()
            .ok()
            .and_then(|s| s.get(self.idx()).map(|(r, _)| *r))
            .unwrap_or(10.0)
    }

    /// The height this widget's content actually needs at the width it
    /// has been given, or None when the widget grows to whatever height
    /// it gets — a table that shows more rows, a terminal that shows
    /// more lines. Measured once a frame, before the layout runs.
    pub fn intrinsic_h(self) -> Option<f32> {
        intrinsic()
            .read()
            .ok()
            .and_then(|v| v.get(self.idx()).copied())
            .flatten()
    }

    /// Minimum content height (vh) the layout engine keeps for it.
    pub fn min_h_vh(self) -> f32 {
        sizes()
            .read()
            .ok()
            .and_then(|s| s.get(self.idx()).map(|(_, m)| *m))
            .unwrap_or(6.0)
    }

}

/// Which kind of board a widget can be placed on. The directory a
/// widget is installed under decides: `widgets/board` for the ordinary
/// boards, `widgets/appgrid` for APPGRID (the bottom fixture board),
/// `widgets/search_and_ai` for SEARCH AND AI (the top one).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum WidgetCategory {
    /// Home and the horizontal arms.
    #[default]
    Board,
    /// The bottom fixture board.
    Appgrid,
    /// The top fixture board.
    SearchAi,
}

/// Everything the program knows about one widget. The widget itself is
/// its file — `<name>.rhai` or `<name>.so` in its directory; what is
/// kept here is only what the layout engine and the editor need to
/// know about it before it draws.
#[derive(Clone, Debug)]
pub struct WidgetDef {
    /// Name used in .layaut files and as the directory name.
    pub name: String,
    /// Label shown in the layout editor.
    pub label: String,
    pub ref_h_vh: f32,
    pub min_h_vh: f32,
    /// Which kind of board this widget may be placed on.
    pub category: WidgetCategory,
}

static REGISTRY: OnceLock<Vec<WidgetDef>> = OnceLock::new();

/// Per-panel (reference height, minimum height) in vh, indexed like the
/// registry. Held apart from the registry and mutable, because these
/// belong to the LAYOUT: selecting another layout replaces them, while
/// the registry itself is fixed once panel indices are in use.
fn sizes() -> &'static RwLock<Vec<(f32, f32)>> {
    static S: OnceLock<RwLock<Vec<(f32, f32)>>> = OnceLock::new();
    S.get_or_init(|| RwLock::new(default_sizes()))
}

/// The sizes a layout falls back to when it names none of its own —
/// each widget's own defaults, straight from the registry that the
/// directory scan built.
pub fn default_sizes() -> Vec<(f32, f32)> {
    registry().iter().map(|d| (d.ref_h_vh, d.min_h_vh)).collect()
}

/// What each widget measured itself at this frame, indexed like the
/// registry. None means the widget grows into whatever it is given.
fn intrinsic() -> &'static RwLock<Vec<Option<f32>>> {
    static I: OnceLock<RwLock<Vec<Option<f32>>>> = OnceLock::new();
    I.get_or_init(|| RwLock::new(Vec::new()))
}

/// The height the host's container adds around each panel's content —
/// border, vertical padding, and the title band when the widget
/// declares one. Indexed like the registry; 0.0 for a panel nobody
/// measured. The layout engine adds it to the content minimums, so a
/// panel kept "at its minimum" still shows its last content row under
/// a band, instead of losing exactly the band's height of content.
fn chrome() -> &'static RwLock<Vec<f32>> {
    static C: OnceLock<RwLock<Vec<f32>>> = OnceLock::new();
    C.get_or_init(|| RwLock::new(Vec::new()))
}

/// The per-world size table (u3 §3): what the flex solver solves
/// against and what `panel_font_scale` rescales by. Per-WORLD, not
/// per-process — two outputs under a compositor hold two of these,
/// each with its own intrinsic measurements. The process-wide setters
/// below keep feeding one global instance for the desktop of today;
/// `size_table()` snapshots it, and an embedder that owns its world
/// builds its own.
#[derive(Clone, Debug, Default)]
pub struct SizeTable {
    sizes: Vec<(f32, f32)>,
    intrinsic: Vec<Option<f32>>,
    chrome: Vec<f32>,
}

impl SizeTable {
    pub fn new(
        sizes: Vec<(f32, f32)>,
        intrinsic: Vec<Option<f32>>,
        chrome: Vec<f32>,
    ) -> Self {
        Self { sizes, intrinsic, chrome }
    }

    /// Reference height (vh); 10.0 for a panel the table does not name.
    pub fn ref_h_vh(&self, p: Panel) -> f32 {
        self.sizes.get(p.idx()).map(|s| s.0).unwrap_or(10.0)
    }

    /// Minimum content height (vh); 6.0 when unnamed.
    pub fn min_h_vh(&self, p: Panel) -> f32 {
        self.sizes.get(p.idx()).map(|s| s.1).unwrap_or(6.0)
    }

    /// What the widget measured itself at this frame; None grows.
    pub fn intrinsic_h(&self, p: Panel) -> Option<f32> {
        self.intrinsic.get(p.idx()).copied().flatten()
    }

    /// What the host's container adds around this panel's content
    /// (border, padding, title band); 0.0 when nobody measured it.
    pub fn chrome_h(&self, p: Panel) -> f32 {
        self.chrome.get(p.idx()).copied().unwrap_or(0.0)
    }
}

/// A snapshot of the process-wide table the setters below feed.
pub fn size_table() -> SizeTable {
    SizeTable {
        sizes: sizes().read().map(|s| s.clone()).unwrap_or_default(),
        intrinsic: intrinsic().read().map(|i| i.clone()).unwrap_or_default(),
        chrome: chrome().read().map(|c| c.clone()).unwrap_or_default(),
    }
}

/// Publishes this frame's measurements. The layout engine reads them to
/// give a widget with finite content exactly the height it needs, and
/// to share what is left among the ones that can use more.
pub fn set_panel_intrinsic(new: &[Option<f32>]) {
    if let Ok(mut i) = intrinsic().write() {
        i.clear();
        i.extend_from_slice(new);
    }
}

/// Publishes what the container will draw around each panel this frame.
/// Kept apart from the intrinsic heights: clearing the measurements for
/// a probe pass must not also forget the chrome — chrome depends on the
/// widget's title declaration and the theme, not on the box the panel
/// happened to get, so it cannot feed back into the probe.
pub fn set_panel_chrome(new: &[f32]) {
    if let Ok(mut c) = chrome().write() {
        c.clear();
        c.extend_from_slice(new);
    }
}

/// Installs the sizes the current layout asks for. Entries past the end
/// keep their defaults, so a layout only has to name what it changes.
pub fn set_panel_sizes(new: &[(Panel, f32, f32)]) {
    let mut table = default_sizes();
    for (p, r, m) in new {
        if let Some(slot) = table.get_mut(p.idx()) {
            *slot = (
                if r.is_finite() && *r > 0.0 { *r } else { slot.0 },
                if m.is_finite() && *m > 0.0 { *m } else { slot.1 },
            );
        }
    }
    if let Ok(mut s) = sizes().write() {
        *s = table;
    }
}

/// Installs the widget registry. The FIRST call wins; later ones are
/// ignored, because panel indices are baked into layouts and rectangles
/// the moment the first frame is drawn.
pub fn set_registry(defs: Vec<WidgetDef>) {
    let _ = REGISTRY.set(defs);
}

/// The widget registry. Falls back to the built-in set, so a missing or
/// unreadable widgets directory can never leave the program with no
/// widgets at all.
pub fn registry() -> &'static [WidgetDef] {
    REGISTRY.get_or_init(builtin_widgets)
}

pub fn panel_count() -> usize {
    registry().len()
}

/// The widget names the program knows: the last-resort registry when
/// the widgets directory yields nothing, and the label/default-size
/// table the directory scan uses for the shipped names. It is not a set
/// of widgets — the widgets themselves are files on disk, installed
/// from the widgets repository.
pub fn builtin_widgets() -> Vec<WidgetDef> {
    // (name, label, ref height vh, min height vh)
    //
    // hardware's reference was 5.5 and uptime's minimum 6.0; both sat on
    // the 0.62 font-scale clamp (u2 §6.3 sanctions the raise). uptime's
    // 7.0 minimum is what keeps the KERNEL row on machines that report
    // one; the other ten are untouched — a layout that wants different
    // boxes names its own sizes (flex.rs::builtin_sizes for the default).
    const DEFS: [(&str, &str, f32, f32); 12] = [
        ("clock", "CLOCK", 7.0, 5.0),
        ("sysinfo", "SYSTEM INFO", 4.5, 4.5),
        ("uptime", "UPTIME", 8.0, 7.0),
        ("hardware", "HARDWARE", 6.5, 6.5),
        ("cpu", "CPU", 15.5, 8.0),
        ("memory", "MEMORY", 10.5, 8.0),
        ("processes", "PROCESSES", 11.5, 6.0),
        ("shell", "SHELL", 60.0, 10.0),
        ("network", "NETWORK", 12.4, 8.0),
        ("filesystem", "FILESYSTEM", 57.0, 8.0),
        ("keyboard", "KEYBOARD", 32.5, 10.0),
        ("control", "CONTROL", 22.0, 13.0),
    ];
    DEFS.into_iter()
        .map(|(name, label, ref_h, min_h)| WidgetDef {
            name: name.to_string(),
            label: label.to_string(),
            ref_h_vh: ref_h,
            min_h_vh: min_h,
            category: WidgetCategory::Board,
        })
        .collect()
}

/// A panel placed far outside the window = hidden.
pub const OFF_SPEC: PanelSpec = PanelSpec { x: 200.0, y: 0.0, w: 20.0, h: 25.0 };

/// Panel layout — positions of all panels loaded from a legacy .layaut
/// file (percent of the window at the 16:9 reference). Panels missing
/// from the file stay hidden.
#[derive(Clone)]
pub struct LayoutSpec {
    pub panels: Vec<PanelSpec>,
}

impl LayoutSpec {
    pub fn p(&self, p: Panel) -> &PanelSpec {
        self.panels.get(p.idx()).unwrap_or(&OFF_SPEC)
    }
    pub fn set(&mut self, p: Panel, s: PanelSpec) {
        if self.panels.len() <= p.idx() {
            self.panels.resize(p.idx() + 1, OFF_SPEC);
        }
        self.panels[p.idx()] = s;
    }
}

impl Default for LayoutSpec {
    fn default() -> Self {
        LayoutSpec { panels: vec![OFF_SPEC; panel_count()] }
    }
}

/// One flexbox column: CSS-like width constraints plus panels stacked
/// top to bottom with height weights.
#[derive(Clone)]
pub struct FlexColumn {
    /// Preferred width as a percentage of the row (flex-basis).
    pub basis: f32,
    /// Minimum width in px (min-width).
    pub min: f32,
    /// Maximum width in px (max-width); INFINITY = unlimited.
    pub max: f32,
    /// Share of the leftover space (flex-grow).
    pub grow: f32,
    /// Collapse priority when space runs out: 1 disappears first,
    /// then 2, ...; 0 = never hidden.
    pub collapse: u32,
    /// Vertical gap between the panels, in height weight units.
    pub gap: f32,
    /// Panels top to bottom with their height weights.
    pub panels: Vec<(Panel, f32)>,
}

/// A flexbox layout: columns laid out left to right.
#[derive(Clone)]
pub struct FlexLayaut {
    pub columns: Vec<FlexColumn>,
    /// `units = px` in the file: min/max are literal device pixels. Default
    /// false = device-independent units scaled by the window height
    /// (flex.rs::lu), so one composition comes out at 720p and at 4K.
    pub units_px: bool,
    /// `pad_x = <percent>` in the file: page padding per side, percent of
    /// the window width. None = the engine's own margin. A layout that
    /// wants clear outer margin (u1 §4.3's instrument arrangement, room
    /// for decor.dump columns) is the one that names it.
    pub pad_x: Option<f32>,
}

/// How the panel layout is produced (see src/flex.rs).
#[derive(Clone)]
pub enum LayoutMode {
    /// Built-in responsive default: a flexbox tree computed from the
    /// actual window size every frame.
    Flex,
    /// A custom flexbox .layaut file — same engine as the default.
    Custom(FlexLayaut),
    /// A legacy .layaut file: a fixed 16:9 base, re-adapted to the
    /// window every frame.
    Fixed(LayoutSpec),
}

impl Default for LayoutMode {
    fn default() -> Self {
        LayoutMode::Flex
    }
}

/// Computed panel rectangles (in physical pixels).
pub struct Layout {
    pub panels: Vec<Rect>,
}

impl Layout {
    /// All panels off-screen (starting point for layout engines).
    pub fn empty(w: f32, h: f32) -> Layout {
        Layout { panels: vec![Rect::new(w * 2.0, 0.0, w * 0.16, h * 0.6); panel_count()] }
    }

    /// The rectangle of a panel.
    pub fn p(&self, p: Panel) -> Rect {
        self.panels
            .get(p.idx())
            .copied()
            .unwrap_or(Rect::new(-1.0e6, 0.0, 1.0, 1.0))
    }

    pub fn set(&mut self, p: Panel, r: Rect) {
        if self.panels.len() <= p.idx() {
            self.panels.resize(p.idx() + 1, Rect::new(-1.0e6, 0.0, 1.0, 1.0));
        }
        self.panels[p.idx()] = r;
    }

    /// Derives the INNER content containers from the OUTER panel
    /// rectangles: the inner container is exactly the widget's content
    /// area, and the outer rectangle (the resize edge) is ALWAYS `pad`
    /// larger than it on every side. Drawing and hit-testing use the
    /// inner containers; the outer rectangles stay authoritative for
    /// layout files and the grid editor (which keeps panels large
    /// enough for the padding plus some content).
    pub fn padded(&self, pad: f32) -> Layout {
        let pad = pad.max(0.0);
        let ins = |r: &Rect| {
            Rect::new(
                r.x + pad,
                r.y + pad,
                (r.w - 2.0 * pad).max(2.0),
                (r.h - 2.0 * pad).max(2.0),
            )
        };
        Layout { panels: self.panels.iter().map(ins).collect() }
    }

    pub fn compute(w: f32, h: f32, spec: &LayoutSpec) -> Self {
        let vw = w / 100.0;
        let vh = h / 100.0;
        Layout {
            panels: (0..panel_count())
                .map(|i| {
                    let p = spec.panels.get(i).unwrap_or(&OFF_SPEC);
                    Rect::new(p.x * vw, p.y * vh, p.w * vw, p.h * vh)
                })
                .collect(),
        }
    }
}

/// Drawing context passed to the panels.
pub struct Ctx<'a> {
    pub dl: &'a mut DrawList,
    pub fonts: &'a mut FontSystem,
    /// Window width/height in px.
    pub w: f32,
    pub h: f32,
    /// Time since application start, in seconds.
    pub t: f64,
    /// Mouse cursor position.
    pub mouse: (f32, f32),
    /// Terminal font size multiplier (TermFontSize= in nacelle-desktop.conf).
    pub term_font_scale: f32,
    /// Interface font size multiplier (UIFontSize= in nacelle-desktop.conf).
    pub ui_font_scale: f32,
    /// Font scale of the panel being drawn (container-query style):
    /// narrow columns shrink their text. Panels set it on entry and
    /// reset it to 1.0 when done; full-width panels leave it at 1.0.
    pub panel_scale: f32,
    /// The focus chain of the world being drawn — how a control asks
    /// "am I focused?" and joins the Tab order ([`crate::focus`]).
    /// Per-world like `SizeTable`, owned by the application. None while
    /// a caller draws without one (tests, an embedder with no keyboard)
    /// — every control treats that as "never focused".
    pub focus: Option<&'a mut FocusCtl>,
    /// Where a control files "the pointer is resting on me and there is
    /// more to say than what I drew" ([`crate::object::tooltip`]).
    /// Owned by the application, which draws the manager LAST — the
    /// tooltip covers whatever it explains, so nothing may be drawn over
    /// it. None while a caller draws without one: a request is then
    /// simply not made, which is what a headless test and a plugin's
    /// own surface both want.
    pub tips: Option<&'a mut crate::object::tooltip::Tooltips>,
}

impl<'a> Ctx<'a> {
    pub fn vh(&self, v: f32) -> f32 {
        self.h / 100.0 * v
    }
    pub fn vw(&self, v: f32) -> f32 {
        self.w / 100.0 * v
    }
    /// Interface font size: scaled by UIFontSize= (text only) and by the
    /// width of the panel being drawn, min 8 px.
    pub fn font_px(&self, v: f32) -> f32 {
        (self.vh(v) * UI_FONT_BASE * self.ui_font_scale * self.panel_scale).max(8.0)
    }
    /// Panel-relative font scale (like a CSS container query): 100% when
    /// the panel matches its reference box (a classic side-column width =
    /// 30% of the window height, and the panel's default height).
    /// Enlarging the panel scales the whole widget UP proportionally
    /// (the smaller of the two axes wins, so proportions are kept);
    /// narrow columns still shrink down to 62%.
    /// Both axes, smaller wins: a widget keeps its proportions whichever
    /// way its panel is stretched. Width alone would blow the on-screen
    /// keyboard up to two and a half times its size the moment it got a
    /// wide panel.
    pub fn panel_font_scale(&self, r: &Rect, p: Panel) -> f32 {
        self.panel_font_scale_in(r, p, &size_table())
    }

    /// The same rescale against a CALLER's size table — the per-world
    /// form (u3 L2); the method above is its process-wide shorthand.
    pub fn panel_font_scale_in(&self, r: &Rect, p: Panel, t: &SizeTable) -> f32 {
        let ws = r.w / (self.h * 0.30);
        let hs = r.h / (self.h * t.ref_h_vh(p) / 100.0);
        ws.min(hs).clamp(0.62, 3.0)
    }
}

/// Trims text (with a trailing ellipsis) so it fits the given width —
/// shared by the telemetry widgets.
pub fn fit_end(ctx: &mut Ctx, px: f32, text: &str, max_w: f32) -> String {
    if ctx.fonts.measure(FONT_UI, px, text, px * 0.06) <= max_w {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let mut n = chars.len().saturating_sub(1);
    while n > 1 {
        let cand: String = chars[..n].iter().collect::<String>() + "\u{2026}";
        if ctx.fonts.measure(FONT_UI, px, &cand, px * 0.06) <= max_w {
            return cand;
        }
        n -= 1;
    }
    "\u{2026}".to_string()
}
