//! A theme the person did not write into their own directory — one dropped
//! into `~/.config` by hand, or installed system-wide — is COPIED there by
//! its first save, with everything in it.
//!
//! Its own process because it steers `HOME` and `XDG_DATA_HOME`, which
//! `theme::save_theme` and the loader's search walk both read. It never
//! loads a theme, so the engine stays untouched; the claim is entirely
//! about bytes.
//!
//! Without this branch a save would answer "no file at the write target" by
//! generating one, and a theme the person had been running for weeks would
//! come back holding only the dozen values the editor knows about. It is the
//! same materialisation `layout/store.rs` has done for `.layaut` files since
//! it was written ("Editing a layout that came from a system directory
//! copies it into the user's on the first save").

use nacelle::theme::{self, edit::Edit};

const INSTALLED: &str = r#"# Przyniesiony z zewnatrz. Nikt tego pliku nie pisal edytorem.
[glow.panel_edge]
enabled = false
radius  = 2.40u                       # the author's halo
alpha   = 0.500

[palette]
accent = oklch(0.6000, 0.1000, 300.00)
"#;

#[test]
fn a_theme_found_outside_the_user_directory_is_copied_there_whole() {
    let scratch =
        std::env::temp_dir().join(format!("nacelle-theme-mat-{}", std::process::id()));
    let home = scratch.join("home");
    let data = scratch.join("data");
    let brought = home.join(".config/nacelle-desktop/themes");
    std::fs::create_dir_all(&brought).unwrap();
    std::fs::create_dir_all(&data).unwrap();
    // SAFETY: one test in its own process, so nothing races the environment.
    unsafe {
        std::env::set_var("HOME", &home);
        std::env::set_var("XDG_DATA_HOME", &data);
        std::env::remove_var("NACELLE_THEME_DIR");
        std::env::remove_var("NACELLE_THEME_LOCAL");
    }
    let source = brought.join("przyniesiony.theme");
    std::fs::write(&source, INSTALLED).unwrap();

    let edits = [Edit { token: "glow.panel_edge.enabled", value: "true".to_string() }];
    let saved = theme::save_theme("przyniesiony", &edits).expect("the save refused");

    // The FAMILY folder, not the program's own: the search path learned
    // `nacelle/themes` the same day this test was written, and a save
    // materialises where the program is heading, not where it has been.
    // The old folder is still read — that is the migration contract — so
    // the theme this test brings in is found there and lands here.
    assert_eq!(
        saved,
        data.join("nacelle/themes/przyniesiony.theme"),
        "a save landed outside the user's own theme directory"
    );
    let after = std::fs::read_to_string(&saved).unwrap();
    assert!(
        after.contains("radius  = 2.40u")
            && after.contains("# Przyniesiony z zewnatrz. Nikt tego pliku nie pisal edytorem.")
            && after.contains("accent = oklch(0.6000, 0.1000, 300.00)"),
        "the copy kept only what the editor knows about:\n{after}"
    );
    assert!(after.contains("enabled = true"), "the one edit never landed:\n{after}");

    // And the file it was copied FROM is left exactly as it was: a save is
    // not a licence to write into a directory the person did not name.
    assert_eq!(
        std::fs::read_to_string(&source).unwrap(),
        INSTALLED,
        "the save wrote back into the directory it read the theme from"
    );

    let _ = std::fs::remove_dir_all(&scratch);
}
