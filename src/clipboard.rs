//! The clipboard seam (F1 §2).
//!
//! libnacelle owns the SEAM, never a platform. [`ClipboardBackend`] is
//! implemented by the application — smithay-clipboard and x11-clipboard
//! in nacelle-desktop, the compositor's own data-device state when
//! nacelle IS the compositor — and handed in through [`install`] at
//! startup, the same pattern as the sound device and the telemetry
//! collectors: the toolkit calls [`store`]/[`load`] and never knows
//! which backend answered. Until something is installed the
//! process-local [`LocalClipboard`] answers, so copy/paste keeps
//! working WITHIN nacelle whatever the host offers (tests, headless, a
//! compositor with no data device at all).
//!
//! Process-wide ON PURPOSE, with the sound queue's justification
//! (przynależność audit): there is one clipboard per seat the way there
//! is one audio output, and widgets must be able to ask for it without
//! every call site being handed a handle. Two deliberate consequences:
//!
//! * **Plugins reach the clipboard through [`Action`]s** —
//!   `Action::PastePrimary`, the host's copy bindings — never by
//!   calling this module: a `.so` widget's copy of the toolkit is
//!   ATTACHED, owns no state, and `HostApi` carries no clipboard entry
//!   yet. A call from an attached copy is dropped with a one-time
//!   warning, exactly like an orphaned `sound::emit`. When a plugin
//!   genuinely needs direct access, `HostApi` grows an APPENDED,
//!   `api_size`-gated entry and this module forwards through it.
//! * **The trait speaks seats now.** For the desktop there is exactly
//!   one ([`Seat::DEFAULT`]) and every backend may ignore the argument;
//!   for the future multi-seat compositor a per-process selection would
//!   be simply wrong, and retrofitting the parameter later would break
//!   every out-of-tree backend. Same reasoning for the MIME stubs: the
//!   compositor bridging `wl_data_device` will need `text/uri-list` and
//!   friends, and a `dyn` trait cannot grow required methods without
//!   breaking — so the growth points are default-implemented today.
//!
//! [`Action`]: crate::Action

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

/// Which selection a text goes to. `Clipboard` is the explicit-gesture
/// board (Ctrl+Shift+C/V); `Primary` is the select/middle-click one —
/// the terminal convention. A host with no primary selection (gamescope
/// often exposes none) fails those calls SILENTLY: primary is a nicety.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Board {
    Clipboard,
    Primary,
}

/// Whose selection it is. The desktop has one seat and passes
/// [`Seat::DEFAULT`] everywhere; the compositor will have real ones.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Seat(pub u32);

impl Seat {
    pub const DEFAULT: Seat = Seat(0);
}

/// What the application installs: the actual clipboard, wherever it
/// lives. `Send` because window libraries deliver events on whatever
/// thread they like, and the backend must be movable to where the
/// state is guarded.
pub trait ClipboardBackend: Send {
    fn store(&mut self, seat: Seat, board: Board, text: &str);
    fn load(&mut self, seat: Seat, board: Board) -> Option<String>;

    /// MIME growth point — see the module header. Default: dropped.
    fn store_mime(&mut self, _seat: Seat, _board: Board, _mime: &str, _data: &[u8]) {}

    /// MIME growth point. Default: nothing.
    fn load_mime(&mut self, _seat: Seat, _board: Board, _mime: &str) -> Option<Vec<u8>> {
        None
    }
}

/// The process-local fallback: always present, used until [`install`]
/// replaces it. Copy/paste works within the program with no compositor,
/// no display and no protocol — which is also what makes the seam
/// testable. Single-seat by construction, so the seat is ignored.
#[derive(Default)]
pub struct LocalClipboard {
    clipboard: Option<String>,
    primary: Option<String>,
}

impl LocalClipboard {
    pub fn new() -> LocalClipboard {
        LocalClipboard::default()
    }
}

impl ClipboardBackend for LocalClipboard {
    fn store(&mut self, _seat: Seat, board: Board, text: &str) {
        let slot = match board {
            Board::Clipboard => &mut self.clipboard,
            Board::Primary => &mut self.primary,
        };
        *slot = Some(text.to_string());
    }

