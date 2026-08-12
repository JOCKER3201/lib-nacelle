//! The addon directory scan (u3 §3.3 `widget::registry`).
//!
//! `<root>/addons/` holds exactly two directories and nothing else:
//! `scripts/<name>.rhai` and `plugins/<name>.so`, flat — the file IS
//! the addon, its stem is its name. There is no table of known widgets
//! anywhere in the toolkit or in the program: what exists is what the
//! scan finds, exactly as every other shell does it (Plasma scans
//! plasmoids, GNOME extensions, COSMIC applets), and a machine with
//! nothing installed has no widgets — the same honesty as a program
//! with no theme installed drawing like a page with no stylesheet.
//!
//! # The addon carries its own metadata
//!
//! Everything the layout engine and the editor need to know BEFORE the
//! addon draws — its label, its reference and minimum heights, the kind
//! of board it belongs on, where in a generated arrangement it wants to
//! stand — is declared by the addon itself:
//!
//! * a script declares it in header pragmas within its first lines,
//!   `// label: SYSTEM INFO`, `// ref_h: 4.5`, `// min_h: 4.5`,
//!   `// category: appgrid`, `// slot: left`, read as TEXT and never by
//!   running the script — the scan must stay cheap, and a broken script
//!   must still be listed (its failure is reported when it runs);
//! * a compiled plugin declares it in a `<name>.meta` file installed
//!   beside `<name>.so`, `key = value` per line. It is a separate file
//!   for the reason Plasma and GNOME keep one: the host has to know
//!   what a widget IS before it decides to open it, and reading
//!   metadata must never mean loading somebody's code.
//!
//! An addon that declares nothing still works: the label is its name in
//! capitals, the heights are the standard ones, it is a board widget,
//! it flows into whichever side of the generated arrangement has room
//! and the user may switch it off. Declaring is how an addon asks for
//! something else, never how it becomes visible.
//!
//! # The keys
//!
//! | key | what it asks for | not named |
//! |-----|------------------|-----------|
//! | `label` | the name the editor shows | the file stem in capitals |
//! | `ref_h` / `min_h` | the heights, in vh | 10 / 6 |
//! | `category` | `board`, `appgrid`, `search_and_ai` | `board` |
//! | `slot` | `left`, `center`, `right` — the column of a generated arrangement | the emptier side |
//! | `order` | where in that column, lowest first | registry order |
//! | `weight` | how much of a shared column | as much as it is tall |
//! | `anchor` | `top`, `bottom`, `bar` — a pinned edge | it flows |
//! | `essential` | `true`: removing it would leave no way back | removable |
//!
//! First root holding a NAME wins (a user install shadows a system
//! one), files are sorted before the walk and the merge is sorted by
//! name, so panel order never depends on the filesystem.

use crate::assets::{safe_component, AssetRoots};
use crate::base::{PanelAnchor, PanelSlot, WidgetCategory, WidgetDef};
use std::path::{Path, PathBuf};

/// How many lines of a script the pragma reader looks at. A header is a
/// header: past this the file is code, and a `// label:` in the middle
/// of a function is a comment about that code.
const PRAGMA_LINES: usize = 16;

/// The heights an addon that asks for nothing is given. They are not a
/// look — a layout gives every panel its own ref/min column — but the
/// engine needs a number before any layout has spoken.
const DEFAULT_REF_H_VH: f32 = 10.0;
const DEFAULT_MIN_H_VH: f32 = 6.0;

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

/// What an addon that declares nothing is: its name in capitals, the
/// standard heights, a board widget that flows wherever the generated
/// composition has room and that the user may switch off.
pub fn bare_def(name: String) -> WidgetDef {
    WidgetDef {
        label: name.to_uppercase(),
        name,
        ref_h_vh: DEFAULT_REF_H_VH,
        min_h_vh: DEFAULT_MIN_H_VH,
        category: WidgetCategory::Board,
        slot: PanelSlot::Auto,
        order: 0.0,
        weight: None,
        anchor: PanelAnchor::Flow,
        essential: false,
    }
}

