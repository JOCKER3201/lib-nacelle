//! Terminal emulation: character grid + VT sequence handling (parser: vte).

use std::collections::VecDeque;
use crate::theme::{self, Color, TokenId};
use std::sync::OnceLock;
use unicode_width::UnicodeWidthChar;

fn tok(cell: &'static OnceLock<TokenId>, name: &'static str) -> TokenId {
    *cell.get_or_init(|| theme::id(name).unwrap_or(TokenId::MISSING))
}

pub const FLAG_BOLD: u8 = 1;
pub const FLAG_UNDERLINE: u8 = 2;
pub const FLAG_INVERSE: u8 = 4;
pub const FLAG_DIM: u8 = 8;
/// Second cell of a double-width character.
pub const FLAG_WIDE_SPACER: u8 = 16;
/// FIRST cell of a double-width character.
///
/// Recorded when the character is written, rather than worked out later
/// from "the next cell is a spacer". Editing sequences move cells
/// around — ICH, DCH and ECH all do it — and a lead whose spacer they
/// took away is still a wide character. Asking the next cell gets that
/// one wrong, and it is the ordinary case in an editor, not a corner.
pub const FLAG_WIDE_LEAD: u8 = 32;

/// Final colours of a cell: the foreground, and a background only when
/// one was actually set.
///
/// This lives with the emulation rather than with whatever draws it,
/// because every rule in it comes from the same specification as the
/// escape sequence that set the colour — that bold may brighten one of
/// the first eight indices but not an explicit colour, that dim applies
/// before inverse, that an unset background means nothing is painted
/// rather than the terminal's own background. A second copy of those
/// rules elsewhere is a shade that is quietly wrong. How far each rule
/// reaches — the dim factor, whether bold brightens, what inverse does —
/// belongs to the theme.
pub fn resolve(cell: &Cell) -> (Color, Option<Color>) {
    let t = theme::resolved();
    let mut fg = match cell.fg {
        CellColor::Default => t.color(theme::ids::term_fg()),
        CellColor::Indexed(i) => {
            static BOLD_IS_BRIGHT: OnceLock<TokenId> = OnceLock::new();
            let i = if cell.flags & FLAG_BOLD != 0
                && i < 8
                && t.flag(tok(&BOLD_IS_BRIGHT, "term.bold_is_bright"))
            {
                i + 8
            } else {
                i
            };
            indexed(i)
        }
        CellColor::Rgb(r, g, b) => Color::rgb8(r, g, b),
    };
    let mut bg = match cell.bg {
        CellColor::Default => None,
        CellColor::Indexed(i) => Some(indexed(i)),
        CellColor::Rgb(r, g, b) => Some(Color::rgb8(r, g, b)),
    };
    if cell.flags & FLAG_DIM != 0 {
        static DIM_FACTOR: OnceLock<TokenId> = OnceLock::new();
        static DIM_FLOOR: OnceLock<TokenId> = OnceLock::new();
        // The floor is a legibility guarantee: a theme may soften dim,
        // never fade output out of existence.
        let f = t
            .px(tok(&DIM_FACTOR, "term.dim_factor"))
            .max(t.px(tok(&DIM_FLOOR, "term.dim_factor_floor")));
        fg = fg.dim(f);
    }
    if cell.flags & FLAG_INVERSE != 0 {
        static INVERSE_MODE: OnceLock<TokenId> = OnceLock::new();
        static TINT_WORD: OnceLock<Option<u16>> = OnceLock::new();
        let mode = tok(&INVERSE_MODE, "term.inverse_mode");
        let tint = *TINT_WORD.get_or_init(|| theme::enum_index(mode, "tint"));
        if tint == Some(t.enum_of(mode)) {
            // tint: the glyph keeps its colour and the cell is washed the
            // way a selection is — SGR 7 regions in `less` or `fzf` ARE
            // selections, and term.selection is the declared wash.
            static SELECTION: OnceLock<TokenId> = OnceLock::new();
            let wash = t.color(tok(&SELECTION, "term.selection"));
            bg = Some(match bg {
                Some(b) => Color::over(wash, b),
                None => wash,
            });
        } else {
            // swap — and any word this build does not know falls back here,
            // because an unreadable inverse cell is worse than a plain one.
            let old_fg = fg;
            fg = bg.unwrap_or_else(|| t.color(theme::ids::term_bg()));
            bg = Some(old_fg);
        }
    }
    (fg, bg)
}

/// The 256-colour lookup with the theme's tint applied to the generated
/// range. `term.ansi.cube_tint` / `term.ansi.grey_tint` pull the xterm
/// arithmetic (indices 16..=255) toward the accent hue the way
/// `term.ansi.pull` moves the sixteen — without them a theme's tint stops
/// dead at index 15 and a modern TUI snaps back to foreign colour.
/// Explicit truecolour is never touched; neither is the sixteen, which the
/// theme already owns outright.
fn indexed(i: u8) -> Color {
    let t = theme::resolved();
    if i < 16 {
        // The sixteen are the theme's own, token by token.
        return t.color(theme::ids::term_ansi(i as usize));
    }
    let c = xterm_gen(i);
    static CUBE_TINT: OnceLock<TokenId> = OnceLock::new();
    static GREY_TINT: OnceLock<TokenId> = OnceLock::new();
    let pull = if i >= 232 {
        t.px(tok(&GREY_TINT, "term.ansi.grey_tint"))
    } else {
        t.px(tok(&CUBE_TINT, "term.ansi.cube_tint"))
    }
    .clamp(0.0, 1.0);
    if pull <= 0.0 {
        return c;
    }
    static HUE_ACCENT: OnceLock<TokenId> = OnceLock::new();
    static CHROMA_ACCENT: OnceLock<TokenId> = OnceLock::new();
    let hue = t.px(tok(&HUE_ACCENT, "hue.accent"));
    // xterm_gen hands the generated range back sRGB-encoded; OKLab wants
    // linear light, and the caller expects the encoding it gave us.
    let mut p = c.to_linear().to_oklch();
    if p.c < 1e-4 {
        // A neutral has no hue to walk: pulling it toward the accent
        // means granting it accent chroma, scaled by the pull.
        p.h = hue;
        p.c = pull * t.px(tok(&CHROMA_ACCENT, "chroma.accent"));
    } else {
        // Walk the hue toward the accent along the shorter arc.
        let mut d = (hue - p.h).rem_euclid(360.0);
        if d > 180.0 {
            d -= 360.0;
        }
        p.h += d * pull;
    }
    Color::from_oklch(p).to_srgb()
}

