//! A tree, flattened to the row list the views already draw.
//!
//! The trick — the one every mature toolkit's tree list eventually
//! arrives at — is that a tree is not a second kind of view. It is a
//! MODEL: [`FlatTree`] walks nested data, emits the rows that are
//! currently visible, and implements [`RowModel`]. Drawing, scrolling,
//! selection and virtualisation are then in 100% the machinery of the
//! list and the table, and a bug fixed in one is fixed in all three.
//!
//! Expansion is state on the FLATTENER, not on the data. Collapsing a
//! node removes its descendants from the flat list and changes nothing
//! else; the set of expanded nodes is keyed by PATH (`"usr/share/fonts"`),
//! so a data refresh — a new snapshot, a re-read directory — keeps the
//! shape the user opened. That is the same rule the table's selection
//! follows for the same reason: an index means nothing across two
//! snapshots, and a path means the same thing in both.

use super::model::{RowBuf, RowModel};
use std::collections::HashSet;

/// The separator between path segments.
///
/// A key containing it makes an ambiguous path — `"a/b"` under `"x"`
/// reads as the child `"b"` of `"x/a"`. Callers building a tree from
/// arbitrary strings should say so in their own key rule; the file
/// system, where this notation comes from, has the same constraint and
/// solves it the same way.
pub const SEP: char = '/';

/// Nested data a [`FlatTree`] can walk.
///
/// Addressed by PATH rather than by node handle so a model may be lazy:
/// a real file tree answers `child_count` with one `readdir` and never
/// materialises the branches nobody opened. A model with everything in
/// memory ignores the distinction, which is why [`MemTree`] is three
/// lines of navigation.
pub trait TreeModel {
    /// How many children the node at `path` has; `path` empty is the
    /// root. A path the model does not know answers 0 — a refresh that
    /// dropped a branch must not panic the view that was showing it.
    fn child_count(&self, path: &str) -> usize;

    /// Writes child `i` of `path` into `out` and answers its KEY — the
    /// last segment of its own path, and what makes it findable again
    /// after a refresh.
    fn child(&self, path: &str, i: usize, out: &mut RowBuf) -> String;

    /// The model's rewrite counter: what tells the flattener "new data"
    /// from "the same data again".
    fn generation(&self) -> u64 {
        0
    }
}

/// One row of the flattened tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlatNode {
    /// The node's full path — its identity, and the key the view
    /// selects by.
    pub path: String,
    /// Its parent's path, and the index it sits at there: together, how
    /// the row is fetched from the model without a second walk.
    pub parent: String,
    pub index: usize,
    /// How deep it sits; 0 for a root.
    pub depth: u16,
    pub has_children: bool,
    pub expanded: bool,
}

/// Nested data seen as a flat row list.
///
/// Rebuilt lazily: the walk happens when the model's generation moves or
/// the user expands something, never per frame. Between those it is a
/// `Vec` and answering [`RowModel::len`] is a load.
pub struct FlatTree<M: TreeModel> {
    model: M,
    /// The PATHS the user has opened. Survives a data refresh — that is
    /// the whole reason it is paths and not indices.
    expanded: HashSet<String>,
    rows: Vec<FlatNode>,
    /// Bumped by every expand/collapse, so the rebuild key notices a
    /// change the model's generation cannot see.
    shape: u64,
    /// `(generation, shape)` the current `rows` were built from.
    built: Option<(u64, u64)>,
    /// How deep the walk may go. A cyclic or pathological model would
    /// otherwise flatten forever; the view can only show what a screen
    /// holds anyway.
    max_depth: u16,
}

/// An empty tree of an empty model — what a state map hands out before
/// the first draw has said what the model is.
impl<M: TreeModel + Default> Default for FlatTree<M> {
    fn default() -> FlatTree<M> {
        FlatTree::new(M::default())
    }
}

impl<M: TreeModel> FlatTree<M> {
    pub fn new(model: M) -> FlatTree<M> {
        FlatTree {
            model,
            expanded: HashSet::new(),
            rows: Vec::new(),
            shape: 0,
            built: None,
            max_depth: 64,
        }
    }

    /// The model underneath, for a caller that has to reach it (the row
    /// fetch does).
    pub fn model(&self) -> &M {
        &self.model
    }

    /// Swaps in fresh data, KEEPING the expansion. The one operation
    /// this whole file exists for: a refreshed process list, a re-read
    /// directory, a new snapshot — same shape, new contents.
    pub fn set_model(&mut self, model: M) {
        self.model = model;
        self.built = None;
    }

    pub fn is_expanded(&self, path: &str) -> bool {
        self.expanded.contains(path)
    }

