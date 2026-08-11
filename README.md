# libnacelle

The toolkit the [nacelle-desktop](https://github.com/JOCKER3201/nacelle-desktop) project
is built on — one crate, the way `libcosmic` is one crate for COSMIC.

Splitting this into a base, an object library and a widget framework
bought nothing: the three always moved together, and every change meant
three commits, three pushes and a lock file to reconcile. They are one
crate again.

| module | what it is |
|---|---|
| `draw`, `font`, `theme` | drawing primitives, the glyph atlas, colours |
| `base` | geometry, the drawing context, the panel model, the widget registry |
| `flex` | the responsive layout engine, recomputed from the window every frame |
| `geometry` | the control rectangles an application and a widget must agree on before either has drawn |
| `object` | windows, buttons, sliders, drop-downs, checkboxes |
| `ui` | the drawing vocabulary widgets are composed from |
| `widget` | the contract widgets implement and an application drives them through |
| `script` | widgets written as Rhai scripts |
| `term` | terminal emulation — a pure VT state machine |
| `telemetry` | the system data model widgets render |
| `sound` | sound events, themes and mixing |
| `runtime` | the state that must exist once per process, and how a plugin shares the host's copy instead of getting its own |
| `plugin` | the host side of the plugin boundary: the table a plugin draws through, and the wrapper that makes one look like any other widget |

Everything here is platform-independent. Creating a window, opening a
PTY, collecting telemetry and handing audio frames to a device belong to
the application.

> **THIS PROJECT WAS WRITTEN ENTIRELY BY ANTHROPIC'S CLAUDE AI MODELS.**

## Widgets

A widget implements the `widget::Widget` contract. It reads only what
the host hands it and asks for anything it wants done, so the same
widget works in any application built on this toolkit.

Most widgets are Rhai scripts, rendered by `script` through `ui`. That
path is sandboxed by construction: a script is given the host data and
the drawing vocabulary and nothing else, so it has no way to reach the
filesystem, the network or another process.

Compiled widgets exist for what a script cannot do — drawing thousands
of terminal cells a frame, or reading a directory. One is a `cdylib`
exporting `nacelle_plugin_attach`, which takes the host's function table
and returns its own; `runtime` and `plugin` are the two ends of that
boundary.

> **The following concerns compiled (`.so`) widgets only, and nothing
> about it applies to scripts.** A compiled widget is native code with
> the host's full privileges and no sandbox, and it has to be rebuilt
> for every release and for each platform and architecture. A script is
> written once, runs everywhere, and can do no harm.

## Usage

```toml
[dependencies]
libnacelle = { git = "https://github.com/JOCKER3201/libnacelle" }
```

```rust
use nacelle::{ui, Action, Ctx, Host, Rect, Widget};

pub struct Uptime;

impl Widget for Uptime {
    fn draw(&mut self, ctx: &mut Ctx, r: Rect, host: &Host) {
        ui::rows_label_value(ctx, r, &[("UPTIME".into(), host.snap.uptime.to_string())]);
    }
}
```

## License

MIT — see [LICENSE](LICENSE).
