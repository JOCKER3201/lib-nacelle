//! The one wall between view logic and painting.
//!
//! A table has two ways to reach a pixel: the host's [`Ctx`] and its
//! draw list, or the plugin ABI's function table. Without something in
//! between, an interactive table exists twice and the two copies drift
//! — which is not a hypothesis, it is what happened to `fit_end`
//! (`ui::fit_end_tracked` and the file panel's own `fit_name` do the
//! same job in two places and no longer agree).
//!
//! [`Surface`] is that something: everything a view may do to the
//! outside world, and nothing else. [`CtxSurface`] draws through the
//! host's draw list; [`AbiSurface`] draws through [`HostApi`]. View code
//! is generic over the trait and cannot tell which it has, which is what
//! keeps it single-sourced.
//!
//! **Tokens are named by string here**, where the rest of the host names
//! them through a `OnceLock<TokenId>` per site. That is deliberate: the
//! same view code has to name tokens on the far side of an ABI, where a
//! `TokenId` means nothing. The lookup is memoised per name for the life
//! of the process (token ids are stable — [`crate::theme::id`] and
//! [`HostApi::theme_token`] both say so), but a name lookup is still a
//! hash where a static is a load: a view reads its tokens into a local
//! `Look` struct ONCE per draw and never inside a row loop.

use crate::draw::{Corner, CornerStyle};
use crate::font::FONT_UI;
use crate::theme::parse::State;
use crate::theme::{self, Color, TokenId};
use crate::ui::Align;
use crate::{Ctx, Rect};
use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::c_void;

/// One rung of a class's state ladder, in the colours a draw call takes.
///
/// The host bakes this as [`theme::bake::StateStyle`] and the ABI ships
/// it as [`crate::runtime::StateStyleC`]; both arrive here, so view code
/// reads one shape.
#[derive(Clone, Copy, Debug)]
pub struct StateInk {
    pub fill: Color,
    pub edge: Color,
    pub text: Color,
    pub glyph: Color,
    pub edge_width: f32,
    pub glow_radius: f32,
    pub glow_alpha: f32,
    pub elevation: f32,
}

impl StateInk {
    /// What a control looks like when no theme says otherwise — the
    /// engine's own raw rung, so a build with no such class still draws
    /// something honest instead of nothing.
    pub fn raw() -> StateInk {
        StateInk::from(theme::bake::StateStyle::RAW)
    }
}

impl From<theme::bake::StateStyle> for StateInk {
    fn from(s: theme::bake::StateStyle) -> StateInk {
        StateInk {
            fill: s.fill,
            edge: s.edge,
            text: s.text,
            glyph: s.glyph,
            edge_width: s.edge_width,
            glow_radius: s.glow_radius,
            glow_alpha: s.glow_alpha,
            elevation: s.elevation,
        }
    }
}

/// Everything a view object may do to the outside world.
///
/// Drawing, the theme, and the two facts about the frame a view needs to
/// answer the pointer. Nothing else: a view that could reach further
/// would stop being portable across the boundary, which is the whole
/// point of the trait.
/// Corners and tessellation for a ring at this size: the radius can
/// never exceed half the short side (past it two corners would cross),
/// and the arc count is the toolkit's own quarter-pixel rule.
/// The boundary's corner vocabulary. The theme's enum indices intern in
/// load order and mean nothing across a library edge, so the numbers
/// travel instead.
fn corner_code(style: CornerStyle) -> u32 {
    match style {
        CornerStyle::Round => crate::runtime::CORNER_ROUND,
        CornerStyle::Chamfer => crate::runtime::CORNER_CHAMFER,
        CornerStyle::Square => crate::runtime::CORNER_SQUARE,
    }
}

fn ring_parts(style: CornerStyle, radius: f32, r: Rect) -> ([Corner; 4], u8) {
    let size = radius.max(0.0).min(r.w.min(r.h) / 2.0);
    ([Corner { style, size }; 4], crate::draw::ring_segments(size, 0.25, 16))
}

