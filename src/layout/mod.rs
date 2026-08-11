//! The layout engine's home (u3 §3.2): the `.layaut` format, the
//! layout definition, the named-file store — and the flex solver as
//! [`flex`], re-exported at the crate root so `nacelle::flex` keeps
//! working. WHERE panels sit is decided here; WHAT they look like is
//! the theme engine's, and the two never mix.

pub mod def;
pub mod flex;
pub mod layaut;
pub mod store;

pub use def::{board_key, stale_screen_section, BoardDef, BoardId, LayoutDef, ResOverride, ScreenKey};
pub use store::LayautStore;