/// The def an addon's own `key = value` metadata describes — the text
/// of a `<name>.meta` file, or the identical text a linked-in plugin
/// crate carries. Unknown keys and unreadable values are ignored, each
/// leaving its default: an addon written for a later version of the
/// program must still load in this one.
pub fn def_from_meta(name: String, meta: &str) -> WidgetDef {
    let mut def = bare_def(name);
    for line in meta.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            apply(&mut def, k.trim(), v.trim());
        }
    }
    def
}

/// The def a script's header pragmas describe: `// <key>: <value>` in
/// the first [`PRAGMA_LINES`] lines. Read as text, never executed.
fn def_from_script(name: String, path: &Path) -> WidgetDef {
    let mut def = bare_def(name);
    let Ok(text) = std::fs::read_to_string(path) else {
        return def;
    };
    for line in text.lines().take(PRAGMA_LINES) {
        let Some(rest) = line.trim().strip_prefix("//") else { continue };
        let Some((k, v)) = rest.split_once(':') else { continue };
        apply(&mut def, k.trim(), v.trim());
    }
    def
}

/// One declared key. The name is NOT a key: it is the file's stem, so
/// an addon can never claim to be another one by editing a comment.
fn apply(def: &mut WidgetDef, key: &str, value: &str) {
    match key {
        "label" if !value.is_empty() => def.label = value.to_string(),
        // A height must be a positive, finite number of vh; anything
        // else keeps the default, the same degradation a theme token
        // with a bad value takes.
        "ref_h" => {
            if let Some(v) = positive(value) {
                def.ref_h_vh = v;
            }
        }
        "min_h" => {
            if let Some(v) = positive(value) {
                def.min_h_vh = v;
            }
        }
        // An unknown category word is a board widget: the program may
        // grow boards this addon has never heard of, and the other way
        // round.
        "category" => {
            def.category = match value {
                "appgrid" => WidgetCategory::Appgrid,
                "search_and_ai" => WidgetCategory::SearchAi,
                _ => WidgetCategory::Board,
            }
        }
        // Which column of a generated composition the addon asks for,
        // and where in it. An unknown word is no column at all, which
        // is what an addon that says nothing already is.
        "slot" => {
            def.slot = match value {
                "left" => PanelSlot::Left,
                "center" => PanelSlot::Center,
                "right" => PanelSlot::Right,
                _ => PanelSlot::Auto,
            }
        }
        "order" => {
            if let Some(v) = finite(value) {
                def.order = v;
            }
        }
        "weight" => {
            if let Some(v) = positive(value) {
                def.weight = Some(v);
            }
        }
        // Pinning is a request, so an unknown word grants none of it
        // and the panel flows with its column.
        "anchor" => {
            def.anchor = match value {
                "top" => PanelAnchor::Top,
                "bottom" => PanelAnchor::Bottom,
                "bar" => PanelAnchor::Bar,
                _ => PanelAnchor::Flow,
            }
        }
        // Only the word that means yes turns it on: everything else,
        // including nonsense, leaves the widget removable.
        "essential" => def.essential = value == "true",
        _ => {}
    }
}

fn positive(value: &str) -> Option<f32> {
    value.parse::<f32>().ok().filter(|v| v.is_finite() && *v > 0.0)
}

fn finite(value: &str) -> Option<f32> {
    value.parse::<f32>().ok().filter(|v| v.is_finite())
}

