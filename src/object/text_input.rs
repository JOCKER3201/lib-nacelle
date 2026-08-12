//! Single-line text input (F1 §3): a pure model, an immediate-mode view,
//! and nothing else — the platform (IME, clipboard, key translation)
//! stays in the application.
//!
//! Three layers, deliberately apart:
//!
//! * [`InputModel`] — the state machine: text, caret, selection, undo,
//!   IME preedit. No `Ctx`, no clock, no clipboard: `apply` is a pure
//!   function of its messages, and side effects leave as INTENTS
//!   ([`InputEdited::CopyRequest`], [`InputEdited::PasteRequest`]) the
//!   caller resolves against [`crate::clipboard`]. That is what makes
//!   the whole caret/undo logic unit-testable without a window.
//! * [`draw`] — the view, in the object idiom: theme tokens and the
//!   `FontSystem` only. The theme already owns this object as **field**
//!   (`[field]`, `component.field.*`, class `field`); the view reads
//!   those tokens and no literal.
//! * IME — the model speaks [`InputMsg::Preedit`]/[`InputMsg::Insert`]
//!   and the application translates its window library's events into
//!   them. Preedit is NOT text: it never enters the value, the undo
//!   stack, the validator or `max_len` until the platform commits it.
//!
//! Caret coordinates are BYTE offsets into the value, always on a char
//! boundary, moved by GRAPHEME clusters (`ř` plus a combining mark,
//! ZWJ emoji — one caret step each). No bidi/shaping logic lives here:
//! the caret is strictly grapheme-linear, so the later shaping phase
//! can replace the measure function without touching the model.

use super::focus_ring;
use crate::draw::Corner;
use crate::focus::{Caps, FocusId, Key, KeyEv, Mods};
use crate::font::FONT_UI;
use crate::theme::{self, bake::StateStyle, parse::State, Color, TokenId};
use crate::{ui, Ctx, Rect};
use std::sync::OnceLock;
use unicode_segmentation::UnicodeSegmentation;

fn tok(cell: &'static OnceLock<TokenId>, name: &'static str) -> TokenId {
    *cell.get_or_init(|| theme::id(name).unwrap_or(TokenId::MISSING))
}

/// The engine's colour, in the draw list's clothes.
fn col(c: theme::ThemeColor) -> Color {
    Color { r: c.r, g: c.g, b: c.b, a: c.a }
}

// ---------------------------------------------------------------------
// Model
// ---------------------------------------------------------------------

/// Undo depth — an engine constant, not a token (behaviour, not look).
pub const UNDO_DEPTH: usize = 64;

/// Where a caret motion goes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Motion {
    Left,
    Right,
    WordLeft,
    WordRight,
    Home,
    End,
}

/// What may be typed. `Charset` and `Digits` filter characters;
/// `Custom` judges the whole would-be value. The validator runs on
/// every text MUTATION (insert and delete alike — it validates the
/// value, not keystrokes); a rejected edit is a no-op that answers
/// [`InputEdited::Rejected`].
pub enum Validator {
    Charset(fn(char) -> bool),
    Digits,
    Custom(Box<dyn Fn(&str) -> bool + Send>),
}

impl Validator {
    fn accepts(&self, s: &str) -> bool {
        match self {
            Validator::Charset(f) => s.chars().all(|c| f(c)),
            Validator::Digits => s.chars().all(|c| c.is_ascii_digit()),
            Validator::Custom(f) => f(s),
        }
    }
}

/// One message into the model. The application translates keys (see
/// [`key_msg`]), IME events and pointer hits into these; nothing else
/// mutates an [`InputModel`].
#[derive(Clone, PartialEq, Debug)]
pub enum InputMsg {
    /// Typed text, an IME commit, or a resolved paste.
    Insert(String),
    /// IME composing text with an optional cursor byte range WITHIN it.
    /// An empty string clears the preedit (winit's cancel).
    Preedit(String, Option<(usize, usize)>),
    /// The composition ended without a commit (focus left, IME closed).
    PreeditEnd,
    Backspace,
    Delete,
    DeleteWordBack,
    /// Caret motion; `true` extends the selection.
    Move(Motion, bool),
    SelectAll,
    /// These answer with INTENTS — the model never touches the
    /// clipboard (purity; see the module header).
    Cut,
    Copy,
    Paste,
    Undo,
    Redo,
    /// Enter. Mid-composition it cancels the preedit instead — a stray
    /// Enter must not submit half a name.
    Enter,
    /// Escape: cancels the preedit if one is live, else answers
    /// [`InputEdited::Cancel`] for the caller to bubble.
    Escape,
    /// Pointer: caret to the byte offset (clamped to a grapheme
    /// boundary), from the view's [`hit`]. `extend` keeps the anchor.
    Point { at: usize, extend: bool },
    /// Double-click: select the word around the byte offset.
    PointWord { at: usize },
}

/// What one [`InputModel::apply`] changed — the caller reacts (redraw,
/// caret-blink restart, submit) and RESOLVES the two clipboard intents.
#[derive(Clone, PartialEq, Debug)]
pub enum InputEdited {
    /// Nothing observable happened.
    None,
    /// Caret, selection or preedit changed; the committed text did not.
    Moved,
    /// The committed text changed.
    Edited,
    /// The validator or `max_len` refused the edit; value untouched.
    Rejected,
    /// Enter on a settled value — the caller decides what submit means.
    Submit,
    /// Escape with no live preedit — the caller closes/bubbles.
    Cancel,
    /// Store `text` on the clipboard. With `cut == true` the selection
    /// was also removed (the value DID change).
    CopyRequest { text: String, cut: bool },
    /// Load the clipboard and send the text back as
    /// [`InputMsg::Insert`]; the model cannot reach it itself.
    PasteRequest,
}

/// Undo snapshots: edit groups, preedit NEVER enters. Consecutive
/// single word-character inserts coalesce into one group; any motion,
/// deletion or paste seals it.
#[derive(Default)]
struct UndoStack {
    undo: Vec<(String, usize)>,
    redo: Vec<(String, usize)>,
    /// The top undo entry is an open typing group that further
    /// word-character inserts fold into.
    group_open: bool,
}

impl UndoStack {
    /// Snapshots the pre-edit state, unless this edit coalesces into
    /// the open group. Every recorded edit clears the redo branch.
    fn record(&mut self, text: &str, cursor: usize, coalesce: bool) {
        if !(coalesce && self.group_open) {
            if self.undo.len() == UNDO_DEPTH {
                self.undo.remove(0);
            }
            self.undo.push((text.to_string(), cursor));
        }
        self.group_open = coalesce;
        self.redo.clear();
    }

    /// Any motion, deletion or paste seals the typing group.
    fn seal(&mut self) {
        self.group_open = false;
    }
}

/// The single-line input state machine. See the module header for what
/// deliberately is NOT here (clipboard, clock, platform).
pub struct InputModel {
    text: String,
    /// Byte offset, always on a grapheme (hence char) boundary.
    cursor: usize,
    /// Selection = `anchor..cursor`, either order. None = no selection.
    sel_anchor: Option<usize>,
    /// IME composing text and the cursor byte range within it.
    preedit: Option<(String, Option<(usize, usize)>)>,
    undo: UndoStack,
    /// Password rendering — the view asks and draws `field.mask_glyph`
    /// per grapheme; the model also refuses Copy/Cut while set (a
    /// masked value must not leave through the clipboard).
    pub mask: bool,
    validator: Option<Validator>,
    /// Maximum length in CHARS, enforced on edit.
    max_len: Option<usize>,
    /// Horizontal view offset in px. View-owned state that has to
    /// survive the immediate-mode frame, so it rides in the model the
    /// way scroll state rides in a `Term`.
    pub(crate) scroll_px: f32,
    /// Bumped on every accepted edit and preedit change — the view
    /// restarts the caret blink when it sees a new value.
    edit_seq: u32,
    /// The view's blink bookkeeping: (edit_seq it saw, ctx.t then).
    pub(crate) blink: (u32, f64),
    /// The view's measure cache — see [`draw`].
    cache: ViewCache,
}

