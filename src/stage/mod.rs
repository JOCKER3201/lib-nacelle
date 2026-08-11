//! The board world (u3 §3.4, first landing): which boards exist, what
//! each position shows, which widgets are present anywhere — the
//! identity questions the desktop's event loop answered with five
//! macros over four locals, and the compositor will have to answer
//! again for every output. The ANIMATION between boards (the cube, the
//! ride, the gesture) stays with the embedder for now; what moves in
//! first is the part that can be wrong silently: a position the
//! gesture can stand on is not the same as a board that exists, and a
//! widget nobody can see must not be instantiated (u3 §5's trap 3 and
//! trap 1). Everything here is pure and window-free, which is what
//! makes it testable at all.

pub mod world;

pub use world::BoardWorld;

use crate::layout::BoardId;

/// What a board-move tick reports back (u3 §3.4). Declared with the
/// world so embedders share the vocabulary; the state machine that
/// produces it is still the embedder's until the animation moves in.
pub enum Tick {
    Idle,
    Moving,
    Landed(BoardId),
}

/// The move in progress, for an embedder that draws it itself.
#[derive(Clone, Copy)]
pub struct Transit {
    pub horizontal: bool,
    pub amount: f32,
}
