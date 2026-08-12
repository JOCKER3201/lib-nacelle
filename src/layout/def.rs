//! The layout DEFINITION (u3 §3.2): what a `.layaut` file means once
//! parsed — the instances it places, the mode each board arranges them
//! in, the per-screen overrides and the size table the layout asks for.
//! Pure data plus the solve; the format lives in [`super::layaut`], the
//! files in [`super::store`].

use super::instance::{Instance, InstanceId, InstanceList};
use crate::base::{Layout, LayoutMode, Panel, PanelSpec, Rect, SizeTable};
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
    /// Which INSTANCE moves where on this screen. By identity, because
    /// "the terminal" is no longer one rectangle: a user who moved the
    /// second terminal on his 4K monitor moved that one.
    pub rects: Vec<(InstanceId, PanelSpec)>,
}

/// A board's own arrangement: the same modes home has, plus the sizes
/// it asks for. A board can be flexbox — `[column]` lines inside a
/// `[board x y]` section parse with the same engine and restack
/// responsively; a rect-only board section parses as `Rects`.
///
/// WHICH instances the board holds is not here: they are in the
/// layout's one [`InstanceList`], each carrying the board it stands on,
/// so moving a widget between boards is a field and not a transplant.
#[derive(Clone)]
pub struct BoardDef {
    pub base: LayoutMode,
    /// Per-widget reference/minimum heights the board's own columns
    /// name; empty = the selected layout's table.
    pub sizes: Vec<(Panel, f32, f32)>,
}

/// A whole layout: what it places, how each board arranges it, its
/// overrides, its sizes.
#[derive(Clone, Default)]
pub struct LayoutDef {
    /// How the HOME board arranges its instances.
    pub base: LayoutMode,
    pub overrides: Vec<ResOverride>,
    /// The extra widget boards this layout carries, by position on a
    /// cross centred on the home board: (x, 0) to its left and right,
    /// (0, y) above and below (y grows downwards, like the screen).
    /// Part of the layout, so choosing another layout chooses its
    /// whole world of boards.
    pub boards: Vec<(BoardId, BoardDef)>,
    /// Per-widget reference and minimum heights the layout asks for.
    /// Sizes belong to the layout, not to the widget: the same widget
    /// may be given a different reference box by a different layout.
    pub sizes: Vec<(Panel, f32, f32)>,
    /// EVERY widget this layout places, on any of its boards — the one
    /// list, in which the same widget may appear as often as the user
    /// dragged it out.
    pub instances: InstanceList,
    /// The screen the base was authored on (`screen = 1920x1080@27`).
    /// Saving on that screen rewrites the base; saving on any other
    /// writes a `[WxH@D]` section instead.
    pub base_screen: Option<ScreenKey>,
}

impl LayoutDef {
    pub fn from_base(base: LayoutMode) -> Self {
        Self { base, ..Self::default() }
    }

    /// A board that holds nothing yet: rectangles, and none of them.
    pub fn empty_board() -> Self {
        Self::from_base(LayoutMode::Rects)
    }

    /// The override matching the given screen (resolution + diagonal).
    pub fn pick(&self, key: ScreenKey) -> Option<&ResOverride> {
        self.overrides.iter().find(|o| (o.w, o.h, o.diag) == key)
    }

    /// The instances of one board, in placement order.
    pub fn board_instances(&self, k: BoardId) -> Vec<Instance> {
        self.instances.on_board(k)
    }

    /// Which boards write their placements down: everything except the
    /// ones still showing the GENERATED arrangement, which is composed
    /// from the registry on every read and named in a file only by
    /// widget.
    fn boards_that_save(&self) -> Vec<BoardId> {
        let mut out: Vec<BoardId> = Vec::new();
        if !matches!(self.base, LayoutMode::Flex) {
            out.push((0, 0));
        }
        for (k, bd) in &self.boards {
            if !matches!(bd.base, LayoutMode::Flex) {
                out.push(*k);
            }
        }
        out
    }

    /// Gives the COMPOSED placements of every board that saves its own
    /// a saved identity, and follows the change through everything in
    /// this definition that names one: the columns of a flexbox base or
    /// board, and the per-screen sections.
    ///
    /// This is what writing a layout down means. A generated placement
    /// has no identity a file could carry — the arrangement is composed
    /// from the registry every time it is read — so the moment the user
    /// arranges a board himself, its placements stop being composed and
    /// become his.
    pub fn materialize(&mut self) -> Vec<(InstanceId, InstanceId)> {
        let saving = self.boards_that_save();
        let map = self.instances.materialize_on(&saving);
        if map.is_empty() {
            return map;
        }
        let new_of = |id: InstanceId| -> InstanceId {
            map.iter().find(|(was, _)| *was == id).map(|(_, now)| *now).unwrap_or(id)
        };
        let mut modes: Vec<&mut LayoutMode> = vec![&mut self.base];
        modes.extend(self.boards.iter_mut().map(|(_, bd)| &mut bd.base));
        for mode in modes {
            if let LayoutMode::Custom(fl) = mode {
                for c in fl.columns.iter_mut() {
                    for it in c.panels.iter_mut() {
                        it.id = new_of(it.id);
                    }
                }
            }
        }
        for ov in self.overrides.iter_mut() {
            for (id, _) in ov.rects.iter_mut() {
                *id = new_of(*id);
            }
        }
        map
    }

    /// Instance rectangles in device pixels, OUTER (before padding),
    /// for the HOME board — the whole solve for a layout whose world is
    /// one board.
    pub fn solve(&self, w: f32, h: f32, pad: f32, screen: ScreenKey, t: &SizeTable) -> Layout {
        self.solve_on((0, 0), w, h, pad, screen, t)
    }

    /// The same for ONE named board: the flex solve for this window
    /// size against the caller's size table, then the per-screen
    /// override section laid over it.
    pub fn solve_on(
        &self,
        k: BoardId,
        w: f32,
        h: f32,
        pad: f32,
        screen: ScreenKey,
        t: &SizeTable,
    ) -> Layout {
        let insts = self.board_instances(k);
        let mut l = flex::compute_in(w, h, &self.base, pad, t, &insts);
        if let Some(ov) = self.pick(screen) {
            for (id, ps) in &ov.rects {
                // An override names an instance of ITS OWN board only;
                // one section covers the whole world, so a rectangle
                // for a widget standing elsewhere is not ours to place.
                let Some(inst) = insts.iter().find(|i| i.id == *id) else { continue };
                l.place(
                    *id,
                    inst.widget,
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
/// underneath it: it overrides some instances and silently leaves the
/// rest to the base, which is how a user ends up with half of one
/// arrangement and half of another (u1 §5.2 — told rather than fixed
/// behind the user's back). Returns (pinned, placed) when the section
/// for this screen names fewer instances than the layout places.
pub fn stale_screen_section(def: &LayoutDef, key: ScreenKey) -> Option<(usize, usize)> {
    let ov = def.pick(key)?;
    let placed = def.instances.len();
    (ov.rects.len() < placed).then_some((ov.rects.len(), placed))
}
