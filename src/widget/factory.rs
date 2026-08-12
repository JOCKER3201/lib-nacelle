//! Turning a name into a running widget (u3 §3.3 `WidgetFactory`):
//! linked-in first, then a `.so` on the search path, then a `.rhai`
//! script. The factory owns the policy the desktop's three lookup
//! functions used to share — the safe-name filter, the directory
//! search, the plugins kill-switch — and an embedder hands over the
//! crates it links in, each describing itself, instead of the toolkit
//! knowing anybody's names.

use super::registry;
use super::Widget;
use crate::assets::{safe_component, AssetRoots};
use crate::plugin::PluginWidget;
use crate::runtime::HostApi;

/// What a compiled widget crate exports when it is linked in rather
/// than dlopened. Same shape as the dlopen attach point, minus the
/// symbol.
pub type BuiltinAttach = fn(&'static HostApi) -> *const crate::runtime::PluginApi;

/// A widget crate linked into the program instead of installed as a
/// file — everything the directory scan would have read, plus the
/// symbol no directory can hold.
///
/// The crate declares the whole of it and the host declares none of
/// it: a linked-in addon carries its own name and its own metadata
/// exactly as a file-installed one does, so linking a widget in is a
/// packaging decision and never a second description of the widget.
/// A crate exports one of these as a `pub const` and the embedder
/// hands it over untouched.
#[derive(Clone, Copy)]
pub struct BuiltinWidget {
    /// The name layouts use, from the crate — the same string its
    /// `<name>.so` would be called.
    pub name: &'static str,
    /// The addon's own metadata, in the `key = value` text of the
    /// `<name>.meta` file installed beside a compiled addon. A crate
    /// that ships both usually points this at the very file with
    /// `include_str!`, so the two can never drift. Empty = declares
    /// nothing, which is allowed.
    pub meta: &'static str,
    /// In-process attach point.
    pub attach: BuiltinAttach,
}

pub struct WidgetFactory {
    roots: AssetRoots,
    builtins: Vec<BuiltinWidget>,
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

    /// Registers a linked-in widget crate. The same plugin crates that
    /// build a shipped `.so` link in statically (without their dlopen
    /// attach symbol, which several copies would collide on) and attach
    /// in process, so a core widget is never a file that could be
    /// deleted. The crate's own [`BuiltinWidget`] is passed through
    /// whole — the embedder adds nothing to it.
    pub fn with_builtin(mut self, widget: BuiltinWidget) -> Self {
        self.builtins.push(widget);
        self
    }

    pub fn plugins_enabled(mut self, on: bool) -> Self {
        self.plugins_enabled = on;
        self
    }

    /// The registry this factory's world offers: the linked-in crates
    /// and the directory scan — the two ways an addon can be installed,
    /// and the only two. Neither the toolkit nor the embedder
    /// contributes a name or a size of its own; a linked-in crate is
    /// read from the metadata IT carries, exactly as a file is read
    /// from the metadata beside it.
    ///
    /// A linked-in name wins over a file of the same name because
    /// [`Self::make`] runs the linked-in code — what the editor shows
    /// has to describe what will actually draw. Sorted by name, like
    /// the scan itself.
    pub fn registry(&self) -> Vec<crate::base::WidgetDef> {
        let mut out: Vec<crate::base::WidgetDef> = self
            .builtins
            .iter()
            .filter_map(|b| {
                safe_component(b.name).map(|name| registry::def_from_meta(name, b.meta))
            })
            .collect();
        for def in registry::scan(&self.roots) {
            if !out.iter().any(|d| d.name == def.name) {
                out.push(def);
            }
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }

    /// A running widget for the name, or None with a said reason:
    /// linked-in → compiled on the search path → script.
    pub fn make(&self, name: &str) -> Option<Box<dyn Widget>> {
        let name = safe_component(name)?;
        if let Some(b) = self.builtins.iter().find(|b| b.name == name) {
            let table = (b.attach)(crate::plugin::host_api());
            if let Some(w) = unsafe { PluginWidget::new(table) } {
                return Some(Box::new(w) as Box<dyn Widget>);
            }
            return None;
        }
        if let Some(lib) = registry::plugin_path(&self.roots, &name) {
            #[cfg(unix)]
            if self.plugins_enabled {
                if let Some(w) = super::loader::load(&lib, &name) {
                    return Some(w);
                }
            } else {
                eprintln!("nacelle: plugins disabled — skipping compiled widget '{name}'");
            }
            #[cfg(not(unix))]
            let _ = lib;
        }
        if let Some(script) = registry::script_path(&self.roots, &name) {
            if let Some(s) = crate::script::Script::load(&script) {
                return Some(Box::new(crate::script::ScriptWidget::new(s)) as Box<dyn Widget>);
            }
        }
        None
    }
}
