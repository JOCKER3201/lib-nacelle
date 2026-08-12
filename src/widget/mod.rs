//! The widget contract.
//!
//! Nothing here is a widget. This is the interface widgets are written
//! against and that an application drives them through:
//!
//!   * [`Widget`] — the one interface every widget implements, so the
//!     application can draw and route input to all of them uniformly
//!     instead of knowing each by name.
//!   * [`Host`] — everything a widget may read (telemetry, the terminal
//!     state, the session tabs). Widgets never reach for the system
//!     themselves; whatever they need arrives here.
//!   * [`Action`] — everything a widget may ask the application to do.
//!     A widget cannot exit the program or spawn a shell; it asks.
//!   * [`ui`](crate::ui) — the shared drawing vocabulary widgets are
//!     built from.
//!
//! The widgets themselves live outside this crate: each is a file in the
//! widgets directory — a Rhai script rendered through [`script`], or a
//! compiled library loaded through [`plugin`]. Nothing here depends on
//! any of them, so a different set of widgets needs no change to the
//! toolkit.
//!
//! [`script`]: crate::script
//! [`plugin`]: crate::plugin

use crate::telemetry::Snapshot;
use crate::term::{SelKind, Term};
use crate::{Ctx, Rect};
use std::path::PathBuf;

/// Everything a widget may read about the machine and the session.
///
/// Passing this in — rather than letting widgets query the system — is
/// what keeps them platform-independent: the application fills it with
/// whatever its platform can collect.
pub struct Host<'a> {
    /// System telemetry sample for this frame.
    pub snap: &'a Snapshot,
    /// The active terminal, for widgets that render it.
    pub term: Option<&'a Term>,
    /// Which session tabs are occupied.
    pub tabs: &'a [bool],
    /// Index of the active tab.
    pub tab_active: usize,
    /// Working directory of the active shell, for widgets that follow it.
    pub shell_cwd: Option<PathBuf>,
    /// Seconds since the program started, for anything that animates.
    pub t: f64,
    /// Window size in px. A few widgets size themselves against the
    /// window rather than their own panel — the terminal's tab strip and
    /// the control panel's buttons — and hit-testing them needs the same
    /// number the drawing used.
    pub window: (f32, f32),
}

/// Where a pointer capture stands. Delivered through [`Widget::drag`]:
/// the host captures the pointer on a press the widget's `drag(Begin)`
/// accepted, routes every motion as `Move`, and the release as `End` —
/// ONE capture path, which F2's press/release will be synthesized into
/// rather than duplicating (F1 §5.1 red-team).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DragPhase {
    Begin,
    Move,
    End,
}

/// What a [`Action::TermSelect`] does to the selection. The kind rides
/// on `Begin` — cells, or the word/line kinds a double or triple click
/// means (the HOST tracks click counts; a widget has no way to).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SelectOp {
    Begin(SelKind),
    Extend,
    End,
}

/// What a widget asks the application to do. A widget never acts on the
/// system itself — it returns one of these and the application decides.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    None,
    /// Send bytes to the active shell (the on-screen keyboard).
    Bytes(Vec<u8>),
    /// Change directory in the active shell.
    OpenDir(PathBuf),
    /// Open a file with its associated application.
    OpenFile(PathBuf),
    /// Switch to (or start) a session tab.
    SelectTab(usize),
    /// Quit the program.
    Exit,
    /// Open the settings window.
    OpenSettings,
    /// Scroll the terminal's scrollback by this many lines. The terminal
    /// state belongs to the session, not to the widget drawing it, so
    /// the widget asks rather than scrolls.
    ScrollTerminal(i32),
    /// The shell widget translated a drag into cell coordinates; the
    /// HOST applies it to the session's `Term` (widgets get `&Term`,
    /// never `&mut`). `row` is a row of the view THE WIDGET DREW, and
    /// `base` is the line id of that view's first row, echoed from
    /// `term_view`'s reply — the host resolves `base + row`, never
    /// "row against the terminal now", because a PTY feed between the
    /// draw and this event scrolls the screen and would shift every
    /// resolved row by N (F1 §2.7 red-team).
    TermSelect { op: SelectOp, col: usize, row: usize, base: u64 },
    /// Paste the PRIMARY selection into the active session — the
    /// middle-click convention. The host does the clipboard work; a
    /// widget only ever asks.
    PastePrimary,
    /// The gesture is mine, and I want nothing. The answer a widget
    /// gives [`Widget::drag`]`(Begin)` when the press landed on
    /// something it drives itself — a scroll thumb, a column edge — so
    /// the host captures the pointer and neither the board nor the
    /// click path sees the rest of the gesture. Not a request: the
    /// application does nothing with it but remember who owns the hand.
    Capture,
}

