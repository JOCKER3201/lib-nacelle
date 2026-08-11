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

    fn scroll_up(&mut self, n: usize) {
        // Scrolling more than the region height clears it — clamp so a
        // crafted CSI parameter (up to 65535) cannot spin the CPU.
        let n = n.min(self.rows);
        for _ in 0..n {
            let top = self.scroll_top;
            let bottom = self.scroll_bottom;
            let bg = self.pen.bg;
            let cols = self.cols;
            let alt = self.alt_active;
            let removed = self.grid_mut()[top].clone();
            if !alt && top == 0 {
                self.scrollback.push_back(removed);
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
                // The scrollback is gone — any scrolled-back view is invalid.
                self.view_offset = 0;
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
            47 | 1047 | 1049 => {
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
                // Full reset
                let (cols, rows) = (t.cols, t.rows);
                **t = Term::new(cols, rows);
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
