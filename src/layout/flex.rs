//! Window-driven responsive layout — "like a website".
//!
//! Every frame the panel layout is computed from the ACTUAL window size,
//! so resizing or moving the window reflows the interface live. Layouts
//! are flexbox column descriptions (FlexLayaut) solved by the `taffy`
//! crate — the same layout algorithm web pages use: columns have real
//! min/max pixel widths (side columns shrink before the terminal does)
//! and collapse priorities (when a column can no longer fit its minimum
//! width it disappears — collapse=1 first, then 2, ...). If the control
//! panel loses its column, it comes back as a full-width bar at the
//! bottom. On portrait windows the visible panels restack vertically
//! (the body columns merge from the right until they fit the width;
//! nothing is ever hidden for being short). Every registered widget is
//! an
//! individual panel. The built-in default layout and custom
//! flexbox .layaut files share this engine; legacy .layaut files (fixed
//! x/y/w/h at the 16:9 reference) are re-adapted with an edge-anchored
//! transform on landscape and the flex restack on portrait.

use crate::base::{
    panel_count, FlexColumn, FlexLayaut, Layout, LayoutMode, LayoutSpec, Panel, Rect, SizeTable,
};
use taffy::prelude::{auto, length, percent};
use taffy::style::{AvailableSpace, FlexDirection};
use taffy::{Size, Style, TaffyTree};

/// CSS-like pixel constraints of the built-in default columns.
const SIDE_MIN: f32 = 168.0;
const SIDE_MAX: f32 = 340.0;
const CENTER_MIN: f32 = 430.0;

/// The three anchored roles the layout engine treats specially. They are
/// matched BY NAME, because widgets come from the registry now: a set
/// without a terminal or an on-screen keyboard simply has no anchor.
fn named(name: &str) -> Option<Panel> {
    Panel::from_name(name)
}

fn is(p: Panel, name: &str) -> bool {
    p.name() == name
}

/// The built-in default layout as a flexbox description — the same
/// structure a theme author writes in a flexbox .layaut file. It is the
/// HOME arrangement of u1 §1.1: instruments stacked on the left with
/// the two-button bar anchored bottom-left, the terminal and keyboard
/// in the wide centre, the feeds on the right with the clock in its own
/// box at the bottom. Mirrored verbatim by the shipped console.layaut;
/// the desktop test `shipped_console_layaut_matches_builtin_default`
/// proves they cannot diverge.
pub fn default_flex() -> FlexLayaut {
    let col = |basis, min, max, grow, collapse, gap, panels: &[(Panel, f32)]| FlexColumn {
        basis,
        min,
        max,
        grow,
        collapse,
        gap,
        panels: panels.to_vec(),
    };
    // Named rather than hard-coded: a registry without one of these
    // widgets just leaves it out of the default layout.
    let by_name = |list: &[(&str, f32)]| -> Vec<(Panel, f32)> {
        list.iter()
            .filter_map(|(n, wt)| named(n).map(|p| (p, *wt)))
            .collect()
    };
    FlexLayaut {
        columns: vec![
            // Left: the instrument stack. Weights only matter between
            // the growers (cpu, memory); the Content-sized panels take
            // exactly what they measure whatever their number says.
            col(16.4, SIDE_MIN, SIDE_MAX, 0.0, 2, 1.0, &by_name(&[
                ("sysinfo", 5.0),
                ("hardware", 7.0),
                ("cpu", 26.0),
                ("memory", 18.0),
                ("uptime", 8.0),
                ("control", 12.0),
            ])),
            // Centre: the work surface. shell/keyboard are re-anchored
            // by normalize() to the top and bottom of this column.
            col(65.0, CENTER_MIN, f32::INFINITY, 1.0, 0, 1.7, &by_name(&[
                ("shell", 62.0),
                ("keyboard", 28.0),
            ])),
            // Right: the feeds, and the clock in its own box at the
            // bottom (u1 §1.2 #12 — the images' clearest instruction).
            col(16.4, SIDE_MIN, SIDE_MAX, 0.0, 1, 2.5, &by_name(&[
                ("filesystem", 44.0),
                ("processes", 26.0),
                ("network", 10.0),
                ("clock", 8.0),
            ])),
        ],
        units_px: false,
        pad_x: None,
    }
}

