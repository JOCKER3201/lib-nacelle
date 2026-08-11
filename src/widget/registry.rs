//! The addon directory scan (u3 §3.3 `widget::registry`).
//!
//! `<root>/addons/` holds exactly two directories and nothing else:
//! `scripts/<name>.rhai` and `plugins/<name>.so`, flat — the file IS
//! the addon, its stem is its name. The category that used to live in
//! a directory name (`board`, `appgrid`, `search_and_ai`) lives in
//! the addon itself now: a script declares it in a header pragma
//! (`// category: appgrid`) within its first lines, a compiled plugin
//! has no way to say yet (a future host-table entry can add one), and
//! anything that names no category is a board widget — which every
//! shipped addon is. First root holding a NAME wins (a user install
//! shadows a system one), files are sorted before the walk and the
//! merge is sorted by name, so panel order never depends on the
//! filesystem.

use crate::assets::{safe_component, AssetRoots};
use crate::base::{builtin_widgets, WidgetCategory, WidgetDef};
use std::path::{Path, PathBuf};

/// The scan over every root, merged: the first root holding a given
/// name wins, sorted by name after the merge.
pub fn scan(roots: &AssetRoots) -> Vec<WidgetDef> {
    let mut out: Vec<WidgetDef> = Vec::new();
    for dir in roots.dirs("addons") {
        for def in scan_dir(&dir) {
            if !out.iter().any(|d| d.name == def.name) {
                out.push(def);
            }
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// The installed script for the name, on the search path.
pub fn script_path(roots: &AssetRoots, name: &str) -> Option<PathBuf> {
    let name = safe_component(name)?;
    roots.find("addons", &format!("scripts/{name}.rhai")).filter(|p| p.is_file())
}

/// The installed compiled plugin for the name, on the search path.
pub fn plugin_path(roots: &AssetRoots, name: &str) -> Option<PathBuf> {
    let name = safe_component(name)?;
    roots.find("addons", &format!("plugins/{name}.so")).filter(|p| p.is_file())
}

/// One root's scan: scripts, then plugins, each flat.
fn scan_dir(dir: &Path) -> Vec<WidgetDef> {
    let known = builtin_widgets();
    let mut out: Vec<WidgetDef> = Vec::new();
    for (sub, ext) in [("scripts", "rhai"), ("plugins", "so")] {
        let Ok(rd) = std::fs::read_dir(dir.join(sub)) else { continue };
        let mut files: Vec<PathBuf> = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.is_file() && p.extension().and_then(|e| e.to_str()) == Some(ext)
            })
            .collect();
        // Sorted, so panel order does not depend on the filesystem.
        files.sort();
        for path in files {
            let Some(name) = path.file_stem().and_then(|n| n.to_str()) else { continue };
            let Some(name) = safe_component(name) else { continue };
            if out.iter().any(|d| d.name == name) {
                continue;
            }
            // A compiled plugin cannot declare a category through the
            // table yet, so it is a board widget until an entry lands.
            let cat = if ext == "rhai" { script_category(&path) } else { WidgetCategory::Board };
            // The editor label and the default sizes come from the
            // built-in table for the shipped names; anything else gets
            // its file stem and the standard sizes.
            out.push(match known.iter().find(|d| d.name == name) {
                Some(d) => WidgetDef { category: cat, ..d.clone() },
                None => WidgetDef {
                    label: name.to_uppercase(),
                    name,
                    ref_h_vh: 10.0,
                    min_h_vh: 6.0,
                    category: cat,
                },
            });
        }
    }
    out
}

/// The category a script declares about itself: a `// category: <word>`
/// pragma within the first lines. Read as text, never executed — the
/// scan must stay cheap and a broken script must still be listed (its
/// failure is reported when it runs). An unknown word is a board
/// widget, the same degradation every theme token takes.
fn script_category(path: &Path) -> WidgetCategory {
    let Ok(text) = std::fs::read_to_string(path) else {
        return WidgetCategory::Board;
    };
    for line in text.lines().take(16) {
        let Some(rest) = line.trim().strip_prefix("//") else { continue };
        let Some(word) = rest.trim().strip_prefix("category:") else { continue };
        return match word.trim() {
            "appgrid" => WidgetCategory::Appgrid,
            "search_and_ai" => WidgetCategory::SearchAi,
            _ => WidgetCategory::Board,
        };
    }
    WidgetCategory::Board
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The scan over the addons layout: names, order, labels, sizes
    /// and categories — first root wins, sort by name, unknown names
    /// get uppercase labels and the standard sizes, a stray file of
    /// the wrong extension is not an addon, and a script's category
    /// pragma is honoured.
    #[test]
    fn the_scan_reads_the_addons_layout() {
        let base = std::env::temp_dir().join(format!("nacelle-regscan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (a, b) = (base.join("user"), base.join("system"));
        for (root, files) in [
            (
                &a,
                vec![
                    ("scripts/zeta.rhai", ""),
                    ("plugins/launcher.so", ""),
                ],
            ),
            (
                &b,
                vec![
                    ("scripts/zeta.rhai", "// category: appgrid\nfn draw() { [] }"),
                    ("scripts/clock.rhai", ""),
                    ("scripts/grid.rhai", "// category: appgrid\nfn draw() { [] }"),
                    ("scripts/readme.txt", "not an addon"),
                ],
            ),
        ] {
            for (rel, body) in files {
                let path = root.join("addons").join(rel);
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(&path, body).unwrap();
            }
        }
        let roots = AssetRoots::new(vec![a.clone(), b.clone()], a.clone());
        let defs = scan(&roots);
        let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["clock", "grid", "launcher", "zeta"], "sorted, merged, no .txt");
        let zeta = defs.iter().find(|d| d.name == "zeta").unwrap();
        assert_eq!(zeta.label, "ZETA");
        assert_eq!((zeta.ref_h_vh, zeta.min_h_vh), (10.0, 6.0));
        assert!(
            matches!(zeta.category, WidgetCategory::Board),
            "the FIRST root's zeta wins, and it declares nothing"
        );
        assert!(matches!(
            defs.iter().find(|d| d.name == "grid").unwrap().category,
            WidgetCategory::Appgrid
        ));
        let clock = defs.iter().find(|d| d.name == "clock").unwrap();
        assert_ne!(clock.label, "CLOCK".to_lowercase(), "shipped name keeps its table entry");
        assert!(matches!(
            defs.iter().find(|d| d.name == "launcher").unwrap().category,
            WidgetCategory::Board
        ));
        assert_eq!(script_path(&roots, "zeta"), Some(a.join("addons/scripts/zeta.rhai")));
        assert_eq!(plugin_path(&roots, "launcher"), Some(a.join("addons/plugins/launcher.so")));
        assert_eq!(script_path(&roots, "../zeta"), None);
        let _ = std::fs::remove_dir_all(base);
    }
}
