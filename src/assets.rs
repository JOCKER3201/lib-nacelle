//! Where an embedder keeps toolkit data (u3 §3.1).
//!
//! The toolkit owns what is INSIDE these directories — `layauts/`,
//! `widgets/<category>/`, `sounds/` — and never how they are found:
//! only the embedder knows its own name. The desktop passes
//! `AssetRoots::xdg("nacelle-desktop")`; a compositor or a test passes
//! whatever directories it likes, and two embedders in one process can
//! hold two different roots, because nothing here is process-wide.

use std::path::{Component, Path, PathBuf};

/// Read search path plus the one write target.
#[derive(Clone, Debug)]
pub struct AssetRoots {
    /// Read search path, most specific first. The first directory
    /// holding a given name wins.
    pub read: Vec<PathBuf>,
    /// The one directory anything is ever WRITTEN to.
    pub write: PathBuf,
}

impl AssetRoots {
    pub fn new(read: Vec<PathBuf>, write: PathBuf) -> Self {
        Self { read, write }
    }

    /// The XDG arrangement, for an embedder that wants it:
    /// `$XDG_DATA_HOME/<app>` (or `~/.local/share/<app>`) first — which
    /// is also the write root — then every `$XDG_DATA_DIRS` entry (or
    /// the classic `/usr/local/share:/usr/share` pair) joined with
    /// `<app>`, duplicates dropped.
    pub fn xdg(app: &str) -> Self {
        let user = match std::env::var("XDG_DATA_HOME") {
            Ok(x) if !x.is_empty() => PathBuf::from(x).join(app),
            _ => {
                let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
                PathBuf::from(home).join(".local").join("share").join(app)
            }
        };
        let mut read = vec![user.clone()];
        let system = std::env::var("XDG_DATA_DIRS")
            .ok()
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| "/usr/local/share:/usr/share".to_string());
        for base in system.split(':').filter(|b| !b.is_empty()) {
            let dir = PathBuf::from(base).join(app);
            if !read.contains(&dir) {
                read.push(dir);
            }
        }
        Self { read, write: user }
    }

    /// The XDG CONFIGURATION arrangement, the same shape one rung over:
    /// `$XDG_CONFIG_HOME/<app>` (or `~/.config/<app>`) first and also
    /// the write root, then every `$XDG_CONFIG_DIRS` entry (or `/etc/xdg`)
    /// joined with `<app>`, duplicates dropped.
    ///
    /// It is the same type because it is the same question — an ordered
    /// read path and one write target — asked about a different pair of
    /// variables. What differs is only what belongs on each side: a
    /// theme, a layout and a sound set are DATA and live under the data
    /// dirs; what the user chose is configuration and lives here. See
    /// [`crate::settings`], which reads addon settings through this.
    pub fn xdg_config(app: &str) -> Self {
        let user = match std::env::var("XDG_CONFIG_HOME") {
            Ok(x) if !x.is_empty() => PathBuf::from(x).join(app),
            _ => {
                let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
                PathBuf::from(home).join(".config").join(app)
            }
        };
        let mut read = vec![user.clone()];
        let system = std::env::var("XDG_CONFIG_DIRS")
            .ok()
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| "/etc/xdg".to_string());
        for base in system.split(':').filter(|b| !b.is_empty()) {
            let dir = PathBuf::from(base).join(app);
            if !read.contains(&dir) {
                read.push(dir);
            }
        }
        Self { read, write: user }
    }

    /// Sub-directories named `sub` that exist, in search order.
    pub fn dirs(&self, sub: &str) -> Vec<PathBuf> {
        self.read
            .iter()
            .map(|d| d.join(sub))
            .filter(|d| d.is_dir())
            .collect()
    }

    /// The first `<root>/<sub>/<rel>` that exists, in search order.
    pub fn find(&self, sub: &str, rel: &str) -> Option<PathBuf> {
        self.read
            .iter()
            .map(|d| d.join(sub).join(rel))
            .find(|p| p.exists())
    }

    /// Where `sub` is written — no filesystem access, no creation.
    pub fn write_dir(&self, sub: &str) -> PathBuf {
        self.write.join(sub)
    }

    /// The write directory for `sub`, created if missing.
    pub fn ensure(&self, sub: &str) -> std::io::Result<PathBuf> {
        let dir = self.write_dir(sub);
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }
}

