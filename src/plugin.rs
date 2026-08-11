//! The host side of the plugin boundary.
//!
//! [`host_api`] builds the table of functions a plugin draws through,
//! and [`PluginWidget`] wraps a loaded plugin so the application drives
//! it through the ordinary [`Widget`](crate::Widget) contract, exactly
//! like a script.
//!
//! Opening the library is the one part that is not here: `dlopen` is a
//! platform call, so the loader lives in the application beside its
//! other platform code. What it must do is fixed, though — see
//! [`crate::runtime::ATTACH_SYMBOL`].

use crate::runtime::{
    ActionC, CellC, ChromeC, ColorC, HostApi, PluginApi, RectC, TermReqC, TermViewC, ABI_VERSION,
    CELL_HAS_BG, CELL_SIZE_MIN, CELL_UNDERLINE, TERM_REQ_SIZE_MIN, TERM_VIEW_SIZE_MIN,
    VIEW_CURSOR, VIEW_LIVE, VIEW_TRUNCATED, ACTION_BYTES, ACTION_EXIT, SIZING_ROWS,
    ACTION_NONE, ACTION_OPEN_DIR, ACTION_OPEN_FILE, ACTION_OPEN_SETTINGS, ACTION_SCROLL_TERMINAL,
    ACTION_SELECT_TAB, CHROME_BUTTONS_CLOSE, CHROME_BUTTONS_MIN_CLOSE,
    CHROME_BUTTONS_MIN_MAX_CLOSE, PLUGIN_API_HAS_CHROME, PLUGIN_API_SIZE_MIN, StateStyleC,
};
use crate::font::FONT_COUNT;
use crate::term::{Cell, FLAG_UNDERLINE, FLAG_WIDE_LEAD, FLAG_WIDE_SPACER};
use crate::theme::{Color, Theme};
use crate::widget::Sizing;
use crate::{Action, Ctx, Host, Rect, Widget};
use std::ffi::c_void;
use std::path::PathBuf;

fn color_in(c: ColorC) -> Color {
    Color { r: c.r, g: c.g, b: c.b, a: c.a }
}

fn color_out(c: Color) -> ColorC {
    ColorC { r: c.r, g: c.g, b: c.b, a: c.a }
}

fn rect_out(r: Rect) -> RectC {
    RectC { x: r.x, y: r.y, w: r.w, h: r.h }
}

/// A font id from a plugin indexes a fixed array, so it is clamped
/// rather than trusted: a typo or a plugin built when there were more
/// fonts must draw in the wrong face, not read past the end of one.
fn font_in(font: u32) -> u8 {
    (font as u8).min(FONT_COUNT - 1)
}


/// Reads a UTF-8 span a plugin passed. Anything that is not valid UTF-8
/// is dropped rather than trusted: this crossed a library boundary.
fn text_in<'a>(p: *const u8, len: u32) -> &'a str {
    if p.is_null() || len == 0 {
        return "";
    }
    let bytes = unsafe { std::slice::from_raw_parts(p, len as usize) };
    std::str::from_utf8(bytes).unwrap_or("")
}

/// Turns the opaque handle back into the drawing context.
///
/// # Safety
/// Only valid inside a call the host made, where the pointer is the
/// context it passed.
unsafe fn ctx_of<'a>(p: *mut c_void) -> Option<&'a mut Ctx<'a>> {
    (p as *mut Ctx).as_mut()
}

macro_rules! with_ctx {
    ($p:expr, $ctx:ident, $body:expr) => {{
        let Some($ctx) = (unsafe { ctx_of($p) }) else { return Default::default() };
        $body
    }};
}

extern "C" fn h_rect(p: *mut c_void, r: RectC, c: ColorC) {
    let Some(ctx) = (unsafe { ctx_of(p) }) else { return };
    ctx.dl.rect(r.x, r.y, r.w, r.h, color_in(c));
}

extern "C" fn h_rect_outline(p: *mut c_void, r: RectC, t: f32, c: ColorC) {
    let Some(ctx) = (unsafe { ctx_of(p) }) else { return };
    ctx.dl.rect_outline(r.x, r.y, r.w, r.h, t, color_in(c));
}

extern "C" fn h_quad(p: *mut c_void, pts: *const f32, c: ColorC) {
    let Some(ctx) = (unsafe { ctx_of(p) }) else { return };
    if pts.is_null() {
        return;
    }
    let v = unsafe { std::slice::from_raw_parts(pts, 8) };
    ctx.dl.quad(
        [[v[0], v[1]], [v[2], v[3]], [v[4], v[5]], [v[6], v[7]]],
        color_in(c),
    );
}