impl Default for InputModel {
    fn default() -> Self {
        InputModel::new()
    }
}

impl InputModel {
    pub fn new() -> InputModel {
        InputModel {
            text: String::new(),
            cursor: 0,
            sel_anchor: None,
            preedit: None,
            undo: UndoStack::default(),
            mask: false,
            validator: None,
            max_len: None,
            scroll_px: 0.0,
            edit_seq: 0,
            blink: (0, 0.0),
            cache: ViewCache::default(),
        }
    }

    pub fn with_validator(mut self, v: Validator) -> InputModel {
        self.validator = Some(v);
        self
    }

    pub fn with_max_len(mut self, chars: usize) -> InputModel {
        self.max_len = Some(chars);
        self
    }

    pub fn with_mask(mut self, mask: bool) -> InputModel {
        self.mask = mask;
        self
    }

    pub fn value(&self) -> &str {
        &self.text
    }

    /// Replaces the value wholesale — a programmatic set is a new
    /// document: caret to the end, selection, preedit and undo history
    /// gone. The validator is NOT consulted (the caller's authority).
    pub fn set_value(&mut self, s: &str) {
        self.text = s.to_string();
        self.cursor = self.text.len();
        self.sel_anchor = None;
        self.preedit = None;
        self.undo = UndoStack::default();
        self.scroll_px = 0.0;
        self.edit_seq = self.edit_seq.wrapping_add(1);
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// The ordered selection byte range, when one exists and is
    /// non-empty.
    pub fn selection(&self) -> Option<(usize, usize)> {
        let a = self.sel_anchor?;
        if a == self.cursor {
            return None;
        }
        Some((a.min(self.cursor), a.max(self.cursor)))
    }

    pub fn selected_text(&self) -> Option<&str> {
        self.selection().map(|(a, b)| &self.text[a..b])
    }

    pub fn has_preedit(&self) -> bool {
        self.preedit.is_some()
    }

    pub fn preedit(&self) -> Option<&(String, Option<(usize, usize)>)> {
        self.preedit.as_ref()
    }

    /// Applies one message. Pure: no clock, no clipboard, no theme —
    /// what cannot be done here leaves as an intent in the result.
    pub fn apply(&mut self, m: InputMsg) -> InputEdited {
        match m {
            InputMsg::Insert(s) => self.insert(&s),
            InputMsg::Preedit(text, range) => {
                if text.is_empty() {
                    return self.end_preedit();
                }
                self.preedit = Some((text, range));
                self.edit_seq = self.edit_seq.wrapping_add(1);
                InputEdited::Moved
            }
            InputMsg::PreeditEnd => self.end_preedit(),
            InputMsg::Backspace => match self.selection() {
                Some((a, b)) => self.delete_range(a, b),
                None => {
                    let a = prev_grapheme(&self.text, self.cursor);
                    self.delete_range(a, self.cursor)
                }
            },
            InputMsg::Delete => match self.selection() {
                Some((a, b)) => self.delete_range(a, b),
                None => {
                    let b = next_grapheme(&self.text, self.cursor);
                    self.delete_range(self.cursor, b)
                }
            },
            InputMsg::DeleteWordBack => match self.selection() {
                Some((a, b)) => self.delete_range(a, b),
                None => {
                    let a = word_left(&self.text, self.cursor);
                    self.delete_range(a, self.cursor)
                }
            },
            InputMsg::Move(motion, extend) => self.motion(motion, extend),
            InputMsg::SelectAll => {
                self.undo.seal();
                let all = (Some(0), self.text.len());
                if (self.sel_anchor, self.cursor) == all {
                    return InputEdited::None;
                }
                self.sel_anchor = Some(0);
                self.cursor = self.text.len();
                InputEdited::Moved
            }
            InputMsg::Cut => {
                // A masked value must not leave through the clipboard.
                if self.mask {
                    return InputEdited::None;
                }
                let Some((a, b)) = self.selection() else {
                    return InputEdited::None;
                };
                let text = self.text[a..b].to_string();
                match self.delete_range(a, b) {
                    InputEdited::Edited => InputEdited::CopyRequest { text, cut: true },
                    other => other,
                }
            }
            InputMsg::Copy => {
                if self.mask {
                    return InputEdited::None;
                }
                match self.selected_text() {
                    Some(t) if !t.is_empty() => {
                        InputEdited::CopyRequest { text: t.to_string(), cut: false }
                    }
                    _ => InputEdited::None,
                }
            }
            InputMsg::Paste => InputEdited::PasteRequest,
            InputMsg::Undo => {
                self.undo.seal();
                let Some((text, cursor)) = self.undo.undo.pop() else {
                    return InputEdited::None;
                };
                self.undo.redo.push((std::mem::take(&mut self.text), self.cursor));
                self.text = text;
                self.cursor = cursor;
                self.sel_anchor = None;
                self.edit_seq = self.edit_seq.wrapping_add(1);
                InputEdited::Edited
            }
            InputMsg::Redo => {
                self.undo.seal();
                let Some((text, cursor)) = self.undo.redo.pop() else {
                    return InputEdited::None;
                };
                self.undo.undo.push((std::mem::take(&mut self.text), self.cursor));
                self.text = text;
                self.cursor = cursor;
                self.sel_anchor = None;
                self.edit_seq = self.edit_seq.wrapping_add(1);
                InputEdited::Edited
            }
            InputMsg::Enter => {
                if self.preedit.is_some() {
                    // A stray Enter mid-composition must not submit
                    // half a name; it cancels the composition only.
                    return self.end_preedit();
                }
                InputEdited::Submit
            }
            InputMsg::Escape => {
                if self.preedit.is_some() {
                    return self.end_preedit();
                }
                InputEdited::Cancel
            }
            InputMsg::Point { at, extend } => {
                self.undo.seal();
                let at = floor_grapheme(&self.text, at);
                let old = (self.cursor, self.sel_anchor);
                if extend {
                    if self.sel_anchor.is_none() {
                        self.sel_anchor = Some(self.cursor);
                    }
                } else {
                    self.sel_anchor = None;
                }
                self.cursor = at;
                if (self.cursor, self.sel_anchor) == old {
                    InputEdited::None
                } else {
                    InputEdited::Moved
                }
            }
            InputMsg::PointWord { at } => {
                self.undo.seal();
                let (a, b) = word_at(&self.text, floor_grapheme(&self.text, at));
                self.sel_anchor = Some(a);
                self.cursor = b;
                InputEdited::Moved
            }
        }
    }

    // ---- edits ------------------------------------------------------

    fn insert(&mut self, s: &str) -> InputEdited {
        if s.is_empty() {
            return InputEdited::None;
        }
        let (a, b) = self.selection().unwrap_or((self.cursor, self.cursor));
        let mut would = String::with_capacity(self.text.len() + s.len());
        would.push_str(&self.text[..a]);
        would.push_str(s);
        would.push_str(&self.text[b..]);
        if !self.admits(&would) {
            return InputEdited::Rejected;
        }
        // Coalesce single word-character graphemes typed over no
        // selection; a paste (many graphemes) or a replace seals.
        let coalesce = self.selection().is_none() && is_word_grapheme(s);
        self.undo.record(&self.text, self.cursor, coalesce);
        self.text = would;
        self.cursor = a + s.len();
        self.sel_anchor = None;
        self.edit_seq = self.edit_seq.wrapping_add(1);
        InputEdited::Edited
    }

    fn delete_range(&mut self, a: usize, b: usize) -> InputEdited {
        if a >= b {
            return InputEdited::None;
        }
        let mut would = String::with_capacity(self.text.len());
        would.push_str(&self.text[..a]);
        would.push_str(&self.text[b..]);
        if !self.admits(&would) {
            return InputEdited::Rejected;
        }
        self.undo.record(&self.text, self.cursor, false);
        self.undo.seal();
        self.text = would;
        self.cursor = a;
        self.sel_anchor = None;
        self.edit_seq = self.edit_seq.wrapping_add(1);
        InputEdited::Edited
    }

    fn admits(&self, would: &str) -> bool {
        if let Some(v) = &self.validator {
            if !v.accepts(would) {
                return false;
            }
        }
        if let Some(m) = self.max_len {
            if would.chars().count() > m {
                return false;
            }
        }
        true
    }

    fn end_preedit(&mut self) -> InputEdited {
        if self.preedit.take().is_none() {
            return InputEdited::None;
        }
        self.edit_seq = self.edit_seq.wrapping_add(1);
        InputEdited::Moved
    }

    fn motion(&mut self, motion: Motion, extend: bool) -> InputEdited {
        self.undo.seal();
        let old = (self.cursor, self.sel_anchor);
        if extend {
            if self.sel_anchor.is_none() {
                self.sel_anchor = Some(self.cursor);
            }
        } else if let Some((a, b)) = self.selection() {
            // Plain Left/Right on a selection collapse to its edge —
            // the convention every toolkit shares.
            self.sel_anchor = None;
            match motion {
                Motion::Left => {
                    self.cursor = a;
                    return InputEdited::Moved;
                }
                Motion::Right => {
                    self.cursor = b;
                    return InputEdited::Moved;
                }
                _ => {}
            }
        }
        if !extend {
            self.sel_anchor = None;
        }
        self.cursor = match motion {
            Motion::Left => prev_grapheme(&self.text, self.cursor),
            Motion::Right => next_grapheme(&self.text, self.cursor),
            Motion::WordLeft => word_left(&self.text, self.cursor),
            Motion::WordRight => word_right(&self.text, self.cursor),
            Motion::Home => 0,
            Motion::End => self.text.len(),
        };
        if (self.cursor, self.sel_anchor) == old {
            InputEdited::None
        } else {
            InputEdited::Moved
        }
    }
}

/// The [`InputMsg`] a neutral key event means, if any. Text inserts
/// come from `ev.text` (the platform's produced text — dead keys and
/// compose already applied) or, without one, from the bare character.
/// Ctrl/Super chords never insert; Tab is not the field's (fields are
/// not greedy for Tab — the chain keeps it). `None` bubbles the key to
/// the caller.
pub fn key_msg(ev: &KeyEv) -> Option<InputMsg> {
    let ctrl = ev.mods.contains(Mods::CTRL);
    let shift = ev.mods.contains(Mods::SHIFT);
    let alt = ev.mods.contains(Mods::ALT);
    let sup = ev.mods.contains(Mods::SUPER);
    match ev.key {
        Key::Left => {
            return Some(InputMsg::Move(
                if ctrl { Motion::WordLeft } else { Motion::Left },
                shift,
            ))
        }
        Key::Right => {
            return Some(InputMsg::Move(
                if ctrl { Motion::WordRight } else { Motion::Right },
                shift,
            ))
        }
        Key::Home => return Some(InputMsg::Move(Motion::Home, shift)),
        Key::End => return Some(InputMsg::Move(Motion::End, shift)),
        Key::Backspace => {
            return Some(if ctrl { InputMsg::DeleteWordBack } else { InputMsg::Backspace })
        }
        Key::Delete => return Some(InputMsg::Delete),
        Key::Enter => return Some(InputMsg::Enter),
        Key::Escape => return Some(InputMsg::Escape),
        _ => {}
    }
    if ctrl && !alt {
        if let Key::Char(c) = ev.key {
            return match c.to_ascii_lowercase() {
                'a' => Some(InputMsg::SelectAll),
                'c' => Some(InputMsg::Copy),
                'x' => Some(InputMsg::Cut),
                'v' => Some(InputMsg::Paste),
                'z' if shift => Some(InputMsg::Redo),
                'z' => Some(InputMsg::Undo),
                'y' => Some(InputMsg::Redo),
                _ => None,
            };
        }
        return None;
    }
    if ctrl || sup {
        return None;
    }
    if let Some(t) = &ev.text {
        if !t.is_empty() && t.chars().all(|c| !c.is_control()) {
            return Some(InputMsg::Insert(t.clone()));
        }
        return None;
    }
    match ev.key {
        Key::Char(c) if !c.is_control() => Some(InputMsg::Insert(c.to_string())),
        Key::Space => Some(InputMsg::Insert(" ".to_string())),
        _ => None,
    }
}

// ---- grapheme walking -----------------------------------------------

/// The grapheme boundary before `at`; 0 at the start.
fn prev_grapheme(s: &str, at: usize) -> usize {
    s[..at].grapheme_indices(true).last().map(|(i, _)| i).unwrap_or(0)
}

/// The grapheme boundary after `at`; `s.len()` at the end.
fn next_grapheme(s: &str, at: usize) -> usize {
    s[at..].graphemes(true).next().map(|g| at + g.len()).unwrap_or(s.len())
}

/// Clamps an arbitrary byte offset onto the nearest grapheme boundary
/// at or before it — a hit-test may land mid-cluster.
fn floor_grapheme(s: &str, at: usize) -> usize {
    if at >= s.len() {
        return s.len();
    }
    let mut prev = 0;
    for (i, g) in s.grapheme_indices(true) {
        if i > at {
            return prev;
        }
        if i == at {
            return i;
        }
        prev = i;
        let end = i + g.len();
        if end > at {
            return i;
        }
    }
    prev
}

/// Start of the word before `at`: the nearest alphanumeric-bearing
/// segment's start strictly before the caret, whitespace and
/// punctuation skipped; 0 when none.
fn word_left(s: &str, at: usize) -> usize {
    let mut best = 0;
    for (i, seg) in s[..at].split_word_bound_indices() {
        if seg.chars().any(|c| c.is_alphanumeric()) {
            best = i;
        }
    }
    best
}

/// End of the word after `at`: the end of the first
/// alphanumeric-bearing segment at or past the caret; `s.len()` when
/// none.
fn word_right(s: &str, at: usize) -> usize {
    for (i, seg) in s[at..].split_word_bound_indices() {
        if seg.chars().any(|c| c.is_alphanumeric()) {
            return at + i + seg.len();
        }
    }
    s.len()
}

/// The word-boundary segment containing `at` — what a double-click
/// selects (a run of spaces counts as its own segment, like every
/// toolkit). Past the end, the last segment.
fn word_at(s: &str, at: usize) -> (usize, usize) {
    let mut last = (s.len(), s.len());
    for (i, seg) in s.split_word_bound_indices() {
        if at >= i && at < i + seg.len() {
            return (i, i + seg.len());
        }
        last = (i, i + seg.len());
    }
    last
}

/// Whether an insert is a single word-character grapheme — the typing
/// undo groups coalesce over exactly these.
fn is_word_grapheme(s: &str) -> bool {
    s.graphemes(true).count() == 1 && s.chars().all(|c| c.is_alphanumeric())
}

// ---------------------------------------------------------------------
// View
// ---------------------------------------------------------------------

/// The measure cache: caret/selection x positions are re-measured only
/// when the value, caret, selection or resolved size changed — a frame
/// that merely redraws re-uses last frame's numbers (§3.7: measure per
/// edit, not per frame).
#[derive(Default)]
struct ViewCache {
    /// The theme epoch leads, because the other four describe the TEXT
    /// and the widths cached here are text measured through the theme:
    /// tracking, leading and the bound role all move without the value,
    /// the caret or the resolved size moving with them.
    key: (u32, u32, usize, Option<usize>, u32),
    caret_x: f32,
    sel_x: Option<(f32, f32)>,
    text_w: f32,
}

/// How the caller styles one field. Everything VISUAL comes from theme
/// tokens; this is only wiring.
pub struct InputStyle<'a> {
    /// Drawn in `component.field.placeholder` while the value and
    /// preedit are empty.
    pub placeholder: &'a str,
    /// The pointer is inside the field (the caller hit-tests, as with
    /// every object) — feeds the class ladder's hover rung.
    pub hover: bool,
    /// Feeds the ladder's disabled rung and suppresses the caret.
    pub disabled: bool,
    /// The answer to "is this field focused" while the world has no
    /// focus chain (`ctx.focus` is None): a modal prompt is focused by
    /// construction. Ignored whenever a chain exists — the chain
    /// decides.
    pub focused_fallback: bool,
}