pub trait Surface {
    // ----------------------------------------------------------- paint
    fn rect(&mut self, r: Rect, c: Color);
    fn rect_outline(&mut self, r: Rect, w: f32, c: Color);
    fn line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, w: f32, c: Color);
    /// A run of points; `closed` joins the last back to the first.
    fn polyline(&mut self, pts: &[[f32; 2]], w: f32, c: Color, closed: bool);
    /// A filled rectangle wearing the family's corners, and its stroke.
    /// The default draws the plain rectangle, which is what a surface
    /// without the primitive can honestly do — and what a theme asking
    /// for square corners wants anyway.
    fn ring_fill(&mut self, r: Rect, style: CornerStyle, radius: f32, c: Color) {
        let _ = (style, radius);
        self.rect(r, c);
    }
    fn ring(&mut self, r: Rect, style: CornerStyle, radius: f32, w: f32, c: Color) {
        let _ = (style, radius);
        self.rect_outline(r, w, c);
    }
    /// A filled convex quadrilateral — the sheared plate a tab is drawn
    /// on, which no rectangle can stand in for.
    ///
    /// Both real surfaces have one (`DrawList::quad`, `HostApi::quad`);
    /// the default is here for a surface that does not, and fills the
    /// bounding box instead — which is exactly the shape `tab.skew = 0`
    /// already asks for.
    fn quad(&mut self, pts: [[f32; 2]; 4], c: Color) {
        let (mut x0, mut y0) = (f32::MAX, f32::MAX);
        let (mut x1, mut y1) = (f32::MIN, f32::MIN);
        for p in pts {
            x0 = x0.min(p[0]);
            y0 = y0.min(p[1]);
            x1 = x1.max(p[0]);
            y1 = y1.max(p[1]);
        }
        self.rect(Rect::new(x0, y0, x1 - x0, y1 - y0), c);
    }
    fn text(&mut self, px: f32, x: f32, y: f32, s: &str, c: Color, track: f32, align: Align);
    fn measure(&mut self, px: f32, s: &str, track: f32) -> f32;

    /// Clips everything drawn until the matching [`Surface::unclip`] to
    /// `r`, intersected with whatever clip is already in force.
    ///
    /// **False when the surface cannot clip** — an old host across the
    /// ABI. The caller then degrades to whole-row snapping
    /// ([`crate::view::Snap::Row`]), which is exactly what the file
    /// panel does today, and must NOT call [`Surface::unclip`].
    fn clip(&mut self, r: Rect) -> bool;
    fn unclip(&mut self);

    /// Whether [`Surface::clip`] would succeed, asked BEFORE anything is
    /// drawn.
    ///
    /// §1.1 gives only `clip() -> bool`, and that is enough to draw
    /// safely — but not to decide. A scrolled view has to pick its snap
    /// (whole rows, or free pixels) while it is working out its window,
    /// which is before the clip would be pushed, and pushing one early
    /// would clip the header the window sits under. One extra question,
    /// asked once per draw.
    fn can_clip(&self) -> bool {
        true
    }

    /// A chamfered fill. Degrades to a square [`Surface::rect`] on a
    /// surface with no chamfer of its own — the ABI has no chamfer
    /// primitive, and a square badge is what `badge.corner = 0` draws
    /// anyway, so the degradation is a look the theme can already ask
    /// for rather than an invention.
    fn chamfer_fill(&mut self, r: Rect, cut: f32, c: Color) {
        let _ = cut;
        self.rect(r, c);
    }

    /// A chamfered outline, degrading like [`Surface::chamfer_fill`].
    fn chamfer_frame(&mut self, r: Rect, cut: f32, w: f32, c: Color) {
        let _ = cut;
        self.rect_outline(r, w, c);
    }

    // ----------------------------------------------------------- theme
    /// Whether the master declares this token at all. The one question
    /// a missing-token default cannot answer: a role that does not exist
    /// has to fall back to `body`, and `px() == 0.0` is also what a real
    /// token holding zero says.
    fn has_token(&mut self, name: &str) -> bool;
    fn px(&mut self, name: &str) -> f32;
    /// A colour used as INK. A missing token degrades to the engine's
    /// raw grey.
    fn color(&mut self, name: &str) -> Color;
    /// A colour used as a BED (a fill under things). A missing token
    /// degrades to the raw background, not to raw ink — an unthemed
    /// stripe must read as background rather than as a grey slab.
    fn bed(&mut self, name: &str) -> Color;
    fn flag(&mut self, name: &str) -> bool;
    /// The word an enum token currently resolves to.
    fn word(&mut self, name: &str) -> String;
    /// Whether an enum token stands at `word`. Written in terms of
    /// [`Surface::word`] so both sides answer it the same way: the ABI
    /// can compare words but cannot look an index up by name.
    fn enum_is(&mut self, name: &str, word: &str) -> bool {
        self.word(name) == word
    }
    fn class_state(&mut self, class: &str, state: State) -> StateInk;
    /// Bumped on every theme swap. A view caching resolved values
    /// invalidates when this moves.
    fn epoch(&mut self) -> u32;

    // ----------------------------------------------------------- frame
    /// Seconds since the application started — `Ctx::t` / `elapsed`.
    fn now(&self) -> f64;
    fn mouse(&self) -> (f32, f32);
    /// The container-query font scale of the panel being drawn. Runtime
    /// state, never a look decision: it multiplies a role's baked size
    /// exactly where `Ctx::panel_scale` does.
    fn scale(&self) -> f32;

    /// "The pointer is resting on `anchor`, and the whole of what I drew
    /// there is `text`" — the tooltip request a view files while drawing
    /// (F2 §8.1). The manager, not the view, decides whether the pointer
    /// has rested long enough and where the box goes; `id` is what tells
    /// two neighbouring targets apart across frames.
    ///
    /// Filed only when the pointer really is inside `anchor` — the view
    /// has both in hand, and asking here saves the manager a registry of
    /// every hover-able rectangle on screen.
    ///
    /// **Nothing, by default.** A tooltip is drawn over its neighbours,
    /// and a plugin's surface may not paint outside the box it was given
    /// — a host-drawn tooltip for plugins needs an ABI entry of its own
    /// and is out of this phase (F2 §11).
    fn tooltip(&mut self, id: u64, anchor: Rect, text: &str) {
        let _ = (id, anchor, text);
    }
}