/// The reference and minimum heights (vh) the built-in default asks
/// for. Sizes belong to the LAYOUT, not to the widget (base.rs keeps
/// them apart for exactly this reason), so the built-in layout has to
/// name its own instead of inheriting the registry's — otherwise the
/// ref/min column of console.layaut would silently not apply to the
/// default. The minimums are u1 §0's arithmetic: each keeps the
/// widget's last content row on screen (memory's SWAP meter, network's
/// PING, cpu's LOAD line, uptime's KERNEL row).
pub fn builtin_sizes() -> Vec<(Panel, f32, f32)> {
    [
        ("sysinfo", 4.5, 4.5),
        ("hardware", 6.5, 6.5),
        ("cpu", 15.5, 9.0),
        ("memory", 12.0, 9.0),
        ("uptime", 8.0, 7.0),
        ("control", 13.0, 13.0),
        ("shell", 60.0, 12.0),
        ("keyboard", 28.0, 12.0),
        ("filesystem", 40.0, 10.0),
        ("processes", 22.0, 8.0),
        ("network", 8.0, 8.0),
        ("clock", 7.0, 5.5),
    ]
    .iter()
    .filter_map(|(n, r, m)| named(n).map(|p| (p, *r, *m)))
    .collect()
}

/// Device-independent layout unit. `min` / `max` in a .layaut and the
/// built-in column constants are written at a 1080-line reference and
/// scale with the window height, so one composition comes out at 720p
/// and at 4K instead of two thin ribbons around a giant terminal.
/// Clamped, so a 300-line window still gets usable columns and an 8K
/// wall does not get 1200px side columns.
fn lu(h: f32) -> f32 {
    (h / 1080.0).clamp(0.75, 2.5)
}

/// Layout for the current window size, recomputed every frame. `pad`
/// is the widget padding: every panel is kept tall enough for the
/// padding on both sides plus a minimum of content.
pub fn compute(w: f32, h: f32, mode: &LayoutMode, pad: f32) -> Layout {
    compute_in(w, h, mode, pad, &crate::base::size_table())
}

/// The same solve against a CALLER's size table — the per-world form
/// (u3 L2); `compute` above is its process-wide shorthand.
pub fn compute_in(w: f32, h: f32, mode: &LayoutMode, pad: f32, t: &SizeTable) -> Layout {
    match mode {
        LayoutMode::Flex => engine(&default_flex(), w, h, pad, t),
        LayoutMode::Custom(fl) => engine(fl, w, h, pad, t),
        LayoutMode::Fixed(base) => {
            if h > w {
                // Portrait: restack the panels VISIBLE in the base using
                // the flex engine (the default structure filtered down).
                portrait_flex(&filtered_default(base), w, h, pad, t)
            } else {
                Layout::compute(w, h, &edge_adapt(base, w / h))
            }
        }
    }
}

fn engine(fl: &FlexLayaut, w: f32, h: f32, pad: f32, t: &SizeTable) -> Layout {
    let fl = normalize(fl);
    if h > w {
        portrait_flex(&fl, w, h, pad, t)
    } else {
        landscape(&fl, w, h, pad, t)
    }
}