impl Default for InputStyle<'_> {
    fn default() -> Self {
        InputStyle { placeholder: "", hover: false, disabled: false, focused_fallback: false }
    }
}

/// What [`draw`] tells the caller back.
pub struct FieldDraw {
    /// The field owns the keyboard this frame.
    pub focused: bool,
    /// The caret's rectangle in device px while it is drawn — where the
    /// platform anchors its IME candidate window (`set_ime_cursor_area`
    /// wants it; remember winit's call takes LOGICAL coordinates).
    pub caret: Option<Rect>,
}

/// The resolved mask glyph: `field.mask_glyph` is an enum word (open
/// sets deliver words); the map word→char lives here, with bullet as
/// the unknown-word fallback.
fn mask_char() -> char {
    static MODE: OnceLock<TokenId> = OnceLock::new();
    static IDX: OnceLock<(Option<u16>, Option<u16>)> = OnceLock::new();
    let t = theme::resolved();
    let mode = tok(&MODE, "field.mask_glyph");
    let (asterisk, block) = *IDX.get_or_init(|| {
        (theme::enum_index(mode, "asterisk"), theme::enum_index(mode, "block"))
    });
    let cur = Some(t.enum_of(mode));
    if cur == asterisk {
        '*'
    } else if cur == block {
        '\u{2588}'
    } else {
        // "bullet", plus anything the vocabulary does not name.
        '\u{2022}'
    }
}

