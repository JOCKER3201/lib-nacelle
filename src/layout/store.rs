//! Named `.layaut` files on a filesystem (u3 §3.2 `LayautStore`).
//!
//! Reads from every root of an [`AssetRoots`] in order; writes only
//! ever into its write root. Editing a layout that came from a system
//! directory copies it into the user's on the first save, rather than
//! failing on a path only root can write.

use super::def::{BoardDef, BoardId, LayoutDef, ResOverride, ScreenKey};
use super::layaut;
use crate::assets::AssetRoots;
use crate::base::{LayoutMode, LayoutSpec, Panel, PanelSpec};
use std::path::Path;

pub struct LayautStore {
    roots: AssetRoots,
}

impl LayautStore {
    pub fn new(roots: AssetRoots) -> Self {
        Self { roots }
    }

    /// "default" plus every `<name>.layaut` on the search path, first
    /// root holding a name wins. Dotfiles are the toolkit's own
    /// bookkeeping and are not offered.
    pub fn list(&self) -> Vec<String> {
        let mut out = vec!["default".to_string()];
        for dir in self.roots.dirs("layauts") {
            for stem in list_stems(&dir, "layaut") {
                if stem != "default" && !out.contains(&stem) {
                    out.push(stem);
                }
            }
        }
        out
    }

    /// None when the name is not installed. "default" with no file is
    /// the generated responsive arrangement, carrying the size table it
    /// was composed from: the ref/min heights belong to the LAYOUT — a
    /// .layaut names its own in its ref/min column — and the generated
    /// one has no numbers of its own, so it hands on what the installed
    /// addons declared, spelled out rather than left empty.
    pub fn load(&self, name: &str) -> Option<LayoutDef> {
        if let Some(text) = self
            .roots
            .find("layauts", &format!("{name}.layaut"))
            .and_then(|p| std::fs::read_to_string(p).ok())
        {
            return Some(layaut::parse(&text, name));
        }
        if name == "default" {
            return Some(LayoutDef {
                base: LayoutMode::Flex,
                sizes: crate::flex::builtin_sizes(),
                overrides: Vec::new(),
                boards: Vec::new(),
            });
        }
        None
    }

    /// The layaut file's current text: the user's copy, or the
    /// installed one it would be copied from on first save.
    fn read_text(&self, name: &str) -> Option<String> {
        std::fs::read_to_string(self.roots.write_dir("layauts").join(format!("{name}.layaut")))
            .ok()
            .or_else(|| {
                self.roots
                    .find("layauts", &format!("{name}.layaut"))
                    .and_then(|p| std::fs::read_to_string(p).ok())
            })
    }

    /// SAVE AS: a new base for the (possibly new) file; the per-screen
    /// sections and the boards it carries survive the rewrite.
    pub fn save_full(&self, name: &str, spec: &LayoutSpec, key: ScreenKey) -> std::io::Result<()> {
        let dir = self.roots.ensure("layauts")?;
        let old = self.read_text(name).unwrap_or_default();
        let (_, sections, boards) = layaut::split_sections(&old);
        let mut out = layaut::serialize_base(spec, key);
        layaut::serialize_sections(&mut out, &sections);
        layaut::serialize_boards(&mut out, &boards);
        std::fs::write(dir.join(format!("{name}.layaut")), out)
    }

    /// SAVE: on the screen the base was created on, the base itself is
    /// rewritten with the full layout; on ANY OTHER screen only the
    /// changed panels are written into that screen's `[WxH@D]` section.
    /// The rest of the file always stays untouched.
    pub fn save_overrides(
        &self,
        name: &str,
        key: ScreenKey,
        changes: &[(Panel, PanelSpec)],
        full: &LayoutSpec,
    ) -> std::io::Result<()> {
        let dir = self.roots.ensure("layauts")?;
        let path = dir.join(format!("{name}.layaut"));
        let text = self.read_text(name).unwrap_or_default();
        let (base, mut sections, boards) = layaut::split_sections(&text);

        if layaut::base_screen_of(&base) == Some(key) {
            // Editing on the base's own screen: rewrite the base in full.
            let mut out = layaut::serialize_base(full, key);
            layaut::serialize_sections(&mut out, &sections);
            layaut::serialize_boards(&mut out, &boards);
            return std::fs::write(path, out);
        }

        // Another screen: merge the changes into its section.
        let sec = match sections.iter_mut().find(|o| (o.w, o.h, o.diag) == key) {
            Some(s) => s,
            None => {
                sections.push(ResOverride {
                    w: key.0,
                    h: key.1,
                    diag: key.2,
                    panels: Vec::new(),
                });
                sections.last_mut().unwrap()
            }
        };
        for (panel, spec) in changes {
            sec.panels.retain(|(p, _)| p != panel);
            sec.panels.push((*panel, *spec));
        }

        let mut out = String::new();
        let base_trim = base.trim_end();
        if !base_trim.is_empty() {
            out.push_str(base_trim);
            out.push('\n');
        } else {
            out.push_str(
                "# nacelle layout: per-screen overrides on top of the default layout.\n",
            );
        }
        layaut::serialize_sections(&mut out, &sections);
        layaut::serialize_boards(&mut out, &boards);
        std::fs::write(path, out)
    }