    /// Opens a node. Expanding one that is already open, or one the
    /// model does not have, changes nothing — a click that arrives
    /// twice must not cost a rebuild.
    pub fn expand(&mut self, path: &str) {
        if self.expanded.insert(path.to_string()) {
            self.shape = self.shape.wrapping_add(1);
        }
    }

    /// Closes a node. Its descendants leave the flat list; nothing else
    /// changes, and re-expanding it puts them back exactly as they were
    /// — including their own expansion, which was never forgotten.
    pub fn collapse(&mut self, path: &str) {
        if self.expanded.remove(path) {
            self.shape = self.shape.wrapping_add(1);
        }
    }

    pub fn toggle(&mut self, path: &str) {
        if self.is_expanded(path) {
            self.collapse(path);
        } else {
            self.expand(path);
        }
    }

    /// Everything closed.
    pub fn collapse_all(&mut self) {
        if !self.expanded.is_empty() {
            self.expanded.clear();
            self.shape = self.shape.wrapping_add(1);
        }
    }

    /// The expansion set, for a caller that wants to carry it across a
    /// model it is rebuilding from scratch.
    pub fn expansion(&self) -> Vec<String> {
        let mut v: Vec<String> = self.expanded.iter().cloned().collect();
        v.sort();
        v
    }

    /// Puts back an expansion taken with [`FlatTree::expansion`].
    pub fn set_expansion<I: IntoIterator<Item = String>>(&mut self, paths: I) {
        self.expanded = paths.into_iter().collect();
        self.shape = self.shape.wrapping_add(1);
    }

    /// Walks the model if anything can have changed since the last
    /// walk. Cheap and idempotent: a view calls it at the top of every
    /// draw and pays a comparison.
    pub fn sync(&mut self) {
        let key = (self.model.generation(), self.shape);
        if self.built == Some(key) {
            return;
        }
        self.built = Some(key);
        self.rows.clear();
        let mut buf = RowBuf::new();
        walk(
            &self.model,
            &self.expanded,
            String::new(),
            0,
            self.max_depth,
            &mut buf,
            &mut self.rows,
        );
    }

    /// The flattened rows, as of the last [`FlatTree::sync`].
    pub fn rows(&self) -> &[FlatNode] {
        &self.rows
    }

    /// The display position of a path, for putting a selection back in
    /// view after a refresh. Linear — used on a click, never per frame.
    pub fn position_of(&self, path: &str) -> Option<usize> {
        self.rows.iter().position(|n| n.path == path)
    }
}

/// The walk itself: children in model order, each followed immediately
/// by its own subtree when it is open. Pre-order, which is the order a
/// tree reads in.
fn walk<M: TreeModel>(
    model: &M,
    expanded: &HashSet<String>,
    parent: String,
    depth: u16,
    max_depth: u16,
    buf: &mut RowBuf,
    out: &mut Vec<FlatNode>,
) {
    if depth > max_depth {
        return;
    }
    let n = model.child_count(&parent);
    for i in 0..n {
        buf.reset();
        let key = model.child(&parent, i, buf);
        let path = if parent.is_empty() {
            key
        } else {
            format!("{parent}{SEP}{key}")
        };
        let open = expanded.contains(&path);
        // A node's children are counted, not fetched: `has_children`
        // decides whether an expander is drawn, and a lazy model answers
        // it without reading the branch.
        let has_children = model.child_count(&path) > 0;
        out.push(FlatNode {
            path: path.clone(),
            parent: parent.clone(),
            index: i,
            depth,
            has_children,
            expanded: open && has_children,
        });
        if open && has_children {
            walk(model, expanded, path, depth + 1, max_depth, buf, out);
        }
    }
}

/// The whole trick: a tree IS a row list, so every view that draws rows
/// draws a tree without knowing it is one.
impl<M: TreeModel> RowModel for FlatTree<M> {
    fn len(&self) -> usize {
        self.rows.len()
    }

    fn row(&self, index: usize, out: &mut RowBuf) {
        out.reset();
        let Some(node) = self.rows.get(index) else { return };
        self.model.child(&node.parent, node.index, out);
        // The flattener's own three facts overwrite whatever the model
        // put there: only it knows where the node ended up.
        out.key = node.path.clone();
        out.depth = node.depth;
        out.has_children = node.has_children;
        out.expanded = node.expanded;
    }

    fn generation(&self) -> u64 {
        self.model.generation() ^ self.shape.rotate_left(32)
    }

    fn key(&self, index: usize) -> String {
        self.rows.get(index).map(|n| n.path.clone()).unwrap_or_default()
    }
}