extern "C" fn h_line(p: *mut c_void, x0: f32, y0: f32, x1: f32, y1: f32, t: f32, c: ColorC) {
    let Some(ctx) = (unsafe { ctx_of(p) }) else { return };
    ctx.dl.line(x0, y0, x1, y1, t, color_in(c));
}

#[allow(clippy::too_many_arguments)]
extern "C" fn h_polyline(
    p: *mut c_void,
    pts: *const f32,
    count: u32,
    t: f32,
    c: ColorC,
    closed: bool,
) {
    let Some(ctx) = (unsafe { ctx_of(p) }) else { return };
    if pts.is_null() || count < 2 || count > POLYLINE_MAX {
        return;
    }
    let v = unsafe { std::slice::from_raw_parts(pts, count as usize * 2) };
    let points: Vec<[f32; 2]> = v.chunks_exact(2).map(|c| [c[0], c[1]]).collect();
    ctx.dl.polyline(&points, t, color_in(c), closed);
}

#[allow(clippy::too_many_arguments)]
extern "C" fn h_text(
    p: *mut c_void,
    font: u32,
    px: f32,
    x: f32,
    y: f32,
    text: *const u8,
    len: u32,
    c: ColorC,
    spacing: f32,
    align: u32,
) {
    let Some(ctx) = (unsafe { ctx_of(p) }) else { return };
    let s = text_in(text, len);
    let font = font_in(font);
    let color = color_in(c);
    match align {
        1 => ctx.dl.text_center(ctx.fonts, font, px, x, y, s, color, spacing),
        2 => ctx.dl.text_right(ctx.fonts, font, px, x, y, s, color, spacing),
        _ => {
            ctx.dl.text(ctx.fonts, font, px, x, y, s, color, spacing);
        }
    }
}

extern "C" fn h_measure(
    p: *mut c_void,
    font: u32,
    px: f32,
    text: *const u8,
    len: u32,
    spacing: f32,
) -> f32 {
    with_ctx!(
        p,
        ctx,
        ctx.fonts.measure(font_in(font), px, text_in(text, len), spacing)
    )
}

#[allow(clippy::too_many_arguments)]
extern "C" fn h_module_title(
    p: *mut c_void,
    x: f32,
    y: f32,
    w: f32,
    px: f32,
    left: *const u8,
    left_len: u32,
    right: *const u8,
    right_len: u32,
    c: ColorC,
) {
    let Some(ctx) = (unsafe { ctx_of(p) }) else { return };
    // The ABI keeps the underline on; a plugin that wants a different
    // header has the text and line primitives to build its own.
    ctx.dl.module_title(
        ctx.fonts,
        x,
        y,
        w,
        px,
        text_in(left, left_len),
        text_in(right, right_len),
        color_in(c),
        true,
    );
}

// ---- ABI 5: the theme as tokens ------------------------------------------
// A plugin's ctx pointer is irrelevant to these — the resolved theme is the
// process's — but the parameter stays in the signatures so the calls read
// like every other HostApi entry and a future per-window theme has room.

extern "C" fn h_theme_token(name: *const u8, len: u32) -> u32 {
    if name.is_null() || len == 0 || len > 256 {
        return u32::MAX;
    }
    let bytes = unsafe { std::slice::from_raw_parts(name, len as usize) };
    match std::str::from_utf8(bytes).ok().and_then(crate::theme::id) {
        Some(t) => t.0 as u32,
        None => u32::MAX,
    }
}

fn tid(id: u32) -> crate::theme::TokenId {
    if id > u16::MAX as u32 {
        crate::theme::TokenId::MISSING
    } else {
        crate::theme::TokenId(id as u16)
    }
}

fn ccol(c: crate::theme::ThemeColor) -> ColorC {
    ColorC { r: c.r, g: c.g, b: c.b, a: c.a }
}

extern "C" fn h_theme_color(_p: *mut c_void, id: u32) -> ColorC {
    ccol(crate::theme::resolved().color(tid(id)))
}

extern "C" fn h_theme_bed(_p: *mut c_void, id: u32) -> ColorC {
    ccol(crate::theme::resolved().bed(tid(id)))
}

extern "C" fn h_theme_px(_p: *mut c_void, id: u32) -> f32 {
    crate::theme::resolved().px(tid(id))
}