// ------------------------------------------------------------ host side

/// A token id resolved once per NAME, for the life of the process.
///
/// The same memo `ui::role` keeps for type roles, generalised: ids are
/// stable, so the map never goes stale, and a theme swap changes what
/// the id resolves TO without changing the id.
fn token_id(name: &str) -> TokenId {
    thread_local! {
        static IDS: RefCell<HashMap<String, TokenId>> = RefCell::new(HashMap::new());
    }
    IDS.with(|m| {
        if let Some(id) = m.borrow().get(name) {
            return *id;
        }
        let id = theme::id(name).unwrap_or(TokenId::MISSING);
        m.borrow_mut().insert(name.to_string(), id);
        id
    })
}

/// The index of `word` in a token's declared enum list, memoised per
/// (token, word).
///
/// The default [`Surface::enum_is`] compares WORDS, which is the only
/// question the ABI can answer — but on the host that would allocate a
/// `String` for every ask, and the asks are in draw paths. Comparing
/// indices costs a hash and no allocation, and answers the same.
fn enum_index(id: TokenId, word: &str) -> Option<u16> {
    thread_local! {
        static IDX: RefCell<HashMap<usize, HashMap<String, Option<u16>>>> =
            RefCell::new(HashMap::new());
    }
    IDX.with(|m| {
        let mut m = m.borrow_mut();
        let per_token = m.entry(id.index()).or_default();
        if let Some(i) = per_token.get(word) {
            return *i;
        }
        let i = theme::enum_index(id, word);
        per_token.insert(word.to_string(), i);
        i
    })
}

/// A class index resolved once per name; `None` in a build whose master
/// declares no such class.
fn class_index(name: &str) -> Option<u16> {
    thread_local! {
        static IDS: RefCell<HashMap<String, Option<u16>>> = RefCell::new(HashMap::new());
    }
    IDS.with(|m| {
        if let Some(c) = m.borrow().get(name) {
            return *c;
        }
        let c = theme::class_id(name);
        m.borrow_mut().insert(name.to_string(), c);
        c
    })
}

