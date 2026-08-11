//! Opening a compiled widget (u3 §3.3): the dlopen half of the
//! factory, unix-only because it is a platform call.
//!
//! What is loaded is native code with the embedder's full privileges
//! and no sandbox around it. Everything below is about robustness
//! rather than trust: an outdated or broken plugin must cost its own
//! panel and nothing more. Nothing is ever unloaded — the one rule
//! that keeps this simple, and one exception is how exceptions start.

#![cfg(unix)]

use crate::plugin::PluginWidget;
use crate::runtime::{AttachFn, ATTACH_SYMBOL};
use crate::widget::Widget;
use std::ffi::CString;
use std::path::Path;

/// Loads the given `.so` and attaches it. Every failure returns
/// None with a message: a plugin that will not open, does not export
/// the attach point, or speaks a different interface version leaves
/// its panel empty and the embedder running.
pub fn load(path: &Path, name: &str) -> Option<Box<dyn Widget>> {
    if !path.is_file() {
        return None;
    }

    let cpath = CString::new(path.as_os_str().as_encoded_bytes()).ok()?;
    // RTLD_LOCAL so a plugin's symbols cannot capture the program's,
    // and no RTLD_GLOBAL games between plugins either.
    let lib = unsafe { libc::dlopen(cpath.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
    if lib.is_null() {
        eprintln!("nacelle: cannot open plugin '{name}': {}", dl_error());
        return None;
    }

    // The attach point is what makes this a widget plugin rather than
    // some library that happens to sit in the directory. Refusing
    // without it is also what stops a plugin from quietly keeping its
    // own copy of the toolkit's shared state.
    let sym_name = CString::new(ATTACH_SYMBOL).ok()?;
    let sym = unsafe { libc::dlsym(lib, sym_name.as_ptr()) };
    if sym.is_null() {
        eprintln!("nacelle: '{name}' exports no attach point — not a widget plugin, not loaded");
        return None;
    }

    let attach: AttachFn = unsafe { std::mem::transmute::<*mut libc::c_void, AttachFn>(sym) };
    let api = unsafe { attach(crate::plugin::host_api()) };
    if api.is_null() {
        eprintln!("nacelle: plugin '{name}' refused the host interface — not loaded");
        return None;
    }
    let widget = unsafe { PluginWidget::new(api) }?;
    eprintln!("nacelle: loaded plugin '{name}'");
    Some(Box::new(widget))
}

fn dl_error() -> String {
    let e = unsafe { libc::dlerror() };
    if e.is_null() {
        return "unknown error".into();
    }
    unsafe { std::ffi::CStr::from_ptr(e) }
        .to_string_lossy()
        .into_owned()
}