extern "C" fn h_theme_flag(_p: *mut c_void, id: u32) -> u32 {
    crate::theme::resolved().flag(tid(id)) as u32
}

extern "C" fn h_theme_enum(_p: *mut c_void, id: u32) -> u32 {
    crate::theme::resolved().enum_of(tid(id)) as u32
}

extern "C" fn h_theme_class(name: *const u8, len: u32) -> u32 {
    if name.is_null() || len == 0 || len > 256 {
        return u32::MAX;
    }
    let bytes = unsafe { std::slice::from_raw_parts(name, len as usize) };
    match std::str::from_utf8(bytes).ok().and_then(crate::theme::class_id) {
        Some(c) => c as u32,
        None => u32::MAX,
    }
}

extern "C" fn h_theme_class_state(
    _p: *mut c_void,
    class: u32,
    state: u32,
    out: *mut StateStyleC,
    out_size: u32,
) -> u32 {
    if out.is_null() || out_size == 0 {
        return 0;
    }
    let st = if class <= u16::MAX as u32 && state < 7 {
        let s = crate::theme::parse::STATE_NAMES[state as usize];
        let s = crate::theme::parse::State::from_name(s).unwrap_or(crate::theme::parse::State::Idle);
        crate::theme::resolved().class_state(class as u16, s)
    } else {
        crate::theme::bake::StateStyle::RAW
    };
    let c = StateStyleC {
        fill: ccol(st.fill),
        edge: ccol(st.edge),
        text: ccol(st.text),
        glyph: ccol(st.glyph),
        edge_width: st.edge_width,
        glow_radius: st.glow_radius,
        glow_alpha: st.glow_alpha,
        elevation: st.elevation,
    };
    // Prefix write: an older caller with a smaller struct gets the front of
    // it, which is what lets StateStyleC grow by appending later.
    let n = (out_size as usize).min(std::mem::size_of::<StateStyleC>());
    unsafe {
        std::ptr::copy_nonoverlapping(&c as *const StateStyleC as *const u8, out as *mut u8, n);
    }
    n as u32
}

extern "C" fn h_theme_epoch(_p: *mut c_void) -> u32 {
    crate::theme::epoch()
}

extern "C" fn h_theme_base(p: *mut c_void) -> ColorC {
    let Some(ctx) = (unsafe { ctx_of(p) }) else {
        return ColorC { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };
    };
    color_out(ctx.theme.base)
}

extern "C" fn h_theme_bg(p: *mut c_void) -> ColorC {
    let Some(ctx) = (unsafe { ctx_of(p) }) else {
        return ColorC { r: 0.0, g: 0.0, b: 0.0, a: 1.0 };
    };
    color_out(ctx.theme.bg)
}

extern "C" fn h_vh(p: *mut c_void, v: f32) -> f32 {
    with_ctx!(p, ctx, ctx.vh(v))
}

extern "C" fn h_font_px(p: *mut c_void, v: f32) -> f32 {
    with_ctx!(p, ctx, ctx.font_px(v))
}

extern "C" fn h_mouse(p: *mut c_void, x: *mut f32, y: *mut f32) {
    let Some(ctx) = (unsafe { ctx_of(p) }) else { return };
    unsafe {
        if !x.is_null() {
            *x = ctx.mouse.0;
        }
        if !y.is_null() {
            *y = ctx.mouse.1;
        }
    }
}

extern "C" fn h_window(p: *mut c_void, w: *mut f32, h: *mut f32) {
    let Some(ctx) = (unsafe { ctx_of(p) }) else { return };
    unsafe {
        if !w.is_null() {
            *w = ctx.w;
        }
        if !h.is_null() {
            *h = ctx.h;
        }
    }
}

extern "C" fn h_elapsed(p: *mut c_void) -> f64 {
    with_ctx!(p, ctx, ctx.t)
}

/// The host data behind the opaque handle a plugin is given.
unsafe fn host_of<'a>(p: *const c_void) -> Option<&'a Host<'a>> {
    (p as *const Host).as_ref()
}

extern "C" fn h_shell_cwd(p: *const c_void, buf: *mut u8, cap: u32) -> u32 {
    let (Some(host), false) = (unsafe { host_of(p) }, buf.is_null()) else {
        return 0;
    };
    let Some(cwd) = host.shell_cwd.as_ref() else { return 0 };
    let bytes = cwd.as_os_str().as_encoded_bytes();
    let n = bytes.len().min(cap as usize);
    unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), buf, n) };
    n as u32
}