/// The generated half of the xterm 256-colour palette — the 6x6x6 colour
/// cube (16..=231) and the grey ramp (232..=255), exactly as xterm
/// computes them. The first sixteen are the theme's own and never come
/// here.
fn xterm_gen(idx: u8) -> Color {
    match idx {
        16..=231 => {
            let i = idx as u32 - 16;
            let steps = [0u8, 95, 135, 175, 215, 255];
            let r = steps[(i / 36) as usize];
            let g = steps[((i / 6) % 6) as usize];
            let b = steps[(i % 6) as usize];
            Color::rgb8(r, g, b)
        }
        _ => {
            let v = 8 + (idx as u32).saturating_sub(232) * 10;
            Color::rgb8(v as u8, v as u8, v as u8)
        }
    }
}

// ------------------------------------------------------ the cell's measure

/// The widest grid this build will report, in either axis.
///
/// Not a look and not a theme's business: the cells cross the plugin ABI
/// in one buffer, and a window whose font measured almost nothing must
/// not be able to ask for a hundred thousand columns of it. A theme that
/// wants a denser grid says so with `terminal.cell_font`.
const GRID_MAX: f32 = 4096.0;

/// The most cells this build will report at once, across both axes.
///
/// [`GRID_MAX`] bounds each axis on its own, which was enough while the
/// cell's size was arithmetic in the ABI with a floor written beside it.
/// It is not enough now that the size is the THEME's: a theme is a
/// user's file, `terminal.min_px` is that file asking ITSELF for a
/// readable cell, and a file that asks for none leaves a cell of one
/// device pixel — 3840 by 2160 of them on a 4K screen, eight million
/// cells that every widget downstream allocates and walks once a frame.
/// So the pair is bounded too, and this is the engine's own floor under
/// the cell rather than the theme's.
///
/// A million cells is past any screen a person owns, so it can only bite
/// on a grid nobody could read: measured against the master's own
/// `terminal.min_px` of 8px, whose cell is 4.8 by 10.56 px, a 4K screen
/// asks for 163 000 cells and an 8K screen for 654 000.
const GRID_CELLS_MAX: f32 = (1u32 << 20) as f32;

/// One terminal cell, measured from the theme.
///
/// The emulator counts columns and rows, everything that draws counts
/// pixels, and this is the single conversion between the two — so there
/// is one place a theme can move it from. Three tokens decide it:
///
/// * `terminal.cell_font` — the size the cell's face is set at. It is
///   the BASE, not the answer: the user's `TermFontSize=` multiplier
///   rides on top of it, because a preference scales what the theme
///   chose rather than standing in for it.
/// * `terminal.min_px` — the floor the THEME puts under that product, so
///   that a theme which writes a tiny size (or a user who scales one
///   down) still leaves a grid a person can read. It is a request the
///   theme makes of itself and nothing more: a file may write `0px`, and
///   the promise this type can keep about that one is not readability
///   but `GRID_CELLS_MAX` below — the engine refusing to report a grid
///   no screen could show. §5.25 spells this key without the `cell_font`
///   prefix that §3.2's `_min_px` law would need for the bake to pair
///   the two automatically, so it is applied by hand here; the master's
///   own TODO records the spelling and who has to pick.
/// * `terminal.line_height` — a multiplier on the FONT's own line box,
///   never a synthetic figure. At 1.0 the grid is exactly the metrics
///   the face declares, which is what a grid that has to agree with the
///   PTY wants; a theme that wants air between rows writes more, and the
///   air is shared above and below, so the glyph keeps the middle of its
///   row and everything hung off the glyph — its underline, the caret's
///   — travels with it. The share is done where the cells are drawn,
///   which is the widget: this type reports the row's full height, and
///   the widget divides it by the same token to find the line box back.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grid {
    /// The px the cell's glyphs are rasterised at.
    pub px: f32,
    /// One cell's width: the mono face's own advance at `px`.
    pub cell_w: f32,
    /// One cell's height: the face's line box times `terminal.line_height`.
    pub cell_h: f32,
    /// The baseline's drop from the top of the FACE's line box, at `px`.
    /// Where that box sits inside a cell taller than itself is the
    /// widget's arithmetic, not this number's.
    pub ascent: f32,
}

impl Grid {
    /// Measure the grid this theme asks for. `user_scale` is the user's
    /// own `TermFontSize=` multiplier, which stands ABOVE the token.
    pub fn measure(fonts: &mut crate::font::FontSystem, user_scale: f32) -> Grid {
        static CELL_FONT: OnceLock<TokenId> = OnceLock::new();
        static MIN_PX: OnceLock<TokenId> = OnceLock::new();
        static LINE_HEIGHT: OnceLock<TokenId> = OnceLock::new();
        let t = theme::resolved();
        let px = (t.px(tok(&CELL_FONT, "terminal.cell_font")) * user_scale)
            .max(t.px(tok(&MIN_PX, "terminal.min_px")));
        let (ascent, line_h) = fonts.line_metrics(crate::font::FONT_MONO, px);
        Grid {
            px,
            // The two floors are arithmetic, not a minimum size: `span`
            // divides by these numbers, and a cell narrower than one
            // pixel is a division a grid cannot survive.
            cell_w: fonts.mono_advance(px).max(1.0),
            cell_h: (line_h * t.px(tok(&LINE_HEIGHT, "terminal.line_height"))).max(1.0),
            ascent,
        }
    }

    /// The grid that fits a rectangle: whole columns across `w`, whole
    /// rows down `h`.
    ///
    /// Both axes are answered together because the bound that matters is
    /// on the pair. A rectangle that would take more than
    /// `GRID_CELLS_MAX` cells is answered with a grid of the same
    /// shape, scaled down until it fits — a grid that no longer covers
    /// the window, which is the honest answer to a cell size that could
    /// not have covered it legibly either.
    pub fn span(&self, w: f32, h: f32) -> (u32, u32) {
        let cols = axis(w, self.cell_w);
        let rows = axis(h, self.cell_h);
        // f32 all the way: 4096 * 4096 overflows nothing here, and the
        // scale has to be a ratio anyway.
        let cells = cols as f32 * rows as f32;
        if cells <= GRID_CELLS_MAX {
            return (cols, rows);
        }
        let s = (GRID_CELLS_MAX / cells).sqrt();
        (whole(cols as f32 * s), whole(rows as f32 * s))
    }
}

/// How many whole cells of `cell` pixels fit across `extent`.
fn axis(extent: f32, cell: f32) -> u32 {
    whole((extent / cell).floor())
}

/// A grid is at least two cells on an axis — one column is not a
/// terminal, and zero is a size the emulator divides by — and never more
/// than `GRID_MAX`. Written as `max` then `min` rather than `clamp`
/// because a measurement that came out NaN must land on the floor, and
/// `clamp` panics on it where `f32::max` answers the other operand.
fn whole(n: f32) -> u32 {
    n.floor().max(2.0).min(GRID_MAX) as u32
}

#[derive(Clone, Copy, PartialEq)]
pub enum CellColor {
    Default,
    Indexed(u8),
    Rgb(u8, u8, u8),
}

