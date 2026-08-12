//! The layout as a LIST OF INSTANCES (u3 §3.2).
//!
//! A layout used to be a table indexed by WIDGET: one widget, one
//! rectangle, one place in the world. That made two ordinary wishes
//! impossible — a second terminal next to the first, and a second
//! screen with a board of its own — because there was nowhere to put
//! the second rectangle.
//!
//! Here a layout is a flat list instead. Every entry is an INSTANCE:
//! which widget it runs, which board it stands on, where it sits, and
//! a stable identity of its own. The same widget may appear as many
//! times as the user drags it out, on one board or on several, and the
//! two instances are two separate things — two shells, two current
//! directories — because they are two entries with two identities.
//!
//! The identity is the whole point of the module, so it is worth
//! saying what it is NOT: it is not the position in the list. Removing
//! the middle entry of a vector renumbers everything after it, and
//! every reference held elsewhere — the editor's selection, a screen
//! section's override, the host's map of running widgets — would then
//! quietly point at its neighbour.

use super::def::BoardId;
use crate::base::{Panel, PanelSpec};

/// The stable identity of one placed widget inside one layout.
///
/// Handed out in ascending order by [`InstanceList::add`], written into
/// the layaut file, and never handed out a second time: a layout that
/// has lost instance 3 still has 4 and 5, and gives the next one 6. The
/// counter that guarantees it is saved with the file ([`InstanceList::
/// next_free`]), so the promise survives a restart as well as a
/// removal.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub struct InstanceId(u32);

impl InstanceId {
    /// The id no instance ever has: what a caller with nothing selected
    /// holds, and what an unreadable id in a file degrades to.
    pub const NONE: InstanceId = InstanceId(0);

    /// Where the GENERATED range starts.
    ///
    /// The default arrangement is composed from the registry on every
    /// load rather than written down, so its placements need identities
    /// that no file ever hands out — otherwise composing it would walk
    /// the saved counter forward on every start, or worse, hand a
    /// generated placement the id of a saved one. A generated id is its
    /// widget's registry position in this range: stable while the
    /// installation is, and unreachable for [`InstanceList::add`],
    /// which would have to hand out two billion ids to get here.
    pub const GENERATED: u32 = 0x8000_0000;

    /// Whether this identity was composed rather than saved. A
    /// generated placement becomes a saved one the moment the layout
    /// is written down ([`InstanceList::materialize`]).
    pub fn is_generated(self) -> bool {
        self.0 >= Self::GENERATED
    }

    /// The id with this number. Only the file reader and tests mint ids
    /// by hand — everything else is handed one by [`InstanceList::add`],
    /// which is the only thing that can promise it is unused.
    pub fn new(n: u32) -> Self {
        InstanceId(n)
    }

    pub fn get(self) -> u32 {
        self.0
    }

    pub fn is_some(self) -> bool {
        self.0 != 0
    }
}

impl std::fmt::Display for InstanceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// One placed widget: what it is, where it stands, who it is.
#[derive(Clone, Copy, Debug)]
pub struct Instance {
    pub id: InstanceId,
    /// Which widget runs here — the registry entry, whose `name()` is
    /// what the layaut file writes down.
    pub widget: Panel,
    /// Which board it stands on: home is (0, 0), the rest of the cross
    /// around it.
    pub board: BoardId,
    /// Where it sits when its board places by rectangle, in vw/vh of
    /// the window. None = it FLOWS: the board's columns decide its box
    /// every frame, and there is no rectangle to save.
    pub rect: Option<PanelSpec>,
}

impl Instance {
    /// A flowing instance: on a board, in no rectangle of its own.
    pub fn flowing(id: InstanceId, widget: Panel, board: BoardId) -> Self {
        Instance { id, widget, board, rect: None }
    }

    /// Whether this instance is parked outside the window — the
    /// OFF_SPEC convention a rectangle board hides a widget with. A
    /// flowing instance is never hidden: its board decides its box.
    pub fn hidden(&self) -> bool {
        self.rect.map(|r| r.x >= 100.0).unwrap_or(false)
    }
}

