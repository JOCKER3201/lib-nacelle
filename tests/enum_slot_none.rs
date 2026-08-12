//! `none` in a slot whose vocabulary is words.
//!
//! §5.0's sentinel table says what `none` means where a LENGTH was
//! expected — `list.rule = none` draws no rule — and it has nothing to
//! say about a slot whose values are words. The bake folded every
//! sentinel word to its `f32` regardless, so on an enum slot the one
//! word a theme could never deliver was `none`: it landed in the scalar
//! array, the enum index stayed at zero, and the consumer read back the
//! MASTER's own literal as though the theme had asked for it.
//!
//! `winframe.button.order` is where that bites: the master documents
//! `none` as the way to drop a window control, and it was the only word
//! in the vocabulary that could not arrive. Every other word — even a
//! made-up one — came through.
//!
//! ONE test function, for the reason `mood_engine` gives: the resolved
//! theme is process-wide, so a test that swaps it must not run beside a
//! test that reads it.

use nacelle::theme::{self, LoadRequest};

/// Loads a fixture theme whose base is the master.
fn skin(tag: &str, body: &str) {
    let path =
        std::env::temp_dir().join(format!("nacelle-slot-{tag}-{}.theme", std::process::id()));
    std::fs::write(
        &path,
        format!("[meta]\nschema = 1\nname = \"{tag}\"\nbase = \"default\"\n\n{body}"),
    )
    .expect("the fixture theme must be writable");
    let _ = theme::load_with(LoadRequest { path: Some(path.clone()), ..Default::default() });
    let _ = std::fs::remove_file(&path);
}

/// The word a slot stands at, the way every consumer of an ordered row
/// asks it.
fn slot(i: usize) -> String {
    let id = theme::id(&format!("winframe.button.order[{i}]"))
        .expect("the master declares three slots");
    theme::enum_word_of(id).unwrap_or_default()
}

#[test]
fn a_slot_reading_none_says_none() {
    // The master's own row, so the reading below is a change and not a
    // starting position.
    let _ = theme::load_with(LoadRequest::default());
    assert_eq!(slot(1), "maximise", "the master's middle control");

    // Any other word arrives — this is the control for the experiment,
    // and it is what made the defect invisible: the mechanism looked
    // fine from every direction except the one word that mattered.
    skin("word", "[winframe]\nbutton.order = [close, dropped, minimise]\n");
    assert_eq!(slot(1), "dropped");
    assert_eq!(slot(0), "close", "the neighbouring slots move too");

    // And so does `none`, which is the word the master documents for
    // dropping a control.
    skin("none", "[winframe]\nbutton.order = [close, none, minimise]\n");
    assert_eq!(
        slot(1),
        "none",
        "the slot answered the MASTER's own word: `none` was eaten as a length sentinel"
    );

    // A LENGTH token is untouched by the same change: `none` there is
    // still §5.0's zero, which is the whole reason the table exists.
    skin("len", "[list]\nrule = none\n");
    let rule = theme::id("list.rule").expect("the master declares list.rule");
    assert_eq!(theme::resolved().px(rule), 0.0, "`none` on a length is still zero");

    let _ = theme::load_with(LoadRequest::default());
}