// ------------------------------------------------------- a tree in memory

/// One node of a [`MemTree`].
#[derive(Clone, Debug, Default)]
pub struct MemNode {
    pub row: RowBuf,
    pub children: Vec<MemNode>,
}

impl MemNode {
    /// A leaf carrying a label, keyed by that label.
    pub fn leaf(label: &str) -> MemNode {
        MemNode {
            row: RowBuf { key: label.into(), label: label.into(), ..RowBuf::default() },
            children: Vec::new(),
        }
    }

    pub fn with_children(mut self, children: Vec<MemNode>) -> MemNode {
        self.children = children;
        self
    }
}

/// A tree already in memory — what a script's `tree` element produces.
///
/// A script's answer is bounded by `max_array_size`, so the whole tree
/// was materialised before the renderer ever saw it; there is nothing
/// lazy left to be. A real file tree is the other case, and belongs to a
/// plugin with a [`TreeModel`] that reads a directory when it is asked.
#[derive(Clone, Debug, Default)]
pub struct MemTree {
    roots: Vec<MemNode>,
    generation: u64,
}

impl MemTree {
    pub fn new(roots: Vec<MemNode>) -> MemTree {
        MemTree { roots, generation: 0 }
    }

    pub fn with_generation(mut self, generation: u64) -> MemTree {
        self.generation = generation;
        self
    }

    /// The children of `path`, or `None` when no such node exists.
    fn at(&self, path: &str) -> Option<&Vec<MemNode>> {
        if path.is_empty() {
            return Some(&self.roots);
        }
        let mut level = &self.roots;
        for seg in path.split(SEP) {
            let node = level.iter().find(|n| n.row.key == seg)?;
            level = &node.children;
        }
        Some(level)
    }
}

impl TreeModel for MemTree {
    fn child_count(&self, path: &str) -> usize {
        self.at(path).map(|c| c.len()).unwrap_or(0)
    }

    fn child(&self, path: &str, i: usize, out: &mut RowBuf) -> String {
        let Some(node) = self.at(path).and_then(|c| c.get(i)) else {
            return String::new();
        };
        out.clone_from(&node.row);
        node.row.key.clone()
    }

    fn generation(&self) -> u64 {
        self.generation
    }
}

