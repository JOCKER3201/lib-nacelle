//! One layout's world of boards, moved verbatim from the desktop's
//! refresh_boards!/def_of!/has_board!/all_boards! macros: home at
//! (0, 0), the horizontal row it sits on, and the two fixtures above
//! and below — SEARCH AND AI at (0, -1) and APPGRID at (0, 1) — which
//! exist whether or not the layout's file has anything on them.

use crate::base::{LayoutMode, LayoutSpec, SizeTable};
use crate::layout::{board_key, BoardId, LayoutDef, ScreenKey};
use std::collections::HashMap;

pub struct BoardWorld {
    /// The selected layaut itself — what position (0, 0) shows.
    home: LayoutDef,
    /// The extra boards, keyed by their FOLDED position.
    boards: HashMap<BoardId, LayoutDef>,
    /// How far the horizontal row reaches: (left, right).
    ext: (u32, u32),
    current: BoardId,
    /// What a position without a board shows: fixed, every panel
    /// hidden. Owned so `def` can always answer a reference.
    empty: LayoutDef,
}

impl BoardWorld {
    /// Builds the world of a layout. Boards named by the file keep
    /// their own sizes when they name any and share the layout's
    /// otherwise; the two fixtures exist regardless.
    pub fn new(home: LayoutDef) -> Self {
        let mut w = Self {
            home: LayoutDef::default(),
            boards: HashMap::new(),
            ext: (0, 0),
            current: (0, 0),
            empty: LayoutDef::empty_board(),
        };
        w.rebuild(home);
        w
    }

    /// Re-reads the world from a (new) layout, keeping the current
    /// position when it still exists — home is the one place that
    /// always does.
    pub fn rebuild(&mut self, home: LayoutDef) {
        self.boards.clear();
        let (mut l, mut r) = (0u32, 0u32);
        for (k, bd) in &home.boards {
            let (x, y) = *k;
            if y == 0 && x < 0 {
                l = l.max(-x as u32);
            } else if y == 0 {
                r = r.max(x as u32);
            }
            // A board is whatever its section holds — fixed rects or
            // flexbox columns; a board that names its own sizes uses
            // them, the rest share the layout's.
            self.boards.insert(
                *k,
                LayoutDef {
                    base: bd.base.clone(),
                    overrides: Vec::new(),
                    sizes: if bd.sizes.is_empty() {
                        home.sizes.clone()
                    } else {
                        bd.sizes.clone()
                    },
                    boards: Vec::new(),
                },
            );
        }
        // The fixtures exist whether or not the file has anything on
        // them. Under the project's own compositor these two will live
        // in the OVERLAY layer, above every window; here they are
        // ordinary boards that ride over home when opened.
        for k in [(0, -1), (0, 1)] {
            self.boards.entry(k).or_insert_with(|| LayoutDef {
                base: LayoutMode::Fixed(LayoutSpec::default()),
                overrides: Vec::new(),
                sizes: home.sizes.clone(),
                boards: Vec::new(),
            });
        }
        self.ext = (l, r);
        self.home = home;
        if !self.has_board(self.current) {
            self.current = (0, 0);
        }
    }

    pub fn current(&self) -> BoardId {
        self.current
    }

    /// Moves the current position; a position outside the world is
    /// refused and the current stands.
    pub fn set_current(&mut self, k: BoardId) {
        if self.has_board(k) {
            self.current = k;
        }
    }

    /// Whether the gesture can stand on this position: any place on
    /// the row, and the fixed top and bottom above and below EACH of
    /// them — (x, ±1) shows the one fixture, x only remembers where
    /// the hand came from.
    pub fn has_board(&self, k: BoardId) -> bool {
        let (x, y) = k;
        let (l, r) = self.ext;
        x >= -(l as i32) && x <= r as i32 && (-1..=1).contains(&y)
    }

    /// The definition a position shows — home for (0, 0), the folded
    /// board for anything else, the empty board for a position that
    /// exists but holds nothing.
    pub fn def(&self, k: BoardId) -> &LayoutDef {
        let key = board_key(k);
        if key == (0, 0) {
            &self.home
        } else {
            self.boards.get(&key).unwrap_or(&self.empty)
        }
    }

    pub fn current_def(&self) -> &LayoutDef {
        self.def(self.current)
    }

    /// How far the horizontal row reaches: (left, right).
    pub fn arms(&self) -> (u32, u32) {
        self.ext
    }

    /// Every board that exists, home first, the rest sorted — the
    /// order every scan and every save walks, so it can never depend
    /// on a hash map's whim.
    pub fn ids(&self) -> Vec<BoardId> {
        let mut ids: Vec<BoardId> = vec![(0, 0)];
        let mut rest: Vec<BoardId> = self.boards.keys().copied().collect();
        rest.sort();
        ids.extend(rest);
        ids
    }