extern "C" fn h_emit_sound(event: u32) {
    if let Some(e) = crate::sound::Event::from_id(event) {
        crate::sound::emit(e);
    }
}

extern "C" fn h_panel_count() -> u32 {
    crate::base::panel_count() as u32
}

const CELL_BYTES: usize = std::mem::size_of::<CellC>();

/// A grid no window this program can open will reach. It exists so that
/// a nonsense rectangle cannot turn a division into a four-billion
/// iteration loop; `f32::max` returns the non-NaN operand, so the
/// `.max(2.0)` below already absorbs a NaN before the cast, which
/// saturates.
const GRID_MAX: f32 = 4096.0;

/// A polyline longer than any icon or frame in the interface. Without a
/// bound, `from_raw_parts` on a bad count is a multi-gigabyte read.
const POLYLINE_MAX: u32 = 8192;

fn cell_out(cell: &Cell, theme: &Theme) -> CellC {
    let (fg, bg) = crate::term::resolve(cell, theme);
    let mut flags = 0u32;
    if cell.flags & FLAG_UNDERLINE != 0 {
        flags |= CELL_UNDERLINE;
    }
    if bg.is_some() {
        flags |= CELL_HAS_BG;
    }
    CellC {
        ch: cell.ch as u32,
        flags,
        width: if cell.flags & FLAG_WIDE_SPACER != 0 {
            0
        } else if cell.flags & FLAG_WIDE_LEAD != 0 {
            2
        } else {
            1
        },
        font: crate::font::FONT_MONO,
        reserved: 0,
        fg: color_out(fg),
        // The FLAG says whether there is a background; this value is
        // only ever read when it does, so nothing depends on what goes
        // here when there is none.
        bg: color_out(bg.unwrap_or(theme.term_bg)),
    }
}

extern "C" fn h_term_view(
    hp: *const c_void,
    cp: *mut c_void,
    req: *const TermReqC,
    req_size: u32,
    out: *mut TermViewC,
    out_size: u32,
) -> u32 {
    if req.is_null() || out.is_null() {
        return 0;
    }
    if (req_size as usize) < TERM_REQ_SIZE_MIN || (out_size as usize) < TERM_VIEW_SIZE_MIN {
        return 0;
    }
    // The caller's structs may be shorter than this build's, so no
    // reference to either is ever formed: only the prefix both sides
    // agree on is copied, in each direction.
    let mut r = TermReqC::empty();
    unsafe {
        std::ptr::copy_nonoverlapping(
            req as *const u8,
            &mut r as *mut TermReqC as *mut u8,
            (req_size as usize).min(std::mem::size_of::<TermReqC>()),
        );
    }
    if r.session != 0 || r.flags != 0 {
        return 0;
    }
    let Some(ctx) = (unsafe { ctx_of(cp) }) else { return 0 };

    let px = (ctx.vh(1.45) * ctx.term_font_scale).max(8.0);
    let cell_w = ctx.fonts.mono_advance(px).max(1.0);
    let (ascent, line_h) = ctx.fonts.line_metrics(crate::font::FONT_MONO, px);
    let cell_h = line_h.max(1.0);
    let theme = ctx.theme;

    let mut v = TermViewC::empty();
    v.cell_w = cell_w;
    v.cell_h = cell_h;
    v.px = px;
    v.ascent = ascent;
    v.cols = (r.area.w / cell_w).floor().max(2.0).min(GRID_MAX) as u32;
    v.rows = (r.area.h / cell_h).floor().max(2.0).min(GRID_MAX) as u32;
    v.cursor_bg = color_out(theme.cursor);
    v.cursor_fg = color_out(theme.term_bg);
    v.cursor_ch = b' ' as u32;

    let mut written = 0u32;
    if let Some(host) = unsafe { host_of(hp) } {
        v.tab_count = host.tabs.len().min(32) as u32;
        for (i, on) in host.tabs.iter().take(32).enumerate() {
            if *on {
                v.tabs |= 1u32 << i;
            }
        }
        v.tab_active = host.tab_active.min(u32::MAX as usize) as u32;

        if let Some(term) = host.term {
            v.flags |= VIEW_LIVE;
            v.view_offset = term.view_offset.min(u32::MAX as usize) as u32;
            let vcols = (v.cols as usize).min(term.cols);
            let vrows = (v.rows as usize).min(term.rows);

            if term.cursor_visible && term.view_offset == 0 && term.cur_y < vrows {
                v.flags |= VIEW_CURSOR;
                v.cursor_col = term.cur_x.min(u32::MAX as usize) as u32;
                v.cursor_row = term.cur_y as u32;
                // Read here rather than out of the delivered cells: the
                // cursor may sit past `view_cols`, where the widget has
                // nothing to look at and has never clipped the block.
                v.cursor_ch = term
                    .view_row(term.cur_y)
                    .and_then(|row| row.get(term.cur_x))
                    .map(|c| c.ch as u32)
                    .unwrap_or(b' ' as u32);
            }

            let stride = r.cell_stride as usize;
            // The capacity is in BYTES, so whatever stride the caller
            // claims, `room * stride <= cells_bytes` — the two numbers
            // cannot disagree into a write past the end.
            let room = if r.cells.is_null() || r.cell_stride < CELL_SIZE_MIN {
                0
            } else {
                r.cells_bytes as usize / stride
            };
            let fit_rows = if vcols == 0 { 0 } else { (room / vcols).min(vrows) };
            let n = CELL_BYTES.min(stride);
            for y in 0..fit_rows {
                let row = term.view_row(y);
                for x in 0..vcols {
                    // Scrollback rows keep the width they scrolled off
                    // with — `resize` never touches them — so a short
                    // row is PADDED here rather than trusted anywhere.
                    // An absent cell draws nothing, which is exactly
                    // what breaking out of the row used to produce.
                    let c = match row.and_then(|rw| rw.get(x)) {
                        Some(cell) => cell_out(cell, theme),
                        None => CellC::absent(),
                    };
                    unsafe {
                        std::ptr::copy_nonoverlapping(
                            &c as *const CellC as *const u8,
                            (r.cells as *mut u8).add((y * vcols + x) * stride),
                            n,
                        );
                    }
                }
            }
            v.view_cols = vcols as u32;
            v.view_rows = fit_rows as u32;
            if fit_rows < vrows {
                v.flags |= VIEW_TRUNCATED;
            }
            written = (fit_rows * vcols).min(u32::MAX as usize) as u32;
        }
    }

    unsafe {
        std::ptr::copy_nonoverlapping(
            &v as *const TermViewC as *const u8,
            out as *mut u8,
            (out_size as usize).min(std::mem::size_of::<TermViewC>()),
        );
    }
    written
}