#[derive(Clone, Copy)]
pub struct Cell {
    pub ch: char,
    pub fg: CellColor,
    pub bg: CellColor,
    pub flags: u8,
}

impl Cell {
    fn blank(bg: CellColor) -> Self {
        Cell {
            ch: ' ',
            fg: CellColor::Default,
            bg,
            flags: 0,
        }
    }
}

/// What a selection is made of: cells as dragged, whole words, whole
/// lines — the double- and triple-click kinds every terminal has.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SelKind {
    Cells,
    Words,
    Lines,
}

/// A selection over the terminal's text.
///
/// Coordinates are `(line id, column)` where the line id is MONOTONIC —
/// [`Term::line_id_of_view_row`] — never a view row or a grid row. The
/// scrollback is a `VecDeque` that trims from the front; any index
/// stored across frames without this translation is a future off-by-N.
#[derive(Clone, Copy, Debug)]
pub struct Selection {
    pub anchor: (u64, usize),
    pub head: (u64, usize),
    pub kind: SelKind,
}

impl Selection {
    /// The endpoints ordered by (line, column) — reading order.
    fn ordered(&self) -> ((u64, usize), (u64, usize)) {
        if self.head < self.anchor {
            (self.head, self.anchor)
        } else {
            (self.anchor, self.head)
        }
    }
}

#[derive(Clone, Copy)]
struct Pen {
    fg: CellColor,
    bg: CellColor,
    flags: u8,
}

impl Pen {
    fn default() -> Self {
        Pen {
            fg: CellColor::Default,
            bg: CellColor::Default,
            flags: 0,
        }
    }
}

pub struct Term {
    pub cols: usize,
    pub rows: usize,
    screen: Vec<Vec<Cell>>,
    alt_screen: Vec<Vec<Cell>>,
    pub scrollback: VecDeque<Vec<Cell>>,
    pub cur_x: usize,
    pub cur_y: usize,
    saved_cursor: (usize, usize, Pen),
    pen: Pen,
    scroll_top: usize,
    scroll_bottom: usize,
    pub alt_active: bool,
    pub cursor_visible: bool,
    pub app_cursor: bool,
    wrap_pending: bool,
    autowrap: bool,
    /// View scrolled up (number of scrollback lines).
    pub view_offset: usize,
    /// Responses to send to the PTY (DA, CPR etc.).
    pub responses: Vec<u8>,
    /// Total lines ever pushed off the top of the main screen into the
    /// scrollback. Never decremented — a trim or an `ESC[3J` forgets
    /// CONTENT, not history — which is what makes a line id monotonic
    /// for the life of the session.
    scrolled_total: u64,
    /// The selection, in monotonic line ids (see [`Selection`]).
    pub selection: Option<Selection>,
    /// DECSET/DECRST 2004: wrap pastes in `ESC[200~ … ESC[201~`.
    pub bracketed_paste: bool,
}

impl Term {
    pub fn new(cols: usize, rows: usize) -> Self {
        let cols = cols.max(2);
        let rows = rows.max(2);
        Term {
            cols,
            rows,
            screen: vec![vec![Cell::blank(CellColor::Default); cols]; rows],
            alt_screen: vec![vec![Cell::blank(CellColor::Default); cols]; rows],
            scrollback: VecDeque::new(),
            cur_x: 0,
            cur_y: 0,
            saved_cursor: (0, 0, Pen::default()),
            pen: Pen::default(),
            scroll_top: 0,
            scroll_bottom: rows - 1,
            alt_active: false,
            cursor_visible: true,
            app_cursor: false,
            wrap_pending: false,
            autowrap: true,
            view_offset: 0,
            responses: Vec::new(),
            scrolled_total: 0,
            selection: None,
            bracketed_paste: false,
        }
    }

    fn grid(&self) -> &Vec<Vec<Cell>> {
        if self.alt_active { &self.alt_screen } else { &self.screen }
    }

    fn grid_mut(&mut self) -> &mut Vec<Vec<Cell>> {
        if self.alt_active { &mut self.alt_screen } else { &mut self.screen }
    }

    pub fn resize(&mut self, cols: usize, rows: usize) {
        let cols = cols.max(2);
        let rows = rows.max(2);
        if cols == self.cols && rows == self.rows {
            return;
        }
        // A resize reflows nothing (rows are cut or padded), but the
        // cells a selection meant may no longer be where it points —
        // the conservative first-cut rule clears it.
        self.selection = None;
        for grid in [&mut self.screen, &mut self.alt_screen] {
            for row in grid.iter_mut() {
                row.resize(cols, Cell::blank(CellColor::Default));
            }
            while grid.len() < rows {
                grid.push(vec![Cell::blank(CellColor::Default); cols]);
            }
            while grid.len() > rows {
                grid.pop();
            }
        }
        self.cols = cols;
        self.rows = rows;
        self.scroll_top = 0;
        self.scroll_bottom = rows - 1;
        self.cur_x = self.cur_x.min(cols - 1);
        self.cur_y = self.cur_y.min(rows - 1);
        self.wrap_pending = false;
    }

    pub fn scroll_view(&mut self, delta: i32) {
        if delta > 0 {
            self.view_offset =
                (self.view_offset + delta as usize).min(self.scrollback.len());
        } else {
            self.view_offset = self.view_offset.saturating_sub((-delta) as usize);
        }
    }

    /// Line visible on screen, accounting for scrolling.
    pub fn view_row(&self, y: usize) -> Option<&Vec<Cell>> {
        if self.view_offset == 0 || self.alt_active {
            return self.grid().get(y);
        }
        let sb = self.scrollback.len();
        // Defensive: view_offset is normally clamped to scrollback.len(),
        // but if the scrollback shrinks (e.g. cleared) the invariant can
        // lag by a frame — saturating_sub avoids an unsigned underflow.
        let start = sb.saturating_sub(self.view_offset);
        if start + y < sb {
            self.scrollback.get(start + y)
        } else {
            self.grid().get(start + y - sb)
        }
    }

    // ---- selection, in monotonic line ids ---------------------------
    //
    // The only public selection coordinates are line ids: an id names a
    // LINE OF CONTENT and follows it from the screen into the scrollback
    // and out of it, so trim and append never shift a selection. All the
    // span logic sits in `selection_span_on_line`, and both consumers —
    // the copied text and the drawn wash — read the same answer, which
    // is what keeps one authority over what "selected" means.

    /// The monotonic id of the line shown on view row `y`. Uniform
    /// across the scrollback boundary: the id of view row 0 plus `y`.
    pub fn line_id_of_view_row(&self, y: usize) -> u64 {
        (self.scrolled_total + y as u64).saturating_sub(self.view_offset as u64)
    }

