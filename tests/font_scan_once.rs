//! The font directories are read ONCE, and every face slot is answered
//! from what that one reading found.
//!
//! This is a counting test, not a timing test: it asks the loader's own
//! meter how many traversals, directory opens, stats and file parses a
//! theme load spent, so the answer is a number a system trace of the
//! running program shows the same way. Timing would measure this machine's
//! disk cache; the defect being guarded here is arithmetical — the search
//! was run once per (face x family x weight-spelling), and eight slots
//! with four families each is fifty-odd traversals of the same tree for
//! the same answer.
//!
//! One test function, and its own file, on purpose: the meter is
//! process-wide, so "what did the FIRST load cost" is only answerable in a
//! process where nothing else has loaded a face beside it. Cargo gives
//! every integration test file its own binary; a file with one test has
//! nothing to interleave with.

use nacelle::font::{self, FaceChoice, FontSystem, ScanCount, FONT_COUNT};
use nacelle::theme::{self, LoadRequest};

fn since(before: ScanCount) -> ScanCount {
    let now = font::scan_count();
    ScanCount {
        walks: now.walks - before.walks,
        dirs: now.dirs - before.dirs,
        stats: now.stats - before.stats,
        parses: now.parses - before.parses,
    }
}

/// The DISK-READING half of a measurement. A reload parses files again by
/// design — a slot holds a parsed table and the slots are being rebuilt —
/// but it has no business reading the directories again, and these are the
/// three numbers that say whether it did.
fn scan_only(c: ScanCount) -> ScanCount {
    ScanCount { parses: 0, ..c }
}

/// The six face slots the master gives a family to. `icon` and `reserved`
/// are the two it does not: they alias onto a slot that has one, which is
/// why "six slots, one file" is the whole of the parse count below.
const FAMILIED_SLOTS: [&str; 6] = ["ui", "mono", "ui_medium", "ui_bold", "display", "mono_bold"];

/// A theme file that exists for as long as the test needs it and no
/// longer.
///
/// A `Drop` guard rather than a line at the end of the test, because the
/// interesting runs are the ones that end in a failed assertion — a test
/// that only tidies up when it passes leaves its litter exactly on the
/// days somebody is running it over and over.
struct Fixture(std::path::PathBuf);