/// The interface handed to every plugin. Its address must stay valid for
/// as long as any plugin is loaded, which is why it is a static.
pub fn host_api() -> &'static HostApi {
    static API: HostApi = HostApi {
        abi_version: ABI_VERSION,
        emit_sound: h_emit_sound,
        panel_count: h_panel_count,
        rect: h_rect,
        rect_outline: h_rect_outline,
        quad: h_quad,
        line: h_line,
        polyline: h_polyline,
        text: h_text,
        measure: h_measure,
        module_title: h_module_title,
        theme_base: h_theme_base,
        theme_bg: h_theme_bg,
        vh: h_vh,
        font_px: h_font_px,
        mouse: h_mouse,
        window: h_window,
        elapsed: h_elapsed,
        shell_cwd: h_shell_cwd,
        term_view: h_term_view,
        theme_token: h_theme_token,
        theme_color: h_theme_color,
        theme_bed: h_theme_bed,
        theme_px: h_theme_px,
        theme_flag: h_theme_flag,
        theme_enum: h_theme_enum,
        theme_class: h_theme_class,
        theme_class_state: h_theme_class_state,
        theme_epoch: h_theme_epoch,
    };
    &API
}

fn action_in(a: &ActionC) -> Action {
    let bytes = || -> Vec<u8> {
        if a.data.is_null() || a.data_len == 0 {
            Vec::new()
        } else {
            unsafe { std::slice::from_raw_parts(a.data, a.data_len as usize) }.to_vec()
        }
    };
    let path = || -> PathBuf {
        PathBuf::from(String::from_utf8_lossy(&bytes()).into_owned())
    };
    match a.kind {
        ACTION_BYTES => Action::Bytes(bytes()),
        ACTION_OPEN_DIR => Action::OpenDir(path()),
        ACTION_OPEN_FILE => Action::OpenFile(path()),
        ACTION_SELECT_TAB => Action::SelectTab(a.index as usize),
        ACTION_EXIT => Action::Exit,
        ACTION_OPEN_SETTINGS => Action::OpenSettings,
        ACTION_SCROLL_TERMINAL => Action::ScrollTerminal(a.lines),
        _ => Action::None,
    }
}