/// The value in its display clothes: masked fields draw the mask glyph
/// once per grapheme, everything else draws itself.
fn shown(s: &str, mask: Option<char>) -> String {
    match mask {
        Some(c) => s.graphemes(true).map(|_| c).collect(),
        None => s.to_string(),
    }
}

/// The caret's blink factor for this field: `motion.caret_blink`
/// consumed with a PER-FIELD phase — the phase restarts on every
/// accepted edit, so a typing caret is always visible. Frozen fully
/// visible when the effect is off or under reduced motion (the
/// freeze-at-visible rule).
fn caret_on(model: &mut InputModel, t_now: f64) -> bool {
    static PERIOD: OnceLock<TokenId> = OnceLock::new();
    static DUTY: OnceLock<TokenId> = OnceLock::new();
    static ENABLED: OnceLock<TokenId> = OnceLock::new();
    static SCALE: OnceLock<TokenId> = OnceLock::new();
    if model.blink.0 != model.edit_seq {
        model.blink = (model.edit_seq, t_now);
    }
    let t = theme::resolved();
    let scale = t.px(tok(&SCALE, "motion.scale"));
    if scale <= 0.0 || !t.flag(tok(&ENABLED, "motion.caret_blink.enabled")) {
        return true;
    }
    let period = (t.px(tok(&PERIOD, "motion.caret_blink.period_ms")) * scale) as f64;
    if period <= 0.0 {
        return true;
    }
    let phase = ((t_now - model.blink.1) * 1000.0 % period) / period;
    (phase as f32) < t.px(tok(&DUTY, "motion.caret_blink.duty"))
}

/// The `field.caret_style` word, resolved to a shape.
#[derive(Clone, Copy, PartialEq, Eq)]
enum CaretShape {
    Bar,
    Block,
    Underline,
}

fn caret_shape() -> CaretShape {
    static MODE: OnceLock<TokenId> = OnceLock::new();
    static IDX: OnceLock<(Option<u16>, Option<u16>)> = OnceLock::new();
    let t = theme::resolved();
    let mode = tok(&MODE, "field.caret_style");
    let (block, underline) = *IDX.get_or_init(|| {
        (theme::enum_index(mode, "block"), theme::enum_index(mode, "underline"))
    });
    let cur = Some(t.enum_of(mode));
    if cur == block {
        CaretShape::Block
    } else if cur == underline {
        CaretShape::Underline
    } else {
        CaretShape::Bar
    }
}