/// Enforces the anchor rules of the flex layout (only for panels that
/// exist in the layaut): the CONTROL panel always sits in the BOTTOM
/// LEFT corner (bottom of the leftmost column), the terminal in the
/// CENTER at the very TOP and the on-screen keyboard in the CENTER at
/// the very BOTTOM. All remaining panels flow wherever the algorithm
/// puts them.
fn normalize(fl: &FlexLayaut) -> FlexLayaut {
    let mut fl = fl.clone();
    let mut shell_wt = None;
    let mut keyboard_wt = None;
    let mut control_wt = None;
    for c in fl.columns.iter_mut() {
        c.panels.retain(|(p, wt)| match p {
            p if is(*p, "shell") => {
                shell_wt = Some(*wt);
                false
            }
            p if is(*p, "keyboard") => {
                keyboard_wt = Some(*wt);
                false
            }
            p if is(*p, "control") => {
                control_wt = Some(*wt);
                false
            }
            _ => true,
        });
    }
    if fl.columns.is_empty() {
        return fl;
    }
    // The CENTER column: the growing one, else the widest basis.
    let center = fl
        .columns
        .iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| {
            (a.grow, a.basis)
                .partial_cmp(&(b.grow, b.basis))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(i, _)| i)
        .unwrap_or(0);
    if let (Some(wt), Some(p)) = (shell_wt, named("shell")) {
        fl.columns[center].panels.insert(0, (p, wt));
    }
    if let (Some(wt), Some(p)) = (control_wt, named("control")) {
        fl.columns[0].panels.push((p, wt));
    }
    if let (Some(wt), Some(p)) = (keyboard_wt, named("keyboard")) {
        fl.columns[center].panels.push((p, wt));
    }
    fl.columns.retain(|c| !c.panels.is_empty());
    fl
}

/// Splits `span` between stacked panels by their weights, enforcing the
/// per-panel minimum heights (content + padding). Space for the minimums
/// is taken from panels above their minimum; when even the minimums do
/// not fit, everything scales down proportionally to them.
fn stack_heights(
    weights: &[f32],
    mins: &[f32],
    wants: &[Option<f32>],
    gap_units: f32,
    span: f32,
) -> (Vec<f32>, f32) {
    let n = weights.len() as f32;
    let total: f32 = weights.iter().sum::<f32>() + gap_units * (n - 1.0).max(0.0);
    let gap_px = gap_units / total.max(0.001) * span;
    let content_span = (span - gap_px * (n - 1.0).max(0.0)).max(1.0);
    let min_sum: f32 = mins.iter().sum();
    if min_sum >= content_span {
        let k = content_span / min_sum.max(0.001);
        return (mins.iter().map(|m| m * k).collect(), gap_px);
    }
    let wsum: f32 = weights.iter().sum();
    // A widget that measured itself takes exactly what its content
    // needs; the ones that grow into whatever they get share the rest,
    // by weight. That is what keeps the box around a clock the size of
    // a clock and gives the height it did not need to the process list.
    let asked: f32 = wants.iter().flatten().sum();
    let grow_sum: f32 = (0..weights.len())
        .filter(|i| wants.get(*i).copied().flatten().is_none())
        .map(|i| weights[i])
        .sum();
    let mut hs: Vec<f32> = if grow_sum > 0.0 && asked < content_span {
        let left = content_span - asked;
        (0..weights.len())
            .map(|i| match wants.get(i).copied().flatten() {
                Some(h) => h,
                None => weights[i] / grow_sum * left,
            })
            .collect()
    } else if asked > 0.0 && grow_sum <= 0.0 {
        // Every panel measured itself: they share the column in the
        // proportions they asked for, shrunk together if they do not fit.
        let k = (content_span / asked).min(1.0);
        wants
            .iter()
            .enumerate()
            .map(|(i, w)| w.unwrap_or(weights[i]) * k)
            .collect()
    } else {
        weights
            .iter()
            .map(|wt| wt / wsum.max(0.001) * content_span)
            .collect()
    };
    for _ in 0..4 {
        let mut deficit = 0.0;
        for i in 0..hs.len() {
            if hs[i] < mins[i] {
                deficit += mins[i] - hs[i];
                hs[i] = mins[i];
            }
        }
        if deficit <= 0.5 {
            break;
        }
        let surplus: f32 = (0..hs.len()).map(|i| (hs[i] - mins[i]).max(0.0)).sum();
        if surplus <= 0.0 {
            break;
        }
        let k = (deficit / surplus).min(1.0);
        for i in 0..hs.len() {
            let s = (hs[i] - mins[i]).max(0.0);
            hs[i] -= s * k;
        }
    }
    (hs, gap_px)
}