fn empty_action() -> ActionC {
    ActionC {
        kind: ACTION_NONE,
        index: 0,
        lines: 0,
        data: std::ptr::null(),
        data_len: 0,
    }
}

/// A widget living in a loaded plugin.
///
/// The library itself is deliberately never unloaded: function pointers
/// into it stay reachable for the life of the program, and closing it
/// while any remain is how a plugin system earns a reputation for
/// mysterious crashes.
pub struct PluginWidget {
    api: PluginApi,
    instance: *mut c_void,
}

/// The stand-in for a [`PluginApi`] entry a plugin's table ends before:
/// no chrome, so the widget gets the plain container.
extern "C" fn chrome_absent(
    _: *mut c_void,
    _: *mut c_void,
    _: *const c_void,
    _: *mut ChromeC,
    _: u32,
) -> u32 {
    0
}

impl PluginWidget {
    /// Wraps a plugin's interface. None when the plugin reports an
    /// interface this build does not speak, or cannot make an instance.
    ///
    /// # Safety
    /// `api` must come from a plugin that is loaded and stays loaded.
    pub unsafe fn new(api: *const PluginApi) -> Option<PluginWidget> {
        if api.is_null() {
            return None;
        }
        // Only the two header words are read before the size is known:
        // a shorter table than this build's must never be dereferenced
        // whole, or the copy itself reads past the plugin's static.
        let head = api as *const u32;
        let version = unsafe { *head };
        if version != ABI_VERSION {
            eprintln!(
                "nacelle: plugin speaks interface version {version}, this build speaks {} \
                 — not loaded",
                ABI_VERSION
            );
            return None;
        }
        let size = (unsafe { *head.add(1) }) as usize;
        if size < PLUGIN_API_SIZE_MIN {
            eprintln!(
                "nacelle: plugin's interface table is {size} bytes, the version-{ABI_VERSION} \
                 minimum is {PLUGIN_API_SIZE_MIN} — not loaded"
            );
            return None;
        }
        // The prefix both sides agree on; optional entries the plugin's
        // table ends before are filled with their documented defaults.
        // Byte arithmetic, never a whole-struct read — dereferencing a
        // shorter table as `PluginApi` would itself read past its end.
        let take = size.min(std::mem::size_of::<PluginApi>());
        let mut slot = std::mem::MaybeUninit::<PluginApi>::uninit();
        let table = unsafe {
            std::ptr::copy_nonoverlapping(api as *const u8, slot.as_mut_ptr() as *mut u8, take);
            if size < PLUGIN_API_HAS_CHROME {
                std::ptr::addr_of_mut!((*slot.as_mut_ptr()).chrome).write(chrome_absent);
            }
            slot.assume_init()
        };
        let instance = (table.create)();
        if instance.is_null() {
            eprintln!("nacelle: plugin made no widget — not loaded");
            return None;
        }
        Some(PluginWidget { api: table, instance })
    }

    /// Whether the plugin's table reaches the `chrome` entry at all.
    fn has_chrome(&self) -> bool {
        self.api.api_size as usize >= PLUGIN_API_HAS_CHROME
    }
}

impl Drop for PluginWidget {
    fn drop(&mut self) {
        (self.api.destroy)(self.instance);
    }
}

impl Widget for PluginWidget {
    fn draw(&mut self, ctx: &mut Ctx, r: Rect, host: &Host) {
        let c = ctx as *mut Ctx as *mut c_void;
        let h = host as *const Host as *const c_void;
        (self.api.draw)(self.instance, c, h, rect_out(r));
    }

