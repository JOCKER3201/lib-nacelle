//! ②③: a SAVE writes ONE complete arrangement, shared by every screen.
//!
//! The grid editor's SAVE used to fold the user's changes into the
//! per-screen `[WxH@D]` section of the monitor he happened to be on and
//! leave every other screen with whatever it already had — the "half of
//! one arrangement, half of another" a user hits when he drags the
//! program across two monitors. A SAVE now rewrites the whole layout as
//! the shared base and keeps NO per-screen section, so each screen reads
//! the same placements, scaled to its own pixels.

use nacelle::assets::AssetRoots;
use nacelle::base::{self, LayoutMode, WidgetDef};
use nacelle::layout::{layaut, LayautStore};
use nacelle::widget::registry;
use std::path::PathBuf;

fn setup() {
    base::set_registry(
        ["clock", "cpu", "memory", "shell"]
            .iter()
            .map(|name| {
                let mut d: WidgetDef = registry::bare_def((*name).to_string());
                d.ref_h_vh = 10.0;
                d.min_h_vh = 6.0;
                d
            })
            .collect(),
    );
}

fn roots(tag: &str) -> (AssetRoots, PathBuf) {
    let dir = std::env::temp_dir().join(format!("nacelle-layaut-universal-{tag}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("layauts")).expect("scratch root");
    (AssetRoots::new(vec![dir.clone()], dir.clone()), dir)
}

/// A file with a base AND a per-screen section — the exact shape a SAVE
/// on a second monitor used to leave behind.
const SPLIT: &str = "\
# nacelle layout.
version = 2
next_instance = 5
screen = 2560x1440@32
clock = 2 2 16 7
cpu = 20 2 16 7

[1920x1080@27]
clock = 50 50 16 7
";

#[test]
fn save_writes_one_arrangement_and_drops_every_section() {
    setup();
    let (roots, dir) = roots("drops-sections");
    let path = dir.join("layauts").join("split.layaut");
    std::fs::write(&path, SPLIT).unwrap();
    let store = LayautStore::new(roots);

    let mut def = store.load("split").expect("it loads");
    assert_eq!(def.overrides.len(), 1, "the fixture starts with one section");

    // SAVE — done on a THIRD screen, the one the old code would have
    // opened yet another section for.
    store.save_full("split", &mut def, (3840, 2160, 32)).unwrap();

    let reread = layaut::parse(&std::fs::read_to_string(&path).unwrap(), "split");
    assert!(
        reread.overrides.is_empty(),
        "SAVE keeps no per-screen section, still has {:?}",
        reread.overrides.iter().map(|o| (o.w, o.h, o.diag)).collect::<Vec<_>>()
    );
    assert!(
        !matches!(reread.base, LayoutMode::Flex),
        "the whole arrangement is written as the shared base"
    );
}

#[test]
fn every_screen_reads_the_same_base() {
    setup();
    let (roots, dir) = roots("same-base");
    let path = dir.join("layauts").join("split.layaut");
    std::fs::write(&path, SPLIT).unwrap();
    let store = LayautStore::new(roots);

    let mut def = store.load("split").unwrap();
    store.save_full("split", &mut def, (2560, 1440, 32)).unwrap();

    // A screen that never had a section of its own — clock lands where
    // the base put it (2% in, 2% down), not on the built-in fallback and
    // not parked off-screen. A 16:9 window so the landscape edge-adapt is
    // the identity and the percentages read straight through.
    let world = nacelle::stage::BoardWorld::new(store.load("split").unwrap());
    let t = base::size_table();
    let lay = world.solve((0, 0), 1600.0, 900.0, 8.0, (1600, 900, 24), &t);
    let clk = lay.p(base::Panel::from_name("clock").unwrap());
    assert!((clk.x - 32.0).abs() < 0.5, "clock x = 2% of 1600 = 32, got {}", clk.x);
    assert!((clk.y - 18.0).abs() < 0.5, "clock y = 2% of 900 = 18, got {}", clk.y);
}