fn has_panel(fl: &FlexLayaut, p: Panel) -> bool {
    fl.columns.iter().any(|c| c.panels.iter().any(|(k, _)| *k == p))
}

/// The outer height a panel must never be squeezed under: its minimum
/// CONTENT (min_h_vh names the widget's last content row), plus the
/// container's chrome around that content — border, padding, the title
/// band — plus the widget padding on both sides. The published wants
/// carry the chrome already (the sizing pass adds `chrome_extra`);
/// before the chrome term the minimums did not, so a stacked column
/// under pressure was solved as if every band were free, and each
/// titled panel came out exactly one band short of its own content.
fn min_outer(p: Panel, h: f32, pad: f32, t: &SizeTable) -> f32 {
    t.min_h_vh(p) / 100.0 * h + t.chrome_h(p) + 2.0 * pad
}

/// A stacked column needs room for its own panels' minimums; below that
/// it is better dropped than crushed. Asked per column, not once for
/// the window — the old flat `h >= 520` test dropped every collapsible
/// column of a 3840×500 strip at once, nine widgets gone with 3793 px
/// of width available.
fn column_fits(c: &FlexColumn, h: f32, pad: f32, span: f32, t: &SizeTable) -> bool {
    let need: f32 = c
        .panels
        .iter()
        .map(|(p, _)| min_outer(*p, h, pad, t))
        .sum();
    need <= span
}

