//! Where the pointer is — and, which is the harder half, who is allowed
//! to see it.
//!
//! A toolkit whose frame is immediate has no scene graph to ask "what is
//! on top here": every control tests the pointer against a rectangle it
//! has just drawn, and a control drawn under a window tests exactly as
//! confidently as the window drawn over it. Both answer yes, both light
//! up, and only one of them is under the hand. That is the whole of the
//! defect this type exists to close — reported as an on-screen keyboard
//! whose caps lit through an open settings window, which is one pair out
//! of the many the same reading produces.
//!
//! The rule belongs here rather than in an application because it is a
//! statement about the TOOLKIT: a control covered by something else is
//! not under the pointer, whatever the two of them are and whoever drew
//! them. An application that had to write it out would write it once per
//! pair of things it happens to know about, and every pair it forgot
//! would keep the fault while looking closed.
//!
//! # How the answer is arrived at
//!
//! Draw order IS z-order in an immediate frame, so "what is on top of me"
//! is the same question as "what is drawn after me" — and at the moment a
//! control asks, that has not happened yet. So it is answered from the
//! frame just gone: whatever drew over the pointer last frame is what
//! stands over it now. Two consequences, both deliberate:
//!
//! * moving the pointer is answered EXACTLY, because the covers are
//!   re-tested against the pointer's current position every frame — which
//!   is the reported case, a hand travelling under an open window;
//! * a window that has just appeared, moved or closed is one frame stale.
//!   One frame is 16 ms of a highlight that was already on the screen; it
//!   cannot be pointed at, clicked or seen.
//!
//! A caller that knows better may say so earlier: [`Pointer::cover`] is
//! honoured from the moment it is called, so an application that declares
//! its window's rectangle before drawing the board underneath is exact on
//! the first frame too. Nothing in the toolkit requires it.
//!
//! # What a cover is
//!
//! [`Pointer::cover`] means "I have drawn something over this rectangle
//! that the user cannot see through". The toolkit's own overlays claim
//! for themselves — the modal scrim claims the screen, a window frame
//! claims its box, a context menu claims its rows — so an application
//! gets the rule by drawing the objects, not by remembering to ask for
//! it.
//!
//! The claim must come BEFORE the covering object reads the pointer, and
//! that is the natural order anyway: a window draws its frame first and
//! its controls into it afterwards.

use crate::Rect;

/// The pointer, and the covers standing between it and whoever is
/// asking.
///
/// Held by the application across frames (the way [`crate::focus::
/// FocusCtl`] and [`crate::object::tooltip::Tooltips`] are) and handed to
/// each frame's [`crate::Ctx`]. A default one — no position, no covers —
/// is what a headless caller wants: nothing is ever hovered.
#[derive(Clone, Debug, Default)]
pub struct Pointer {
    /// Where the device says the pointer is.
    at: (f32, f32),
    /// The rectangles claimed so far this frame, in the order they were
    /// drawn.
    covers: Vec<Rect>,
    /// How many covers must stand between the start of the frame and the
    /// caller before the caller may see the pointer.
    ///
    /// Counted off the PREVIOUS frame: the index after the last cover
    /// that held the pointer. Zero — nothing covered it — lets everybody
    /// see it, which is what a desktop with no window open is.
    reveal: usize,
}

impl Pointer {
    /// The position a control that may not see the pointer is given.
    ///
    /// Far away rather than absent, so that the reading every control
    /// already performs — `rect.contains(x, y)` — answers false without
    /// the control being rewritten, including the ones on the far side of
    /// the plugin ABI, which cannot be rewritten from here at all.
    /// Negative infinity and not a small negative number: a rectangle may
    /// legitimately sit at a negative coordinate (a panel scrolled off
    /// the top), and no rectangle contains this.
    pub const AWAY: (f32, f32) = (f32::NEG_INFINITY, f32::NEG_INFINITY);

    /// A pointer at a position that nothing covers — a test, an embedder
    /// with no overlays, a plugin drawing on its own surface.
    pub fn new(x: f32, y: f32) -> Pointer {
        Pointer { at: (x, y), covers: Vec::new(), reveal: 0 }
    }

    /// Starts a frame with the pointer at `at`.
    ///
    /// The covers of the frame just gone decide who may see it, and are
    /// then dropped — the vector they were in is kept, so a steady frame
    /// allocates nothing.
    pub fn begin(&mut self, at: (f32, f32)) {
        self.reveal = self
            .covers
            .iter()
            .rposition(|r| r.contains(at.0, at.1))
            .map_or(0, |i| i + 1);
        self.covers.clear();
        self.at = at;
    }

    /// "I have drawn over this rectangle."
    ///
    /// Everything that asked for the pointer BEFORE this call keeps the
    /// answer it was given — it was on top at the time it asked, which in
    /// an immediate frame it was not, and that is what the next frame
    /// corrects. Everything that asks after it is unaffected by its own
    /// cover: an object claims its box and then draws its controls into
    /// it, and those controls are on top of it, not under it.
    pub fn cover(&mut self, r: Rect) {
        self.covers.push(r);
    }