/// Draws the field and returns what the platform glue needs. The caller
/// keeps the [`InputModel`] and routes messages into it; this only
/// renders the state and maintains the model's view bookkeeping
/// (horizontal scroll, blink phase, measure cache).
///
/// `id` is the field's stable focus path. With a live chain the field
/// registers `Caps::TEXT | GREEDY_ARROWS` (arrows edit, Tab leaves —
/// fields are not greedy for Tab); without one,
/// `style.focused_fallback` answers instead.
pub fn draw(
    ctx: &mut Ctx,
    r: Rect,
    model: &mut InputModel,
    id: FocusId,
    style: &InputStyle,
) -> FieldDraw {
    static FILL: OnceLock<TokenId> = OnceLock::new();
    static BORDER_C: OnceLock<TokenId> = OnceLock::new();
    static BORDER_W: OnceLock<TokenId> = OnceLock::new();
    static BORDER_WF: OnceLock<TokenId> = OnceLock::new();
    static CORNER: OnceLock<TokenId> = OnceLock::new();
    static CORNER_STYLE: OnceLock<TokenId> = OnceLock::new();
    static CORNER_IDX: OnceLock<(Option<u16>, Option<u16>)> = OnceLock::new();
    static SEGMENTS: OnceLock<TokenId> = OnceLock::new();
    static PAD_X: OnceLock<TokenId> = OnceLock::new();
    static SCROLL_MARGIN: OnceLock<TokenId> = OnceLock::new();
    static TEXT_C: OnceLock<TokenId> = OnceLock::new();
    static PLACEHOLDER_C: OnceLock<TokenId> = OnceLock::new();
    static CARET_C: OnceLock<TokenId> = OnceLock::new();
    static CARET_W: OnceLock<TokenId> = OnceLock::new();
    static CARET_H: OnceLock<TokenId> = OnceLock::new();
    static SEL_C: OnceLock<TokenId> = OnceLock::new();
    static SEL_TEXT_C: OnceLock<TokenId> = OnceLock::new();
    static PRE_C: OnceLock<TokenId> = OnceLock::new();
    static PRE_UL: OnceLock<TokenId> = OnceLock::new();
    static ROLE: OnceLock<TokenId> = OnceLock::new();
    static CLASS: OnceLock<Option<u16>> = OnceLock::new();

    let f = ctx
        .focus
        .as_deref_mut()
        .map(|fc| fc.register(id, r, Caps::TEXT | Caps::GREEDY_ARROWS));
    let focused = f.map(|f| f.focused).unwrap_or(style.focused_fallback) && !style.disabled;

    let t = theme::resolved();

    // ---- the box ----------------------------------------------------
    // `field.corner_style` follows the button's, which follows the
    // panel's: a theme that chamfers its controls must not be left with
    // the one control you type into still rounded.
    let cut = super::window::corner_style(t, tok(&CORNER_STYLE, "field.corner_style"), &CORNER_IDX);
    // The radius goes through `Corner::sized`, which is where §5.0's
    // `pill` stops being a negative number and becomes half this box —
    // a clamp at zero would spell it "square" and never say so.
    let corner = Corner::sized(cut, t.px(tok(&CORNER, "field.corner")), r);
    let c = [corner; 4];
    let seg = super::window::corner_segments(t, &SEGMENTS, corner.size);
    ctx.dl.ring_fill(r, &c, seg, col(t.bed(tok(&FILL, "component.field.fill"))));
    // The ladder's wash over the bed (idle is a wash too, the button
    // idiom); the field has no press/drag rung of its own.
    let state = if style.disabled {
        State::Disabled
    } else if style.hover {
        State::Hover
    } else {
        State::Idle
    };
    let wash = match *CLASS.get_or_init(|| theme::class_id("field")) {
        Some(cl) => t.class_state(cl, state),
        None => StateStyle::RAW,
    };
    ctx.dl.ring_fill(r, &c, seg, col(wash.fill));
    // The ring: colour from the component group, width stepping up
    // while the field holds the caret (`field.border_focused`).
    let bw = if focused {
        t.px(tok(&BORDER_WF, "field.border_focused"))
    } else {
        t.px(tok(&BORDER_W, "field.border"))
    }
    .max(0.0);
    if bw > 0.0 {
        ctx.dl.ring(r, &c, seg, bw, col(t.color(tok(&BORDER_C, "component.field.border"))));
    }

    // ---- type metrics -----------------------------------------------
    // The bound role (`field.role`, an open word set). Role::px carries
    // the theme's size and the panel's container query; the user's
    // UIFontSize= does NOT go through the shrink slot — it is
    // `metric.ui_scale`, the bake already applied it, and a second
    // multiply squares it. The same arithmetic as every other object
    // (button.rs).
    let role = ui::bound_role(&ROLE, "field.role");
    let px = role.px(ctx, 1.0);
    let track = role.tracking_px(px);
    let leading = role.leading();
    let line_h = px * leading;
    let ty = r.y + (r.h - line_h) / 2.0;

    let pad = t.px(tok(&PAD_X, "field.pad_x")).max(0.0);
    let area = Rect::new(r.x + pad, r.y, (r.w - 2.0 * pad).max(1.0), r.h);

    // ---- content ----------------------------------------------------
    let mask = model.mask.then(mask_char);
    // The preedit exists only while the field is focused — the IME
    // composes into the control that owns the keyboard.
    let pre = if focused { model.preedit.clone() } else { None };
    let before = shown(&model.text[..model.cursor], mask);
    let (pre_disp, pre_cursor) = match &pre {
        Some((p, range)) => {
            let d = shown(p, mask);
            // The caret inside the composition: at the range start when
            // the platform names one (and the field is unmasked — a
            // masked composition keeps the caret at its end), else at
            // the composition's end. The platform's offset is clamped
            // onto a char boundary — an IME's arithmetic is not to be
            // trusted with a slice.
            let cur = match (mask, range) {
                (None, Some((a, _))) => {
                    let mut cur = (*a).min(d.len());
                    while cur > 0 && !d.is_char_boundary(cur) {
                        cur -= 1;
                    }
                    cur
                }
                _ => d.len(),
            };
            (d, cur)
        }
        None => (String::new(), 0),
    };
    let after = shown(&model.text[model.cursor..], mask);
    let mut disp = String::with_capacity(before.len() + pre_disp.len() + after.len());
    disp.push_str(&before);
    disp.push_str(&pre_disp);
    disp.push_str(&after);

    let empty = disp.is_empty();
    // Selection in display space — not drawn while a composition is
    // live (the commit replaces it) or the field is unfocused.
    let sel_disp = if focused && pre.is_none() {
        model.selection().map(|(a, b)| {
            (shown(&model.text[..a], mask).len(), shown(&model.text[..b], mask).len())
        })
    } else {
        None
    };
    let caret_disp = before.len() + pre_cursor;

    // ---- measures (cached per edit, §3.7) ---------------------------
    let key =
        (theme::epoch(), model.edit_seq, model.cursor, model.sel_anchor, px.to_bits());
    if model.cache.key != key {
        let caret_x = ctx.fonts.measure(FONT_UI, px, &disp[..caret_disp], track);
        let sel_x = sel_disp.map(|(a, b)| {
            (
                ctx.fonts.measure(FONT_UI, px, &disp[..a], track),
                ctx.fonts.measure(FONT_UI, px, &disp[..b], track),
            )
        });
        let text_w = ctx.fonts.measure(FONT_UI, px, &disp, track);
        model.cache = ViewCache { key, caret_x, sel_x, text_w };
    }
    let ViewCache { caret_x, sel_x, text_w, .. } = model.cache;

    // ---- horizontal scroll ------------------------------------------
    // Keep the caret `field.scroll_margin` clear of either edge; a
    // value shorter than the field never scrolls.
    let margin = t.px(tok(&SCROLL_MARGIN, "field.scroll_margin")).max(0.0);
    let max_scroll = (text_w - area.w).max(0.0);
    if focused {
        let m = margin.min(area.w / 2.0);
        if caret_x - model.scroll_px > area.w - m {
            model.scroll_px = caret_x - (area.w - m);
        }
        if caret_x - model.scroll_px < m {
            model.scroll_px = caret_x - m;
        }
    }
    model.scroll_px = model.scroll_px.clamp(0.0, max_scroll);
    let x0 = area.x - model.scroll_px;

    // ---- text -------------------------------------------------------
    ctx.dl.push_clip(area.x, r.y, area.w, r.h);
    if empty {
        model.scroll_px = 0.0;
        if !style.placeholder.is_empty() {
            ctx.dl.text(
                ctx.fonts,
                FONT_UI,
                px,
                area.x,
                ty,
                style.placeholder,
                col(t.color(tok(&PLACEHOLDER_C, "component.field.placeholder"))),
                track,
            );
        }
    } else {
        let ink = col(t.color(tok(&TEXT_C, "component.field.text")));
        let sel_fill = col(t.color(tok(&SEL_C, "component.field.selection")));
        let sel_ink = col(t.color(tok(&SEL_TEXT_C, "component.field.selection_text")));
        // The selection wash first, under its own ink.
        if let (Some(_), Some((xa, xb))) = (sel_disp, sel_x) {
            ctx.dl.rect(x0 + xa, ty, xb - xa, line_h, sel_fill);
        }
        // The runs: plain / selected / plain, or around the preedit.
        let mut runs: Vec<(usize, usize, Color, bool)> = Vec::new();
        if let Some((a, b)) = sel_disp {
            runs.push((0, a, ink, false));
            runs.push((a, b, sel_ink, false));
            runs.push((b, disp.len(), ink, false));
        } else if !pre_disp.is_empty() {
            let p0 = before.len();
            let p1 = p0 + pre_disp.len();
            let pre_ink = col(t.color(tok(&PRE_C, "component.field.preedit")));
            runs.push((0, p0, ink, false));
            runs.push((p0, p1, pre_ink, true));
            runs.push((p1, disp.len(), ink, false));
        } else {
            runs.push((0, disp.len(), ink, false));
        }
        for (a, b, run_ink, is_pre) in runs {
            if a >= b {
                continue;
            }
            let rx = x0 + ctx.fonts.measure(FONT_UI, px, &disp[..a], track);
            ctx.dl.text(ctx.fonts, FONT_UI, px, rx, ty, &disp[a..b], run_ink, track);
            if is_pre {
                // The composition underline, `field.preedit_underline`
                // thick, in the composition's own ink.
                let ul = t.px(tok(&PRE_UL, "field.preedit_underline")).max(0.0);
                if ul > 0.0 {
                    let w = ctx.fonts.measure(FONT_UI, px, &disp[a..b], track);
                    let pre_ink = col(t.color(tok(&PRE_C, "component.field.preedit")));
                    ctx.dl.rect(rx, ty + line_h - ul, w, ul, pre_ink);
                }
            }
        }
    }

    // ---- caret ------------------------------------------------------
    let mut caret_rect = None;
    if focused {
        let ch = t.px(tok(&CARET_H, "field.caret_h")).max(0.0).min(r.h);
        let cw = t.px(tok(&CARET_W, "field.caret_w")).max(0.0);
        if cw > 0.0 && ch > 0.0 {
            let cx = x0 + caret_x;
            // A block or underline caret is as wide as the grapheme it
            // sits on; past the end it falls back to the space advance
            // (the terminal's rule) — a font metric, not a design.
            let gw = match caret_shape() {
                CaretShape::Bar => cw,
                _ => {
                    let g = disp[caret_disp.min(disp.len())..].graphemes(true).next();
                    match g {
                        Some(g) => ctx.fonts.measure(FONT_UI, px, g, track),
                        None => ctx.fonts.measure(FONT_UI, px, " ", track),
                    }
                }
            };
            let rect = match caret_shape() {
                CaretShape::Bar => Rect::new(cx, r.y + (r.h - ch) / 2.0, cw, ch),
                CaretShape::Block => Rect::new(cx, r.y + (r.h - ch) / 2.0, gw, ch),
                CaretShape::Underline => {
                    Rect::new(cx, r.y + (r.h + ch) / 2.0 - cw, gw, cw)
                }
            };
            if caret_on(model, ctx.t) {
                ctx.dl.rect(
                    rect.x,
                    rect.y,
                    rect.w,
                    rect.h,
                    col(t.color(tok(&CARET_C, "component.field.caret"))),
                );
            }
            // The IME anchor is the caret's box whether or not the
            // blink has it lit this instant.
            caret_rect = Some(rect);
        }
    }
    ctx.dl.pop_clip();

    if f.map_or(false, |f| f.ring) {
        focus_ring::draw(ctx, r);
    }
    FieldDraw { focused, caret: caret_rect }
}