/// Landscape flexbox layout: the columns in a row, solved by taffy.
fn landscape(fl: &FlexLayaut, w: f32, h: f32, pad: f32, t: &SizeTable) -> Layout {
    // Page padding: the layout's own when it names one (pad_x in the
    // file, percent per side), the engine's thin margin otherwise.
    let pad_x = match fl.pad_x {
        Some(p) => (w * p / 100.0).max(4.0),
        None => (w * 0.006).max(4.0),
    };
    let gap = (w * 0.005).max(4.0);
    let inner = w - 2.0 * pad_x;
    // min/max are device-independent (a 1080-line reference) unless the
    // file said `units = px`.
    let u = if fl.units_px { 1.0 } else { lu(h) };
    // The vertical span a column stacks into (the classic 2.5vh..97vh).
    let span = h * (0.97 - 0.025);

    // Collapse, two questions in order. First HEIGHT, per column: a
    // stacked column whose panels' minimums do not fit the span is
    // dropped rather than crushed — lowest collapse value first, never
    // the collapse = 0 columns.
    let mut vis: Vec<&FlexColumn> = fl.columns.iter().collect();
    loop {
        let idx = vis
            .iter()
            .enumerate()
            .filter(|(_, c)| c.collapse > 0 && !column_fits(c, h, pad, span, t))
            .min_by_key(|(_, c)| c.collapse)
            .map(|(i, _)| i);
        match idx {
            Some(i) => {
                vis.remove(i);
            }
            None => break,
        }
    }
    // Then WIDTH: drop columns (lowest collapse value first) while the
    // visible minimum widths do not fit.
    loop {
        let mins: f32 = vis.iter().map(|c| (c.min * u).max(60.0)).sum::<f32>()
            + gap * (vis.len().saturating_sub(1)) as f32;
        let any_collapsible = vis.iter().any(|c| c.collapse > 0);
        if mins <= inner || !any_collapsible {
            break;
        }
        let idx = vis
            .iter()
            .enumerate()
            .filter(|(_, c)| c.collapse > 0)
            .min_by_key(|(_, c)| c.collapse)
            .map(|(i, _)| i)
            .unwrap();
        vis.remove(idx);
    }

    // A layout that lost the control panel's column gets a full-width
    // control bar at the bottom instead.
    let control_dropped = named("control").is_some_and(|ctl| {
        has_panel(fl, ctl)
            && !vis
                .iter()
                .any(|c| c.panels.iter().any(|(p, _)| *p == ctl))
    });
    let bar_h = if control_dropped { h * 0.135 } else { 0.0 };

    // Column widths via taffy (flex-basis/grow/shrink + min/max).
    let mut tf: TaffyTree<()> = TaffyTree::new();
    let mut nodes = Vec::new();
    for c in &vis {
        // Sanitize NaN/negative values from a malformed .layaut so taffy
        // never produces NaN geometry (which would render off-screen or
        // panic downstream comparisons).
        let basis = if c.basis.is_finite() { c.basis.max(0.0) } else { 16.0 };
        let grow = if c.grow.is_finite() { c.grow.max(0.0) } else { 0.0 };
        let min = if c.min.is_finite() { (c.min * u).max(60.0) } else { 60.0 };
        let style = Style {
            flex_basis: percent(basis / 100.0),
            flex_grow: grow,
            flex_shrink: 1.0,
            min_size: Size { width: length(min), height: auto() },
            max_size: Size {
                width: if c.max.is_finite() { length(c.max * u) } else { auto() },
                height: auto(),
            },
            ..Default::default()
        };
        nodes.push(tf.new_leaf(style).unwrap());
    }
    let root = tf
        .new_with_children(
            Style {
                flex_direction: FlexDirection::Row,
                size: Size { width: length(w), height: length(h) },
                padding: taffy::Rect {
                    left: length(pad_x),
                    right: length(pad_x),
                    top: length(0.0),
                    bottom: length(0.0),
                },
                gap: Size { width: length(gap), height: length(0.0) },
                ..Default::default()
            },
            &nodes,
        )
        .unwrap();
    tf.compute_layout(
        root,
        Size { width: AvailableSpace::Definite(w), height: AvailableSpace::Definite(h) },
    )
    .unwrap();

    // Vertical placement: panels stacked by their height weights; gaps
    // count as weight units, so the classic proportions (a 94.5vh span
    // from 2.5vh to 97vh) come out exactly for the default layout.
    let top = h * 0.025;
    let mut content_bottom = h * 0.97;
    if control_dropped {
        content_bottom -= bar_h + h * 0.015;
    }
    let hi = (content_bottom - top).max(1.0);

    let mut out = Layout::empty(w, h);
    for (c, node) in vis.iter().zip(&nodes) {
        let tl = tf.layout(*node).unwrap();
        let (cx, cw) = (tl.location.x, tl.size.width);
        let weights: Vec<f32> = c.panels.iter().map(|(_, wt)| *wt).collect();
        let mins: Vec<f32> = c
            .panels
            .iter()
            .map(|(p, _)| min_outer(*p, h, pad, t))
            .collect();
        let wants: Vec<Option<f32>> = c
            .panels
            .iter()
            .map(|(p, _)| t.intrinsic_h(*p).map(|ih| ih + 2.0 * pad))
            .collect();
        let (hs, gap_px) = stack_heights(&weights, &mins, &wants, c.gap, hi);
        let mut y = top;
        for ((p, _), ph) in c.panels.iter().zip(&hs) {
            out.set(*p, Rect::new(cx, y, cw, *ph));
            y += ph + gap_px;
        }
    }
    if let (true, Some(ctl)) = (control_dropped, named("control")) {
        out.set(ctl, Rect::new(pad_x, content_bottom + h * 0.015, inner, bar_h));
    }
    out
}

