//! The layout DEFINITION (u3 §3.2): what a `.layaut` file means once
//! parsed — the base mode, the per-screen overrides, the boards on the
//! cross and the size table the layout asks for. Pure data plus the
//! solve; the format lives in [`super::layaut`], the files in
//! [`super::store`].

use crate::base::{Layout, LayoutMode, LayoutSpec, Panel, PanelSpec, Rect, SizeTable};
use crate::flex;

/// Monitor width, height and diagonal in inches — the key of a
/// `[WxH@D]` section. An embedder without a monitor supplies its own
/// stable triple; `(0, 0, 0)` means "unknown".
pub type ScreenKey = (u32, u32, u32);

/// Where a board sits: (x, y) relative to home at (0, 0), on the axes
/// only.
pub type BoardId = (i32, i32);

/// The board a position shows. The top and bottom boards are single
/// places reachable from anywhere on the row, so every (x, ±1) the
/// gesture can stand on shows the one board stored at (0, ±1); x is
/// only the place the slide down returns to.
pub fn board_key(k: BoardId) -> BoardId {
    if k.1 != 0 { (0, k.1) } else { k }
}

/// A per-resolution override section of a `.layaut` file.
#[derive(Clone)]
pub struct ResOverride {
    pub w: u32,
    pub h: u32,
    pub diag: u32,
    pub panels: Vec<(Panel, PanelSpec)>,
}

/// A board's own layout: the same modes home has, plus the sizes it
/// asks for. A board can be flexbox — `[column]` lines inside a
/// `[board x y]` section parse with the same engine and restack
/// responsively; a rect-only board section parses exactly as before.
#[derive(Clone)]
pub struct BoardDef {
    pub base: LayoutMode,
    /// Per-panel reference/minimum heights the board's own columns
    /// name; empty = the selected layout's table.
    pub sizes: Vec<(Panel, f32, f32)>,
}

/// A whole layout: the base, its overrides, its boards, its sizes.
#[derive(Clone, Default)]
pub struct LayoutDef {
    pub base: LayoutMode,
    pub overrides: Vec<ResOverride>,
    /// The extra widget boards this layout carries, by position on a
    /// cross centred on the home board: (x, 0) to its left and right,
    /// (0, y) above and below (y grows downwards, like the screen).
    /// Part of the layout, so choosing another layout chooses its
    /// whole world of boards.
    pub boards: Vec<(BoardId, BoardDef)>,
    /// Per-panel reference and minimum heights the layout asks for.
    /// Sizes belong to the layout, not to the widget: the same widget
    /// may be given a different reference box by a different layout.
    pub sizes: Vec<(Panel, f32, f32)>,
}

impl LayoutDef {
    pub fn from_base(base: LayoutMode) -> Self {
        Self { base, ..Self::default() }
    }

    /// A board that holds nothing yet: fixed, every panel hidden.
    pub fn empty_board() -> Self {
        Self::from_base(LayoutMode::Fixed(LayoutSpec::default()))
    }

    /// The override matching the given screen (resolution + diagonal).
    pub fn pick(&self, key: ScreenKey) -> Option<&ResOverride> {
        self.overrides.iter().find(|o| (o.w, o.h, o.diag) == key)
    }

    /// Panel rectangles in device pixels, OUTER (before padding): the
    /// flex solve for this window size against the caller's size
    /// table, then the per-screen override section laid over it.
    pub fn solve(&self, w: f32, h: f32, pad: f32, screen: ScreenKey, t: &SizeTable) -> Layout {
        let mut l = flex::compute_in(w, h, &self.base, pad, t);
        if let Some(ov) = self.pick(screen) {
            for (p, ps) in &ov.panels {
                l.set(
                    *p,
                    Rect::new(
                        ps.x / 100.0 * w,
                        ps.y / 100.0 * h,
                        ps.w / 100.0 * w,
                        ps.h / 100.0 * h,
                    ),
                );
            }
        }
        l
    }
}

/// A pinned screen section that predates a change to the layout
/// underneath it: it overrides some panels and silently leaves the rest
/// to the base, which is how a user ends up with half of one
/// arrangement and half of another (u1 §5.2 — told rather than fixed
/// behind the user's back). Returns (pinned, placed) when the section
/// for this screen names fewer panels than the registry places.
pub fn stale_screen_section(def: &LayoutDef, key: ScreenKey) -> Option<(usize, usize)> {
    let ov = def.pick(key)?;
    let placed = Panel::all().len();
    (ov.panels.len() < placed).then_some((ov.panels.len(), placed))
}