/// One safe path component: not empty, not `.`, not `..`, no separator,
/// no NUL. The rule that keeps a `.layaut` name or a widget directory
/// name from escaping the data directory.
pub fn safe_component(name: &str) -> Option<String> {
    let n = name.trim();
    if n.is_empty() || n == "." || n == ".." {
        return None;
    }
    if n.contains('/') || n.contains('\\') || n.contains('\0') {
        return None;
    }
    // Must be exactly one normal path component.
    let mut comps = Path::new(n).components();
    match (comps.next(), comps.next()) {
        (Some(Component::Normal(c)), None) if c == n => Some(n.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_component_blocks_every_escape() {
        for bad in ["", ".", "..", "a/b", "a\\b", "a\0b", "/etc", " . "] {
            assert!(safe_component(bad).is_none(), "{bad:?} must be refused");
        }
        assert_eq!(safe_component(" console "), Some("console".into()));
        assert_eq!(safe_component("my.layaut"), Some("my.layaut".into()));
    }

    /// The config search path reads the config VARIABLES, not the data
    /// ones. The two builders are one function apart, and mixing them
    /// would put the user's settings where a theme goes — silently,
    /// because both directories exist.
    ///
    /// The environment is READ here and never written: `theme` and
    /// `font` read `HOME` and `XDG_CONFIG_HOME` too, the harness runs
    /// tests in parallel, and a test that set a variable would decide
    /// what another one saw.
    #[test]
    fn config_roots_are_not_data_roots() {
        let cfg = AssetRoots::xdg_config("nacelle");
        let data = AssetRoots::xdg("nacelle");
        assert_ne!(cfg.write, data.write, "settings do not live where themes do");
        assert_eq!(cfg.read[0], cfg.write, "the user's own directory is the write target");
        assert!(cfg.read.iter().all(|p| p.ends_with("nacelle")));
        // The conventional ends of the cascade, on a machine that has
        // not overridden them — `~/.config/nacelle` over `/etc/xdg/nacelle`,
        // which is the arrangement the owner's decision names.
        if std::env::var_os("XDG_CONFIG_HOME").is_none() {
            if let Ok(home) = std::env::var("HOME") {
                assert_eq!(cfg.read[0], PathBuf::from(home).join(".config").join("nacelle"));
            }
        }
        if std::env::var_os("XDG_CONFIG_DIRS").is_none() {
            assert!(cfg.read.contains(&PathBuf::from("/etc/xdg/nacelle")));
        }
    }

    #[test]
    fn the_first_root_holding_a_name_wins() {
        let base = std::env::temp_dir().join(format!("nacelle-assets-{}", std::process::id()));
        let (a, b) = (base.join("a"), base.join("b"));
        std::fs::create_dir_all(a.join("layauts")).unwrap();
        std::fs::create_dir_all(b.join("layauts")).unwrap();
        std::fs::write(b.join("layauts/x.layaut"), "fixed").unwrap();
        let roots = AssetRoots::new(vec![a.clone(), b.clone()], a.clone());
        assert_eq!(roots.dirs("layauts").len(), 2);
        assert_eq!(roots.find("layauts", "x.layaut"), Some(b.join("layauts/x.layaut")));
        std::fs::write(a.join("layauts/x.layaut"), "fixed").unwrap();
        assert_eq!(roots.find("layauts", "x.layaut"), Some(a.join("layauts/x.layaut")));
        assert_eq!(roots.write_dir("layauts"), a.join("layauts"));
        let _ = std::fs::remove_dir_all(base);
    }
}