/// Every widget a layout places, across all of its boards.
///
/// The list owns the identities: `add` is the only way to get one, and
/// it never repeats itself, not even after `remove` has emptied the
/// list. `next_free` is what the file carries so that a restart cannot
/// undo that promise.
#[derive(Clone, Debug, Default)]
pub struct InstanceList {
    items: Vec<Instance>,
    /// The next id to hand out. Only ever grows.
    next: u32,
}

impl InstanceList {
    pub fn new() -> Self {
        Self::default()
    }

    /// Places a widget and returns its brand-new identity.
    pub fn add(&mut self, widget: Panel, board: BoardId, rect: Option<PanelSpec>) -> InstanceId {
        let id = InstanceId(self.next.max(1));
        self.next = id.0 + 1;
        self.items.push(Instance { id, widget, board, rect });
        id
    }

    /// Places a widget the layout composes rather than saves: `nth` is
    /// its position in the generated arrangement, and its identity is
    /// that position in the reserved range ([`InstanceId::GENERATED`]).
    /// Stable across loads, and outside the counter entirely, so
    /// composing the default arrangement on every start neither
    /// collides with a saved id nor moves the file's promise.
    pub fn add_generated(&mut self, widget: Panel, board: BoardId, nth: u32) -> InstanceId {
        let id = InstanceId(InstanceId::GENERATED.saturating_add(nth));
        self.items.push(Instance { id, widget, board, rect: None });
        id
    }

    /// Gives the composed placements of the named boards a saved
    /// identity — what writing those boards down means. Returns the
    /// (old, new) pairs, so a caller holding generated ids can follow
    /// its own references over.
    ///
    /// Only the named boards, because a board the file does not write
    /// placements for — one still showing the generated arrangement —
    /// must keep composing them, or installing an addon would stop
    /// changing what it shows.
    pub fn materialize_on(&mut self, boards: &[BoardId]) -> Vec<(InstanceId, InstanceId)> {
        let old: Vec<InstanceId> = self
            .items
            .iter()
            .filter(|i| i.id.is_generated() && boards.contains(&i.board))
            .map(|i| i.id)
            .collect();
        let mut map = Vec::with_capacity(old.len());
        for was in old {
            let id = InstanceId(self.next.max(1));
            self.next = id.0 + 1;
            if let Some(i) = self.items.iter_mut().find(|i| i.id == was) {
                i.id = id;
            }
            map.push((was, id));
        }
        map
    }

    /// Puts back an instance that already has an identity — the file
    /// reader's door in. The counter is dragged past it, so the ids a
    /// file carries can never be handed out again by `add`.
    ///
    /// An id already in the list, the reserved [`InstanceId::NONE`], or
    /// one inside the generated range (which is not a file's to use) is
    /// refused: a duplicate identity is exactly the bug this whole
    /// module exists to make impossible.
    pub fn restore(&mut self, inst: Instance) -> bool {
        if !inst.id.is_some() || inst.id.is_generated() || self.get(inst.id).is_some() {
            return false;
        }
        self.next = self.next.max(inst.id.0 + 1);
        self.items.push(inst);
        true
    }

    /// Drops one instance. The others keep their identities — that is
    /// the property the whole design is for — and the id is retired.
    pub fn remove(&mut self, id: InstanceId) -> bool {
        let before = self.items.len();
        self.items.retain(|i| i.id != id);
        before != self.items.len()
    }

    /// Drops every instance of a board — what removing a board does.
    pub fn remove_board(&mut self, k: BoardId) {
        self.items.retain(|i| i.board != k);
    }

    pub fn get(&self, id: InstanceId) -> Option<&Instance> {
        self.items.iter().find(|i| i.id == id)
    }

    pub fn get_mut(&mut self, id: InstanceId) -> Option<&mut Instance> {
        self.items.iter_mut().find(|i| i.id == id)
    }

    /// Moves one instance's rectangle; false when it is not here.
    pub fn set_rect(&mut self, id: InstanceId, rect: Option<PanelSpec>) -> bool {
        match self.get_mut(id) {
            Some(i) => {
                i.rect = rect;
                true
            }
            None => false,
        }
    }

    /// Moves one instance to another board — dragging a widget from one
    /// room to the next, identity and all.
    pub fn set_board(&mut self, id: InstanceId, board: BoardId) -> bool {
        match self.get_mut(id) {
            Some(i) => {
                i.board = board;
                true
            }
            None => false,
        }
    }