    /// The line a monotonic id names now — a MAIN-screen row or a
    /// retained scrollback row. None once it is trimmed away, and always
    /// None on the alt screen, which has no stable lines to name.
    fn row_of_line(&self, id: u64) -> Option<&Vec<Cell>> {
        if id >= self.scrolled_total {
            if self.alt_active {
                return None;
            }
            self.screen.get((id - self.scrolled_total) as usize)
        } else {
            let sb = self.scrollback.len() as u64;
            let first = self.scrolled_total - sb;
            if id < first {
                None
            } else {
                self.scrollback.get((id - first) as usize)
            }
        }
    }

    /// Whether the selection reaches into the live screen — the lines a
    /// feed can still move. Scrollback-only selections are settled.
    fn selection_touches_screen(&self) -> bool {
        self.selection
            .map_or(false, |s| s.anchor.0.max(s.head.0) >= self.scrolled_total)
    }

    /// Starts a selection: anchor and head on the same cell.
    pub fn selection_begin(&mut self, line: u64, col: usize, kind: SelKind) {
        self.selection = Some(Selection { anchor: (line, col), head: (line, col), kind });
    }

    /// Moves the head of the selection in progress; nothing without one.
    pub fn selection_extend(&mut self, line: u64, col: usize) {
        if let Some(sel) = self.selection.as_mut() {
            sel.head = (line, col);
        }
    }

    /// Sets the whole selection at once.
    pub fn selection_set(&mut self, anchor: (u64, usize), head: (u64, usize), kind: SelKind) {
        self.selection = Some(Selection { anchor, head, kind });
    }

    pub fn selection_clear(&mut self) {
        self.selection = None;
    }

    /// The selected column span on one line, endpoints INCLUSIVE, or
    /// None when the line is outside the selection. The end column may
    /// be `usize::MAX`, meaning "to the end of the line" — a consumer
    /// clamps it to the width it has. This one function is what both
    /// the copied text and the drawn wash are made from.
    pub fn selection_span_on_line(&self, id: u64) -> Option<(usize, usize)> {
        let sel = self.selection?;
        let (s, e) = sel.ordered();
        if id < s.0 || id > e.0 {
            return None;
        }
        let (mut c0, mut c1) = (
            if id == s.0 { s.1 } else { 0 },
            if id == e.0 { e.1 } else { usize::MAX },
        );
        match sel.kind {
            SelKind::Lines => return Some((0, usize::MAX)),
            SelKind::Cells => {}
            SelKind::Words => {
                // The endpoints snap outward to word boundaries; the
                // lines between are already whole.
                if let Some(row) = self.row_of_line(id) {
                    if id == s.0 {
                        c0 = word_edge(row, c0, false);
                    }
                    if id == e.0 && c1 != usize::MAX {
                        c1 = word_edge(row, c1, true);
                    }
                }
            }
        }
        Some((c0, c1))
    }

    /// The selected text: wide-cell spacers skipped, trailing blanks
    /// trimmed per line, lines joined with `\n`. Lines trimmed out of
    /// the scrollback contribute nothing but their newline.
    pub fn selection_text(&self) -> Option<String> {
        let sel = self.selection?;
        let (s, e) = sel.ordered();
        // A stale selection from another epoch must not become a
        // hundred-thousand-iteration loop.
        if e.0 - s.0 > 200_000 {
            return None;
        }
        let mut out = String::new();
        for id in s.0..=e.0 {
            if id != s.0 {
                out.push('\n');
            }
            let Some(row) = self.row_of_line(id) else { continue };
            let Some((c0, c1)) = self.selection_span_on_line(id) else { continue };
            if row.is_empty() {
                continue;
            }
            let c1 = c1.min(row.len() - 1);
            let mut line = String::new();
            for cell in row.iter().take(c1 + 1).skip(c0.min(c1)) {
                if cell.flags & FLAG_WIDE_SPACER != 0 {
                    continue;
                }
                line.push(cell.ch);
            }
            out.push_str(line.trim_end_matches(' '));
        }
        Some(out)
    }

    // ---- paste ------------------------------------------------------

    /// What a paste becomes on the wire. Sanitised ALWAYS, bracketed or
    /// not: C0 controls except `\t` and `\r` are stripped (a paste is
    /// text, never an escape sequence), `\r\n` and `\n` normalise to
    /// `\r` (the terminal's Enter), and any literal `ESC[201~` is
    /// excised first — the bracket-escape injection that lets a crafted
    /// paste end its own bracket and smuggle a command is a real
    /// terminal CVE class, and stripping ESC alone would still leave
    /// the tail behind. Wrapped in `ESC[200~ … ESC[201~` when the
    /// application enabled DECSET 2004.
    pub fn paste_bytes(&self, text: &str) -> Vec<u8> {
        let normalised = text.replace("\r\n", "\r").replace('\n', "\r");
        let excised = normalised.replace("\x1b[201~", "");
        let clean: String = excised
            .chars()
            .filter(|&c| c == '\t' || c == '\r' || (c as u32 >= 0x20 && c != '\u{7f}'))
            .collect();
        let mut out = Vec::with_capacity(clean.len() + 12);
        if self.bracketed_paste {
            out.extend_from_slice(b"\x1b[200~");
        }
        out.extend_from_slice(clean.as_bytes());
        if self.bracketed_paste {
            out.extend_from_slice(b"\x1b[201~");
        }
        out
    }

    fn scroll_up(&mut self, n: usize) {
        // Scrolling more than the region height clears it — clamp so a
        // crafted CSI parameter (up to 65535) cannot spin the CPU.
        let n = n.min(self.rows);
        // Any feed that scrolls the selected screen region invalidates
        // the selection (the conservative first cut — a finer damage
        // test is F2). Scrollback-only selections survive output: their
        // line ids are settled and nothing moves them.
        if n > 0 && self.selection_touches_screen() {
            self.selection = None;
        }
        for _ in 0..n {
            let top = self.scroll_top;
            let bottom = self.scroll_bottom;
            let bg = self.pen.bg;
            let cols = self.cols;
            let alt = self.alt_active;
            let removed = self.grid_mut()[top].clone();
            if !alt && top == 0 {
                self.scrollback.push_back(removed);
                self.scrolled_total += 1;
                if self.scrollback.len() > 5000 {
                    self.scrollback.pop_front();
                }
            }
            let grid = self.grid_mut();
            for y in top..bottom {
                grid[y] = grid[y + 1].clone();
            }
            grid[bottom] = vec![Cell::blank(bg); cols];
        }
    }

    fn scroll_down(&mut self, n: usize) {
        let n = n.min(self.rows);
        if n > 0 && self.selection_touches_screen() {
            self.selection = None;
        }
        for _ in 0..n {
            let top = self.scroll_top;
            let bottom = self.scroll_bottom;
            let bg = self.pen.bg;
            let cols = self.cols;
            let grid = self.grid_mut();
            for y in (top + 1..=bottom).rev() {
                grid[y] = grid[y - 1].clone();
            }
            grid[top] = vec![Cell::blank(bg); cols];
        }
    }

