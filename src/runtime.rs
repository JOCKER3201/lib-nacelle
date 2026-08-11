//! Process-wide state, and how a plugin shares the host's copy of it.
//!
//! Some state has to exist exactly once per running program — today
//! that is the queue of sound events. In a
//! single binary a static is the obvious way to hold it.
//!
//! A compiled plugin breaks that assumption. A `.so` widget links its
//! own copy of this toolkit, and that copy has its own statics. A plugin
//! calling `sound::emit` would push into a queue the host never drains:
//! no error, no log line, the sound simply never plays.
//!
//! Failures that leave no trace are the worst kind to chase, so the
//! shared state is reached through this module rather than touched
//! directly:
//!
//! * A copy owns the state until it is told otherwise, so an ordinary
//!   application needs to do nothing at all.
//! * When the application loads a plugin it calls the plugin's exported
//!   attach point with a [`HostApi`], and the plugin's copy calls
//!   [`attach`]. From then on that copy forwards instead of keeping its
//!   own state, and the plugin's `sound::emit` reaches the host's queue.
//! * A plugin that forgets to attach would silently become its own host
//!   again, so the decision is not left to it: the loader REFUSES to
//!   load a plugin that does not export the attach point. That is the
//!   enforcement, and it lives on the side that can be trusted to run.
//!
//! `HostApi` is `#[repr(C)]` and made of plain function pointers,
//! because it crosses a dynamic library boundary where Rust's own
//! layout guarantees do not apply.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};

/// A colour crossing the boundary. `Color` itself is a Rust type whose
/// layout is not promised; this one is.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ColorC {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

/// A rectangle crossing the boundary.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RectC {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// What a plugin asks the application to do, as a number and a payload,
/// because [`crate::Action`] is a Rust enum and its layout is not
/// something to rely on here. The codes match `Action`'s order.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ActionC {
    pub kind: u32,
    /// Tab index for SelectTab.
    pub index: u32,
    /// Scrollback lines for ScrollTerminal.
    pub lines: i32,
    /// Bytes or a path, owned by the plugin until the next call.
    pub data: *const u8,
    pub data_len: u32,
}

pub const ACTION_NONE: u32 = 0;
pub const ACTION_BYTES: u32 = 1;
pub const ACTION_OPEN_DIR: u32 = 2;
pub const ACTION_OPEN_FILE: u32 = 3;
pub const ACTION_SELECT_TAB: u32 = 4;
pub const ACTION_EXIT: u32 = 5;
pub const ACTION_OPEN_SETTINGS: u32 = 6;
pub const ACTION_SCROLL_TERMINAL: u32 = 7;