    pub fn iter(&self) -> std::slice::Iter<'_, Instance> {
        self.items.iter()
    }

    /// The instances of one board, to be edited in place — what
    /// renumbering a board walks.
    pub fn iter_mut_on(&mut self, k: BoardId) -> impl Iterator<Item = &mut Instance> {
        self.items.iter_mut().filter(move |i| i.board == k)
    }

    pub fn all(&self) -> &[Instance] {
        &self.items
    }

    /// The instances of one board, in placement order.
    pub fn on_board(&self, k: BoardId) -> Vec<Instance> {
        self.items.iter().filter(|i| i.board == k).copied().collect()
    }

    /// Every instance running the given widget — the answer that used
    /// to be "one, by definition".
    pub fn of_widget(&self, w: Panel) -> Vec<Instance> {
        self.items.iter().filter(|i| i.widget == w).copied().collect()
    }

    /// The first instance of a widget, for the callers that genuinely
    /// mean "the one" — a menu that offers to add a widget the layout
    /// does not have yet, and nothing else.
    pub fn first_of(&self, w: Panel) -> Option<InstanceId> {
        self.items.iter().find(|i| i.widget == w).map(|i| i.id)
    }

    pub fn count_of(&self, w: Panel) -> usize {
        self.items.iter().filter(|i| i.widget == w).count()
    }

    /// Every board any instance stands on, sorted — the boards the file
    /// actually needs to write.
    pub fn boards(&self) -> Vec<BoardId> {
        let mut out: Vec<BoardId> = self.items.iter().map(|i| i.board).collect();
        out.sort_unstable();
        out.dedup();
        out
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// The next id this list will hand out — written into the file, so
    /// that reloading a layout whose highest instance was deleted does
    /// not start reusing that id.
    pub fn next_free(&self) -> u32 {
        self.next.max(1)
    }

    /// Raises the counter to what a file recorded. Lowering it is
    /// refused: nothing may ever make `add` repeat an id. A value from
    /// the generated range is not a promise a file can make and is
    /// ignored.
    pub fn reserve_up_to(&mut self, next: u32) {
        if next < InstanceId::GENERATED {
            self.next = self.next.max(next);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> Panel {
        crate::flex::install_test_registry();
        Panel::from_name("w01").expect("the test registry")
    }

    /// The feature this module exists for: one widget, many instances,
    /// on the same board, each with an identity of its own.
    #[test]
    fn the_same_widget_can_stand_on_one_board_twice() {
        let w = setup();
        let mut l = InstanceList::new();
        let a = l.add(w, (0, 0), None);
        let b = l.add(w, (0, 0), None);
        assert_ne!(a, b);
        assert_eq!(l.count_of(w), 2);
        assert_eq!(l.on_board((0, 0)).len(), 2);
    }

    /// Removing the middle instance leaves every other identity exactly
    /// where it was — the property an index into a vector cannot give.
    #[test]
    fn removing_the_middle_instance_moves_nobody() {
        let w = setup();
        let mut l = InstanceList::new();
        let (a, b, c) = (
            l.add(w, (0, 0), None),
            l.add(w, (0, 0), None),
            l.add(w, (0, 0), None),
        );
        assert!(l.remove(b));
        assert!(l.get(a).is_some() && l.get(c).is_some());
        assert!(l.get(b).is_none());
        // And the retired id is not handed out again.
        let d = l.add(w, (0, 0), None);
        assert_ne!(d, b);
        assert!(d > c);
    }

    /// A file's ids are taken as given, and drag the counter past them.
    #[test]
    fn restored_ids_are_kept_and_never_reissued() {
        let w = setup();
        let mut l = InstanceList::new();
        assert!(l.restore(Instance::flowing(InstanceId::new(7), w, (0, 0))));
        assert!(!l.restore(Instance::flowing(InstanceId::new(7), w, (1, 0))), "no duplicates");
        assert!(!l.restore(Instance::flowing(InstanceId::NONE, w, (0, 0))), "0 is reserved");
        assert_eq!(l.add(w, (0, 0), None), InstanceId::new(8));
    }
}