/// The host's surface: a view drawing into the application's own draw
/// list, with the application's fonts and the active theme.
pub struct CtxSurface<'s, 'c> {
    ctx: &'s mut Ctx<'c>,
}

impl<'s, 'c> CtxSurface<'s, 'c> {
    pub fn new(ctx: &'s mut Ctx<'c>) -> CtxSurface<'s, 'c> {
        CtxSurface { ctx }
    }

    /// The context underneath, for a caller that is the host anyway and
    /// needs a piece of the vocabulary this trait deliberately does not
    /// carry. Host-only by construction — nothing generic over
    /// [`Surface`] can reach it.
    pub fn ctx(&mut self) -> &mut Ctx<'c> {
        self.ctx
    }
}

impl Surface for CtxSurface<'_, '_> {
    fn rect(&mut self, r: Rect, c: Color) {
        self.ctx.dl.rect(r.x, r.y, r.w, r.h, c);
    }

    fn rect_outline(&mut self, r: Rect, w: f32, c: Color) {
        self.ctx.dl.rect_outline(r.x, r.y, r.w, r.h, w, c);
    }

    fn line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, w: f32, c: Color) {
        self.ctx.dl.line(x0, y0, x1, y1, w, c);
    }

    fn polyline(&mut self, pts: &[[f32; 2]], w: f32, c: Color, closed: bool) {
        self.ctx.dl.polyline(pts, w, c, closed);
    }

    fn ring_fill(&mut self, r: Rect, style: CornerStyle, radius: f32, c: Color) {
        let (corners, seg) = ring_parts(style, radius, r);
        self.ctx.dl.ring_fill(r, &corners, seg, c);
    }

    fn ring(&mut self, r: Rect, style: CornerStyle, radius: f32, w: f32, c: Color) {
        let (corners, seg) = ring_parts(style, radius, r);
        self.ctx.dl.ring(r, &corners, seg, w, c);
    }

    fn quad(&mut self, pts: [[f32; 2]; 4], c: Color) {
        self.ctx.dl.quad(pts, c);
    }

    fn text(&mut self, px: f32, x: f32, y: f32, s: &str, c: Color, track: f32, align: Align) {
        match align {
            Align::Left => self.ctx.dl.text(self.ctx.fonts, FONT_UI, px, x, y, s, c, track),
            Align::Center => {
                self.ctx.dl.text_center(self.ctx.fonts, FONT_UI, px, x, y, s, c, track)
            }
            Align::Right => {
                self.ctx.dl.text_right(self.ctx.fonts, FONT_UI, px, x, y, s, c, track)
            }
        }
    }

    fn measure(&mut self, px: f32, s: &str, track: f32) -> f32 {
        self.ctx.fonts.measure(FONT_UI, px, s, track)
    }

    fn clip(&mut self, r: Rect) -> bool {
        self.ctx.dl.push_clip(r.x, r.y, r.w, r.h);
        true
    }

    fn unclip(&mut self) {
        self.ctx.dl.pop_clip();
    }

    fn chamfer_fill(&mut self, r: Rect, cut: f32, c: Color) {
        self.ctx.dl.chamfer_fill(r.x, r.y, r.w, r.h, cut, c);
    }

    fn chamfer_frame(&mut self, r: Rect, cut: f32, w: f32, c: Color) {
        self.ctx.dl.chamfer_frame(r.x, r.y, r.w, r.h, cut, w, c);
    }

    fn has_token(&mut self, name: &str) -> bool {
        token_id(name) != TokenId::MISSING
    }

    fn px(&mut self, name: &str) -> f32 {
        theme::resolved().px(token_id(name))
    }

    fn color(&mut self, name: &str) -> Color {
        theme::resolved().color(token_id(name))
    }

    fn bed(&mut self, name: &str) -> Color {
        theme::resolved().bed(token_id(name))
    }

    fn flag(&mut self, name: &str) -> bool {
        theme::resolved().flag(token_id(name))
    }

    fn word(&mut self, name: &str) -> String {
        crate::ui::theme_word(token_id(name))
    }

    fn enum_is(&mut self, name: &str, word: &str) -> bool {
        let id = token_id(name);
        enum_index(id, word) == Some(theme::resolved().enum_of(id))
    }

    fn class_state(&mut self, class: &str, state: State) -> StateInk {
        match class_index(class) {
            Some(c) => StateInk::from(theme::resolved().class_state(c, state)),
            None => StateInk::raw(),
        }
    }

    fn epoch(&mut self) -> u32 {
        theme::epoch()
    }

    fn now(&self) -> f64 {
        self.ctx.t
    }

    fn mouse(&self) -> (f32, f32) {
        self.ctx.mouse
    }

    fn scale(&self) -> f32 {
        self.ctx.panel_scale
    }

    /// Handed to the application's manager, if it kept one. The
    /// containment test is repeated here rather than trusted: a view
    /// that files a request for a rectangle the pointer is nowhere near
    /// would explain the wrong thing, and one comparison is cheaper than
    /// finding that out on screen.
    fn tooltip(&mut self, id: u64, anchor: Rect, text: &str) {
        if !anchor.contains(self.ctx.mouse.0, self.ctx.mouse.1) {
            return;
        }
        let now = self.ctx.t;
        if let Some(tips) = self.ctx.tips.as_deref_mut() {
            tips.request(id, anchor, text, now);
        }
    }
}