/// One root's scan: scripts, then plugins, each flat.
fn scan_dir(dir: &Path) -> Vec<WidgetDef> {
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
            out.push(if ext == "rhai" {
                def_from_script(name, &path)
            } else {
                // The metadata file sits beside the library and is read
                // INSTEAD of it: nothing here dlopens anything.
                let meta = std::fs::read_to_string(path.with_extension("meta"));
                match meta {
                    Ok(text) => def_from_meta(name, &text),
                    Err(_) => bare_def(name),
                }
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Writes an addon tree and returns the roots over it.
    fn tree(tag: &str, roots: &[(&str, &[(&str, &str)])]) -> (PathBuf, AssetRoots) {
        let base = std::env::temp_dir()
            .join(format!("nacelle-regscan-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let mut dirs = Vec::new();
        for (root, files) in roots {
            let dir = base.join(root);
            for (rel, body) in *files {
                let path = dir.join("addons").join(rel);
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(&path, body).unwrap();
            }
            dirs.push(dir);
        }
        let write = dirs.first().cloned().unwrap_or_else(|| base.clone());
        (base.clone(), AssetRoots::new(dirs, write))
    }

    /// The scan over the addons layout: names, order, merge and the
    /// paths back to the files. A stray file of the wrong extension is
    /// not an addon, and the first root holding a name wins.
    #[test]
    fn the_scan_reads_the_addons_layout() {
        let (base, roots) = tree(
            "layout",
            &[
                ("user", &[("scripts/zeta.rhai", ""), ("plugins/launcher.so", "")]),
                (
                    "system",
                    &[
                        ("scripts/zeta.rhai", "// label: SHADOWED\nfn draw() { [] }"),
                        ("scripts/clock.rhai", ""),
                        ("scripts/readme.txt", "not an addon"),
                    ],
                ),
            ],
        );
        let defs = scan(&roots);
        let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["clock", "launcher", "zeta"], "sorted, merged, no .txt");
        assert_eq!(
            defs.iter().find(|d| d.name == "zeta").unwrap().label,
            "ZETA",
            "the FIRST root's zeta wins, and it declares nothing"
        );
        assert_eq!(
            script_path(&roots, "zeta"),
            Some(base.join("user/addons/scripts/zeta.rhai"))
        );
        assert_eq!(
            plugin_path(&roots, "launcher"),
            Some(base.join("user/addons/plugins/launcher.so"))
        );
        assert_eq!(script_path(&roots, "../zeta"), None);
        let _ = std::fs::remove_dir_all(base);
    }

    /// A script carries its own metadata in header pragmas, read as
    /// text: label, both heights and the category.
    #[test]
    fn a_script_declares_itself_in_header_pragmas() {
        let (base, roots) = tree(
            "pragmas",
            &[(
                "user",
                &[(
                    "scripts/sysinfo.rhai",
                    "// A comment that declares nothing.\n\
                     // label: SYSTEM INFO\n\
                     // ref_h: 4.5\n\
                     // min_h: 4.25\n\
                     // category: appgrid\n\
                     fn draw() { [] }\n",
                )],
            )],
        );
        let defs = scan(&roots);
        let d = defs.first().expect("the script is an addon");
        assert_eq!(d.name, "sysinfo", "the stem is the name, never a pragma");
        assert_eq!(d.label, "SYSTEM INFO");
        assert_eq!((d.ref_h_vh, d.min_h_vh), (4.5, 4.25));
        assert!(matches!(d.category, WidgetCategory::Appgrid));
        let _ = std::fs::remove_dir_all(base);
    }

    /// A compiled plugin carries its metadata in the `<name>.meta`
    /// file beside the library — read INSTEAD of the library, never
    /// after loading it.
    #[test]
    fn a_plugin_declares_itself_in_a_meta_file_beside_it() {
        let (base, roots) = tree(
            "meta",
            &[(
                "user",
                &[
                    ("plugins/shell.so", "not really a library"),
                    (
                        "plugins/shell.meta",
                        "# the terminal\nlabel = SHELL\nref_h = 60.0\nmin_h = 10.0\n",
                    ),
                    ("plugins/grid.so", ""),
                    ("plugins/grid.meta", "label = APPLICATIONS\ncategory = appgrid\n"),
                ],
            )],
        );
        let defs = scan(&roots);
        let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["grid", "shell"], "the .meta file is not itself an addon");
        let shell = defs.iter().find(|d| d.name == "shell").unwrap();
        assert_eq!(shell.label, "SHELL");
        assert_eq!((shell.ref_h_vh, shell.min_h_vh), (60.0, 10.0));
        assert!(matches!(shell.category, WidgetCategory::Board), "none named");
        let grid = defs.iter().find(|d| d.name == "grid").unwrap();
        assert!(matches!(grid.category, WidgetCategory::Appgrid));
        assert_eq!(
            (grid.ref_h_vh, grid.min_h_vh),
            (DEFAULT_REF_H_VH, DEFAULT_MIN_H_VH),
            "what it does not name it does not lose"
        );
        let _ = std::fs::remove_dir_all(base);
    }

    /// Where an addon stands in a generated arrangement is the addon's
    /// own declaration, in the same header as everything else: the
    /// column, the place in it, the share of it, the pinned edge and
    /// whether it may be switched off at all.
    #[test]
    fn an_addon_declares_where_it_wants_to_stand() {
        let (base, roots) = tree(
            "placement",
            &[(
                "user",
                &[
                    (
                        "scripts/left.rhai",
                        "// slot: left\n// order: 3\n// weight: 26\nfn draw() { [] }\n",
                    ),
                    ("plugins/pinned.so", ""),
                    (
                        "plugins/pinned.meta",
                        "slot = center\nanchor = top\nessential = true\n",
                    ),
                    ("plugins/junk.so", ""),
                    (
                        "plugins/junk.meta",
                        "slot = sideways\nanchor = diagonal\nessential = yes\nweight = -2\n",
                    ),
                ],
            )],
        );
        let defs = scan(&roots);
        let left = defs.iter().find(|d| d.name == "left").unwrap();
        assert_eq!(left.slot, PanelSlot::Left);
        assert_eq!(left.order, 3.0);
        assert_eq!(left.weight, Some(26.0));
        assert_eq!(left.anchor, PanelAnchor::Flow, "no anchor named");
        let pinned = defs.iter().find(|d| d.name == "pinned").unwrap();
        assert_eq!(pinned.slot, PanelSlot::Center);
        assert_eq!(pinned.anchor, PanelAnchor::Top);
        assert!(pinned.essential);
        // Words this version has never heard of ask for nothing, which
        // is what an addon written for a later one must degrade to.
        let junk = defs.iter().find(|d| d.name == "junk").unwrap();
        assert_eq!(junk.slot, PanelSlot::Auto);
        assert_eq!(junk.anchor, PanelAnchor::Flow);
        assert!(!junk.essential, "only the word that means yes turns it on");
        assert_eq!(junk.weight, None, "a negative share is no share at all");
        let _ = std::fs::remove_dir_all(base);
    }

    /// An addon that declares nothing — no pragmas, no `.meta` file,
    /// or a file full of nonsense — is still an addon.
    #[test]
    fn an_addon_without_metadata_degrades_instead_of_disappearing() {
        let (base, roots) = tree(
            "bare",
            &[(
                "user",
                &[
                    ("scripts/meter.rhai", "fn draw() { [] }"),
                    ("plugins/gauge.so", ""),
                    ("plugins/dial.so", ""),
                    (
                        "plugins/dial.meta",
                        "ref_h = wide\nmin_h = -3\nlabel =\ncategory = orbit\nnonsense\n",
                    ),
                ],
            )],
        );
        let defs = scan(&roots);
        let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["dial", "gauge", "meter"], "sorted");
        for d in &defs {
            assert_eq!(d.label, d.name.to_uppercase());
            assert_eq!((d.ref_h_vh, d.min_h_vh), (DEFAULT_REF_H_VH, DEFAULT_MIN_H_VH));
            assert!(matches!(d.category, WidgetCategory::Board));
            assert_eq!(d.slot, PanelSlot::Auto);
            assert_eq!(d.anchor, PanelAnchor::Flow);
            assert_eq!(d.weight, None);
            assert!(!d.essential);
        }
        let _ = std::fs::remove_dir_all(base);
    }

    /// Nothing installed = nothing offered. The program says so
    /// elsewhere; here it simply must not invent a widget.
    #[test]
    fn an_empty_tree_is_an_empty_registry() {
        let (base, roots) = tree("empty", &[("user", &[])]);
        assert!(scan(&roots).is_empty());
        let missing = AssetRoots::new(vec![base.join("nowhere")], base.join("nowhere"));
        assert!(scan(&missing).is_empty());
        let _ = std::fs::remove_dir_all(base);
    }
}