/// The functions a plugin uses to reach the host's shared state.
///
/// Adding a field to the END of this struct is compatible with plugins
/// built against an older version, as long as [`ABI_VERSION`] goes up
/// and the host keeps answering the old calls. Reordering or removing a
/// field is not: that is what the version check exists to catch.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct HostApi {
    /// Version of this interface, checked before anything else is used.
    pub abi_version: u32,
    /// Reports a sound event into the host's queue.
    pub emit_sound: extern "C" fn(event: u32),
    /// Number of registered widgets in the host.
    pub panel_count: extern "C" fn() -> u32,

    // --- drawing -------------------------------------------------
    //
    // The drawing context is passed as an opaque pointer: it holds Rust
    // references the plugin has no way to describe, so the host keeps
    // it and the plugin only names it. Every call below is cheap — a
    // function call, not an interpreter — which is why a plugin can
    // afford to draw a terminal cell by cell where a script cannot.
    pub rect: extern "C" fn(ctx: *mut c_void, r: RectC, c: ColorC),
    pub rect_outline: extern "C" fn(ctx: *mut c_void, r: RectC, t: f32, c: ColorC),
    /// Four corners, eight floats.
    pub quad: extern "C" fn(ctx: *mut c_void, pts: *const f32, c: ColorC),
    pub line:
        extern "C" fn(ctx: *mut c_void, x0: f32, y0: f32, x1: f32, y1: f32, t: f32, c: ColorC),
    /// A run of points, two floats each. `closed` joins the last back to
    /// the first — how the file browser's icons are drawn.
    pub polyline: extern "C" fn(
        ctx: *mut c_void,
        pts: *const f32,
        count: u32,
        t: f32,
        c: ColorC,
        closed: bool,
    ),
    /// `align`: 0 left, 1 centre, 2 right. `text` is UTF-8 with a length,
    /// not a C string, so a widget can draw anything a font can.
    pub text: extern "C" fn(
        ctx: *mut c_void,
        font: u32,
        px: f32,
        x: f32,
        y: f32,
        text: *const u8,
        len: u32,
        c: ColorC,
        spacing: f32,
        align: u32,
    ),
    pub measure: extern "C" fn(
        ctx: *mut c_void,
        font: u32,
        px: f32,
        text: *const u8,
        len: u32,
        spacing: f32,
    ) -> f32,
    pub module_title: extern "C" fn(
        ctx: *mut c_void,
        x: f32,
        y: f32,
        w: f32,
        px: f32,
        left: *const u8,
        left_len: u32,
        right: *const u8,
        right_len: u32,
        c: ColorC,
    ),

    // --- context -------------------------------------------------
    pub theme_base: extern "C" fn(ctx: *mut c_void) -> ColorC,
    pub theme_bg: extern "C" fn(ctx: *mut c_void) -> ColorC,
    pub vh: extern "C" fn(ctx: *mut c_void, v: f32) -> f32,
    pub font_px: extern "C" fn(ctx: *mut c_void, v: f32) -> f32,
    pub mouse: extern "C" fn(ctx: *mut c_void, x: *mut f32, y: *mut f32),
    pub window: extern "C" fn(ctx: *mut c_void, w: *mut f32, h: *mut f32),
    pub elapsed: extern "C" fn(ctx: *mut c_void) -> f64,

    // --- session --------------------------------------------------
    /// Working directory of the active shell, written into `buf` as
    /// UTF-8; returns its length, or 0 when there is none. Takes the
    /// HOST handle rather than the drawing one, because this is about
    /// the session rather than the frame.
    pub shell_cwd: extern "C" fn(host: *const c_void, buf: *mut u8, cap: u32) -> u32,

    // --- terminal -------------------------------------------------
    /// The visible terminal, resolved.
    ///
    /// Cells are written row-major, `view_cols` per row, `cell_stride`
    /// bytes apart, ragged scrollback rows padded with absent cells so
    /// addressing never desynchronises. Returns the number of cells
    /// written, and 0 when there is no terminal or the request is
    /// malformed.
    ///
    /// One call per frame. Fetching a cell at a time would repeat the
    /// whole view_row dispatch two hundred times a row for nothing;
    /// DRAWING a cell at a time is a different question and stays the
    /// widget's, because every glyph carries its own colour.
    ///
    /// Both sizes are passed by value rather than read out of the
    /// structs, so this never touches caller memory to learn how much
    /// caller memory it may touch.
    pub term_view: extern "C" fn(
        host: *const c_void,
        ctx: *mut c_void,
        req: *const TermReqC,
        req_size: u32,
        out: *mut TermViewC,
        out_size: u32,
    ) -> u32,

    // ------------------------------------------------------------------
    // ABI 5: the theme crosses the boundary as TOKENS, not as two colours.
    // Appended only — removing or reordering anything above would shift
    // every offset a compiled plugin memorised (see `attach`).
    // ------------------------------------------------------------------
    /// Resolves a dotted token name to a stable id, once, at attach or
    /// first use. u32::MAX = the master declares no such token; every
    /// accessor below answers the engine's raw default for it.
    pub theme_token: extern "C" fn(name: *const u8, len: u32) -> u32,
    /// A colour used as ink. Missing id: the raw mid grey.
    pub theme_color: extern "C" fn(ctx: *mut c_void, id: u32) -> ColorC,
    /// A colour used as a bed. Missing id: the raw near-black.
    pub theme_bed: extern "C" fn(ctx: *mut c_void, id: u32) -> ColorC,
    /// A length in device px, a scalar, a duration, an angle, a fraction —
    /// whatever the token's kind bakes to. Missing id: 0.0.
    pub theme_px: extern "C" fn(ctx: *mut c_void, id: u32) -> f32,
    pub theme_flag: extern "C" fn(ctx: *mut c_void, id: u32) -> u32,
    /// The index of the token's word in its declared enum list — the
    /// `enum: a | b | c` its master comment declares, in that order. A
    /// token declaring no list numbers its words in order of first use,
    /// the master's own value at 0.
    pub theme_enum: extern "C" fn(ctx: *mut c_void, id: u32) -> u32,
    /// Resolves an interaction class name ("button", "key", "tab") to its
    /// index in the baked class x state matrix. u32::MAX = no such class.
    pub theme_class: extern "C" fn(name: *const u8, len: u32) -> u32,
    /// One rung of the state ladder for one class, written whole into
    /// `out`. `state` indexes the seven states in declaration order (idle,
    /// hover, press, selected, selected_hover, dragging, disabled).
    /// Returns the bytes written; a caller with a smaller `out_size` gets
    /// a prefix, which is what lets this struct grow later.
    pub theme_class_state: extern "C" fn(
        ctx: *mut c_void,
        class: u32,
        state: u32,
        out: *mut StateStyleC,
        out_size: u32,
    ) -> u32,
    /// Bumped on every theme swap (reload, mood, resize). A plugin caching
    /// resolved values invalidates when this moves.
    pub theme_epoch: extern "C" fn(ctx: *mut c_void) -> u32,
}