// ------------------------------------------------------------- ABI side

use crate::runtime::{ColorC, HostApi, RectC, StateStyleC};

fn rc(r: Rect) -> RectC {
    RectC { x: r.x, y: r.y, w: r.w, h: r.h }
}

fn cc(c: Color) -> ColorC {
    ColorC { r: c.r, g: c.g, b: c.b, a: c.a }
}

fn uc(c: ColorC) -> Color {
    Color { r: c.r, g: c.g, b: c.b, a: c.a }
}

/// The plugin's surface: the same view code, reaching the host through
/// the ABI's function table.
///
/// The theme crosses as TOKEN IDS, resolved by name once and cached
/// against [`HostApi::theme_epoch`] — the pattern the file panel already
/// uses by hand, generalised so a view does not have to. Two caches,
/// because the ABI has two namespaces: tokens and interaction classes.
pub struct AbiSurface<'a> {
    api: &'a HostApi,
    ctx: *mut c_void,
    tokens: HashMap<String, u32>,
    classes: HashMap<String, u32>,
    /// The epoch the caches were filled under. Ids are stable for the
    /// life of the process, so this is belt and braces — but a host that
    /// ever reloads its master under a running plugin would otherwise
    /// hand it stale ids, and the cost of being right is one compare.
    epoch: u32,
    /// The panel font scale the host applies to this widget. There is no
    /// ABI entry for it and the theme px that crosses the boundary is
    /// already baked, so it is 1.0 unless a caller knows better.
    scale: f32,
    mouse: (f32, f32),
    now: f64,
}

impl<'a> AbiSurface<'a> {
    /// Wraps the host table and the opaque drawing handle a plugin was
    /// handed for this frame.
    pub fn new(api: &'a HostApi, ctx: *mut c_void) -> AbiSurface<'a> {
        let mut x = 0.0f32;
        let mut y = 0.0f32;
        (api.mouse)(ctx, &mut x, &mut y);
        let epoch = (api.theme_epoch)(ctx);
        AbiSurface {
            api,
            ctx,
            tokens: HashMap::new(),
            classes: HashMap::new(),
            epoch,
            scale: 1.0,
            mouse: (x, y),
            now: (api.elapsed)(ctx),
        }
    }

    /// A caller that knows the host shrank its panel's type says so
    /// here; the default is 1.0.
    pub fn with_scale(mut self, scale: f32) -> AbiSurface<'a> {
        self.scale = scale;
        self
    }