    fn chrome(&mut self, ctx: &mut Ctx, host: &Host) -> crate::widget::Chrome {
        use crate::widget::{ButtonSet, Chrome};
        if !self.has_chrome() {
            return Chrome::none();
        }
        let c = ctx as *mut Ctx as *mut c_void;
        let h = host as *const Host as *const c_void;
        let mut out = ChromeC::empty();
        let n = (self.api.chrome)(
            self.instance,
            c,
            h,
            &mut out,
            std::mem::size_of::<ChromeC>() as u32,
        );
        if n == 0 {
            return Chrome::none();
        }
        let title = text_in(out.title, out.title_len);
        let right = text_in(out.right, out.right_len);
        Chrome {
            title: (!title.is_empty()).then(|| title.to_string()),
            right: (!right.is_empty()).then(|| right.to_string()),
            buttons: match out.buttons {
                CHROME_BUTTONS_CLOSE => ButtonSet::Close,
                CHROME_BUTTONS_MIN_CLOSE => ButtonSet::MinClose,
                CHROME_BUTTONS_MIN_MAX_CLOSE => ButtonSet::MinMaxClose,
                // Unknown codes from a newer plugin mean nothing here.
                _ => ButtonSet::None,
            },
            severity: (out.severity != u32::MAX).then_some(out.severity),
        }
    }

    fn click(&mut self, x: f32, y: f32, r: Rect, host: &Host) -> Action {
        let mut out = empty_action();
        (self.api.click)(
            self.instance,
            x,
            y,
            rect_out(r),
            host.window.0,
            host.window.1,
            &mut out,
        );
        action_in(&out)
    }

    fn wheel(&mut self, dy: f32, r: Rect, host: &Host) -> Action {
        let mut out = empty_action();
        (self.api.wheel)(
            self.instance,
            dy,
            rect_out(r),
            host.window.0,
            host.window.1,
            &mut out,
        );
        action_in(&out)
    }

    fn grid(&self) -> Option<(usize, usize)> {
        let (mut c, mut r) = (0u32, 0u32);
        (self.api.grid)(self.instance, &mut c, &mut r);
        (c > 0 && r > 0).then_some((c as usize, r as usize))
    }

    fn sizing(&mut self, ctx: &mut Ctx, host: &Host) -> Sizing {
        let ctx_ptr = ctx as *mut Ctx as *mut c_void;
        let host_ptr = host as *const Host as *const c_void;
        let v = (self.api.sizing)(self.instance, ctx_ptr, host_ptr);
        if v.is_finite() && v > 0.0 {
            Sizing::Content(v)
        } else if v == SIZING_ROWS {
            Sizing::Rows
        } else {
            // Not finite, zero, or a value from a newer interface than
            // this build knows: the reference box is the answer that is
            // never wrong, only unremarkable.
            Sizing::Reference
        }
    }

    /// A plugin draws with baked token values: `h_theme_px` answers
    /// device pixels with no panel scale in them, deliberately — u2
    /// §2.12 keeps a control the same size wherever its panel is put.
    /// So a plugin's measured content does not shrink with its box, and
    /// the host must publish its `Content` want unscaled.
    fn scales_with_panel(&self) -> bool {
        false
    }

