//! Reusable on-screen objects: windows and dialog windows,
//! parallelogram buttons, sliders, accordion drop-downs, checkboxes,
//! and the frame for windows the application does not own.
//!
//! These are the pieces an application builds its own interface from —
//! its settings window, its dialogs — as opposed to [`crate::ui`], which
//! is the vocabulary widgets are composed from.

pub mod button;
pub mod checkbox;
pub mod dropdown;
pub mod panel;
pub mod slider;
pub mod window;
pub mod winframe;