/// The VALUE byte offset a click at window-x `x` means — the nearest
/// grapheme boundary, measured in the field's display clothes (masked
/// fields measure the mask glyph). A live preedit is ignored: pointer
/// placement happens between compositions. Feed the result to
/// [`InputMsg::Point`] / [`InputMsg::PointWord`].
pub fn hit(ctx: &mut Ctx, r: Rect, model: &InputModel, x: f32) -> usize {
    static PAD_X: OnceLock<TokenId> = OnceLock::new();
    static ROLE: OnceLock<TokenId> = OnceLock::new();
    let t = theme::resolved();
    let role = ui::bound_role(&ROLE, "field.role");
    // The measuring twin of the draw above, so the same 1.0.
    let px = role.px(ctx, 1.0);
    let track = role.tracking_px(px);
    let pad = t.px(tok(&PAD_X, "field.pad_x")).max(0.0);
    let pos = x - (r.x + pad) + model.scroll_px;
    let mask = model.mask.then(mask_char);
    let mut acc = 0.0;
    for (i, g) in model.value().grapheme_indices(true) {
        let w = match mask {
            Some(c) => {
                let mut buf = [0u8; 4];
                ctx.fonts.measure(FONT_UI, px, c.encode_utf8(&mut buf), track)
            }
            None => ctx.fonts.measure(FONT_UI, px, g, track),
        };
        if pos < acc + w * 0.5 {
            return i;
        }
        acc += w;
    }
    model.value().len()
}