/// Portrait restack honouring the anchor rules: the terminal at the
/// very top, the on-screen keyboard at the very bottom, the CONTROL
/// panel as its own full-width bar between the body row and the
/// keyboard, and the remaining panels in a row of columns in between.
/// The row is NEVER dropped: hiding eight of twelve widgets because the
/// window is short is a content loss dressed as responsiveness (u1
/// §2.3). A short window only re-proportions the bands, and a narrow
/// one merges the row's columns from the right until they fit.
fn portrait_flex(fl: &FlexLayaut, w: f32, h: f32, pad: f32, t: &SizeTable) -> Layout {
    let small = h < 900.0;
    let edge = (w * 0.008).max(4.0);
    let gap = (h * 0.012).max(4.0);
    let iw = w - 2.0 * edge;
    let mut out = Layout::empty(w, h);

    // Row columns: each source column contributes its panels (minus the
    // anchored shell/keyboard/control), one chunk per source column,
    // merged from the right until at most max_chunks remain. No fixed
    // split at 4 — a five-panel column is one chunk, not 4 + a lonely 1.
    let mut chunks: Vec<Vec<(Panel, f32)>> = Vec::new();
    for c in &fl.columns {
        let body: Vec<(Panel, f32)> = c
            .panels
            .iter()
            .filter(|(p, _)| {
                !is(*p, "shell") && !is(*p, "keyboard") && !is(*p, "control")
            })
            .cloned()
            .collect();
        if !body.is_empty() {
            chunks.push(body);
        }
    }
    let max_chunks = ((iw / (280.0 * lu(h))).floor() as usize).clamp(1, 3);
    while chunks.len() > max_chunks {
        let tail = chunks.pop().unwrap();
        if let Some(last) = chunks.last_mut() {
            last.extend(tail);
        }
    }

    let shell_p = named("shell");
    let kb_p = named("keyboard");
    let ctl_p = named("control");
    let has_shell = shell_p.is_some_and(|p| has_panel(fl, p));
    let has_kb = kb_p.is_some_and(|p| has_panel(fl, p));
    let has_ctl = ctl_p.is_some_and(|p| has_panel(fl, p));
    let has_row = !chunks.is_empty();

    // Band proportions: `small` only changes them, never what exists.
    // (shell, row, control, keyboard) as fractions of the height; shell
    // absorbs the slack the gaps leave.
    let (_shell_f, row_f, ctl_f, kb_f) = if small {
        (0.15, 0.50, 0.12, 0.17)
    } else {
        (0.25, 0.40, 0.13, 0.16)
    };

    let kb_h = if has_kb { h * kb_f } else { 0.0 };
    // Control is ALWAYS its own full-width bar in portrait, between the
    // row and the keyboard — never inside a chunk, where it took 41 % of
    // the row's height for two buttons and crushed the instruments.
    let ctl_h = if has_ctl { h * ctl_f } else { 0.0 };
    let row_h = if has_row {
        if has_shell {
            h * row_f
        } else {
            // No terminal: the row takes the shell band too.
            let mut rest = h - 2.0 * gap;
            if kb_h > 0.0 {
                rest -= kb_h + gap;
            }
            if ctl_h > 0.0 {
                rest -= ctl_h + gap;
            }
            rest.max(h * row_f)
        }
    } else {
        0.0
    };

    let mut used = 0.0;
    for ph in [kb_h, row_h, ctl_h] {
        if ph > 0.0 {
            used += ph + gap;
        }
    }
    let shell_h = (h - 2.0 * gap - used).max(h * 0.2);

    // The terminal at the very top.
    let mut y = gap;
    if let (true, Some(p)) = (has_shell, shell_p) {
        out.set(p, Rect::new(edge, y, iw, shell_h));
        y += shell_h + gap;
    }

    // The keyboard at the very bottom.
    if let (true, Some(p)) = (kb_h > 0.0, kb_p) {
        out.set(p, Rect::new(edge, h - gap - kb_h, iw, kb_h));
    }

    // The control bar directly above it (or at the bottom when there is
    // no keyboard).
    if let (true, Some(p)) = (ctl_h > 0.0, ctl_p) {
        let mut cy = h - gap - ctl_h;
        if kb_h > 0.0 {
            cy -= kb_h + gap;
        }
        out.set(p, Rect::new(edge, cy, iw, ctl_h));
    }

    if has_row {
        // Column headers (e.g. NETWORK) draw above their rect — start
        // the columns slightly lower to leave room for them.
        let d = h * 0.025;
        let cgap = (w * 0.01).max(4.0);
        let units: f32 = chunks
            .iter()
            .map(|body| if body.len() >= 4 { 1.2 } else { 1.0 })
            .sum();
        let ncols = chunks.len() as f32;
        let cw = (iw - cgap * (ncols - 1.0).max(0.0)) / units.max(0.5);
        let mut x = edge;
        for body in chunks.iter() {
            let this_w = cw * if body.len() >= 4 { 1.2 } else { 1.0 };
            let stack_h = row_h - d;
            // Stack the body panels by their weights, with per-panel
            // minimum heights (content + widget padding). When even the
            // minimums do not fit — nine instruments in a phone-sized
            // window — stack_heights scales them down together: small,
            // but present, which the amendment's content test demands.
            let weights: Vec<f32> = body.iter().map(|(_, wt)| *wt).collect();
            let mins: Vec<f32> = body
                .iter()
                .map(|(p, _)| min_outer(*p, h, pad, t))
                .collect();
            let wants: Vec<Option<f32>> = body
                .iter()
                .map(|(p, _)| t.intrinsic_h(*p).map(|ih| ih + 2.0 * pad))
                .collect();
            let (hs, gap_px) = stack_heights(&weights, &mins, &wants, 1.0, stack_h);
            let mut py = y + d;
            for ((p, _), ph) in body.iter().zip(&hs) {
                out.set(*p, Rect::new(x, py, this_w, *ph));
                py += ph + gap_px;
            }
            x += this_w + cgap;
        }
    }
    out
}