impl Fixture {
    fn write(name: &str, text: &str) -> Fixture {
        // Cargo's own scratch directory for integration tests, under
        // `target/`, and not the system's `/tmp`: a test writes into the
        // build it belongs to, so `cargo clean` takes anything this guard
        // somehow missed with it.
        let path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
        std::fs::write(&path, text).expect("the fixture theme must be writable");
        Fixture(path)
    }

    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[test]
fn one_traversal_per_load_not_one_per_face() {
    // ------------------------------------------------ the first load
    let mark = font::scan_count();
    let mut fonts = FontSystem::new();
    let first = since(mark);

    // Fail closed. A machine whose font directories are all missing would
    // pass every assertion below on zero work and prove nothing, so the
    // measurement itself has to show it measured something.
    assert!(
        first.dirs > 0 && first.parses > 0,
        "the first face load read no directory and parsed no file at all \
         ({first:?}) — this machine has nothing for the test to measure, \
         and a green tick here would be a green tick on no evidence"
    );

    assert_eq!(
        first.walks, 1,
        "loading the theme's {FONT_COUNT} face slots traversed the font \
         directory list {} times ({first:?}). It is ONE tree, read for ONE \
         list of file names; every slot after the first is a question about \
         that list, not a reason to read it again",
        first.walks
    );

    // ------------------------------------- and every load after it
    //
    // A theme swap re-resolves the slots (the families and weights are the
    // theme's own words), and the user changing font family or weight in
    // the settings does the same. Neither installs a font, so neither is a
    // reason to go back to the disk. This is the half the running program
    // felt: the owner opened a settings page and the main thread went to
    // the filesystem for a third of a second.
    let mark = font::scan_count();
    fonts.reload_faces(&FaceChoice::default());
    let again = since(mark);
    assert_eq!(
        scan_only(again),
        ScanCount::default(),
        "a second face load went back to the disk ({again:?}) — the index \
         is being rebuilt per load, so a theme swap still costs a full walk \
         of every font directory on the main thread"
    );

    // The user's own settings folded in: a different QUESTION, still not a
    // reason to re-read the directories.
    let mark = font::scan_count();
    fonts.reload_faces(&FaceChoice {
        ui_family: Some("Rajdhani".into()),
        ui_weight: Some("Bold".into()),
        mono_family: Some("Fira Mono".into()),
        mono_weight: Some("Medium".into()),
    });
    let chosen = since(mark);
    assert_eq!(
        scan_only(chosen),
        ScanCount::default(),
        "folding the user's family and weight into the load re-read the \
         directories ({chosen:?}) — picking a font in the settings must \
         cost a lookup, not a scan"
    );

    // ------------------------------------- the settings page's own lists
    //
    // The FONT page asks which families this machine has, once per curated
    // name — nineteen names across the two tables, and the interface list
    // appends the monospace one, so thirty-one questions. Thirty-one full
    // traversals of /usr/share/fonts is what the owner saw as a stutter
    // the moment that page opened.
    //
    // ONE traversal per call, and not zero. This is the single question in
    // the whole loader that has to see the disk again: it is a list of what
    // is INSTALLED, shown to the person who may have installed something a
    // minute ago, and answering it out of a reading taken at startup would
    // hide the font they came to pick. Two rather than one for the page,
    // because the host asks two questions and nothing in the API says they
    // are one moment.
    let mark = font::scan_count();
    let mono = font::available_mono_families();
    let ui = font::available_ui_families();
    let listing = since(mark);
    assert_eq!(
        listing.walks, 2,
        "listing the available families traversed the font directories {} \
         times ({listing:?}) — one per curated NAME is the stutter the trace \
         caught, and zero would mean a font installed while the program runs \
         cannot be picked until it restarts",
        listing.walks
    );
    assert!(
        !mono.is_empty() || !ui.is_empty(),
        "no family from either curated table was found on a machine whose \
         font directories were just read — the index is answering nothing, \
         which would make every assertion above vacuous"
    );

    // ------------------------------------- one parse per FILE
    //
    // The other half of the startup cost, and the one that is not
    // syscalls. §5.16 ends every fallback chain at the interface or the
    // monospace slot, so on a machine missing a display family the eight
    // slots land on two or three files between them — and a file resolved
    // by four slots used to be decoded four times.
    //
    // Measured with a theme that puts the SAME family first in every slot
    // that has a family at all, because that turns "one parse per file"
    // into a number the test can name without knowing which files this
    // machine has: six slots, one file, one parse. (`icon` and `reserved`
    // are the two the master gives no family, so they alias onto a slot
    // that has one and parse nothing — which is the same arithmetic seen
    // from the other side.)
    //
    // `family[0] = ...` and not `family = [...]`: a family list is an
    // indexed family in the cascade, and a row of a different length than
    // the master's is ignored whole, with a warning saying exactly this.
    let family = mono.first().or_else(|| ui.first()).expect("a family to build the fixture on");
    let mut fixture = String::from(
        "[meta]\nschema = 1\nname = \"One file for six slots\"\nbase = \"default\"\n\n",
    );
    for id in FAMILIED_SLOTS {
        fixture.push_str(&format!(
            "[face.{id}]\nfamily[0] = \"{family}\"\nweight = 400\n\n"
        ));
    }
    let fixture_file =
        Fixture::write(&format!("nacelle-font-scan-{}.theme", std::process::id()), &fixture);
    let _ = theme::load_with(LoadRequest {
        path: Some(fixture_file.path().to_path_buf()),
        ..Default::default()
    });

    // The fixture has to be IN FORCE for the count below to mean what its
    // name says. `theme::load_with` always succeeds — a theme it cannot
    // read degrades to the master and says so in the diagnostics rather
    // than refusing — so a fixture with a typo in it, or written against a
    // syntax this file no longer speaks, would leave the master's own
    // families in the slots and the assertion would go on passing while
    // measuring a different theme. Six slots naming ONE family is the
    // premise; this is where it is checked instead of assumed.
    let live = theme::diagnostics();
    for id in FAMILIED_SLOTS {
        let token = format!("face.{id}.family[0]");
        assert_eq!(
            live.text(&token),
            Some(family.as_str()),
            "the fixture theme did not take: {token} reads {:?} and not \
             {family:?}, so the parse count below is a count for whatever \
             families the master happens to name — not for six slots on one \
             file",
            live.text(&token)
        );
    }

    let mark = font::scan_count();
    fonts.reload_faces(&FaceChoice::default());
    let one_file = since(mark);
    assert_eq!(
        one_file.parses, 1,
        "a theme naming one family in every slot that has one parsed {} \
         files ({one_file:?}) — the slots resolve onto ONE file and a \
         parsed face is being built once per slot that wants it",
        one_file.parses
    );
    assert_eq!(
        scan_only(one_file),
        ScanCount::default(),
        "the fixture theme sent the loader back to the directories \
         ({one_file:?}) — a theme swap must not rescan"
    );
}