    fn token(&mut self, name: &str) -> u32 {
        let live = (self.api.theme_epoch)(self.ctx);
        if live != self.epoch {
            self.tokens.clear();
            self.classes.clear();
            self.epoch = live;
        }
        if let Some(id) = self.tokens.get(name) {
            return *id;
        }
        let id = (self.api.theme_token)(name.as_ptr(), name.len() as u32);
        self.tokens.insert(name.to_string(), id);
        id
    }
}

impl Surface for AbiSurface<'_> {
    fn ring_fill(&mut self, r: Rect, style: CornerStyle, radius: f32, c: Color) {
        if self.api.has_ring() {
            (self.api.ring_fill)(self.ctx, rc(r), corner_code(style), radius, cc(c));
        } else {
            self.rect(r, c);
        }
    }

    fn ring(&mut self, r: Rect, style: CornerStyle, radius: f32, w: f32, c: Color) {
        if self.api.has_ring() {
            (self.api.ring)(self.ctx, rc(r), corner_code(style), radius, w, cc(c));
        } else {
            self.rect_outline(r, w, c);
        }
    }

    fn rect(&mut self, r: Rect, c: Color) {
        (self.api.rect)(self.ctx, rc(r), cc(c));
    }

    fn rect_outline(&mut self, r: Rect, w: f32, c: Color) {
        (self.api.rect_outline)(self.ctx, rc(r), w, cc(c));
    }

    fn line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, w: f32, c: Color) {
        (self.api.line)(self.ctx, x0, y0, x1, y1, w, cc(c));
    }

    fn quad(&mut self, pts: [[f32; 2]; 4], c: Color) {
        (self.api.quad)(self.ctx, pts.as_ptr() as *const f32, cc(c));
    }

    fn polyline(&mut self, pts: &[[f32; 2]], w: f32, c: Color, closed: bool) {
        if pts.is_empty() {
            return;
        }
        (self.api.polyline)(
            self.ctx,
            pts.as_ptr() as *const f32,
            pts.len() as u32,
            w,
            cc(c),
            closed,
        );
    }

    fn text(&mut self, px: f32, x: f32, y: f32, s: &str, c: Color, track: f32, align: Align) {
        let a = match align {
            Align::Left => 0,
            Align::Center => 1,
            Align::Right => 2,
        };
        (self.api.text)(
            self.ctx,
            FONT_UI as u32,
            px,
            x,
            y,
            s.as_ptr(),
            s.len() as u32,
            cc(c),
            track,
            a,
        );
    }

    fn measure(&mut self, px: f32, s: &str, track: f32) -> f32 {
        (self.api.measure)(self.ctx, FONT_UI as u32, px, s.as_ptr(), s.len() as u32, track)
    }

    fn clip(&mut self, r: Rect) -> bool {
        if !self.api.has_clip() {
            return false;
        }
        (self.api.push_clip)(self.ctx, rc(r));
        true
    }

    fn can_clip(&self) -> bool {
        self.api.has_clip()
    }

    fn unclip(&mut self) {
        if self.api.has_clip() {
            (self.api.pop_clip)(self.ctx);
        }
    }

    fn has_token(&mut self, name: &str) -> bool {
        self.token(name) != u32::MAX
    }

    fn px(&mut self, name: &str) -> f32 {
        let id = self.token(name);
        (self.api.theme_px)(self.ctx, id)
    }

    fn color(&mut self, name: &str) -> Color {
        let id = self.token(name);
        uc((self.api.theme_color)(self.ctx, id))
    }

    fn bed(&mut self, name: &str) -> Color {
        let id = self.token(name);
        uc((self.api.theme_bed)(self.ctx, id))
    }

    fn flag(&mut self, name: &str) -> bool {
        let id = self.token(name);
        (self.api.theme_flag)(self.ctx, id) != 0
    }

    fn word(&mut self, name: &str) -> String {
        if !self.api.has_theme_enum_word() {
            return String::new();
        }
        let id = self.token(name);
        // 64 bytes is longer than every word the master declares; a
        // longer one arrives truncated rather than reallocating a buffer
        // sixty times a second for a name nobody writes.
        let mut buf = [0u8; 64];
        let n = (self.api.theme_enum_word)(self.ctx, id, buf.as_mut_ptr(), buf.len() as u32);
        String::from_utf8_lossy(&buf[..(n as usize).min(buf.len())]).into_owned()
    }

    fn class_state(&mut self, class: &str, state: State) -> StateInk {
        let live = (self.api.theme_epoch)(self.ctx);
        if live != self.epoch {
            self.tokens.clear();
            self.classes.clear();
            self.epoch = live;
        }
        let id = match self.classes.get(class) {
            Some(id) => *id,
            None => {
                let id = (self.api.theme_class)(class.as_ptr(), class.len() as u32);
                self.classes.insert(class.to_string(), id);
                id
            }
        };
        if id == u32::MAX {
            return StateInk::raw();
        }
        let mut out = StateStyleC {
            fill: ColorC { r: 0.0, g: 0.0, b: 0.0, a: 0.0 },
            edge: ColorC { r: 0.0, g: 0.0, b: 0.0, a: 0.0 },
            text: ColorC { r: 0.0, g: 0.0, b: 0.0, a: 0.0 },
            glyph: ColorC { r: 0.0, g: 0.0, b: 0.0, a: 0.0 },
            edge_width: 0.0,
            glow_radius: 0.0,
            glow_alpha: 0.0,
            elevation: 0.0,
        };
        let n = (self.api.theme_class_state)(
            self.ctx,
            id,
            state as u32,
            &mut out,
            std::mem::size_of::<StateStyleC>() as u32,
        );
        if n == 0 {
            return StateInk::raw();
        }
        StateInk {
            fill: uc(out.fill),
            edge: uc(out.edge),
            text: uc(out.text),
            glyph: uc(out.glyph),
            edge_width: out.edge_width,
            glow_radius: out.glow_radius,
            glow_alpha: out.glow_alpha,
            elevation: out.elevation,
        }
    }

    fn epoch(&mut self) -> u32 {
        (self.api.theme_epoch)(self.ctx)
    }

    fn now(&self) -> f64 {
        self.now
    }

    fn mouse(&self) -> (f32, f32) {
        self.mouse
    }

    fn scale(&self) -> f32 {
        self.scale
    }
}

