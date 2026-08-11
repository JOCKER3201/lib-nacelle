//! Turning a name into a running widget (u3 §3.3 `WidgetFactory`):
//! linked-in first, then a `.so` on the search path, then a `.rhai`
//! script. The factory owns the policy the desktop's three lookup
//! functions used to share — the safe-name filter, the directory
//! search, the plugins kill-switch — and an embedder registers its
//! own built-ins instead of the toolkit knowing anybody's four.

use super::registry;
use super::Widget;
use crate::assets::{safe_component, AssetRoots};
use crate::plugin::PluginWidget;
use crate::runtime::HostApi;

/// What a compiled widget crate exports when it is linked in rather
/// than dlopened. Same shape as the dlopen attach point, minus the
/// symbol.
pub type BuiltinAttach = fn(&'static HostApi) -> *const crate::runtime::PluginApi;

pub struct WidgetFactory {
    roots: AssetRoots,
    builtins: Vec<(&'static str, BuiltinAttach)>,
    plugins_enabled: bool,
}

impl WidgetFactory {
    /// `plugins_enabled` defaults to true; an embedder with a safe
    /// mode turns it off ([`Self::plugins_enabled`]) and no `.so` is
    /// ever opened — a plugin that crashes during startup must not
    /// lock the user out of the settings that would disable it.
    pub fn new(roots: AssetRoots) -> Self {
        Self { roots, builtins: Vec::new(), plugins_enabled: true }
    }

    /// Registers a linked-in widget. The same plugin crates that build
    /// a shipped `.so` link in statically (without their dlopen attach
    /// symbol, which several copies would collide on) and attach in
    /// process, so a core widget is never a file that could be
    /// deleted.
    pub fn with_builtin(mut self, name: &'static str, attach: BuiltinAttach) -> Self {
        self.builtins.push((name, attach));
        self
    }

    pub fn plugins_enabled(mut self, on: bool) -> Self {
        self.plugins_enabled = on;
        self
    }

    /// The registry this factory's world offers: the directory scan,
    /// plus the built-ins whether or not anything on disk mentions
    /// them — an empty machine still gets a working set. Sorted by
    /// name, like the scan itself.
    pub fn registry(&self) -> Vec<crate::base::WidgetDef> {
        let mut out = registry::scan(&self.roots);
        let known = crate::base::builtin_widgets();
        for (name, _) in &self.builtins {
            if !out.iter().any(|d| d.name == *name) {
                if let Some(d) = known.iter().find(|d| d.name == *name) {
                    out.push(d.clone());
                }
            }
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }

    /// A running widget for the name, or None with a said reason:
    /// linked-in → compiled on the search path → script.
    pub fn make(&self, name: &str) -> Option<Box<dyn Widget>> {
        let name = safe_component(name)?;
        if let Some((_, attach)) = self.builtins.iter().find(|(n, _)| *n == name) {
            let table = attach(crate::plugin::host_api());
            if let Some(w) = unsafe { PluginWidget::new(table) } {
                return Some(Box::new(w) as Box<dyn Widget>);
            }
            return None;
        }
        let dir = registry::widget_dir(&self.roots, &name)?;
        #[cfg(unix)]
        if self.plugins_enabled {
            if let Some(w) = super::loader::load(&dir, &name) {
                return Some(w);
            }
        } else if dir.join(format!("{name}.so")).is_file() {
            eprintln!("nacelle: plugins disabled — skipping compiled widget '{name}'");
        }
        let script = dir.join(format!("{name}.rhai"));
        if script.is_file() {
            if let Some(s) = crate::script::Script::load(&script) {
                return Some(Box::new(crate::script::ScriptWidget::new(s)) as Box<dyn Widget>);
            }
        }
        None
    }
}