    fn linefeed(&mut self) {
        if self.cur_y == self.scroll_bottom {
            self.scroll_up(1);
        } else if self.cur_y + 1 < self.rows {
            self.cur_y += 1;
        }
        self.wrap_pending = false;
    }

    fn put_char(&mut self, c: char) {
        let width = c.width().unwrap_or(1);
        if width == 0 {
            return; // combining characters skipped (simplification)
        }
        if self.wrap_pending && self.autowrap {
            self.cur_x = 0;
            self.linefeed();
        }
        self.wrap_pending = false;
        if self.cur_x + width > self.cols {
            if self.autowrap {
                self.cur_x = 0;
                self.linefeed();
            } else {
                self.cur_x = self.cols - width;
            }
        }
        let (x, y) = (self.cur_x, self.cur_y);
        let pen = self.pen;
        let cols = self.cols;
        let grid = self.grid_mut();
        grid[y][x] = Cell {
            ch: c,
            fg: pen.fg,
            bg: pen.bg,
            flags: if width == 2 { pen.flags | FLAG_WIDE_LEAD } else { pen.flags },
        };
        if width == 2 && x + 1 < cols {
            grid[y][x + 1] = Cell {
                ch: ' ',
                fg: pen.fg,
                bg: pen.bg,
                flags: pen.flags | FLAG_WIDE_SPACER,
            };
        }
        self.cur_x += width;
        if self.cur_x >= self.cols {
            self.cur_x = self.cols - 1;
            self.wrap_pending = true;
        }
    }

    fn erase_line(&mut self, mode: u16) {
        let (x, y) = (self.cur_x, self.cur_y);
        let bg = self.pen.bg;
        let cols = self.cols;
        let grid = self.grid_mut();
        let range = match mode {
            0 => x..cols,
            1 => 0..(x + 1).min(cols),
            _ => 0..cols,
        };
        for i in range {
            grid[y][i] = Cell::blank(bg);
        }
    }

    fn erase_display(&mut self, mode: u16) {
        let (x, y) = (self.cur_x, self.cur_y);
        let bg = self.pen.bg;
        let cols = self.cols;
        let rows = self.rows;
        match mode {
            0 => {
                self.erase_line(0);
                let grid = self.grid_mut();
                for r in y + 1..rows {
                    grid[r] = vec![Cell::blank(bg); cols];
                }
            }
            1 => {
                self.erase_line(1);
                let grid = self.grid_mut();
                for r in 0..y {
                    grid[r] = vec![Cell::blank(bg); cols];
                }
            }
            3 => {
                self.scrollback.clear();
                // The scrollback is gone — any scrolled-back view is
                // invalid, and so is anything selected in it.
                self.view_offset = 0;
                self.selection = None;
                let grid = self.grid_mut();
                for r in 0..rows {
                    grid[r] = vec![Cell::blank(bg); cols];
                }
            }
            _ => {
                let grid = self.grid_mut();
                for r in 0..rows {
                    grid[r] = vec![Cell::blank(bg); cols];
                }
            }
        }
        let _ = x;
    }

    fn sgr(&mut self, params: &[u16]) {
        let mut i = 0;
        if params.is_empty() {
            self.pen = Pen::default();
            return;
        }
        while i < params.len() {
            let p = params[i];
            match p {
                0 => self.pen = Pen::default(),
                1 => self.pen.flags |= FLAG_BOLD,
                2 => self.pen.flags |= FLAG_DIM,
                4 => self.pen.flags |= FLAG_UNDERLINE,
                7 => self.pen.flags |= FLAG_INVERSE,
                22 => self.pen.flags &= !(FLAG_BOLD | FLAG_DIM),
                24 => self.pen.flags &= !FLAG_UNDERLINE,
                27 => self.pen.flags &= !FLAG_INVERSE,
                30..=37 => self.pen.fg = CellColor::Indexed((p - 30) as u8),
                39 => self.pen.fg = CellColor::Default,
                40..=47 => self.pen.bg = CellColor::Indexed((p - 40) as u8),
                49 => self.pen.bg = CellColor::Default,
                90..=97 => self.pen.fg = CellColor::Indexed((p - 90 + 8) as u8),
                100..=107 => self.pen.bg = CellColor::Indexed((p - 100 + 8) as u8),
                38 | 48 => {
                    let target_fg = p == 38;
                    if i + 1 < params.len() && params[i + 1] == 5 && i + 2 < params.len() {
                        let c = CellColor::Indexed(params[i + 2] as u8);
                        if target_fg { self.pen.fg = c } else { self.pen.bg = c }
                        i += 2;
                    } else if i + 1 < params.len() && params[i + 1] == 2 && i + 4 < params.len() {
                        let c = CellColor::Rgb(
                            params[i + 2] as u8,
                            params[i + 3] as u8,
                            params[i + 4] as u8,
                        );
                        if target_fg { self.pen.fg = c } else { self.pen.bg = c }
                        i += 4;
                    }
                }
                _ => {}
            }
            i += 1;
        }
    }

    fn set_mode(&mut self, private: bool, param: u16, enable: bool) {
        if !private {
            return;
        }
        match param {
            1 => self.app_cursor = enable,
            7 => self.autowrap = enable,
            25 => self.cursor_visible = enable,
            2004 => self.bracketed_paste = enable,
            47 | 1047 | 1049 => {
                // Switching screens either way orphans a selection: the
                // ids still name main-screen lines, but what is on
                // display is another screen entirely.
                self.selection = None;
                if enable && !self.alt_active {
                    self.alt_active = true;
                    let bg = self.pen.bg;
                    let cols = self.cols;
                    for row in self.alt_screen.iter_mut() {
                        *row = vec![Cell::blank(bg); cols];
                    }
                    if param == 1049 {
                        self.saved_cursor = (self.cur_x, self.cur_y, self.pen);
                        self.cur_x = 0;
                        self.cur_y = 0;
                    }
                } else if !enable && self.alt_active {
                    self.alt_active = false;
                    if param == 1049 {
                        let (x, y, pen) = self.saved_cursor;
                        self.cur_x = x.min(self.cols - 1);
                        self.cur_y = y.min(self.rows - 1);
                        self.pen = pen;
                    }
                }
                self.view_offset = 0;
            }
            _ => {}
        }
    }
}

