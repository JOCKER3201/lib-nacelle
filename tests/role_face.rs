//! `type.<role>.face` — which FAMILY and which WEIGHT a role is set in —
//! read from the role rather than chosen at the call site.
//!
//! The master declares `face` for all twenty-four roles and eight
//! `[face.*]` blocks for them to name, each with its own family list and
//! its own weight. Two things used to stand between that and the screen:
//! nothing in the toolkit read a role's `face` at all — every text call
//! named `FONT_UI` or `FONT_MONO` by hand — and the atlas had two slots,
//! so even a reader could only have answered "monospace or not".
//!
//! Both are gone. `Role::font` answers one of the master's EIGHT slots by
//! word, and the loader resolves each slot's own family and weight down
//! §5.16's ladder. This file is where that is measured rather than
//! described: five distinct `face` words in the master have to arrive as
//! five distinct answers, or the owner's "the same as the font family and
//! weight" is still one promise short.
//!
//! ONE test function, on purpose: the resolved theme is process-wide, so
//! a test that switches it must not run beside a test that reads it.

use nacelle::draw::DrawList;
use nacelle::font::{
    FACE_DISPLAY, FACE_IDS, FACE_UI_BOLD, FACE_UI_MEDIUM, FontSystem, FONT_COUNT, FONT_MONO,
    FONT_UI,
};
use nacelle::theme::{self, Color, LoadRequest};
use nacelle::ui;

const WHITE: Color = Color { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };

/// Runs one question on a thread of its own: the toolkit memoises a
/// resolved role per thread, and the WORD an enum token stands at per
/// (token, index), so asking twice on one thread answers the first
/// fixture's face for the second fixture's question.
fn fresh<T: Send>(f: impl FnOnce() -> T + Send) -> T {
    std::thread::scope(|s| s.spawn(f).join().expect("the measuring thread panicked"))
}

fn face_of(role: &str) -> u8 {
    let owned = role.to_string();
    fresh(move || ui::role(&owned).font())
}

fn apply(fixture: Option<&str>) {
    match fixture {
        None => {
            let _ = theme::load();
        }
        Some(text) => {
            let path = std::env::temp_dir()
                .join(format!("nacelle-role-face-{}.theme", std::process::id()));
            std::fs::write(&path, text).expect("the fixture theme must be writable");
            let _ = theme::load_with(LoadRequest { path: Some(path), ..Default::default() });
        }
    }
}

const HEAD: &str = "[meta]\nschema = 1\nname = \"Face fixture\"\nbase = \"default\"\n\n";