// ---------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(key: Key, mods: Mods) -> KeyEv {
        KeyEv { key, mods, repeat: false, text: None }
    }

    fn typed(m: &mut InputModel, s: &str) {
        for g in s.graphemes(true) {
            assert_eq!(m.apply(InputMsg::Insert(g.to_string())), InputEdited::Edited);
        }
    }

    // ---- graphemes and words ----

    #[test]
    fn caret_moves_by_graphemes_not_chars() {
        let mut m = InputModel::new();
        // "e" + combining acute (2 chars, 1 grapheme), then a ZWJ
        // family (7 chars, 1 grapheme).
        m.set_value("e\u{301}\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F466}x");
        assert_eq!(m.cursor(), m.value().len());
        m.apply(InputMsg::Move(Motion::Left, false));
        m.apply(InputMsg::Move(Motion::Left, false));
        // Two steps back: over 'x' and over the whole ZWJ cluster.
        assert_eq!(m.cursor(), "e\u{301}".len());
        m.apply(InputMsg::Move(Motion::Left, false));
        assert_eq!(m.cursor(), 0);
        m.apply(InputMsg::Move(Motion::Right, false));
        assert_eq!(m.cursor(), "e\u{301}".len());
    }

    #[test]
    fn backspace_and_delete_remove_whole_clusters() {
        let mut m = InputModel::new();
        m.set_value("ae\u{301}b");
        assert_eq!(m.apply(InputMsg::Backspace), InputEdited::Edited);
        assert_eq!(m.value(), "ae\u{301}");
        assert_eq!(m.apply(InputMsg::Backspace), InputEdited::Edited);
        assert_eq!(m.value(), "a");
        m.apply(InputMsg::Move(Motion::Home, false));
        assert_eq!(m.apply(InputMsg::Delete), InputEdited::Edited);
        assert_eq!(m.value(), "");
        assert_eq!(m.apply(InputMsg::Delete), InputEdited::None);
        assert_eq!(m.apply(InputMsg::Backspace), InputEdited::None);
    }

    #[test]
    fn torn_offsets_land_on_boundaries() {
        let s = "a\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F466}b";
        // Any byte inside the cluster floors to the cluster's start.
        for at in 2..(1 + "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F466}".len()) {
            let f = floor_grapheme(s, at);
            assert!(f == 1, "offset {at} floored to {f}");
        }
        assert_eq!(floor_grapheme(s, 0), 0);
        assert_eq!(floor_grapheme(s, s.len() + 10), s.len());
    }

    #[test]
    fn word_motion_skips_separators() {
        let mut m = InputModel::new();
        m.set_value("foo  bar-baz");
        m.apply(InputMsg::Move(Motion::Home, false));
        m.apply(InputMsg::Move(Motion::WordRight, false));
        assert_eq!(m.cursor(), 3); // after "foo"
        m.apply(InputMsg::Move(Motion::WordRight, false));
        assert_eq!(m.cursor(), 8); // after "bar"
        m.apply(InputMsg::Move(Motion::WordRight, false));
        assert_eq!(m.cursor(), 12); // after "baz"
        m.apply(InputMsg::Move(Motion::WordLeft, false));
        assert_eq!(m.cursor(), 9); // start of "baz"
        m.apply(InputMsg::Move(Motion::WordLeft, false));
        assert_eq!(m.cursor(), 5); // start of "bar"
        m.apply(InputMsg::Move(Motion::WordLeft, false));
        assert_eq!(m.cursor(), 0);
    }

    #[test]
    fn delete_word_back_eats_one_word() {
        let mut m = InputModel::new();
        m.set_value("foo bar");
        assert_eq!(m.apply(InputMsg::DeleteWordBack), InputEdited::Edited);
        assert_eq!(m.value(), "foo ");
        assert_eq!(m.apply(InputMsg::DeleteWordBack), InputEdited::Edited);
        assert_eq!(m.value(), "");
    }

    // ---- selection ----

    #[test]
    fn shift_motion_selects_and_insert_replaces() {
        let mut m = InputModel::new();
        m.set_value("hello");
        m.apply(InputMsg::Move(Motion::Home, false));
        m.apply(InputMsg::Move(Motion::Right, true));
        m.apply(InputMsg::Move(Motion::Right, true));
        assert_eq!(m.selected_text(), Some("he"));
        assert_eq!(m.apply(InputMsg::Insert("HE".into())), InputEdited::Edited);
        assert_eq!(m.value(), "HEllo");
        assert_eq!(m.selection(), None);
        assert_eq!(m.cursor(), 2);
    }

    #[test]
    fn plain_arrow_collapses_selection_to_its_edge() {
        let mut m = InputModel::new();
        m.set_value("abcd");
        m.apply(InputMsg::Move(Motion::Home, false));
        m.apply(InputMsg::Move(Motion::Right, true));
        m.apply(InputMsg::Move(Motion::Right, true));
        m.apply(InputMsg::Move(Motion::Left, false));
        assert_eq!((m.cursor(), m.selection()), (0, None));
        m.apply(InputMsg::Move(Motion::End, false));
        m.apply(InputMsg::Move(Motion::Left, true));
        m.apply(InputMsg::Move(Motion::Right, false));
        assert_eq!((m.cursor(), m.selection()), (4, None));
    }

    #[test]
    fn select_all_and_backspace_clear_the_field() {
        let mut m = InputModel::new();
        m.set_value("hello");
        m.apply(InputMsg::SelectAll);
        assert_eq!(m.selected_text(), Some("hello"));
        m.apply(InputMsg::Backspace);
        assert_eq!(m.value(), "");
    }

    #[test]
    fn point_and_point_word() {
        let mut m = InputModel::new();
        m.set_value("foo bar");
        m.apply(InputMsg::Point { at: 1, extend: false });
        assert_eq!(m.cursor(), 1);
        m.apply(InputMsg::Point { at: 3, extend: true });
        assert_eq!(m.selected_text(), Some("oo"));
        m.apply(InputMsg::PointWord { at: 5 });
        assert_eq!(m.selected_text(), Some("bar"));
    }

    // ---- undo ----

    #[test]
    fn typed_word_undoes_as_one_group() {
        let mut m = InputModel::new();
        typed(&mut m, "abc");
        assert_eq!(m.value(), "abc");
        assert_eq!(m.apply(InputMsg::Undo), InputEdited::Edited);
        assert_eq!(m.value(), "");
        assert_eq!(m.apply(InputMsg::Redo), InputEdited::Edited);
        assert_eq!(m.value(), "abc");
    }

    #[test]
    fn space_seals_the_typing_group() {
        let mut m = InputModel::new();
        typed(&mut m, "ab");
        m.apply(InputMsg::Insert(" ".into()));
        typed(&mut m, "c");
        assert_eq!(m.value(), "ab c");
        m.apply(InputMsg::Undo);
        assert_eq!(m.value(), "ab ");
        m.apply(InputMsg::Undo);
        assert_eq!(m.value(), "ab");
        m.apply(InputMsg::Undo);
        assert_eq!(m.value(), "");
    }

    #[test]
    fn motion_seals_the_typing_group() {
        let mut m = InputModel::new();
        typed(&mut m, "ab");
        m.apply(InputMsg::Move(Motion::Left, false));
        m.apply(InputMsg::Move(Motion::End, false));
        typed(&mut m, "cd");
        assert_eq!(m.value(), "abcd");
        m.apply(InputMsg::Undo);
        assert_eq!(m.value(), "ab");
        m.apply(InputMsg::Undo);
        assert_eq!(m.value(), "");
    }

    #[test]
    fn undo_restores_the_caret_and_new_edits_clear_redo() {
        let mut m = InputModel::new();
        typed(&mut m, "abc");
        m.apply(InputMsg::Undo);
        assert_eq!((m.value(), m.cursor()), ("", 0));
        typed(&mut m, "x");
        assert_eq!(m.apply(InputMsg::Redo), InputEdited::None, "redo branch cleared");
        assert_eq!(m.value(), "x");
    }

    #[test]
    fn a_paste_sized_insert_is_its_own_group() {
        let mut m = InputModel::new();
        typed(&mut m, "ab");
        m.apply(InputMsg::Insert("pasted".into()));
        m.apply(InputMsg::Undo);
        assert_eq!(m.value(), "ab");
    }

    #[test]
    fn undo_depth_is_capped() {
        let mut m = InputModel::new();
        for _ in 0..(UNDO_DEPTH + 10) {
            // Each space-letter pair makes two groups; more than
            // enough to overflow the stack.
            m.apply(InputMsg::Insert(" ".into()));
        }
        let mut undos = 0;
        while m.apply(InputMsg::Undo) == InputEdited::Edited {
            undos += 1;
        }
        assert_eq!(undos, UNDO_DEPTH);
        assert!(!m.value().is_empty(), "the oldest states fell off the stack");
    }

    #[test]
    fn set_value_clears_history() {
        let mut m = InputModel::new();
        typed(&mut m, "abc");
        m.set_value("fresh");
        assert_eq!(m.apply(InputMsg::Undo), InputEdited::None);
        assert_eq!(m.cursor(), 5);
    }

    // ---- validator and max_len ----

    #[test]
    fn rejected_edits_change_nothing() {
        fn lower(c: char) -> bool {
            c.is_ascii_lowercase()
        }
        let mut m = InputModel::new().with_validator(Validator::Charset(lower));
        assert_eq!(m.apply(InputMsg::Insert("ab".into())), InputEdited::Edited);
        assert_eq!(m.apply(InputMsg::Insert("C".into())), InputEdited::Rejected);
        assert_eq!(m.value(), "ab");
        assert_eq!(m.cursor(), 2);
    }

    #[test]
    fn digits_validator_and_max_len() {
        let mut m = InputModel::new().with_validator(Validator::Digits).with_max_len(3);
        assert_eq!(m.apply(InputMsg::Insert("12".into())), InputEdited::Edited);
        assert_eq!(m.apply(InputMsg::Insert("x".into())), InputEdited::Rejected);
        assert_eq!(m.apply(InputMsg::Insert("34".into())), InputEdited::Rejected);
        assert_eq!(m.apply(InputMsg::Insert("3".into())), InputEdited::Edited);
        assert_eq!(m.value(), "123");
    }

    #[test]
    fn max_len_counts_chars_not_bytes() {
        let mut m = InputModel::new().with_max_len(2);
        assert_eq!(m.apply(InputMsg::Insert("źż".into())), InputEdited::Edited);
        assert_eq!(m.apply(InputMsg::Insert("a".into())), InputEdited::Rejected);
    }

    #[test]
    fn custom_validator_judges_the_whole_value() {
        let mut m = InputModel::new()
            .with_validator(Validator::Custom(Box::new(|s: &str| !s.starts_with(' '))));
        assert_eq!(m.apply(InputMsg::Insert("a".into())), InputEdited::Edited);
        m.apply(InputMsg::Move(Motion::Home, false));
        assert_eq!(m.apply(InputMsg::Insert(" ".into())), InputEdited::Rejected);
        assert_eq!(m.value(), "a");
    }

    // ---- preedit ----

    #[test]
    fn preedit_never_touches_value_or_undo() {
        let mut m = InputModel::new();
        typed(&mut m, "ab");
        assert_eq!(
            m.apply(InputMsg::Preedit("ちょ".into(), Some((0, 3)))),
            InputEdited::Moved
        );
        assert_eq!(m.value(), "ab", "composing text is not text");
        assert!(m.has_preedit());
        // Commit arrives as a plain insert; the preedit was already
        // cleared by the platform glue (or ends here first).
        m.apply(InputMsg::PreeditEnd);
        assert_eq!(m.apply(InputMsg::Insert("ちょ".into())), InputEdited::Edited);
        assert_eq!(m.value(), "abちょ");
        m.apply(InputMsg::Undo);
        assert_eq!(m.value(), "ab", "the commit is one undo step; the preedit none");
    }

    #[test]
    fn escape_cancels_preedit_only_then_bubbles() {
        let mut m = InputModel::new();
        typed(&mut m, "x");
        m.apply(InputMsg::Preedit("あ".into(), None));
        assert_eq!(m.apply(InputMsg::Escape), InputEdited::Moved);
        assert!(!m.has_preedit());
        assert_eq!(m.value(), "x");
        assert_eq!(m.apply(InputMsg::Escape), InputEdited::Cancel);
    }

    #[test]
    fn enter_mid_composition_does_not_submit() {
        let mut m = InputModel::new();
        m.apply(InputMsg::Preedit("あ".into(), None));
        assert_eq!(m.apply(InputMsg::Enter), InputEdited::Moved);
        assert_eq!(m.apply(InputMsg::Enter), InputEdited::Submit);
    }

    #[test]
    fn empty_preedit_is_a_cancel() {
        let mut m = InputModel::new();
        m.apply(InputMsg::Preedit("あ".into(), None));
        assert_eq!(m.apply(InputMsg::Preedit(String::new(), None)), InputEdited::Moved);
        assert!(!m.has_preedit());
        assert_eq!(m.apply(InputMsg::Preedit(String::new(), None)), InputEdited::None);
    }

    #[test]
    fn preedit_survives_no_validator_and_no_max_len() {
        let mut m = InputModel::new().with_validator(Validator::Digits).with_max_len(1);
        // The composition may hold anything at any length; only the
        // commit is judged.
        assert_eq!(
            m.apply(InputMsg::Preedit("abcdef".into(), None)),
            InputEdited::Moved
        );
        assert_eq!(m.apply(InputMsg::Insert("abcdef".into())), InputEdited::Rejected);
        assert_eq!(m.apply(InputMsg::Insert("7".into())), InputEdited::Edited);
    }

    // ---- clipboard intents ----

    #[test]
    fn copy_and_cut_are_intents_not_calls() {
        let mut m = InputModel::new();
        m.set_value("hello");
        m.apply(InputMsg::Move(Motion::Home, false));
        m.apply(InputMsg::Move(Motion::WordRight, true));
        assert_eq!(
            m.apply(InputMsg::Copy),
            InputEdited::CopyRequest { text: "hello".into(), cut: false }
        );
        assert_eq!(m.value(), "hello");
        assert_eq!(
            m.apply(InputMsg::Cut),
            InputEdited::CopyRequest { text: "hello".into(), cut: true }
        );
        assert_eq!(m.value(), "");
    }

    #[test]
    fn copy_without_selection_is_nothing() {
        let mut m = InputModel::new();
        m.set_value("hello");
        assert_eq!(m.apply(InputMsg::Copy), InputEdited::None);
        assert_eq!(m.apply(InputMsg::Cut), InputEdited::None);
    }

    #[test]
    fn masked_fields_never_answer_copy() {
        let mut m = InputModel::new().with_mask(true);
        m.set_value("secret");
        m.apply(InputMsg::SelectAll);
        assert_eq!(m.apply(InputMsg::Copy), InputEdited::None);
        assert_eq!(m.apply(InputMsg::Cut), InputEdited::None);
        assert_eq!(m.value(), "secret");
    }

    #[test]
    fn paste_asks_the_caller() {
        let mut m = InputModel::new();
        assert_eq!(m.apply(InputMsg::Paste), InputEdited::PasteRequest);
        // The caller resolves the intent and sends the text back.
        assert_eq!(m.apply(InputMsg::Insert("clip".into())), InputEdited::Edited);
        assert_eq!(m.value(), "clip");
    }

    // ---- key translation ----

    #[test]
    fn keys_translate_to_field_messages() {
        assert_eq!(
            key_msg(&ev(Key::Left, Mods::SHIFT)),
            Some(InputMsg::Move(Motion::Left, true))
        );
        assert_eq!(
            key_msg(&ev(Key::Right, Mods::CTRL)),
            Some(InputMsg::Move(Motion::WordRight, false))
        );
        assert_eq!(key_msg(&ev(Key::Backspace, Mods::CTRL)), Some(InputMsg::DeleteWordBack));
        assert_eq!(key_msg(&ev(Key::Char('a'), Mods::CTRL)), Some(InputMsg::SelectAll));
        assert_eq!(key_msg(&ev(Key::Char('z'), Mods::CTRL)), Some(InputMsg::Undo));
        assert_eq!(
            key_msg(&ev(Key::Char('Z'), Mods::CTRL | Mods::SHIFT)),
            Some(InputMsg::Redo)
        );
        assert_eq!(key_msg(&ev(Key::Char('v'), Mods::CTRL)), Some(InputMsg::Paste));
        assert_eq!(key_msg(&ev(Key::Enter, Mods::NONE)), Some(InputMsg::Enter));
        assert_eq!(key_msg(&ev(Key::Escape, Mods::NONE)), Some(InputMsg::Escape));
        // Tab bubbles — the chain keeps it, fields are not greedy.
        assert_eq!(key_msg(&ev(Key::Tab, Mods::NONE)), None);
        // Unknown ctrl chords bubble too (the app's shortcuts).
        assert_eq!(key_msg(&ev(Key::Char('q'), Mods::CTRL)), None);
    }

    #[test]
    fn typed_text_prefers_the_platform_string() {
        let mut e = ev(Key::Char('e'), Mods::NONE);
        e.text = Some("é".to_string());
        assert_eq!(key_msg(&e), Some(InputMsg::Insert("é".into())));
        // Without produced text the bare character types itself.
        assert_eq!(
            key_msg(&ev(Key::Char('e'), Mods::NONE)),
            Some(InputMsg::Insert("e".into()))
        );
        assert_eq!(
            key_msg(&ev(Key::Space, Mods::NONE)),
            Some(InputMsg::Insert(" ".into()))
        );
        // Control characters never insert.
        let mut c = ev(Key::Char('c'), Mods::CTRL);
        c.text = Some("\u{3}".to_string());
        assert_eq!(key_msg(&c), Some(InputMsg::Copy));
    }
}