/// What a plugin exports: how to make one of its widgets, draw it and
/// give it input. Returned from the attach point.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PluginApi {
    pub abi_version: u32,
    /// The plugin's own `sizeof(PluginApi)`. The host reads
    /// `min(its own, this)`: an entry past a plugin's size is treated as
    /// absent (`chrome` today answers [`crate::widget::Chrome::none`]),
    /// which is what lets this struct GROW by appending without another
    /// version break.
    pub api_size: u32,
    /// Creates one widget instance.
    pub create: extern "C" fn() -> *mut c_void,
    /// Destroys an instance. Never called while a frame is in flight.
    pub destroy: extern "C" fn(instance: *mut c_void),
    /// `host` is the opaque handle for the session data a widget may
    /// read; `ctx` is the drawing context. They are separate because
    /// one outlives the frame and the other does not.
    pub draw: extern "C" fn(
        instance: *mut c_void,
        ctx: *mut c_void,
        host: *const c_void,
        r: RectC,
    ),
    /// Input arrives outside a frame, so there is no drawing context to
    /// pass — which is why the window size is given directly. A widget
    /// that sizes itself against the window has to hit-test against the
    /// same number it drew with.
    pub click: extern "C" fn(
        instance: *mut c_void,
        x: f32,
        y: f32,
        r: RectC,
        win_w: f32,
        win_h: f32,
        out: *mut ActionC,
    ),
    pub wheel: extern "C" fn(
        instance: *mut c_void,
        dy: f32,
        r: RectC,
        win_w: f32,
        win_h: f32,
        out: *mut ActionC,
    ),
    /// The character grid the widget settled on, or zero for none.
    pub grid: extern "C" fn(instance: *mut c_void, cols: *mut u32, rows: *mut u32),
    /// A key pressed on the physical keyboard, for the on-screen one.
    pub key_feedback:
        extern "C" fn(instance: *mut c_void, ch: u32, label: *const u8, label_len: u32),
    /// How the widget answers being resized. Asked before the layout
    /// runs, which is why no height is passed: it is what the host is
    /// about to decide.
    ///
    /// The answer is one number, because the interface is C:
    ///
    /// * `> 0` — the height the content needs, at scale 1. The panel is
    ///   made exactly that tall and the content is scaled to fit it,
    ///   whichever axis runs out first.
    /// * [`SIZING_ROWS`] — grows downwards. The width decides how big
    ///   the rows are, the height how many.
    /// * anything else, [`SIZING_REFERENCE`] included — sized against
    ///   the reference box on both axes.
    pub sizing: extern "C" fn(
        instance: *mut c_void,
        ctx: *mut c_void,
        host: *const c_void,
    ) -> f32,
    /// What the host should draw around this widget this frame (u2 §4):
    /// the panel container's title band texts, controls and alarm state,
    /// prefix-written into `out`. Answers the bytes written; 0 = no
    /// chrome, and the widget gets the plain container. Asked once per
    /// frame, before `draw`, with the same host data.
    pub chrome: extern "C" fn(
        instance: *mut c_void,
        ctx: *mut c_void,
        host: *const c_void,
        out: *mut ChromeC,
        out_size: u32,
    ) -> u32,
}