/// The default flex structure filtered down to the panels visible in a
/// legacy fixed layout — used for its portrait restack.
fn filtered_default(base: &LayoutSpec) -> FlexLayaut {
    let mut fl = default_flex();
    for c in fl.columns.iter_mut() {
        c.panels.retain(|(p, _)| base.p(*p).x < 100.0);
    }
    fl.columns.retain(|c| !c.panels.is_empty());
    fl
}

/// Landscape adaptation of legacy fixed .layaut files (authored at the
/// 16:9 reference): an edge-anchored horizontal transform — panels keep
/// their distance to the nearer window edge, so side columns keep a sane
/// width on any aspect ratio.
fn edge_adapt(base: &LayoutSpec, ratio: f32) -> LayoutSpec {
    let f = ((16.0 / 9.0) / ratio).clamp(0.5, 1.4);
    if (f - 1.0).abs() < 0.001 {
        return base.clone();
    }
    let mut out = base.clone();
    for i in 0..panel_count() {
        let p = &base.panels[i];
        if p.x >= 100.0 {
            continue;
        }
        let a = p.x;
        let b = p.x + p.w;
        let na = if a <= 50.0 { a * f } else { 100.0 - (100.0 - a) * f };
        let nb = if b <= 50.0 { b * f } else { 100.0 - (100.0 - b) * f };
        out.panels[i] = crate::base::PanelSpec {
            x: na,
            y: p.y,
            w: (nb - na).max(1.0),
            h: p.h,
        };
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn placed(l: &Layout, p: Panel, w: f32) -> bool {
        l.p(p).x < w
    }

    /// u1 §5.5 (1): every registered widget is on the HOME board. This
    /// is the test that failed before the §1.1 arrangement — `uptime`
    /// was registered, drawable and on no board at all.
    #[test]
    fn every_registered_widget_is_placed() {
        let l = compute(1920.0, 1080.0, &LayoutMode::Flex, 8.0);
        for p in Panel::all() {
            assert!(
                placed(&l, p, 1920.0),
                "{} is not on the board at 1920x1080",
                p.name()
            );
        }
    }

    /// u1 §5.5 (2): the same holds in portrait, tall and phone-sized.
    /// This is the test the old `small = h < 900` failed — it hid eight
    /// of the twelve widgets on any window under 900 lines.
    #[test]
    fn every_widget_placed_in_portrait_too() {
        for (w, h) in [(1080.0, 1920.0), (720.0, 1280.0), (400.0, 800.0)] {
            let l = compute(w, h, &LayoutMode::Flex, 8.0);
            for p in Panel::all() {
                assert!(
                    placed(&l, p, w),
                    "{} is not on the board at {}x{}",
                    p.name(),
                    w,
                    h
                );
            }
        }
    }

    /// u1 §5.5 (5): the side column keeps the same proportion to the
    /// reference width (h * 0.30) at every screen size. Before the
    /// device-independent units it was 0.97 at 1080p and 0.62 at 4K —
    /// the console became a terminal with two ribbons.
    #[test]
    fn proportions_are_resolution_independent() {
        let side = Panel::from_name("sysinfo").expect("sysinfo registered");
        let ws_at = |w: f32, h: f32| -> f32 {
            let l = compute(w, h, &LayoutMode::Flex, 8.0);
            l.p(side).w / (h * 0.30)
        };
        let all = [
            ws_at(1280.0, 720.0),
            ws_at(1920.0, 1080.0),
            ws_at(2560.0, 1440.0),
            ws_at(3840.0, 2160.0),
        ];
        let lo = all.iter().cloned().fold(f32::INFINITY, f32::min);
        let hi = all.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        assert!(
            hi - lo <= 0.02,
            "side column proportion drifts with resolution: {all:?}"
        );
    }

    /// The 400x800 portrait row is the honest limit: one merged chunk,
    /// nine instruments, scaled below their minimums together rather
    /// than hidden. The clock must land in the row, not off-screen.
    #[test]
    fn phone_sized_portrait_merges_into_one_chunk() {
        let l = compute(400.0, 800.0, &LayoutMode::Flex, 8.0);
        let clock = Panel::from_name("clock").unwrap();
        let sysinfo = Panel::from_name("sysinfo").unwrap();
        // One chunk means the first and the last instrument share a
        // column: same x.
        assert!((l.p(clock).x - l.p(sysinfo).x).abs() < 0.5);
    }

    /// The minimums the solver keeps are OUTER heights: content plus
    /// the container's chrome around it. Before the chrome term they
    /// were content-only, so a squeezed portrait column was solved as
    /// if every title band were free — and each titled panel came out
    /// exactly one band short, its widget painting over the neighbour
    /// below (FILESYSTEM's tiles over the process header, 900x1600).
    #[test]
    fn published_chrome_raises_a_panels_minimum() {
        let (w, h, pad) = (900.0, 1600.0, 8.0);
        let net = Panel::from_name("network").unwrap();
        let sizes = crate::base::default_sizes();
        let n = sizes.len();
        let mut chrome = vec![0.0; n];
        chrome[net.idx()] = 40.0;
        let bare =
            SizeTable::new(sizes.clone(), vec![None; n], vec![0.0; n]);
        let dressed = SizeTable::new(sizes, vec![None; n], chrome);
        let l0 = compute_in(w, h, &LayoutMode::Flex, pad, &bare);
        let l1 = compute_in(w, h, &LayoutMode::Flex, pad, &dressed);
        // The band is not free: the panel's share must grow by (most
        // of) the published chrome, not stay at the content minimum.
        assert!(
            l1.p(net).h > l0.p(net).h + 20.0,
            "chrome ignored: {} -> {}",
            l0.p(net).h,
            l1.p(net).h
        );
        // And the column still holds: panels sharing the network's
        // column may not overlap each other.
        let col: Vec<Rect> = Panel::all()
            .into_iter()
            .filter(|p| (l1.p(*p).x - l1.p(net).x).abs() < 0.5)
            .map(|p| l1.p(p))
            .collect();
        for a in 0..col.len() {
            for b in (a + 1)..col.len() {
                let (ra, rb) = (col[a], col[b]);
                assert!(
                    ra.y + ra.h <= rb.y + 0.5 || rb.y + rb.h <= ra.y + 0.5,
                    "column panels overlap: {ra:?} vs {rb:?}"
                );
            }
        }
    }
}
