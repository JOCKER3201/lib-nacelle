//! The migration from version 1 of the `.layaut` format to version 2.
//!
//! The two fixtures under `tests/fixtures/` were not written by hand:
//! `legacy_v1.layaut` was produced by the pre-instance code's own
//! serializer, and `legacy_v1_rects.txt` holds the rectangles the
//! pre-instance ENGINE solved that file to, at three window sizes, on
//! home and on the extra board. Together they are the contract this
//! change had to keep — a user's saved layouts are the one thing in the
//! program he cannot make again from memory, so "it still loads" is not
//! enough: it has to come out pixel for pixel where it went in.

use nacelle::assets::AssetRoots;
use nacelle::base::{self, WidgetDef};
use nacelle::layout::store::BACKUP_SUFFIX;
use nacelle::layout::{layaut, LayautStore};
use nacelle::widget::registry;
use std::path::PathBuf;

const LEGACY: &str = include_str!("fixtures/legacy_v1.layaut");
const RECTS: &str = include_str!("fixtures/legacy_v1_rects.txt");

/// The registry the fixture was generated against — same names, same
/// order, same heights. Panel indices bake into everything, so this is
/// the one registry this binary ever has.
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

/// A private data root for one test, under the scratch directory the
/// harness gives us.
fn roots(tag: &str) -> (AssetRoots, PathBuf) {
    let dir = std::env::temp_dir().join(format!("nacelle-layaut-migration-{tag}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("layauts")).expect("scratch root");
    (AssetRoots::new(vec![dir.clone()], dir.clone()), dir)
}

/// One frozen expectation: window size, board, widget, rectangle.
struct Expect {
    w: f32,
    h: f32,
    board: String,
    widget: String,
    r: [f32; 4],
}

fn expectations() -> Vec<Expect> {
    RECTS
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            Expect {
                w: f[0].parse().unwrap(),
                h: f[1].parse().unwrap(),
                board: f[2].to_string(),
                widget: f[3].to_string(),
                r: [
                    f[4].parse().unwrap(),
                    f[5].parse().unwrap(),
                    f[6].parse().unwrap(),
                    f[7].parse().unwrap(),
                ],
            }
        })
        .collect()
}

#[test]
fn a_real_version_1_file_migrates_and_lays_out_identically() {
    setup();
    let (roots, dir) = roots("identical");
    let path = dir.join("layauts").join("legacy.layaut");
    std::fs::write(&path, LEGACY).unwrap();
    let store = LayautStore::new(roots);

    // Loading is what migrates: the user opens his layout and it is
    // simply there, in the current grammar, with the old bytes kept.
    let def = store.load("legacy").expect("the layout loads");
    let after = std::fs::read_to_string(&path).unwrap();
    assert!(!layaut::is_legacy(&after), "the file on disk is version 2 now");
    assert_eq!(layaut::version_of(&after), layaut::FORMAT_VERSION);
    let backup = dir.join("layauts").join(format!("legacy{BACKUP_SUFFIX}"));
    assert_eq!(
        std::fs::read_to_string(&backup).unwrap(),
        LEGACY,
        "the original bytes are kept, unchanged"
    );

    // One instance per placement the file named, exactly as before:
    // four on home, and the memory the extra board also carried — the
    // one case where version 1 did name a widget twice, in two places.
    let named = |k: (i32, i32)| -> Vec<&'static str> {
        def.board_instances(k).into_iter().map(|i| i.widget.name()).collect()
    };
    assert_eq!(named((0, 0)), ["clock", "cpu", "memory", "shell"]);
    assert_eq!(named((1, 0)), ["memory"]);
    assert_eq!(def.instances.len(), 5);
    assert_eq!(def.base_screen, Some((1920, 1080, 27)));

    // And the layout is the SAME layout, at every size the fixture
    // recorded, on home and on the extra board.
    let world = nacelle::stage::BoardWorld::new(def);
    let t = base::size_table();
    for e in expectations() {
        let k = if e.board == "home" { (0, 0) } else { (1, 0) };
        let screen = match (e.w as u32, e.h as u32) {
            (1920, 1080) => (1920, 1080, 27),
            (2560, 1440) => (2560, 1440, 32),
            _ => (0, 0, 0),
        };
        let lay = world.solve(k, e.w, e.h, 8.0, screen, &t);
        let p = base::Panel::from_name(&e.widget).unwrap();
        let got = lay.p(p);
        if e.r[0] >= e.w {
            // A widget the board never held: parked outside, which is
            // the only thing that box ever meant. The two versions
            // parked it at two different sizes and always did — a
            // rectangle board used OFF_SPEC, a flex board its own
            // sentinel — so only "outside" is the contract.
            assert!(got.x >= e.w, "{} must stay off {} at {}x{}", e.widget, e.board, e.w, e.h);
            continue;
        }
        for (i, (g, wv)) in [got.x, got.y, got.w, got.h].iter().zip(e.r.iter()).enumerate() {
            assert!(
                (g - wv).abs() < 0.01,
                "{} on {} at {}x{}: field {i} is {g}, was {wv}",
                e.widget,
                e.board,
                e.w,
                e.h
            );
        }
    }
}

#[test]
fn migration_happens_once_and_keeps_the_first_backup() {
    setup();
    let (roots, dir) = roots("once");
    let path = dir.join("layauts").join("legacy.layaut");
    std::fs::write(&path, LEGACY).unwrap();
    let store = LayautStore::new(roots);

    assert!(store.migrate("legacy").unwrap(), "the first pass rewrites");
    let migrated = std::fs::read_to_string(&path).unwrap();
    assert!(!store.migrate("legacy").unwrap(), "the second pass has nothing to do");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), migrated, "and changes nothing");

    // A user who drops the old file back in gets it migrated again —
    // but the backup he already has is not overwritten by it.
    std::fs::write(&path, "clock = 0 0 5 5\n").unwrap();
    assert!(store.migrate("legacy").unwrap());
    let backup = dir.join("layauts").join(format!("legacy{BACKUP_SUFFIX}"));
    assert_eq!(std::fs::read_to_string(&backup).unwrap(), LEGACY);
}

#[test]
fn a_layaut_in_a_system_directory_is_never_rewritten() {
    setup();
    // Two roots: a system one that is only read, and the user's.
    let base_dir = std::env::temp_dir().join("nacelle-layaut-migration-system");
    let _ = std::fs::remove_dir_all(&base_dir);
    let sys = base_dir.join("sys");
    let user = base_dir.join("user");
    std::fs::create_dir_all(sys.join("layauts")).unwrap();
    std::fs::create_dir_all(user.join("layauts")).unwrap();
    let sys_file = sys.join("layauts").join("shipped.layaut");
    std::fs::write(&sys_file, LEGACY).unwrap();

    let store = LayautStore::new(AssetRoots::new(vec![user.clone(), sys.clone()], user));
    let first = store.load("shipped").expect("it loads");
    assert_eq!(
        std::fs::read_to_string(&sys_file).unwrap(),
        LEGACY,
        "a file we do not own stays as it is"
    );

    // Read as version 1 every time — and identically every time,
    // because the identities come from the file's own order.
    let second = store.load("shipped").expect("it loads again");
    let ids = |d: &nacelle::layout::LayoutDef| -> Vec<(u32, &'static str)> {
        d.instances.iter().map(|i| (i.id.get(), i.widget.name())).collect()
    };
    assert_eq!(ids(&first), ids(&second));
}