/// The prefix of [`PluginApi`] every version-6 plugin must fill —
/// everything up to and including `sizing`. A table shorter than this is
/// refused; entries appended after it (`chrome` is the first) are
/// optional by `api_size`, absent ones answered by their documented
/// defaults.
pub const PLUGIN_API_SIZE_MIN: usize =
    std::mem::offset_of!(PluginApi, chrome);

/// The prefix that includes `chrome`; a plugin whose `api_size` reaches
/// this far declared the entry.
pub const PLUGIN_API_HAS_CHROME: usize =
    std::mem::offset_of!(PluginApi, chrome) + std::mem::size_of::<usize>();

/// The attach point every plugin must export:
///
/// ```ignore
/// #[no_mangle]
/// pub extern "C" fn nacelle_plugin_attach(host: *const HostApi) -> *const PluginApi
/// ```
///
/// It calls [`attach`] with the host interface and returns its own, or
/// null when the versions do not match.
pub type AttachFn = unsafe extern "C" fn(*const HostApi) -> *const PluginApi;

/// One character cell, already resolved against the theme.
///
/// What crosses is what gets painted, never an index into a palette the
/// plugin would have to keep its own copy of. The alternative would put
/// half the terminal emulator in a `.so`: that bold brightens an index
/// below 8 but not a Default or an Rgb, that dim multiplies BEFORE
/// inverse, that an unset background means no rectangle at all rather
/// than `term_bg` painted. Those are xterm semantics, defined by the
/// same specification as the escape sequence that produced them, and a
/// second copy of them is a shade that is quietly wrong on cells nobody
/// stares at.
///
/// Colours are [`ColorC`] rather than packed bytes because the renderer
/// works in `f32` throughout. Packing would be the one place the
/// pipeline narrows, and it would narrow permanently.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CellC {
    /// Unicode scalar value. Rebuild it with
    /// `char::from_u32(v).unwrap_or(' ')` — never a transmute, because
    /// an invalid scalar value is undefined behaviour, and never an
    /// `unwrap`, because a panic across `extern "C"` is a dead process.
    pub ch: u32,
    /// `CELL_*` bits. A bit this build does not know is ignored, not
    /// rejected — that is what makes adding one free.
    pub flags: u32,
    /// Columns the cell covers: 1 ordinarily, 2 for the first half of a
    /// double-width character, 0 for anything with nothing to draw —
    /// the second half of a pair, or a position no cell exists at.
    pub width: u8,
    /// Font id to pass to [`HostApi::text`]. The monospace font today;
    /// if the toolkit ever loads a bold face, bold cells start arriving
    /// with a different id and no plugin is rebuilt.
    pub font: u8,
    pub reserved: u16,
    pub fg: ColorC,
    /// Only meaningful with `CELL_HAS_BG`. A flag rather than a zero
    /// alpha, so a translucent cell background stays expressible; an
    /// overloaded sentinel cannot be taken back once it ships.
    pub bg: ColorC,
}

/// The cell is underlined.
pub const CELL_UNDERLINE: u32 = 1;
/// `bg` is a colour to paint. Without it the panel shows through, which
/// is what an unset SGR background has always meant here.
pub const CELL_HAS_BG: u32 = 2;
/// No cell exists at this position — a scrollback row that kept an
/// older, shorter width, or a row past the end of the view.
pub const CELL_ABSENT: u32 = 4;

impl CellC {
    /// A position with nothing in it. `width` 0 is what makes it draw
    /// nothing, matching the row the terminal view simply stopped
    /// drawing; `CELL_ABSENT` is what tells a later selection or copy
    /// feature that this is not a space anyone typed.
    pub const fn absent() -> CellC {
        const NIL: ColorC = ColorC { r: 0.0, g: 0.0, b: 0.0, a: 0.0 };
        CellC {
            ch: b' ' as u32,
            flags: CELL_ABSENT,
            width: 0,
            font: 0,
            reserved: 0,
            fg: NIL,
            bg: NIL,
        }
    }
}