/// Which window controls a panel's title band carries, right-aligned in
/// the band. Nothing declares any yet; the set exists so the contract
/// does not need a second break when one does.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ButtonSet {
    #[default]
    None,
    Close,
    MinClose,
    MinMaxClose,
}

/// What a widget tells the host to draw AROUND it (u2 §4).
///
/// The container itself — material, ring, title band, content box — is
/// the host's; a widget only declares the band's two texts and, one day,
/// its controls and alarm state. `None` everywhere means "no band": the
/// panel is the plain container and the widget gets the whole content
/// box, which is correct for the widgets that show no heading today.
#[derive(Clone, Debug, Default)]
pub struct Chrome {
    /// The band's left text — the widget's title (`UPTIME`, `CPU`).
    pub title: Option<String>,
    /// The band's right text — a cwd, a CPU model, an interface name.
    /// Trimmed by the HOST to the room the title leaves, from the left,
    /// so the tail of a path survives.
    pub right: Option<String>,
    /// Window controls the band carries. Declared, not yet drawn.
    pub buttons: ButtonSet,
    /// An alarmed panel: an index into the severity set, tinting the
    /// container (image 4's whole screen). None = the resting look.
    /// Declared, not yet consumed by the host.
    pub severity: Option<u32>,
}

impl Chrome {
    /// No band, no controls, no alarm — the default for every widget
    /// that does not override [`Widget::chrome`].
    pub fn none() -> Chrome {
        Chrome::default()
    }
}

/// The interface every widget implements.
///
/// Only `draw` is required. A widget that shows data implements nothing
/// else; the interactive ones override what they need, which is why the
/// application can treat all of them the same way.
pub trait Widget {
    /// Draw into the rectangle the layout engine assigned.
    ///
    /// `r` is the CONTENT BOX: the host has already drawn the panel
    /// container around it (fill, ring, title band) and deflated the
    /// rect past the border, the content padding and the band. A widget
    /// draws content, never chrome.
    fn draw(&mut self, ctx: &mut Ctx, r: Rect, host: &Host);

    /// What the host should draw around this widget this frame. Asked
    /// once per frame, before `draw`, from the same host data. The
    /// default is no chrome at all, which keeps this a non-breaking
    /// change: a widget that says nothing gets a plain container.
    fn chrome(&mut self, _ctx: &mut Ctx, _host: &Host) -> Chrome {
        Chrome::none()
    }

    /// A click landed inside the widget's rectangle.
    fn click(&mut self, _x: f32, _y: f32, _r: Rect, _host: &Host) -> Action {
        Action::None
    }

    /// The wheel turned over the widget.
    fn wheel(&mut self, _dy: f32, _r: Rect, _host: &Host) -> Action {
        Action::None
    }

    /// Whether a control of this widget lies under the point — the
    /// answer an application needs BEFORE it asks anything else, to
    /// turn the pointer into a hand the same frame it arrives.
    ///
    /// The widget is the only one that can answer: the rectangles are
    /// its own, drawn from its own tokens, and an application that
    /// computed them a second time would be duplicating a widget's
    /// geometry and drifting from it. Hover carries no drawing context
    /// and no session data — it is a question about pixels — so only
    /// the rect and the window size are passed. The default is no: a
    /// widget with nothing to click keeps the ordinary cursor.
    fn pointer(&mut self, _x: f32, _y: f32, _r: Rect, _window: (f32, f32)) -> bool {
        false
    }

    /// A pointer drag over the widget — the host's single capture path
    /// (see [`DragPhase`]). `Begin` is the press: a widget that answers
    /// [`Action::None`] declines the capture and the press falls back to
    /// the ordinary click delivery, which is why every existing widget
    /// keeps working untouched. `Move` and `End` follow only an accepted
    /// `Begin`, and their coordinates may leave `r` — a selection
    /// dragged past the edge clamps in the widget, not in the host.
    fn drag(&mut self, _p: DragPhase, _x: f32, _y: f32, _r: Rect, _host: &Host) -> Action {
        Action::None
    }