#[test]
fn a_role_is_set_in_the_face_its_own_token_names() {
    apply(None);

    // The master's own split, which is the first reason the token is
    // there: the instrument roles are monospace and running text is not.
    assert_eq!(face_of("data"), FONT_MONO, "type.data.face = mono");
    assert_eq!(face_of("data.dump"), FONT_MONO, "type.data.dump.face = mono");
    assert_eq!(face_of("terminal"), FONT_MONO, "type.terminal.face = mono");
    assert_eq!(face_of("body"), FONT_UI);
    assert_eq!(face_of("caption"), FONT_UI);

    // ...and the second reason, which two slots could not carry: a face is
    // a family AND a weight, and the master states four of them. `value`
    // and `title.panel` are `ui_medium` (500) and the clock is `display`
    // (600) — three distinct slots where there used to be one, which is
    // the difference between honouring the master and rounding it down to
    // Regular.
    assert_eq!(face_of("value"), FACE_UI_MEDIUM, "type.value.face = ui_medium");
    assert_eq!(face_of("title.panel"), FACE_UI_MEDIUM, "type.title.panel.face = ui_medium");
    assert_eq!(face_of("display.clock"), FACE_DISPLAY, "type.display.clock.face = display");
    // The claim stated as the count, so it cannot pass vacuously: the
    // roles above must land on more than the two slots there used to be.
    let mut slots: Vec<u8> =
        ["data", "body", "value", "display.clock"].iter().map(|r| face_of(r)).collect();
    slots.sort_unstable();
    slots.dedup();
    assert!(slots.len() >= 3, "the master's faces collapsed onto {slots:?}");

    // Every slot the master names is a slot the atlas has.
    assert_eq!(FACE_IDS.len(), FONT_COUNT as usize);
    for (i, id) in FACE_IDS.iter().enumerate() {
        assert_eq!(nacelle::font::face_slot(id), i as u8, "face.{id}");
    }

    // ---- and the WEIGHT reaches the screen ---------------------------
    //
    // The slot exists; that is not the same as the weight arriving. The
    // master asks `face.ui` for 400 and `face.ui_bold` for 700, and
    // §5.16's ladder says the second is either a heavier FILE or the same
    // file drawn twice with an offset — never Regular in silence, which is
    // what the two-slot engine gave it. Either answer changes the ink, so
    // the ink is what is compared: a machine with a Bold file proves it
    // one way and a machine without proves it the other.
    fresh(|| {
        let mut fonts = FontSystem::new();
        let ink = |fs: &mut FontSystem, face: u8| {
            let mut dl = DrawList::new();
            dl.text(fs, face, 32.0, 0.0, 0.0, "8", WHITE, 0.0);
            (dl.verts.len(), fs.synthetic_bold(face))
        };
        let plain = ink(&mut fonts, FONT_UI);
        let bold = ink(&mut fonts, FACE_UI_BOLD);
        assert!(
            bold != plain || fonts.synthetic_bold(FACE_UI_BOLD) > 0.0,
            "face.ui_bold asks for 700 and drew exactly what face.ui drew at 400: \
             the master's weight is not reaching the atlas"
        );
    });

    // A role the master does not declare has no face to name, and the
    // interface slot is where an undesigned run has always landed.
    assert_eq!(face_of("no_such_role"), FONT_UI);

    // ---- and the token is what decides, not the role's name ----------
    apply(Some(&format!(
        "{HEAD}[type]\nbody.face = mono\ndata.face = ui\nvalue.face = display\n"
    )));
    assert_eq!(
        face_of("body"),
        FONT_MONO,
        "a theme moved `type.body.face` to mono and the toolkit kept the \
         interface face — the family is still being chosen at the call site"
    );
    assert_eq!(
        face_of("data"),
        FONT_UI,
        "a theme moved `type.data.face` off mono and nothing followed"
    );
    assert_eq!(
        face_of("value"),
        FACE_DISPLAY,
        "a theme moved a role onto a face that is neither `ui` nor `mono`, \
         and it landed on one of the two slots the atlas used to have"
    );

    // ---- a face named by REFERENCE is the same face -------------------
    // `spare0.face = @type.value.face` has to answer `value`'s slot. It
    // did not until the master declared the eight face ids as the enum's
    // words: an enum assigned by reference carries an INDEX, and an index
    // means nothing unless both tokens number their words alike.
    apply(Some(&format!("{HEAD}[type]\nspare0.face = @type.value.face\n")));
    assert_eq!(
        face_of("spare0"),
        face_of("value"),
        "one role pointed at another's face and got a different family"
    );

    // ---- and a theme swap re-resolves the slots themselves ------------
    //
    // `type.<role>.face` names a slot; `face.<id>.family` says what is IN
    // that slot, and a theme may replace it. So the eight slots have to be
    // resolved again when the theme changes, at a frame boundary — which
    // is what `begin_frame` is for, and where the atlas reset already
    // waits for the same reason. The atlas going dirty is the witness: the
    // faces were reloaded, so every cached glyph was thrown away.
    let mut fonts = FontSystem::new();
    let mut dl = DrawList::new();
    dl.text(&mut fonts, FONT_UI, 24.0, 0.0, 0.0, "A", WHITE, 0.0);
    let _ = fonts.take_dirty_rows();
    fonts.begin_frame();
    assert!(
        fonts.take_dirty_rows().is_none(),
        "nothing changed and the atlas was thrown away anyway"
    );
    apply(Some(&format!("{HEAD}[face.ui]\nweight = 700\n")));
    fonts.begin_frame();
    assert!(
        fonts.take_dirty_rows().is_some(),
        "a theme moved `face.ui.weight` and the atlas kept the old face: a \
         family and a weight the theme states are still not reaching it"
    );

    apply(None);
}
