//! An index a consumer CACHES must name the same word under every theme.
//!
//! `motion.rs`'s header and `view/scroll.rs:176` both state the hazard:
//! "an index only names a word against the schema it was interned in",
//! and `theme::load_with` builds the schema afresh. A consumer that holds
//! an index in a process-lifetime `OnceLock` is therefore only correct if
//! the token's vocabulary is DECLARED — `parse.rs::declared_enum_words`
//! only interns a list when the key's own line spells `enum: a | b | c`,
//! and without one the list is DISCOVERED from whatever values a cascade
//! happens to intern.
//!
//! A discovered list fails two ways, and both were live:
//!
//! * A word the master itself never writes is absent until some theme
//!   uses it, so `enum_index` answers `None` — and a `OnceLock` freezes
//!   that `None` for the life of the process. `script.text_align` could
//!   never answer anything but `left`, and `term.inverse_mode = tint`
//!   never washed a cell.
//! * Worse, the index is REUSED. Under a theme writing `center` the word
//!   `center` interned at 1; under a theme writing `right`, `right`
//!   interned at 1 and `center` was gone. A cache holding 1 for `center`
//!   then matched a theme that had asked for `right`.
//!
//! So this file asserts the property the caches actually depend on, for
//! every token whose consumer holds an index: each word keeps ONE index
//! across every theme that writes any word of the set, and `enum_of`
//! tracks the theme. Declaring the vocabulary is what makes that true.
//!
//! ONE test function, for the reason `mood_engine` gives: the resolved
//! theme is process-wide, so a test that swaps it must not run beside a
//! test that reads it.

use nacelle::theme::{self, LoadRequest};

/// Loads a fixture theme whose base is the master, so every token but the
/// one in `body` is the master's own.
fn skin(section: &str, key: &str, word: &str) {
    let path = std::env::temp_dir()
        .join(format!("nacelle-vocab-{section}-{key}-{word}-{}.theme", std::process::id()));
    std::fs::write(
        &path,
        format!(
            "[meta]\nschema = 1\nname = \"vocab\"\nbase = \"default\"\n\n[{section}]\n{key} = {word}\n"
        ),
    )
    .expect("the fixture theme must be writable");
    let _ = theme::load_with(LoadRequest { path: Some(path.clone()), ..Default::default() });
    let _ = std::fs::remove_file(&path);
}

/// The tokens whose consumers cache an enum index in a `static` that
/// outlives every theme swap. `section`/`key` is how a theme writes it;
/// `token` is how the consumer asks for it.
const CACHED: &[(&str, &str, &str, &[&str])] = &[
    // Fixed here: the master wrote no vocabulary, and script.rs caches
    // BOTH `center` and `right` (`theme_text_align`).
    ("script", "text_align", "script.text_align", &["left", "center", "right"]),
    // Fixed here: term.rs caches `tint` (`FLAG_INVERSE`), a word the
    // master does not write.
    ("term", "inverse_mode", "term.inverse_mode", &["swap", "tint"]),
    // Fixed here: menu.rs caches `grow` (`act_on`). This one was already
    // safe by accident — `grow` IS the master's own word, so a discovered
    // list put it at 0 on every build — but nothing at the call site said
    // so, and the accident is one master edit away from ending.
    ("a11y", "hit_pad_mode", "a11y.hit_pad_mode", &["grow", "none"]),
    // Already declared before this file existed: the controls, which is
    // why they never carried the defect.
    ("scrollbar", "mode", "scrollbar.mode", &["overlay", "inset", "none"]),
    ("scrollbar", "edge", "scrollbar.edge", &["left", "right"]),
    ("checkbox", "tick_shape", "checkbox.tick_shape", &["square", "check", "cross"]),
    ("field", "caret_style", "field.caret_style", &["bar", "block", "underline"]),
    ("field", "mask_glyph", "field.mask_glyph", &["bullet", "asterisk", "block"]),
    ("menu", "anchor_width", "menu.anchor_width", &["anchor", "min_w"]),
];

#[test]
fn a_cached_enum_index_names_the_same_word_under_every_theme() {
    let _ = theme::load_with(LoadRequest::default());

    // The indices as the master hands them out — what a `OnceLock` filled
    // on the first frame of a normal run would hold.
    let baseline: Vec<Vec<Option<u16>>> = CACHED
        .iter()
        .map(|(_, _, token, words)| {
            let id = theme::id(token).unwrap_or_else(|| panic!("the master declares {token}"));
            words.iter().map(|w| theme::enum_index(id, w)).collect()
        })
        .collect();

    for ((section, key, token, words), first) in CACHED.iter().zip(&baseline) {
        let id = theme::id(token).unwrap();
        for (w, i) in words.iter().zip(first) {
            assert!(
                i.is_some(),
                "{token}: `{w}` has no index under the master, so a consumer caching it \
                 freezes `None` for the life of the process — the vocabulary is discovered, \
                 not declared (`enum:` missing from the key's own line in default.theme)"
            );
        }

        // Every word, written by a theme in turn: the numbering must not
        // move, and the resolved index must be that word's.
        for (w, want) in words.iter().zip(first) {
            skin(section, key, w);
            let now: Vec<Option<u16>> = words.iter().map(|x| theme::enum_index(id, x)).collect();
            assert_eq!(
                &now, first,
                "{token}: writing `{w}` renumbered the vocabulary. A cached index now names \
                 a different word than the one it was resolved for"
            );
            assert_eq!(
                Some(theme::resolved().enum_of(id)),
                *want,
                "{token}: the theme wrote `{w}`, so `enum_of` must stand at that word's index"
            );
        }
    }

    // The shipped picture again, for anything reading the theme after
    // this file.
    let _ = theme::load_with(LoadRequest::default());
}