    /// The pointer button went down over the widget.
    ///
    /// The front of the SAME gesture [`Widget::drag`] carries, never a
    /// second capture path: the host delivers this, then asks
    /// `drag(Begin)`, and that answer alone decides who owns what
    /// follows. [`Action::Capture`] from here means nothing and does
    /// nothing — exactly what it means from [`Widget::click`].
    ///
    /// What it is for is the half of a press a capture cannot express:
    /// the PRESS rung of the state ladder (a control that darkens while
    /// it is held), and a grab that has to know it started even when the
    /// widget did not want the drag. The default does nothing, so every
    /// widget written before this existed behaves exactly as it did.
    fn press(&mut self, _x: f32, _y: f32, _r: Rect, _host: &Host) -> Action {
        Action::None
    }

    /// The pointer button came up.
    ///
    /// Delivered after `drag(End)` when a capture was in force, and
    /// before [`Widget::click`] when none was — so a widget tracking its
    /// own down/up pair has closed it before the click that concludes it
    /// arrives. The coordinates may lie outside `r`: a button pressed
    /// and released off its own edge is a press the user took back, and
    /// only the widget can decide that.
    fn release(&mut self, _x: f32, _y: f32, _r: Rect, _host: &Host) -> Action {
        Action::None
    }

    /// A key delivered to THIS widget, because it owns the keyboard.
    ///
    /// The opposite of [`Widget::key_feedback`] in every way that
    /// matters: that one is a broadcast to every instance so an
    /// on-screen keyboard can light up what somebody else is typing;
    /// this one goes to one widget, carries the modifiers, and answers.
    ///
    /// * `None` — not consumed. The host spends the key on itself:
    ///   focus navigation, the shortcut registry, the shell's bytes.
    /// * `Some(`[`Action::None`]`)` — consumed, and nothing is asked of
    ///   the application. What a field answers to an ordinary character.
    /// * `Some(action)` — consumed, and here is what to do about it (a
    ///   search box's Enter opening what it found).
    ///
    /// The default is `None`: a widget that has not been taught about
    /// keys never takes one away from the host.
    fn key(&mut self, _ev: &crate::focus::KeyEv) -> Option<Action> {
        None
    }

    /// The character grid this widget settled on while drawing. Only the
    /// terminal view has one; the application resizes the PTY to match.
    fn grid(&self) -> Option<(usize, usize)> {
        None
    }

    /// A key was pressed on the PHYSICAL keyboard — so the on-screen one
    /// can light the matching key up.
    ///
    /// A BROADCAST: every widget hears every key, whoever it was meant
    /// for, and nobody can consume it. That is right for lighting a key
    /// up and wrong for everything else, which is what [`Widget::key`]
    /// is. Named keys are spelled with the [`crate::runtime::keys`]
    /// words on both paths.
    fn key_feedback(&mut self, _ch: Option<char>, _label: Option<&str>) {}

    /// How this widget answers being resized. Asked once a frame, BEFORE
    /// the layout runs — which is why it may only look at the width.
    fn sizing(&mut self, _ctx: &mut Ctx, _host: &Host) -> Sizing {
        Sizing::Reference
    }

    /// Whether the widget's drawing follows `Ctx::panel_scale`. The
    /// script engine multiplies its type and rows by it, so a measured
    /// script panel may be granted `natural × scale` and the picture
    /// still fits. A compiled widget reads BAKED tokens across the ABI —
    /// `theme_px` carries no panel scale — so its drawing is the same
    /// size in any box, and a `Sizing::Content` want must be granted
    /// whole or the content leaves the panel (the control widget's
    /// buttons through the frame at 1280x800 were exactly this). The
    /// host consults this when it turns a measurement into a want.
    fn scales_with_panel(&self) -> bool {
        true
    }
}

/// What a panel's size means to the widget in it.
///
/// The three answers are the three shapes a widget can have, and they
/// decide two different things: how tall its panel is made, and which
/// edge of that panel changes the size of what is drawn inside.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Sizing {
    /// Finite content, this tall at scale 1. Its panel is made exactly
    /// that tall — so the box hugs the content and the only distance to
    /// the edge is the padding — and the content is scaled to fit,
    /// whichever axis runs out first, so its proportions hold.
    Content(f32),
    /// Grows downwards: a table of rows, a list of files. The WIDTH
    /// decides how big the rows are; the height decides how many there
    /// are, which is why stretching one of these downwards must not
    /// magnify anything.
    Rows,
    /// Sized against its reference box on both axes. The answer for a
    /// widget whose content is neither a stack of rows nor a fixed
    /// height — an on-screen keyboard, a terminal — and the default for
    /// one that has not said otherwise.
    Reference,
}

pub mod factory;
pub mod loader;
pub mod registry;
