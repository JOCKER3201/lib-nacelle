//! The widget directory scan (u3 §3.3 `widget::registry`).
//!
//! `<root>/widgets/{board,appgrid,search_and_ai}/<name>/` holding
//! `<name>.rhai` or `<name>.so` is one widget — the file IS the
//! widget, there is no metadata; the top level itself counts as
//! `board`, the pre-split arrangement. First root holding a NAME wins
//! (a user install shadows a system one), directories are sorted
//! before the walk and the merge is sorted by name, so panel order
//! never depends on the filesystem (§6.2: the scan moved verbatim and
//! must stay byte-identical). The registry GLOBAL — first `set` wins,
//! indices cross the C ABI — stays in `base`; this module only
//! produces the list an embedder feeds it.

use crate::assets::{safe_component, AssetRoots};
use crate::base::{builtin_widgets, WidgetCategory, WidgetDef};
use std::path::{Path, PathBuf};

/// The scan over every root, merged: the first directory holding a
/// given name wins, sorted by name after the merge.
pub fn scan(roots: &AssetRoots) -> Vec<WidgetDef> {
    let mut out: Vec<WidgetDef> = Vec::new();
    for dir in roots.dirs("widgets") {
        for def in scan_dir(&dir) {
            if !out.iter().any(|d| d.name == def.name) {
                out.push(def);
            }
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// The directory of an installed widget, looked up under each category
/// subdirectory and, for installations from before the split, under
/// widgets/ itself.
pub fn widget_dir(roots: &AssetRoots, name: &str) -> Option<PathBuf> {
    let name = safe_component(name)?;
    ["board", "appgrid", "search_and_ai", ""]
        .into_iter()
        .find_map(|sub| {
            let rel = if sub.is_empty() { name.clone() } else { format!("{sub}/{name}") };
            roots.find("widgets", &rel).filter(|p| p.is_dir())
        })
}

/// One root's scan: the three category subdirectories, then the top
/// level as pre-split board widgets.
fn scan_dir(dir: &Path) -> Vec<WidgetDef> {
    let known = builtin_widgets();
    let mut out: Vec<WidgetDef> = Vec::new();
    for (sub, cat) in [
        ("board", WidgetCategory::Board),
        ("appgrid", WidgetCategory::Appgrid),
        ("search_and_ai", WidgetCategory::SearchAi),
    ] {
        scan_category(&dir.join(sub), cat, &known, &mut out);
    }
    // The top level itself: the pre-split arrangement. The category
    // subdirectories hold no widget file of their own name, so the
    // check inside skips them.
    scan_category(dir, WidgetCategory::Board, &known, &mut out);
    out
}

fn scan_category(
    dir: &Path,
    cat: WidgetCategory,
    known: &[WidgetDef],
    out: &mut Vec<WidgetDef>,
) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut dirs: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    // Sorted, so panel order does not depend on the filesystem.
    dirs.sort();
    for dir in dirs {
        let Some(name) = dir.file_name().and_then(|n| n.to_str()) else { continue };
        let Some(name) = safe_component(name) else { continue };
        let script = dir.join(format!("{name}.rhai"));
        let lib = dir.join(format!("{name}.so"));
        if !script.is_file() && !lib.is_file() {
            continue;
        }
        if out.iter().any(|d| d.name == name) {
            continue;
        }
        // The editor label and the default sizes come from the built-in
        // table for the shipped names; anything else gets its directory
        // name and the standard sizes. The CATEGORY always comes from
        // the directory the widget sits under.
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

#[cfg(test)]
mod tests {
    use super::*;

    /// §6.2's acceptance, in miniature: names, order, labels, sizes and
    /// categories of a scan are exactly the old scan's — first root
    /// wins, sort by name, unknown names get uppercase labels and the
    /// standard sizes, and a directory without its widget file is not
    /// a widget.
    #[test]
    fn the_scan_is_the_old_scan() {
        let base = std::env::temp_dir().join(format!("nacelle-regscan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (a, b) = (base.join("user"), base.join("system"));
        for (root, widgets) in [
            (&a, vec![("board/zeta", "zeta.rhai"), ("appgrid/launcher", "launcher.so")]),
            (&b, vec![("board/zeta", "zeta.rhai"), ("board/clock", "clock.rhai"), ("board/empty", "readme.txt")]),
        ] {
            for (d, f) in widgets {
                let dir = root.join("widgets").join(d);
                std::fs::create_dir_all(&dir).unwrap();
                std::fs::write(dir.join(f), "").unwrap();
            }
        }
        let roots = AssetRoots::new(vec![a.clone(), b.clone()], a.clone());
        let defs = scan(&roots);
        let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["clock", "launcher", "zeta"], "sorted, merged, no 'empty'");
        let zeta = defs.iter().find(|d| d.name == "zeta").unwrap();
        assert_eq!(zeta.label, "ZETA");
        assert_eq!((zeta.ref_h_vh, zeta.min_h_vh), (10.0, 6.0));
        let clock = defs.iter().find(|d| d.name == "clock").unwrap();
        assert_ne!(clock.label, "CLOCK".to_lowercase(), "shipped name keeps its table entry");
        assert!(matches!(
            defs.iter().find(|d| d.name == "launcher").unwrap().category,
            WidgetCategory::Appgrid
        ));
        assert_eq!(widget_dir(&roots, "zeta"), Some(a.join("widgets/board/zeta")));
        assert_eq!(widget_dir(&roots, "../zeta"), None);
        let _ = std::fs::remove_dir_all(base);
    }
}