/// The smallest `cell_stride` [`HostApi::term_view`] will fill. `CellC`
/// may GROW: the host writes `min(stride, its own size)` bytes per cell
/// and advances by the CALLER's stride, so a plugin built before a field
/// existed gets the prefix it knows, and one built after it keeps
/// whatever it initialised the tail to.
pub const CELL_SIZE_MIN: u32 = std::mem::size_of::<CellC>() as u32;

// The size is arithmetic a plugin performs, so it is pinned here rather
// than assumed. Four-byte alignment throughout means no interior
// padding on any target this program runs on.
const _: () = assert!(std::mem::size_of::<CellC>() == 44);
const _: () = assert!(std::mem::align_of::<CellC>() == 4);

/// What a widget asks [`HostApi::term_view`] for.
///
/// A struct rather than a parameter list because parameters cannot be
/// appended and fields can: a session index, a starting row for a split
/// view, a "unchanged since" hint all land here later for free.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct TermReqC {
    /// Pixel rectangle the grid is to fill. The HOST divides it, because
    /// the division needs font metrics no plugin can see and because its
    /// result is what the user's PTY is resized to.
    pub area: RectC,
    /// Where the cells go, row-major, `view_cols` per row.
    pub cells: *mut CellC,
    /// Capacity of `cells` in BYTES, not in cells. Bytes is what makes
    /// the two numbers unable to disagree: whatever `cell_stride` the
    /// caller claims, the host writes at most this many bytes.
    pub cells_bytes: u32,
    /// Distance between cells, at least [`CELL_SIZE_MIN`].
    pub cell_stride: u32,
    /// The active session. Reserved; anything but 0 is refused until it
    /// means something.
    pub session: u32,
    /// Reserved, must be 0.
    pub flags: u32,
}

impl TermReqC {
    /// A request with nowhere to put cells, for a caller that only wants
    /// the metrics back.
    pub const fn empty() -> TermReqC {
        TermReqC {
            area: RectC { x: 0.0, y: 0.0, w: 0.0, h: 0.0 },
            cells: std::ptr::null_mut(),
            cells_bytes: 0,
            cell_stride: 0,
            session: 0,
            flags: 0,
        }
    }
}

/// Everything about the terminal view that is not a cell.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct TermViewC {
    /// `VIEW_*` bits.
    pub flags: u32,
    /// The character grid that fits `area`. This is what the widget
    /// reports back through [`PluginApi::grid`], and what the PTY is
    /// resized to.
    pub cols: u32,
    pub rows: u32,
    /// The grid actually delivered: `cols`/`rows` clamped to what the
    /// terminal has. They differ for exactly one frame after a resize,
    /// because the reported grid is applied on the NEXT frame. Rows are
    /// `view_cols` cells apart.
    pub view_cols: u32,
    pub view_rows: u32,
    pub cell_w: f32,
    pub cell_h: f32,
    /// Glyph size for [`HostApi::text`].
    pub px: f32,
    /// Ascent at `px`, for a widget that wants to place something on the
    /// baseline itself. `text` already applies it.
    pub ascent: f32,
    /// Cursor cell, meaningful only with `VIEW_CURSOR`. `cursor_col` may
    /// sit outside `view_cols`: the block is deliberately not clipped to
    /// the grid, which is what the terminal view reports.
    pub cursor_col: u32,
    pub cursor_row: u32,
    /// The scalar under the cursor, so drawing it needs no lookup into a
    /// buffer that may not reach that far.
    pub cursor_ch: u32,
    /// Scrollback lines the view is scrolled up by; 0 when live.
    pub view_offset: u32,
    /// One bit per session tab, bit 0 = tab 0, set when occupied.
    pub tabs: u32,
    /// How many of those bits mean anything. A widget divides its tab
    /// strip by THIS, so the host can grow past five tabs without a
    /// rebuild.
    pub tab_count: u32,
    pub tab_active: u32,
    pub cursor_fg: ColorC,
    pub cursor_bg: ColorC,
}