/// One end of a double-click word: from `col`, walk outward while the
/// cells keep the class of the clicked one. Three classes — blank,
/// word (alphanumeric or `_`), punctuation — so a double click takes
/// `main.rs` apart at the dot the way every terminal does, and a click
/// on a blank run takes the run. Wide-cell spacers ride with their
/// lead: they carry a blank, and stopping at one would cut a CJK word
/// in half.
fn word_edge(row: &[Cell], col: usize, forward: bool) -> usize {
    fn class(c: char) -> u8 {
        if c == ' ' || c.is_whitespace() {
            0
        } else if c.is_alphanumeric() || c == '_' {
            1
        } else {
            2
        }
    }
    if row.is_empty() {
        return col;
    }
    let mut at = col.min(row.len() - 1);
    let want = class(row[at].ch);
    loop {
        let next = if forward {
            if at + 1 >= row.len() {
                break;
            }
            at + 1
        } else {
            if at == 0 {
                break;
            }
            at - 1
        };
        let cell = &row[next];
        if cell.flags & FLAG_WIDE_SPACER == 0 && class(cell.ch) != want {
            break;
        }
        at = next;
    }
    at
}

/// Executor of vte parser events.
pub struct Performer<'a> {
    pub term: &'a mut Term,
}

fn param_or(params: &vte::Params, idx: usize, def: u16) -> u16 {
    params
        .iter()
        .nth(idx)
        .and_then(|p| p.first().copied())
        .filter(|&v| v != 0)
        .unwrap_or(def)
}

fn flat_params(params: &vte::Params) -> Vec<u16> {
    let mut out = Vec::new();
    for sub in params.iter() {
        for &v in sub {
            out.push(v);
        }
    }
    out
}

impl<'a> vte::Perform for Performer<'a> {
    fn print(&mut self, c: char) {
        self.term.put_char(c);
        self.term.view_offset = 0;
    }

    fn execute(&mut self, byte: u8) {
        let t = &mut self.term;
        match byte {
            0x08 => {
                t.cur_x = t.cur_x.saturating_sub(1);
                t.wrap_pending = false;
            }
            0x09 => {
                let next = (t.cur_x / 8 + 1) * 8;
                t.cur_x = next.min(t.cols - 1);
            }
            0x0A | 0x0B | 0x0C => t.linefeed(),
            0x0D => {
                t.cur_x = 0;
                t.wrap_pending = false;
            }
            _ => {}
        }
    }

    fn csi_dispatch(
        &mut self,
        params: &vte::Params,
        intermediates: &[u8],
        _ignore: bool,
        action: char,
    ) {
        let t = &mut self.term;
        let private = intermediates.contains(&b'?');
        let p0 = param_or(params, 0, 1) as usize;
        match action {
            'A' => t.cur_y = t.cur_y.saturating_sub(p0).max(0),
            'B' | 'e' => t.cur_y = (t.cur_y + p0).min(t.rows - 1),
            'C' | 'a' => t.cur_x = (t.cur_x + p0).min(t.cols - 1),
            'D' => t.cur_x = t.cur_x.saturating_sub(p0),
            'E' => {
                t.cur_y = (t.cur_y + p0).min(t.rows - 1);
                t.cur_x = 0;
            }
            'F' => {
                t.cur_y = t.cur_y.saturating_sub(p0);
                t.cur_x = 0;
            }
            'G' | '`' => t.cur_x = (p0 - 1).min(t.cols - 1),
            'H' | 'f' => {
                let row = param_or(params, 0, 1) as usize;
                let col = param_or(params, 1, 1) as usize;
                t.cur_y = (row - 1).min(t.rows - 1);
                t.cur_x = (col - 1).min(t.cols - 1);
                t.wrap_pending = false;
            }
            'd' => t.cur_y = (p0 - 1).min(t.rows - 1),
            'J' => {
                let mode = param_or(params, 0, 0);
                let mode = if params.iter().next().is_none() { 0 } else { mode };
                t.erase_display(mode);
            }
            'K' => {
                let mode = params
                    .iter()
                    .next()
                    .and_then(|p| p.first().copied())
                    .unwrap_or(0);
                t.erase_line(mode);
            }
            'L' => {
                if t.cur_y >= t.scroll_top && t.cur_y <= t.scroll_bottom {
                    let save_top = t.scroll_top;
                    t.scroll_top = t.cur_y;
                    t.scroll_down(p0);
                    t.scroll_top = save_top;
                }
            }
            'M' => {
                if t.cur_y >= t.scroll_top && t.cur_y <= t.scroll_bottom {
                    let save_top = t.scroll_top;
                    t.scroll_top = t.cur_y;
                    t.scroll_up(p0);
                    t.scroll_top = save_top;
                }
            }
            'P' => {
                // DCH — delete characters
                let (x, y) = (t.cur_x, t.cur_y);
                let bg = t.pen.bg;
                let cols = t.cols;
                let n = p0.min(cols - x);
                let grid = t.grid_mut();
                grid[y].drain(x..x + n);
                grid[y].extend(std::iter::repeat(Cell::blank(bg)).take(n));
            }
            '@' => {
                // ICH — insert blank characters
                let (x, y) = (t.cur_x, t.cur_y);
                let bg = t.pen.bg;
                let cols = t.cols;
                let n = p0.min(cols - x);
                let grid = t.grid_mut();
                for _ in 0..n {
                    grid[y].insert(x, Cell::blank(bg));
                }
                grid[y].truncate(cols);
            }
            'X' => {
                // ECH — erase n characters
                let (x, y) = (t.cur_x, t.cur_y);
                let bg = t.pen.bg;
                let cols = t.cols;
                let n = p0.min(cols - x);
                let grid = t.grid_mut();
                for i in 0..n {
                    grid[y][x + i] = Cell::blank(bg);
                }
            }
            'S' => t.scroll_up(p0),
            'T' => t.scroll_down(p0),
            'r' => {
                let top = param_or(params, 0, 1) as usize;
                let bottom = param_or(params, 1, t.rows as u16) as usize;
                if top < bottom && bottom <= t.rows {
                    t.scroll_top = top - 1;
                    t.scroll_bottom = bottom - 1;
                    t.cur_x = 0;
                    t.cur_y = t.scroll_top;
                }
            }
            'm' => {
                let flat = flat_params(params);
                t.sgr(&flat);
            }
            'h' | 'l' => {
                let enable = action == 'h';
                for sub in params.iter() {
                    for &p in sub {
                        t.set_mode(private, p, enable);
                    }
                }
            }
            's' => t.saved_cursor = (t.cur_x, t.cur_y, t.pen),
            'u' => {
                let (x, y, pen) = t.saved_cursor;
                t.cur_x = x.min(t.cols - 1);
                t.cur_y = y.min(t.rows - 1);
                t.pen = pen;
            }
            'c' => {
                // DA — pretend to be a VT102
                t.responses.extend_from_slice(b"\x1b[?6c");
            }
            'n' => {
                let q = params.iter().next().and_then(|p| p.first().copied()).unwrap_or(0);
                if q == 6 {
                    let resp = format!("\x1b[{};{}R", t.cur_y + 1, t.cur_x + 1);
                    t.responses.extend_from_slice(resp.as_bytes());
                } else if q == 5 {
                    t.responses.extend_from_slice(b"\x1b[0n");
                }
            }
            _ => {}
        }
    }