// ---------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::Sev;

    /// usr/{share/{fonts, icons}, lib}, etc/{hosts}
    fn fs(generation: u64, extra_leaf: bool) -> MemTree {
        let mut share = MemNode::leaf("share")
            .with_children(vec![MemNode::leaf("fonts"), MemNode::leaf("icons")]);
        if extra_leaf {
            share.children.push(MemNode::leaf("themes"));
        }
        MemTree::new(vec![
            MemNode::leaf("usr").with_children(vec![share, MemNode::leaf("lib")]),
            MemNode::leaf("etc").with_children(vec![MemNode::leaf("hosts")]),
        ])
        .with_generation(generation)
    }

    fn paths<M: TreeModel>(t: &FlatTree<M>) -> Vec<String> {
        t.rows().iter().map(|n| n.path.clone()).collect()
    }

    #[test]
    fn a_closed_tree_is_its_roots() {
        let mut t = FlatTree::new(fs(1, false));
        t.sync();
        assert_eq!(paths(&t), vec!["usr", "etc"]);
        assert_eq!(t.len(), 2);
        assert!(t.rows()[0].has_children);
        assert!(!t.rows()[0].expanded);
    }

    #[test]
    fn expanding_inserts_the_children_after_the_node_and_nowhere_else() {
        let mut t = FlatTree::new(fs(1, false));
        t.expand("usr");
        t.sync();
        assert_eq!(paths(&t), vec!["usr", "usr/share", "usr/lib", "etc"]);
        t.expand("usr/share");
        t.sync();
        assert_eq!(
            paths(&t),
            vec!["usr", "usr/share", "usr/share/fonts", "usr/share/icons", "usr/lib", "etc"]
        );
        assert_eq!(t.rows()[2].depth, 2);
    }

    #[test]
    fn collapsing_removes_the_descendants_and_remembers_them() {
        let mut t = FlatTree::new(fs(1, false));
        t.expand("usr");
        t.expand("usr/share");
        t.sync();
        assert_eq!(t.len(), 6);
        t.collapse("usr");
        t.sync();
        assert_eq!(paths(&t), vec!["usr", "etc"]);
        // The inner expansion was never forgotten — reopening the parent
        // puts the whole shape back, which is what makes a tree feel
        // like a place rather than a query.
        t.expand("usr");
        t.sync();
        assert_eq!(
            paths(&t),
            vec!["usr", "usr/share", "usr/share/fonts", "usr/share/icons", "usr/lib", "etc"]
        );
    }

    #[test]
    fn a_leaf_has_no_expander_and_expanding_it_shows_nothing() {
        let mut t = FlatTree::new(fs(1, false));
        t.expand("etc");
        t.sync();
        let hosts = &t.rows()[2];
        assert_eq!(hosts.path, "etc/hosts");
        assert!(!hosts.has_children);
        t.expand("etc/hosts");
        t.sync();
        assert_eq!(paths(&t), vec!["usr", "etc", "etc/hosts"]);
    }

    #[test]
    fn a_model_refresh_keeps_the_shape_the_user_opened() {
        // The requirement of §4, and of the whole path-keyed design: new
        // data arrives, and the tree does not close under the hand.
        let mut t = FlatTree::new(fs(1, false));
        t.expand("usr");
        t.expand("usr/share");
        t.sync();
        let before = paths(&t);
        t.set_model(fs(2, true));
        t.sync();
        let after = paths(&t);
        assert_eq!(after.len(), before.len() + 1, "only the new leaf appeared");
        assert_eq!(
            after,
            vec![
                "usr",
                "usr/share",
                "usr/share/fonts",
                "usr/share/icons",
                "usr/share/themes",
                "usr/lib",
                "etc"
            ]
        );
        assert!(t.is_expanded("usr/share"));
    }

    #[test]
    fn a_refresh_that_drops_an_open_branch_leaves_nothing_dangling() {
        let mut t = FlatTree::new(fs(1, false));
        t.expand("usr");
        t.expand("usr/share");
        t.sync();
        t.set_model(MemTree::new(vec![MemNode::leaf("etc")]).with_generation(3));
        t.sync();
        assert_eq!(paths(&t), vec!["etc"]);
        // The expansion is REMEMBERED, not pruned: the branch may come
        // back on the next refresh, and forgetting would close it.
        assert!(t.is_expanded("usr/share"));
    }

    #[test]
    fn the_walk_happens_once_per_change_and_not_per_frame() {
        // A model that counts how often it is asked, so "lazily
        // rebuilt" is a fact rather than a comment.
        struct Counting(std::cell::Cell<usize>, MemTree);
        impl TreeModel for Counting {
            fn child_count(&self, path: &str) -> usize {
                self.0.set(self.0.get() + 1);
                self.1.child_count(path)
            }
            fn child(&self, path: &str, i: usize, out: &mut RowBuf) -> String {
                self.1.child(path, i, out)
            }
            fn generation(&self) -> u64 {
                self.1.generation()
            }
        }
        let mut t = FlatTree::new(Counting(std::cell::Cell::new(0), fs(1, false)));
        t.sync();
        let after_first = t.model().0.get();
        assert!(after_first > 0);
        for _ in 0..10 {
            t.sync();
        }
        assert_eq!(t.model().0.get(), after_first, "ten frames, no second walk");
        t.expand("usr");
        t.sync();
        assert!(t.model().0.get() > after_first);
    }

    #[test]
    fn a_flattened_row_carries_the_model_and_the_shape() {
        let mut roots = vec![MemNode::leaf("cpu")];
        roots[0].row.severity = Some(Sev(2));
        roots[0].row.status = "hot".into();
        roots[0].children = vec![MemNode::leaf("core0")];
        let mut t = FlatTree::new(MemTree::new(roots));
        t.expand("cpu");
        t.sync();
        let mut buf = RowBuf::new();
        t.row(0, &mut buf);
        assert_eq!(buf.key, "cpu", "the KEY is the path, not the label");
        assert_eq!(buf.label, "cpu");
        assert_eq!(buf.status, "hot");
        assert_eq!(buf.severity, Some(Sev(2)));
        assert!(buf.has_children);
        assert!(buf.expanded);
        t.row(1, &mut buf);
        assert_eq!(buf.key, "cpu/core0");
        assert_eq!(buf.depth, 1);
        assert!(!buf.has_children);
        assert_eq!(buf.status, "", "the buffer carries nothing of the row before");
    }

    #[test]
    fn the_generation_moves_when_either_the_data_or_the_shape_does() {
        let mut t = FlatTree::new(fs(1, false));
        t.sync();
        let g0 = RowModel::generation(&t);
        t.expand("usr");
        assert_ne!(RowModel::generation(&t), g0, "the shape changed");
        let g1 = RowModel::generation(&t);
        t.set_model(fs(2, false));
        assert_ne!(RowModel::generation(&t), g1, "the data changed");
    }
}