    fn load(&mut self, _seat: Seat, board: Board) -> Option<String> {
        match board {
            Board::Clipboard => self.clipboard.clone(),
            Board::Primary => self.primary.clone(),
        }
    }
}

fn backend() -> &'static Mutex<Box<dyn ClipboardBackend>> {
    static B: OnceLock<Mutex<Box<dyn ClipboardBackend>>> = OnceLock::new();
    B.get_or_init(|| Mutex::new(Box::new(LocalClipboard::new())))
}

/// Says once that an attached copy called the clipboard directly — the
/// path that is documented to go through Actions instead.
fn warn_attached(what: &str) {
    static WARNED: AtomicBool = AtomicBool::new(false);
    if !WARNED.swap(true, Ordering::Relaxed) {
        eprintln!(
            "nacelle: {what} was called by a plugin's copy of the toolkit — \
             the clipboard is the host's; ask through an Action. The call \
             is being dropped."
        );
    }
}

/// Installs the application's backend, replacing [`LocalClipboard`].
/// Called once at startup, before anything stores or loads; the future
/// compositor calls the same function with its data-device state.
pub fn install(b: Box<dyn ClipboardBackend>) {
    if !crate::runtime::is_host() {
        warn_attached("clipboard::install");
        return;
    }
    if let Ok(mut cur) = backend().lock() {
        *cur = b;
    }
}

/// Stores for the default seat — the desktop's whole world.
pub fn store(board: Board, text: &str) {
    store_for(Seat::DEFAULT, board, text)
}

/// Loads for the default seat. Only ever on an explicit paste gesture,
/// never per frame: an X11 load waits on the selection's owner, and a
/// stuck owner must cost one paste, not the frame rate. A failed load
/// is a no-op for the caller, not an error dialog.
pub fn load(board: Board) -> Option<String> {
    load_for(Seat::DEFAULT, board)
}

pub fn store_for(seat: Seat, board: Board, text: &str) {
    if !crate::runtime::is_host() {
        warn_attached("clipboard::store");
        return;
    }
    if let Ok(mut b) = backend().lock() {
        b.store(seat, board, text);
    }
}

pub fn load_for(seat: Seat, board: Board) -> Option<String> {
    if !crate::runtime::is_host() {
        warn_attached("clipboard::load");
        return None;
    }
    backend().lock().ok().and_then(|mut b| b.load(seat, board))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::Sender;

    /// A backend that reports what reached it, for the install test.
    struct Recorder(Sender<(u32, bool, String)>);

    impl ClipboardBackend for Recorder {
        fn store(&mut self, seat: Seat, board: Board, text: &str) {
            let _ = self.0.send((seat.0, board == Board::Primary, text.to_string()));
        }
        fn load(&mut self, _seat: Seat, _board: Board) -> Option<String> {
            Some("from the recorder".to_string())
        }
    }

    /// One test rather than three, because the seam under test is a
    /// deliberate process-wide singleton: the phases must run in order,
    /// and the test runner runs separate #[test]s in parallel.
    #[test]
    fn the_seam_answers_locally_then_through_the_installed_backend() {
        // Phase 1: nothing installed — the Local fallback answers, and
        // the two boards are separate slots.
        store(Board::Clipboard, "kept");
        store(Board::Primary, "selected");
        assert_eq!(load(Board::Clipboard).as_deref(), Some("kept"));
        assert_eq!(load(Board::Primary).as_deref(), Some("selected"));

        // Phase 2: an installed backend replaces it, and the seat and
        // board survive the crossing.
        let (tx, rx) = std::sync::mpsc::channel();
        install(Box::new(Recorder(tx)));
        store_for(Seat(3), Board::Primary, "routed");
        assert_eq!(rx.recv().unwrap(), (3, true, "routed".to_string()));
        assert_eq!(load(Board::Clipboard).as_deref(), Some("from the recorder"));
    }

    /// The MIME growth points exist and degrade to nothing, so the
    /// trait can grow without breaking a backend written today.
    #[test]
    fn mime_stubs_degrade_to_nothing() {
        let mut local = LocalClipboard::new();
        local.store_mime(Seat::DEFAULT, Board::Clipboard, "text/uri-list", b"file:///x");
        assert!(local.load_mime(Seat::DEFAULT, Board::Clipboard, "text/uri-list").is_none());
    }
}