    fn esc_dispatch(&mut self, _intermediates: &[u8], _ignore: bool, byte: u8) {
        let t = &mut self.term;
        match byte {
            b'D' => t.linefeed(),
            b'E' => {
                t.cur_x = 0;
                t.linefeed();
            }
            b'M' => {
                // Reverse index
                if t.cur_y == t.scroll_top {
                    t.scroll_down(1);
                } else {
                    t.cur_y = t.cur_y.saturating_sub(1);
                }
            }
            b'7' => t.saved_cursor = (t.cur_x, t.cur_y, t.pen),
            b'8' => {
                let (x, y, pen) = t.saved_cursor;
                t.cur_x = x.min(t.cols - 1);
                t.cur_y = y.min(t.rows - 1);
                t.pen = pen;
            }
            b'c' => {
                // Full reset — except the line-id counter, which is
                // monotonic for the SESSION: a stale selection base
                // held by a widget must resolve to nothing, never to
                // a fresh line that happens to reuse the number.
                let (cols, rows) = (t.cols, t.rows);
                let kept = t.scrolled_total;
                **t = Term::new(cols, rows);
                t.scrolled_total = kept;
            }
            _ => {}
        }
    }

    /// OSC carries window titles, clipboard requests and colour queries.
    /// The interface has no title bar and answers no clipboard, so the
    /// sequences are consumed and dropped — which is what a terminal
    /// without those capabilities is supposed to do.
    fn osc_dispatch(&mut self, _params: &[&[u8]], _bell_terminated: bool) {}

    fn hook(&mut self, _: &vte::Params, _: &[u8], _: bool, _: char) {}
    fn put(&mut self, _: u8) {}
    fn unhook(&mut self) {}
}

#[cfg(test)]
mod wide_tests {
    use super::*;

    fn feed(t: &mut Term, bytes: &[u8]) {
        let mut parser = vte::Parser::new();
        let mut performer = Performer { term: t };
        for b in bytes {
            parser.advance(&mut performer, *b);
        }
    }

    fn cell(t: &Term, x: usize) -> Cell {
        t.view_row(0).and_then(|r| r.get(x).copied()).expect("row 0 exists")
    }

    /// A wide character stays recognisable as one after the sequences
    /// that shuffle cells around. Working the width out from "the next
    /// cell is a spacer" gets this wrong, and deleting a character in
    /// an editor does it every time.
    #[test]
    fn a_wide_lead_survives_losing_its_spacer() {
        let mut t = Term::new(10, 2);
        feed(&mut t, "\u{4e2d}".as_bytes());
        assert!(
            cell(&t, 0).flags & FLAG_WIDE_LEAD != 0,
            "the lead must say it is wide"
        );
        assert!(cell(&t, 1).flags & FLAG_WIDE_SPACER != 0);

        // Delete one cell: the spacer is pulled away, the lead stays.
        feed(&mut t, b"\x1b[1;2H\x1b[1P");
        assert!(
            cell(&t, 0).flags & FLAG_WIDE_LEAD != 0,
            "the lead is still a wide character with its spacer gone"
        );
        assert!(
            cell(&t, 1).flags & FLAG_WIDE_SPACER == 0,
            "and the cell after it is no longer a spacer"
        );
    }

    /// Ordinary characters must not claim to be wide.
    #[test]
    fn a_narrow_character_is_not_marked_wide() {
        let mut t = Term::new(10, 2);
        feed(&mut t, b"a");
        assert_eq!(cell(&t, 0).flags & FLAG_WIDE_LEAD, 0);
    }
}

#[cfg(test)]
mod selection_tests {
    use super::*;

    fn feed(t: &mut Term, bytes: &[u8]) {
        let mut parser = vte::Parser::new();
        let mut performer = Performer { term: t };
        for b in bytes {
            parser.advance(&mut performer, *b);
        }
    }

    /// A line id names content, and follows it off the screen, through
    /// the scrollback and past the trim: the selected text is the same
    /// string before and after both moves.
    #[test]
    fn line_ids_survive_scroll_and_trim() {
        let mut t = Term::new(20, 3);
        // The marker scrolls into the scrollback first: a selection
        // made over scrollback lines is settled, and output may not
        // move it.
        feed(&mut t, b"marker\r\n\r\n\r\n");
        assert!(!t.scrollback.is_empty());
        let id = t.scrolled_total - t.scrollback.len() as u64;
        t.selection_set((id, 0), (id, 5), SelKind::Cells);
        assert_eq!(t.selection_text().as_deref(), Some("marker"));
        feed(&mut t, b"more\r\noutput\r\n");
        assert!(t.selection.is_some(), "scrollback-only selections survive output");
        assert_eq!(t.selection_text().as_deref(), Some("marker"));

        // Push it past the 5000-line trim: the id resolves to nothing,
        // never to some other line.
        for _ in 0..5001 {
            t.linefeed();
        }
        assert_eq!(t.selection_text().as_deref(), Some(""));
    }

    /// The uniform formula: view row 0's id plus y, across the
    /// scrollback boundary.
    #[test]
    fn view_row_ids_are_contiguous_across_the_boundary() {
        let mut t = Term::new(10, 4);
        for _ in 0..10 {
            t.linefeed();
        }
        t.scroll_view(2); // two scrollback rows on top, screen below
        let base = t.line_id_of_view_row(0);
        for y in 1..4 {
            assert_eq!(t.line_id_of_view_row(y), base + y as u64);
        }
        assert_eq!(base, t.scrolled_total - 2);
    }

    /// Wide-cell spacers are skipped in the copied text, so CJK copies
    /// as its characters, not as characters plus phantom blanks.
    #[test]
    fn selection_text_skips_wide_spacers() {
        let mut t = Term::new(10, 2);
        feed(&mut t, "a\u{4e2d}b".as_bytes());
        let id = t.line_id_of_view_row(0);
        t.selection_set((id, 0), (id, 3), SelKind::Cells);
        assert_eq!(t.selection_text().as_deref(), Some("a\u{4e2d}b"));
    }

    /// Trailing blanks are trimmed per line and lines join with \n; a
    /// middle line is taken whole.
    #[test]
    fn selection_text_trims_and_joins() {
        let mut t = Term::new(10, 3);
        feed(&mut t, b"one\r\ntwo\r\nthree");
        let top = t.line_id_of_view_row(0);
        t.selection_set((top, 1), (top + 2, 2), SelKind::Cells);
        assert_eq!(t.selection_text().as_deref(), Some("ne\ntwo\nthr"));
    }