    fn key_feedback(&mut self, ch: Option<char>, label: Option<&str>) {
        let l = label.unwrap_or("");
        (self.api.key_feedback)(
            self.instance,
            ch.map(|c| c as u32).unwrap_or(0),
            l.as_ptr(),
            l.len() as u32,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Numbers a plugin passes index fixed arrays on this side, so a
    /// wrong one must be clamped rather than trusted. Getting this wrong
    /// is a read past the end of an array, from a typo in a widget.
    #[test]
    fn out_of_range_numbers_from_a_plugin_are_clamped() {
        assert_eq!(font_in(0), 0);
        assert_eq!(font_in(1), 1);
        // Past the end, and absurd, and wrapped: all land inside.
        assert_eq!(font_in(2), FONT_COUNT - 1);
        assert_eq!(font_in(9999), FONT_COUNT - 1);
        assert_eq!(font_in(u32::MAX), FONT_COUNT - 1);

        // A polyline count is a length for from_raw_parts, so the
        // guard has to reject before the slice is ever formed. A null
        // pointer with a huge count must simply do nothing.
        h_polyline(
            std::ptr::null_mut(),
            std::ptr::null(),
            u32::MAX,
            1.0,
            ColorC { r: 1.0, g: 1.0, b: 1.0, a: 1.0 },
            false,
        );
    }

    extern "C" fn t_create() -> *mut c_void {
        1 as *mut c_void
    }
    extern "C" fn t_create_none() -> *mut c_void {
        std::ptr::null_mut()
    }
    extern "C" fn t_destroy(_: *mut c_void) {}
    extern "C" fn t_draw(_: *mut c_void, _: *mut c_void, _: *const c_void, _: RectC) {}
    extern "C" fn t_click(
        _: *mut c_void, _: f32, _: f32, _: RectC, _: f32, _: f32, _: *mut ActionC,
    ) {}
    extern "C" fn t_wheel(_: *mut c_void, _: f32, _: RectC, _: f32, _: f32, _: *mut ActionC) {}
    extern "C" fn t_grid(_: *mut c_void, _: *mut u32, _: *mut u32) {}
    extern "C" fn t_key(_: *mut c_void, _: u32, _: *const u8, _: u32) {}
    extern "C" fn t_sizing(_: *mut c_void, _: *mut c_void, _: *const c_void) -> f32 {
        SIZING_ROWS
    }
    extern "C" fn t_chrome(
        _: *mut c_void,
        _: *mut c_void,
        _: *const c_void,
        out: *mut ChromeC,
        out_size: u32,
    ) -> u32 {
        static TITLE: &[u8] = b"FILESYSTEM";
        static RIGHT: &[u8] = b"/var/home";
        let Some(out) = (unsafe { out.as_mut() }) else { return 0 };
        out.title = TITLE.as_ptr();
        out.title_len = TITLE.len() as u32;
        out.right = RIGHT.as_ptr();
        out.right_len = RIGHT.len() as u32;
        (out_size as usize).min(std::mem::size_of::<ChromeC>()) as u32
    }

    fn t_api() -> PluginApi {
        PluginApi {
            abi_version: ABI_VERSION,
            api_size: std::mem::size_of::<PluginApi>() as u32,
            create: t_create,
            destroy: t_destroy,
            draw: t_draw,
            click: t_click,
            wheel: t_wheel,
            grid: t_grid,
            key_feedback: t_key,
            sizing: t_sizing,
            chrome: t_chrome,
        }
    }

    #[test]
    fn a_plugin_speaking_another_version_is_refused() {
        let mut api = t_api();
        api.abi_version = ABI_VERSION + 1;
        assert!(unsafe { PluginWidget::new(&api) }.is_none());
        assert!(unsafe { PluginWidget::new(std::ptr::null()) }.is_none());
        // The right version is accepted.
        api.abi_version = ABI_VERSION;
        assert!(unsafe { PluginWidget::new(&api) }.is_some());
    }

    #[test]
    fn a_plugin_that_makes_nothing_is_refused() {
        let api = PluginApi { create: t_create_none, ..t_api() };
        assert!(unsafe { PluginWidget::new(&api) }.is_none());
    }

    /// `api_size` is how the table grows without another version break:
    /// a plugin whose table ends before `chrome` gets the documented
    /// default (no chrome), one that reaches it is asked.
    #[test]
    fn a_shorter_table_means_no_chrome_a_full_one_answers() {
        use crate::runtime::{PLUGIN_API_HAS_CHROME, PLUGIN_API_SIZE_MIN};
        let short = PluginApi { api_size: PLUGIN_API_SIZE_MIN as u32, ..t_api() };
        let w = unsafe { PluginWidget::new(&short) }.expect("a pre-chrome table still loads");
        assert!(!w.has_chrome());

        let full = t_api();
        assert!(full.api_size as usize >= PLUGIN_API_HAS_CHROME);
        let w = unsafe { PluginWidget::new(&full) }.expect("full table loads");
        assert!(w.has_chrome());

        // A table shorter than the version's own minimum is refused.
        let broken = PluginApi { api_size: 8, ..t_api() };
        assert!(unsafe { PluginWidget::new(&broken) }.is_none());
    }

    #[test]
    fn actions_survive_the_crossing() {
        let bytes = b"hello";
        let a = ActionC {
            kind: ACTION_BYTES,
            index: 0,
            lines: 0,
            data: bytes.as_ptr(),
            data_len: 5,
        };
        assert_eq!(action_in(&a), Action::Bytes(b"hello".to_vec()));
        let t = ActionC { kind: ACTION_SELECT_TAB, index: 3, ..empty_action() };
        assert_eq!(action_in(&t), Action::SelectTab(3));
        let s = ActionC { kind: ACTION_SCROLL_TERMINAL, lines: -4, ..empty_action() };
        assert_eq!(action_in(&s), Action::ScrollTerminal(-4));
        // An unknown code is nothing, not a panic.
        assert_eq!(action_in(&ActionC { kind: 9999, ..empty_action() }), Action::None);
        // A null payload where bytes were promised yields no bytes.
        assert_eq!(
            action_in(&ActionC { kind: ACTION_BYTES, ..empty_action() }),
            Action::Bytes(Vec::new())
        );
    }
}