    /// Which panels are visible on ANY board at this window size — the
    /// presence scan widget lifetime hangs on (u3 §5 trap 1): a panel
    /// whose rectangle starts inside the window on some board is
    /// present; one hidden everywhere is not, and its widget must not
    /// run. The x < w rule is the OFF_SPEC convention: hidden panels
    /// park far outside.
    pub fn present(
        &self,
        w: f32,
        h: f32,
        pad: f32,
        screen: ScreenKey,
        t: &SizeTable,
    ) -> Vec<bool> {
        let mut present = vec![false; crate::base::panel_count()];
        for k in self.ids() {
            let def = self.def(k);
            let lay = def.solve(w, h, pad, screen, t);
            for p in crate::base::Panel::all() {
                if lay.p(p).x < w {
                    present[p.idx()] = true;
                }
            }
        }
        present
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::BoardDef;

    fn world_with(boards: &[BoardId]) -> BoardWorld {
        crate::flex::install_test_registry();
        let mut home = LayoutDef::from_base(LayoutMode::Flex);
        home.boards = boards
            .iter()
            .map(|k| {
                (*k, BoardDef { base: LayoutMode::Fixed(LayoutSpec::default()), sizes: Vec::new() })
            })
            .collect();
        BoardWorld::new(home)
    }

    #[test]
    fn the_fixtures_always_exist_and_home_is_first() {
        let w = world_with(&[]);
        assert_eq!(w.arms(), (0, 0));
        assert_eq!(w.ids(), vec![(0, 0), (0, -1), (0, 1)]);
        assert!(w.has_board((0, 0)) && w.has_board((0, -1)) && w.has_board((0, 1)));
        assert!(!w.has_board((1, 0)) && !w.has_board((0, 2)));
    }

    #[test]
    fn every_row_position_carries_the_one_fixture() {
        let w = world_with(&[(-1, 0), (1, 0), (2, 0)]);
        assert_eq!(w.arms(), (1, 2));
        // (2, 1) folds to the ONE bottom board; (2, 2) is not a place.
        assert!(w.has_board((2, 1)));
        assert!(!w.has_board((2, 2)));
        assert!(std::ptr::eq(w.def((2, 1)), w.def((0, 1))), "every (x, 1) shows (0, 1)");
        assert!(std::ptr::eq(w.def((-1, -1)), w.def((0, -1))));
    }

    #[test]
    fn a_position_that_exists_but_holds_nothing_is_the_empty_board() {
        let w = world_with(&[(1, 0)]);
        // The fixture positions exist with no content: the empty def
        // hides every panel (OFF_SPEC parks at x >= 100 percent).
        let d = w.def((0, -1));
        assert!(matches!(d.base, LayoutMode::Fixed(_)));
    }

    #[test]
    fn a_shrunken_world_returns_the_wanderer_home() {
        let mut w = world_with(&[(1, 0), (2, 0)]);
        w.set_current((2, 0));
        assert_eq!(w.current(), (2, 0));
        let mut smaller = LayoutDef::from_base(LayoutMode::Flex);
        smaller.boards = vec![(
            (1, 0),
            BoardDef { base: LayoutMode::Fixed(LayoutSpec::default()), sizes: Vec::new() },
        )];
        w.rebuild(smaller);
        assert_eq!(w.current(), (0, 0), "home is the one place that always exists");
        assert_eq!(w.arms(), (0, 1));
    }

    #[test]
    fn set_current_refuses_a_place_that_is_not_there() {
        let mut w = world_with(&[]);
        w.set_current((3, 0));
        assert_eq!(w.current(), (0, 0));
        w.set_current((0, 1));
        assert_eq!(w.current(), (0, 1));
    }

    #[test]
    fn boards_share_the_layouts_sizes_unless_they_name_their_own() {
        // The registry has to exist before a Panel does — see
        // `flex::install_test_registry`.
        crate::flex::install_test_registry();
        let mut home = LayoutDef::from_base(LayoutMode::Flex);
        home.sizes = vec![(crate::base::Panel::all()[0], 9.0, 5.0)];
        home.boards = vec![
            ((1, 0), BoardDef { base: LayoutMode::Fixed(LayoutSpec::default()), sizes: Vec::new() }),
            (
                (2, 0),
                BoardDef {
                    base: LayoutMode::Fixed(LayoutSpec::default()),
                    sizes: vec![(crate::base::Panel::all()[0], 20.0, 10.0)],
                },
            ),
        ];
        let w = BoardWorld::new(home);
        assert_eq!(w.def((1, 0)).sizes[0].1, 9.0, "no own sizes: the layout's table");
        assert_eq!(w.def((2, 0)).sizes[0].1, 20.0, "own sizes win");
    }
}
