//! Reusable on-screen objects: windows and dialog windows,
//! parallelogram buttons, sliders, accordion drop-downs, checkboxes,
//! the single-line text input, the context menu, and the frame for
//! windows the application does not own.
//!
//! These are the pieces an application builds its own interface from —
//! its settings window, its dialogs — as opposed to [`crate::ui`], which
//! is the vocabulary widgets are composed from.

pub mod button;
pub mod checkbox;
pub mod dropdown;
pub mod focus_ring;
pub mod menu;
pub mod panel;
pub mod slider;
pub mod text_input;
pub mod window;
pub mod winframe;