    /// Rewrites the boards of the named layout, leaving everything else
    /// in its file alone. The shared tail of the three board operations.
    fn write_boards(&self, name: &str, boards: Vec<(BoardId, BoardDef)>) -> std::io::Result<()> {
        let dir = self.roots.ensure("layauts")?;
        let text = self.read_text(name).unwrap_or_default();
        let (base, sections, _) = layaut::split_sections(&text);
        let mut out = String::new();
        let base_trim = base.trim_end();
        if base_trim.is_empty() {
            // No base yet: the boards hang off the built-in default
            // layout, and the file says only what it knows.
            out.push_str("# nacelle layout: boards on top of the default layout.\n");
        } else {
            out.push_str(base_trim);
            out.push('\n');
        }
        layaut::serialize_sections(&mut out, &sections);
        layaut::serialize_boards(&mut out, &layaut::normalize_boards(boards));
        std::fs::write(dir.join(format!("{name}.layaut")), out)
    }

    /// Current boards of the named layout, straight from its file.
    fn boards_of(&self, name: &str) -> Vec<(BoardId, BoardDef)> {
        let text = self.read_text(name).unwrap_or_default();
        layaut::normalize_boards(layaut::split_sections(&text).2)
    }

    /// SAVE while on a board: that board's panels, into the layout's
    /// file. The grid editor speaks rectangles, so a board saved from
    /// it is a fixed board.
    pub fn set_board(&self, name: &str, k: BoardId, spec: &LayoutSpec) -> std::io::Result<()> {
        let mut boards = self.boards_of(name);
        boards.retain(|(i, _)| *i != k);
        boards.push((k, BoardDef { base: LayoutMode::Fixed(spec.clone()), sizes: Vec::new() }));
        self.write_boards(name, boards)
    }

    /// A new, empty board at the given end of the horizontal row:
    /// negative is left, positive right. Only the row grows — the top
    /// and bottom boards are fixtures, one each, like home.
    pub fn add_board(&self, name: &str, side: i8) -> std::io::Result<()> {
        let mut boards = self.boards_of(name);
        let s: i32 = if side < 0 { -1 } else { 1 };
        let next = boards
            .iter()
            .filter_map(|(id, _)| (id.1 == 0 && id.0 * s > 0).then_some(id.0 * s))
            .max()
            .unwrap_or(0)
            + 1;
        boards.push((
            (next * s, 0),
            BoardDef { base: LayoutMode::Fixed(LayoutSpec::default()), sizes: Vec::new() },
        ));
        self.write_boards(name, boards)
    }

    /// Removes a horizontal board; the ones beyond it close ranks,
    /// which normalisation does on the way out. The top and bottom
    /// boards are permanent and stay whatever is asked.
    pub fn remove_board(&self, name: &str, k: BoardId) -> std::io::Result<()> {
        if k.1 != 0 {
            return Ok(());
        }
        let mut boards = self.boards_of(name);
        boards.retain(|(i, _)| *i != k);
        self.write_boards(name, boards)
    }

    /// Deletes just the `[WxH@D]` section of one layaut, leaving its
    /// base, its other screens and its boards untouched. The inverse of
    /// save_overrides.
    pub fn clear_screen_section(&self, name: &str, key: ScreenKey) -> std::io::Result<()> {
        let dir = self.roots.ensure("layauts")?;
        let text = self.read_text(name).unwrap_or_default();
        let (base, mut sections, boards) = layaut::split_sections(&text);
        let before = sections.len();
        sections.retain(|o| (o.w, o.h, o.diag) != key);
        if sections.len() == before {
            // Nothing pinned for this screen: the file is left alone.
            return Ok(());
        }
        let mut out = String::new();
        let base_trim = base.trim_end();
        if base_trim.is_empty() {
            out.push_str(
                "# nacelle layout: per-screen overrides on top of the default layout.\n",
            );
        } else {
            out.push_str(base_trim);
            out.push('\n');
        }
        layaut::serialize_sections(&mut out, &sections);
        layaut::serialize_boards(&mut out, &boards);
        std::fs::write(dir.join(format!("{name}.layaut")), out)
    }
}

/// Stems of `<stem>.<ext>` files in a directory, dotfiles excluded.
fn list_stems(dir: &Path, ext: &str) -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for entry in rd.flatten() {
            let p = entry.path();
            let matches = p.is_file()
                && p.extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e.eq_ignore_ascii_case(ext))
                    .unwrap_or(false);
            if matches {
                if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
                    // Dotfiles are the toolkit's own bookkeeping — the
                    // extra widget boards live in .board<k>.layaut —
                    // and are not offered as selectable layouts.
                    if stem.starts_with('.') {
                        continue;
                    }
                    out.push(stem.to_string());
                }
            }
        }
    }
    out.sort();
    out
}