// ---------------------------------------------------------------------

/// The crate's one fake surface, shared by every test module that needs
/// to hold a view still without a window, a font atlas or a loaded
/// master. It lives here because this is where [`Surface`] lives, and it
/// is `pub(crate)` for the same reason the trait is one trait: a second
/// copy of it in another test module would be a second answer to "what
/// does a view see".
#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A surface that records what it was told to draw and answers the
    /// theme from a fixed table.
    pub struct FakeSurface {
        pub rects: Vec<(Rect, Color)>,
        pub texts: Vec<(f32, f32, String, Align)>,
        pub clips: Vec<Rect>,
        /// The tooltip requests the view filed, in the order it filed
        /// them — the last of a frame is the one the manager answers.
        pub tips: Vec<(u64, Rect, String)>,
        pub depth: i32,
        pub can_clip: bool,
        pub tokens: HashMap<String, f32>,
        /// Where the pointer is. Off-screen by default, so a test that
        /// does not care about hovering gets none of it.
        pub mouse: (f32, f32),
    }

    impl FakeSurface {
        pub fn new() -> FakeSurface {
            FakeSurface {
                rects: Vec::new(),
                texts: Vec::new(),
                clips: Vec::new(),
                tips: Vec::new(),
                depth: 0,
                can_clip: true,
                tokens: HashMap::new(),
                mouse: (-1.0, -1.0),
            }
        }

        /// Declares a token at a value — the builder every test that
        /// draws needs, since a token this table does not hold reads as
        /// zero and a zero-height row cannot be pointed at.
        pub fn token(mut self, name: &str, v: f32) -> FakeSurface {
            self.tokens.insert(name.to_string(), v);
            self
        }

        pub fn at(mut self, x: f32, y: f32) -> FakeSurface {
            self.mouse = (x, y);
            self
        }
    }

    impl Surface for FakeSurface {
        fn rect(&mut self, r: Rect, c: Color) {
            self.rects.push((r, c));
        }
        fn rect_outline(&mut self, _r: Rect, _w: f32, _c: Color) {}
        fn line(&mut self, _a: f32, _b: f32, _c: f32, _d: f32, _w: f32, _col: Color) {}
        fn polyline(&mut self, _p: &[[f32; 2]], _w: f32, _c: Color, _closed: bool) {}
        fn text(&mut self, _px: f32, x: f32, y: f32, s: &str, _c: Color, _t: f32, a: Align) {
            self.texts.push((x, y, s.to_string(), a));
        }
        /// One unit per character: a measure that is wrong about fonts
        /// but right about monotonicity, which is all the trimming and
        /// the column solver ask of it.
        fn measure(&mut self, px: f32, s: &str, _track: f32) -> f32 {
            s.chars().count() as f32 * px * 0.5
        }
        fn clip(&mut self, r: Rect) -> bool {
            if !self.can_clip {
                return false;
            }
            self.clips.push(r);
            self.depth += 1;
            true
        }
        fn unclip(&mut self) {
            self.depth -= 1;
        }
        fn has_token(&mut self, name: &str) -> bool {
            self.tokens.contains_key(name)
        }
        fn px(&mut self, name: &str) -> f32 {
            self.tokens.get(name).copied().unwrap_or(0.0)
        }
        fn color(&mut self, _name: &str) -> Color {
            Color { r: 1.0, g: 1.0, b: 1.0, a: 1.0 }
        }
        fn bed(&mut self, _name: &str) -> Color {
            Color { r: 0.0, g: 0.0, b: 0.0, a: 1.0 }
        }
        fn flag(&mut self, _name: &str) -> bool {
            false
        }
        fn word(&mut self, _name: &str) -> String {
            String::new()
        }
        fn class_state(&mut self, _class: &str, _state: State) -> StateInk {
            StateInk::raw()
        }
        fn epoch(&mut self) -> u32 {
            0
        }
        fn now(&self) -> f64 {
            0.0
        }
        fn mouse(&self) -> (f32, f32) {
            self.mouse
        }
        fn scale(&self) -> f32 {
            1.0
        }
        /// Recorded rather than dropped: the default is nothing, and a
        /// test of a CONSUMER has to be able to tell "nothing was asked"
        /// from "the surface throws requests away".
        fn tooltip(&mut self, id: u64, anchor: Rect, text: &str) {
            self.tips.push((id, anchor, text.to_string()));
        }
    }

    #[test]
    fn a_surface_that_cannot_clip_says_so_and_stays_balanced() {
        let mut sf = FakeSurface::new();
        sf.can_clip = false;
        assert!(!sf.clip(Rect::new(0.0, 0.0, 10.0, 10.0)));
        assert_eq!(sf.depth, 0, "a refused clip is not a clip to undo");
        sf.can_clip = true;
        assert!(sf.clip(Rect::new(0.0, 0.0, 10.0, 10.0)));
        sf.unclip();
        assert_eq!(sf.depth, 0);
    }

    #[test]
    fn a_chamfer_degrades_to_the_square_the_theme_can_already_ask_for() {
        // The ABI has no chamfer; the default impl must still put the
        // pill's fill somewhere, and it must be exactly the rectangle.
        let mut sf = FakeSurface::new();
        let r = Rect::new(2.0, 3.0, 40.0, 12.0);
        sf.chamfer_fill(r, 4.0, Color { r: 1.0, g: 0.0, b: 0.0, a: 1.0 });
        assert_eq!(sf.rects.len(), 1);
        assert_eq!(sf.rects[0].0.x, 2.0);
        assert_eq!(sf.rects[0].0.w, 40.0);
    }

    #[test]
    fn an_enum_is_a_word_comparison_on_both_sides() {
        // `enum_is` has one implementation for both surfaces precisely
        // because the ABI cannot look an index up by name.
        let mut sf = FakeSurface::new();
        assert!(sf.enum_is("scrollbar.track", ""));
        assert!(!sf.enum_is("scrollbar.track", "on"));
    }
}