impl TermViewC {
    /// Zeroed. A caller MUST start from this: a host older than the
    /// header the caller was built against leaves the tail untouched,
    /// and stack garbage read as a colour is the kind of failure that
    /// leaves no trace.
    pub const fn empty() -> TermViewC {
        const NIL: ColorC = ColorC { r: 0.0, g: 0.0, b: 0.0, a: 0.0 };
        TermViewC {
            flags: 0,
            cols: 0,
            rows: 0,
            view_cols: 0,
            view_rows: 0,
            cell_w: 0.0,
            cell_h: 0.0,
            px: 0.0,
            ascent: 0.0,
            cursor_col: 0,
            cursor_row: 0,
            cursor_ch: 0,
            view_offset: 0,
            tabs: 0,
            tab_count: 0,
            tab_active: 0,
            cursor_fg: NIL,
            cursor_bg: NIL,
        }
    }
}

/// There is a terminal. Without this the struct carries only metrics and
/// the tab strip, and the terminal view draws nothing at all.
pub const VIEW_LIVE: u32 = 1;
/// The cursor is visible, the view is live, and its row is inside the
/// delivered grid. Blinking is the widget's business.
pub const VIEW_CURSOR: u32 = 2;
/// Fewer rows were delivered than `view_rows` would have been, because
/// the buffer was too small. Grow it to `cols * rows` cells and ask
/// again.
pub const VIEW_TRUNCATED: u32 = 4;

/// The prefix of each struct that a v2 host understands. Expressed as an
/// offset rather than a literal so it stays right on every target — and
/// so that appending a field does not move it.
pub const TERM_REQ_SIZE_MIN: usize =
    std::mem::offset_of!(TermReqC, flags) + std::mem::size_of::<u32>();
pub const TERM_VIEW_SIZE_MIN: usize =
    std::mem::offset_of!(TermViewC, cursor_bg) + std::mem::size_of::<ColorC>();

/// Version of the plugin interface. Raised whenever [`HostApi`] changes
/// in a way an existing plugin would not survive.
/// One baked rung of the state ladder, as it crosses the boundary. Prefix-
/// writable: the host writes min(out_size, size_of) bytes, so fields may only
/// ever be APPENDED here, exactly like the api structs themselves.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct StateStyleC {
    pub fill: ColorC,
    pub edge: ColorC,
    pub text: ColorC,
    pub glyph: ColorC,
    pub edge_width: f32,
    pub glow_radius: f32,
    pub glow_alpha: f32,
    pub elevation: f32,
}

// 6: `PluginApi` gained `api_size` (so the host can measure a plugin's
// table) and the appended `chrome` entry — the host-drawn container of
// u2 §4. Inserting `api_size` moved every later field, which is exactly
// the break the version number exists to name; from here on an APPENDED
// `PluginApi` entry needs no bump, because `api_size` says how much of
// the table a plugin actually filled.
pub const ABI_VERSION: u32 = 6;

/// A widget's container declaration, crossing the boundary (u2 §4.3).
///
/// The host asks [`PluginApi::chrome`] once per frame, before `draw`;
/// the plugin prefix-writes this and answers the bytes written, 0 for
/// "no chrome". The two strings live in the PLUGIN, valid until its next
/// `chrome` call — the same discipline the click path's `last_path`
/// already uses. The `{version, size}` header is what lets fields be
/// APPENDED later without a new ABI break.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ChromeC {
    /// [`CHROME_VERSION`] of the writer.
    pub version: u32,
    /// The writer's own sizeof; readers touch `min(their own, this)`.
    pub size: u32,
    /// The title band's left text (UTF-8, not a C string); null = none.
    pub title: *const u8,
    pub title_len: u32,
    /// The band's right text — a cwd, a model name; null = none. The
    /// HOST trims it to the room the title leaves.
    pub right: *const u8,
    pub right_len: u32,
    /// `CHROME_BUTTONS_*`. Declared, not yet drawn by the host.
    pub buttons: u32,
    /// Index into the severity set, or `u32::MAX` for none.
    pub severity: u32,
}

pub const CHROME_VERSION: u32 = 1;
pub const CHROME_BUTTONS_NONE: u32 = 0;
pub const CHROME_BUTTONS_CLOSE: u32 = 1;
pub const CHROME_BUTTONS_MIN_CLOSE: u32 = 2;
pub const CHROME_BUTTONS_MIN_MAX_CLOSE: u32 = 3;

/// The prefix of [`ChromeC`] every reader understands.
pub const CHROME_SIZE_MIN: usize =
    std::mem::offset_of!(ChromeC, severity) + std::mem::size_of::<u32>();