    /// Where the pointer is, as far as the code drawing right now is
    /// concerned: [`Pointer::AWAY`] when something covers it.
    ///
    /// This is what every hover reads, directly or through
    /// [`crate::view::Surface::mouse`].
    pub fn at(&self) -> (f32, f32) {
        if self.covers.len() < self.reveal {
            Pointer::AWAY
        } else {
            self.at
        }
    }

    /// Whether the pointer is on `r` — the question a control asks.
    pub fn over(&self, r: Rect) -> bool {
        let (x, y) = self.at();
        r.contains(x, y)
    }

    /// Where the device says the pointer is, covers or no covers.
    ///
    /// For PLACEMENT and nothing else: a tooltip decides which side of
    /// the cursor to open on, a menu opens where the click landed. A
    /// hover asking this instead of [`Pointer::at`] is the defect this
    /// module exists to close, written out by hand.
    pub fn raw(&self) -> (f32, f32) {
        self.at
    }

    /// Whether anything has claimed the ground under the pointer ahead of
    /// the caller — "am I looking at this through something else".
    pub fn covered(&self) -> bool {
        self.covers.len() < self.reveal
    }
}

// ---------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const UNDER: Rect = Rect { x: 0.0, y: 0.0, w: 100.0, h: 100.0 };
    const OVER: Rect = Rect { x: 50.0, y: 50.0, w: 100.0, h: 100.0 };
    /// A point both of them hold.
    const SHARED: (f32, f32) = (60.0, 60.0);

    /// One frame: the thing underneath asks, the thing on top claims and
    /// then asks. Answers the pair of readings.
    fn frame(p: &mut Pointer, at: (f32, f32)) -> (bool, bool) {
        p.begin(at);
        let under = p.over(UNDER);
        p.cover(OVER);
        let over = p.over(OVER);
        (under, over)
    }

    #[test]
    fn with_nothing_claimed_everyone_sees_the_pointer() {
        let p = Pointer::new(SHARED.0, SHARED.1);
        assert!(p.over(UNDER));
        assert!(p.over(OVER));
        assert_eq!(p.at(), SHARED);
    }

    #[test]
    fn the_thing_on_top_takes_the_pointer_from_the_one_under_it() {
        let mut p = Pointer::default();
        // The frame the cover first appears on is answered from a frame
        // that had none, so both still see it — the one stale frame the
        // module documents.
        assert_eq!(frame(&mut p, SHARED), (true, true));
        // Every frame after it names one.
        assert_eq!(frame(&mut p, SHARED), (false, true));
        assert_eq!(frame(&mut p, SHARED), (false, true));
    }

    #[test]
    fn a_cover_takes_only_the_ground_it_stands_on() {
        let mut p = Pointer::default();
        // Settled with the pointer on the shared corner…
        frame(&mut p, SHARED);
        assert_eq!(frame(&mut p, SHARED), (false, true));
        // …and moved to a part of the lower rectangle the cover does not
        // reach, it is answered on the SAME frame: the covers are re-read
        // against the pointer's new position, never remembered as a
        // verdict.
        assert_eq!(frame(&mut p, (10.0, 10.0)), (true, false));
    }

    #[test]
    fn what_is_covered_says_so() {
        let mut p = Pointer::default();
        frame(&mut p, SHARED);
        p.begin(SHARED);
        assert!(p.covered(), "the cover of the last frame still stands");
        assert_eq!(p.at(), Pointer::AWAY);
        assert_eq!(p.raw(), SHARED, "the device position is not a hover");
        p.cover(OVER);
        assert!(!p.covered());
        assert_eq!(p.at(), SHARED);
    }

    #[test]
    fn a_stack_of_three_names_the_top_one() {
        const TOP: Rect = Rect { x: 55.0, y: 55.0, w: 20.0, h: 20.0 };
        let mut p = Pointer::default();
        let run = |p: &mut Pointer| {
            p.begin(SHARED);
            let a = p.over(UNDER);
            p.cover(OVER);
            let b = p.over(OVER);
            p.cover(TOP);
            let c = p.over(TOP);
            (a, b, c)
        };
        run(&mut p);
        assert_eq!(run(&mut p), (false, false, true));
    }

    #[test]
    fn a_cover_the_pointer_is_not_on_hides_nothing() {
        const ELSEWHERE: Rect = Rect { x: 500.0, y: 500.0, w: 10.0, h: 10.0 };
        let mut p = Pointer::default();
        let run = |p: &mut Pointer| {
            p.begin(SHARED);
            let under = p.over(UNDER);
            p.cover(ELSEWHERE);
            under
        };
        run(&mut p);
        assert!(run(&mut p), "a window on the other side of the screen");
    }

    #[test]
    fn a_cover_that_goes_away_gives_the_pointer_back() {
        let mut p = Pointer::default();
        frame(&mut p, SHARED);
        assert_eq!(frame(&mut p, SHARED), (false, true));
        // The window closes: nothing claims this frame…
        p.begin(SHARED);
        assert!(!p.over(UNDER), "the frame the window closed on is stale");
        // …and from the next one the control underneath is pointed at
        // again.
        p.begin(SHARED);
        assert!(p.over(UNDER));
    }
}