    /// Word selection snaps outward by character class: a double click
    /// inside `main.rs` stops at the dot, one on a blank takes the run.
    #[test]
    fn word_selection_snaps_to_class_edges() {
        let mut t = Term::new(20, 2);
        feed(&mut t, b"cat main.rs now");
        let id = t.line_id_of_view_row(0);
        t.selection_set((id, 5), (id, 5), SelKind::Words);
        assert_eq!(t.selection_text().as_deref(), Some("main"));
        t.selection_set((id, 8), (id, 8), SelKind::Words);
        assert_eq!(t.selection_text().as_deref(), Some("."));
        t.selection_set((id, 9), (id, 10), SelKind::Words);
        assert_eq!(t.selection_text().as_deref(), Some("rs"));
    }

    /// Line selection takes whole lines whatever the columns say.
    #[test]
    fn line_selection_takes_whole_lines() {
        let mut t = Term::new(10, 3);
        feed(&mut t, b"alpha\r\nbeta");
        let top = t.line_id_of_view_row(0);
        t.selection_set((top, 4), (top + 1, 0), SelKind::Lines);
        assert_eq!(t.selection_text().as_deref(), Some("alpha\nbeta"));
    }

    /// The wash and the copy read the same span function; endpoints are
    /// inclusive and middle lines answer "to the end of the line".
    #[test]
    fn span_on_line_is_the_single_authority() {
        let mut t = Term::new(10, 4);
        feed(&mut t, b"aa\r\nbb\r\ncc");
        let top = t.line_id_of_view_row(0);
        t.selection_set((top, 1), (top + 2, 0), SelKind::Cells);
        assert_eq!(t.selection_span_on_line(top), Some((1, usize::MAX)));
        assert_eq!(t.selection_span_on_line(top + 1), Some((0, usize::MAX)));
        assert_eq!(t.selection_span_on_line(top + 2), Some((0, 0)));
        assert_eq!(t.selection_span_on_line(top + 3), None);
        // Backwards drags order themselves.
        t.selection_set((top + 2, 0), (top, 1), SelKind::Cells);
        assert_eq!(t.selection_span_on_line(top), Some((1, usize::MAX)));
    }

    /// The conservative clearing rules: a scroll of the selected screen
    /// region, the alt screen, a resize and ESC[3J all drop the
    /// selection; output while a scrollback selection stands does not.
    #[test]
    fn selection_clears_on_the_documented_events() {
        // A feed that scrolls the selected screen region.
        let mut t = Term::new(10, 2);
        feed(&mut t, b"hi");
        let id = t.line_id_of_view_row(0);
        t.selection_set((id, 0), (id, 1), SelKind::Cells);
        feed(&mut t, b"\r\n\r\n\r\n");
        assert!(t.selection.is_none(), "a scrolled screen selection is dropped");

        // Alt-screen switch.
        let mut t = Term::new(10, 2);
        feed(&mut t, b"hi");
        t.selection_set((0, 0), (0, 1), SelKind::Cells);
        feed(&mut t, b"\x1b[?1049h");
        assert!(t.selection.is_none(), "alt switch drops the selection");

        // Resize.
        let mut t = Term::new(10, 2);
        t.selection_set((0, 0), (0, 1), SelKind::Cells);
        t.resize(12, 3);
        assert!(t.selection.is_none(), "resize drops the selection");

        // Scrollback clear.
        let mut t = Term::new(10, 2);
        for _ in 0..4 {
            t.linefeed();
        }
        t.selection_set((0, 0), (0, 1), SelKind::Cells);
        t.erase_display(3);
        assert!(t.selection.is_none(), "ESC[3J drops the selection");
    }
}

#[cfg(test)]
mod paste_tests {
    use super::*;

    fn feed(t: &mut Term, bytes: &[u8]) {
        let mut parser = vte::Parser::new();
        let mut performer = Performer { term: t };
        for b in bytes {
            parser.advance(&mut performer, *b);
        }
    }

    /// DECSET/DECRST 2004 turns the brackets on and off.
    #[test]
    fn mode_2004_brackets_the_paste() {
        let mut t = Term::new(10, 2);
        assert!(!t.bracketed_paste);
        assert_eq!(t.paste_bytes("hi"), b"hi".to_vec());
        feed(&mut t, b"\x1b[?2004h");
        assert!(t.bracketed_paste);
        assert_eq!(t.paste_bytes("hi"), b"\x1b[200~hi\x1b[201~".to_vec());
        feed(&mut t, b"\x1b[?2004l");
        assert!(!t.bracketed_paste);
    }

    /// Newlines become the terminal's Enter, both spellings.
    #[test]
    fn paste_normalises_newlines_to_cr() {
        let t = Term::new(10, 2);
        assert_eq!(t.paste_bytes("a\r\nb\nc"), b"a\rb\rc".to_vec());
    }

    /// C0 controls are stripped except tab and return — a paste is
    /// text, never a control sequence.
    #[test]
    fn paste_strips_c0_except_tab_and_cr() {
        let t = Term::new(10, 2);
        assert_eq!(t.paste_bytes("a\x07b\tc\x00d\x7f"), b"ab\tcd".to_vec());
    }

    /// The bracket-escape injection: a literal ESC[201~ inside the
    /// payload is excised WHOLE, so nothing in a paste can end its own
    /// bracket — and the loose "[201~" tail never reaches the shell.
    #[test]
    fn paste_excises_the_closing_bracket_sequence() {
        let mut t = Term::new(10, 2);
        feed(&mut t, b"\x1b[?2004h");
        let out = t.paste_bytes("safe\x1b[201~; rm -rf /\x1b[201~");
        assert_eq!(out, b"\x1b[200~safe; rm -rf /\x1b[201~".to_vec());
    }
}

#[cfg(test)]
mod scroll_tests {
    use super::*;
    #[test]
    fn erase_display_3_while_scrolled_back_no_underflow() {
        let mut t = Term::new(20, 5);
        // Fill scrollback by feeding many newlines through print/linefeed.
        for _ in 0..60 {
            t.linefeed();
        }
        assert!(!t.scrollback.is_empty());
        // Scroll the view back.
        t.scroll_view(40);
        assert!(t.view_offset > 0);
        // ESC[3J equivalent: clear scrollback.
        t.erase_display(3);
        assert_eq!(t.view_offset, 0, "view_offset must reset when scrollback cleared");
        // view_row must not underflow/panic even if offset lagged.
        for y in 0..5 {
            let _ = t.view_row(y);
        }
    }
    #[test]
    fn scroll_up_clamps_huge_count() {
        let mut t = Term::new(10, 5);
        // A crafted CSI would ask for 65535 — must clamp to rows, not spin.
        t.scroll_up(65535);
        // No assertion needed beyond "returns promptly without panic".
    }
}