impl ChromeC {
    /// No chrome at all. A caller MUST start from this, so a writer
    /// built against a shorter struct leaves a well-defined tail.
    pub const fn empty() -> ChromeC {
        ChromeC {
            version: CHROME_VERSION,
            size: std::mem::size_of::<ChromeC>() as u32,
            title: std::ptr::null(),
            title_len: 0,
            right: std::ptr::null(),
            right_len: 0,
            buttons: CHROME_BUTTONS_NONE,
            severity: u32::MAX,
        }
    }
}

/// [`PluginApi::sizing`]: the widget grows downwards.
pub const SIZING_ROWS: f32 = -1.0;
/// [`PluginApi::sizing`]: the widget is sized against its reference box.
pub const SIZING_REFERENCE: f32 = -2.0;

/// The name a plugin must export for the host to attach it. A library
/// without it is not a widget plugin and is not loaded.
pub const ATTACH_SYMBOL: &[u8] = b"nacelle_plugin_attach";

static API: AtomicPtr<HostApi> = AtomicPtr::new(std::ptr::null_mut());
static ATTACHED: AtomicBool = AtomicBool::new(false);
static WARNED: AtomicBool = AtomicBool::new(false);

/// Whether this copy owns the shared state. True until it is attached
/// to a host, so an application is the host by simply existing and a
/// plugin becomes a forwarder the moment it attaches.
pub fn is_host() -> bool {
    !ATTACHED.load(Ordering::Acquire)
}

/// Points this copy at the host's state. A plugin calls this from its
/// attach point; the pointer must stay valid for the program's life,
/// which it does because the host owns it.
///
/// Returns false when the interface versions do not match, in which case
/// nothing is attached and the plugin must not be used.
///
/// # Safety
/// `api` must point at a `HostApi` the host keeps alive.
pub unsafe fn attach(api: *const HostApi) -> bool {
    // A NEWER host is fine: fields are only ever appended, so every
    // entry this copy was built against is still where it was. An older
    // one is not — the entries it expects may not be there at all, and
    // reading one runs off the end of the host's table. This is the
    // check the header has always promised and never performed.
    if api.is_null() || (*api).abi_version < ABI_VERSION {
        return false;
    }
    API.store(api as *mut HostApi, Ordering::Release);
    ATTACHED.store(true, Ordering::Release);
    true
}

/// The host's interface, if this copy is attached to one.
fn api() -> Option<&'static HostApi> {
    let p = API.load(Ordering::Acquire);
    if p.is_null() {
        None
    } else {
        // The host owns this for the life of the program.
        Some(unsafe { &*p })
    }
}

/// Says once that this copy is orphaned. Called on the paths where a
/// plugin would otherwise fail silently.
fn warn_detached(what: &str) {
    if !WARNED.swap(true, Ordering::Relaxed) {
        eprintln!(
            "nacelle: {what} was called by a copy of the toolkit that attached \
             to a host and then lost it — the call is being dropped."
        );
    }
}

/// Routes a shared-state call: runs `local` in the host, forwards
/// through the host interface in an attached plugin, and complains
/// exactly once in a copy that is neither.
pub(crate) fn shared<T>(
    what: &str,
    local: impl FnOnce() -> T,
    forwarded: impl FnOnce(&HostApi) -> T,
    dropped: T,
) -> T {
    if is_host() {
        return local();
    }
    match api() {
        Some(api) => forwarded(api),
        None => {
            warn_detached(what);
            dropped
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An application is the host without saying anything, which is
    /// what keeps the ordinary case free of ceremony.
    #[test]
    fn a_plain_program_owns_its_state() {
        assert!(is_host());
    }

    #[test]
    fn an_older_interface_refuses_to_attach() {
        // The version is checked before any other field is touched, so
        // a table of the right shape with the wrong number is enough.
        let mut wrong = *crate::plugin::host_api();
        wrong.abi_version = ABI_VERSION - 1;
        assert!(!unsafe { attach(&wrong) }, "an older interface must be refused");
        assert!(!unsafe { attach(std::ptr::null()) }, "null must be refused");
        // Refusing to attach must leave the copy owning its own state
        // rather than half-attached to something it cannot speak to.
        assert!(api().is_none());
        assert!(is_host());
    }
}
