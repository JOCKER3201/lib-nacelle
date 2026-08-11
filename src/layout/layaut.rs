//! The `.layaut` FORMAT (u3 §3.2 `layout::layaut`): parse and
//! serialise, nothing else — no filesystem, no configuration. Moved
//! verbatim from the desktop's config.rs; the golden corpus test in
//! tests/layaut_corpus.rs holds every byte of its behaviour still.

use super::def::{BoardDef, BoardId, LayoutDef, ResOverride};
use crate::base::{FlexColumn, FlexLayaut, LayoutMode, LayoutSpec, Panel, PanelSpec};

fn parse_res_header(line: &str) -> Option<(u32, u32, u32)> {
    let inner = line.strip_prefix('[')?.strip_suffix(']')?;
    let (res, diag) = inner.split_once('@')?;
    let (w, h) = res.split_once('x')?;
    Some((
        w.trim().parse().ok()?,
        h.trim().parse().ok()?,
        diag.trim().parse().ok()?,
    ))
}

/// Splits a .layaut file into its base text and the per-resolution
/// override sections (everything after the first "[WxH@D]" header).
/// `[board <x>]` or `[board <x> <y>]` header of a board section. One
/// number is a horizontal board — the format the sections started
/// with — and two are a position on the cross.
fn parse_board_header(t: &str) -> Option<BoardId> {
    let inner = t.strip_prefix('[')?.strip_suffix(']')?.trim();
    let nums = inner.strip_prefix("board")?.trim();
    let mut it = nums.split_whitespace();
    let x = it.next()?.parse::<i32>().ok()?;
    let y = match it.next() {
        Some(v) => v.parse::<i32>().ok()?,
        None => 0,
    };
    if it.next().is_some() || (x, y) == (0, 0) {
        return None;
    }
    Some((x, y))
}

/// What a section of a .layaut file is being parsed into. A board
/// section accumulates its RAW text, because it speaks either format —
/// rectangles or [column] lines — and which one is only known when the
/// section closes.
enum Section {
    Res(ResOverride),
    Board(BoardId, String),
}

/// A board section's text as a definition: flexbox when it has columns,
/// the legacy fixed rectangles otherwise. A header with nothing under
/// it is an empty fixed board — a place.
fn parse_board_def(text: &str) -> BoardDef {
    if text.contains("[column]") {
        if let Some((fl, sizes)) = parse_flex(text) {
            return BoardDef { base: LayoutMode::Custom(fl), sizes };
        }
    }
    BoardDef { base: LayoutMode::Fixed(parse_fixed(text)), sizes: Vec::new() }
}

pub fn split_sections(text: &str) -> (String, Vec<ResOverride>, Vec<(BoardId, BoardDef)>) {
    let mut base = String::new();
    let mut sections: Vec<ResOverride> = Vec::new();
    let mut boards: Vec<(BoardId, BoardDef)> = Vec::new();
    let mut current: Option<Section> = None;
    let close = |cur: &mut Option<Section>,
                     sections: &mut Vec<ResOverride>,
                     boards: &mut Vec<(BoardId, BoardDef)>| {
        match cur.take() {
            Some(Section::Res(sec)) => sections.push(sec),
            Some(Section::Board(k, text)) => boards.push((k, parse_board_def(&text))),
            None => {}
        }
    };
    for line in text.lines() {
        let trimmed = line.split('#').next().unwrap_or("").trim();
        if let Some((w, h, diag)) = parse_res_header(trimmed) {
            close(&mut current, &mut sections, &mut boards);
            current = Some(Section::Res(ResOverride { w, h, diag, panels: Vec::new() }));
            continue;
        }
        if let Some(k) = parse_board_header(trimmed) {
            close(&mut current, &mut sections, &mut boards);
            // The header alone is a board: an empty one, but a place.
            current = Some(Section::Board(k, String::new()));
            continue;
        }
        match current.as_mut() {
            None => {
                base.push_str(line);
                base.push('\n');
            }
            Some(Section::Board(_, text)) => {
                // Raw, format decided at close — a "[column]" line is a
                // column of the board, not a new section.
                text.push_str(line);
                text.push('\n');
            }
            Some(Section::Res(sec)) => {
                let Some((k, v)) = trimmed.split_once('=') else { continue };
                let nums: Vec<f32> = v
                    .split_whitespace()
                    .filter_map(|t| t.parse::<f32>().ok().filter(|x| x.is_finite()))
                    .collect();
                if nums.len() != 4 {
                    continue;
                }
                let Some(panel) = Panel::from_name(k.trim()) else { continue };
                let spec = PanelSpec { x: nums[0], y: nums[1], w: nums[2], h: nums[3] };
                sec.panels.retain(|(p, _)| *p != panel);
                sec.panels.push((panel, spec));
            }
        }
    }
    close(&mut current, &mut sections, &mut boards);
    (base, sections, boards)
}

/// Parses a complete .layaut file: the base (flex or legacy format; an
/// empty base means the built-in default) plus the resolution sections.
pub fn parse(text: &str, name: &str) -> LayoutDef {
    let (base_text, overrides, boards) = split_sections(text);
    // Board positions are normalised to be contiguous around the
    // centre: a hand-edited [board 5] with nothing between it and home
    // is simply the first board on the right. Switching walks one
    // neighbour at a time, so gaps would be unreachable places.
    let boards = normalize_boards(boards);
    let has_panel_lines = base_text.lines().any(|l| {
        let t = l.split('#').next().unwrap_or("").trim();
        t.split_once('=')
            .and_then(|(k, _)| Panel::from_name(k.trim()))
            .is_some()
    });
    let mut sizes: Vec<(Panel, f32, f32)> = Vec::new();
    let base = if base_text.contains("[column]") {
        match parse_flex(&base_text) {
            Some((fl, s)) => {
                sizes = s;
                LayoutMode::Custom(fl)
            }
            None => {
                eprintln!(
                    "nacelle: no valid columns in '{name}.layaut' — using the default layout"
                );
                LayoutMode::Flex
            }
        }
    } else if has_panel_lines {
        LayoutMode::Fixed(parse_fixed(&base_text))
    } else {
        // Overrides only: the built-in default is the base.
        LayoutMode::Flex
    };
    LayoutDef { base, overrides, sizes, boards }
}

pub fn normalize_boards(boards: Vec<(BoardId, BoardDef)>) -> Vec<(BoardId, BoardDef)> {
    // Four arms, each renumbered contiguously from the centre out; a
    // diagonal position has no gesture that reaches it and is dropped.
    // The vertical arms hold one board each — the top and the bottom
    // board are fixtures like home, so anything beyond the nearest
    // section is dropped.
    type Sel = dyn Fn(BoardId) -> Option<i32>;
    type Place = dyn Fn(i32) -> BoardId;
    let mut out: Vec<(BoardId, BoardDef)> = Vec::new();
    let arms: [(&Sel, &Place, usize); 4] = [
        (&|(x, y)| (y == 0 && x < 0).then_some(-x), &|d| (-d, 0), usize::MAX),
        (&|(x, y)| (y == 0 && x > 0).then_some(x), &|d| (d, 0), usize::MAX),
        (&|(x, y)| (x == 0 && y < 0).then_some(-y), &|d| (0, -d), 1),
        (&|(x, y)| (x == 0 && y > 0).then_some(y), &|d| (0, d), 1),
    ];
    for (sel, place, cap) in arms {
        let mut on_arm: Vec<(i32, &BoardDef)> = boards
            .iter()
            .filter_map(|(id, def)| sel(*id).map(|d| (d, def)))
            .collect();
        on_arm.sort_by_key(|(d, _)| *d);
        for (i, (_, def)) in on_arm.into_iter().take(cap).enumerate() {
            out.push((place(i as i32 + 1), def.clone()));
        }
    }
    out.sort_by_key(|(k, _)| *k);
    out
}

pub fn serialize_boards(out: &mut String, boards: &[(BoardId, BoardDef)]) {
    for ((x, y), bd) in boards {
        out.push('\n');
        // A horizontal board keeps the one-number form the sections
        // started with; the vertical ones carry both coordinates.
        if *y == 0 {
            out.push_str(&format!("[board {x}]\n"));
        } else {
            out.push_str(&format!("[board {x} {y}]\n"));
        }
        // Whichever form the board holds is the form written back, so
        // an edited file round-trips.
        match &bd.base {
            LayoutMode::Custom(fl) => serialize_flex(out, fl, &bd.sizes),
            LayoutMode::Flex => {}
            LayoutMode::Fixed(spec) => {
                for panel in Panel::all() {
                    let ps = spec.p(panel);
                    if ps.x >= 100.0 {
                        continue;
                    }
                    out.push_str(&format!(
                        "{} = {:.2} {:.2} {:.2} {:.2}\n",
                        panel.name(),
                        ps.x,
                        ps.y,
                        ps.w,
                        ps.h
                    ));
                }
            }
        }
    }
}

/// Parses the flexbox .layaut format (see the module header).
pub fn parse_flex(src: &str) -> Option<(FlexLayaut, Vec<(Panel, f32, f32)>)> {
    let mut columns: Vec<FlexColumn> = Vec::new();
    let mut cur: Option<FlexColumn> = None;
    let mut sizes: Vec<(Panel, f32, f32)> = Vec::new();
    let mut units_px = false;
    let mut pad_x: Option<f32> = None;
    for line in src.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if line.eq_ignore_ascii_case("[column]") {
            if let Some(c) = cur.take() {
                columns.push(c);
            }
            cur = Some(FlexColumn {
                basis: 20.0,
                min: 60.0,
                max: f32::INFINITY,
                grow: 0.0,
                collapse: 0,
                gap: 2.5,
                panels: Vec::new(),
            });
            continue;
        }
        let Some(c) = cur.as_mut() else {
            // Keys OUTSIDE any [column] apply to the whole layout.
            // Parsers older than these keys discarded such lines, so a
            // file that carries them still loads everywhere.
            if let Some((k, v)) = line.split_once('=') {
                match k.trim() {
                    // du (default) = min/max at the 1080-line reference,
                    // scaled with the window height; px = literal pixels.
                    "units" => units_px = v.trim().eq_ignore_ascii_case("px"),
                    // Page padding per side, % of the window width — the
                    // instrument variant's clear outer margin (u1 §4.3).
                    "pad_x" => {
                        pad_x = v
                            .trim()
                            .parse::<f32>()
                            .ok()
                            .filter(|x| x.is_finite() && *x >= 0.0)
                    }
                    // "screen" is base metadata of the legacy format;
                    // anything else outside a column is simply not ours.
                    _ => {}
                }
            }
            continue;
        };
        let Some((k, v)) = line.split_once('=') else { continue };
        let (k, v) = (k.trim(), v.trim());
        // Reject non-finite ("nan"/"inf") so they never reach the layout
        // engine, where NaN would produce off-screen/garbled geometry.
        let num = |v: &str| {
            v.trim_end_matches(['%', 'p', 'x'])
                .trim()
                .parse::<f32>()
                .ok()
                .filter(|x| x.is_finite())
        };
        match k {
            "basis" => c.basis = num(v).unwrap_or(c.basis),
            "min" => c.min = num(v).unwrap_or(c.min),
            "max" => c.max = num(v).unwrap_or(c.max),
            "grow" => c.grow = num(v).unwrap_or(c.grow),
            "collapse" => c.collapse = num(v).unwrap_or(0.0) as u32,
            "gap" => c.gap = num(v).unwrap_or(c.gap),
            "panel" => {
                // "<name> <weight> [ref <vh>] [min <vh>]" — the sizes are
                // the layout's business, not the widget's, so they are
                // written here next to the placement.
                let mut it = v.split_whitespace();
                let name = it.next().unwrap_or("");
                let weight = it
                    .next()
                    .and_then(|t| t.parse::<f32>().ok())
                    .filter(|x| x.is_finite())
                    .unwrap_or(50.0);
                let mut ref_h = 0.0f32;
                let mut min_h = 0.0f32;
                while let Some(key) = it.next() {
                    let val = it
                        .next()
                        .and_then(|t| t.parse::<f32>().ok())
                        .filter(|x| x.is_finite() && *x > 0.0)
                        .unwrap_or(0.0);
                    match key {
                        "ref" => ref_h = val,
                        "min" => min_h = val,
                        other => {
                            eprintln!("nacelle: unknown panel field in .layaut: {other}")
                        }
                    }
                }
                match Panel::from_name(name) {
                    Some(p) => {
                        c.panels.push((p, weight.max(1.0)));
                        if ref_h > 0.0 || min_h > 0.0 {
                            sizes.push((p, ref_h, min_h));
                        }
                    }
                    None => eprintln!("nacelle: unknown panel in .layaut: {name}"),
                }
            }
            other => eprintln!("nacelle: unknown option in .layaut: {other}"),
        }
    }
    if let Some(c) = cur.take() {
        columns.push(c);
    }
    columns.retain(|c| !c.panels.is_empty());
    if columns.is_empty() {
        None
    } else {
        Some((FlexLayaut { columns, units_px, pad_x }, sizes))
    }
}

/// A FlexLayaut back as the flexbox .layaut text it parses from — the
/// shared serializer of flexbox board sections and --print-layaut.
pub fn serialize_flex(out: &mut String, fl: &FlexLayaut, sizes: &[(Panel, f32, f32)]) {
    out.push_str(if fl.units_px { "units = px\n" } else { "units = du\n" });
    if let Some(p) = fl.pad_x {
        out.push_str(&format!("pad_x = {p}\n"));
    }
    for c in &fl.columns {
        out.push_str("\n[column]\n");
        out.push_str(&format!("basis    = {}\n", c.basis));
        out.push_str(&format!("min      = {}\n", c.min));
        if c.max.is_finite() {
            out.push_str(&format!("max      = {}\n", c.max));
        }
        out.push_str(&format!("grow     = {}\n", c.grow));
        out.push_str(&format!("collapse = {}\n", c.collapse));
        out.push_str(&format!("gap      = {}\n", c.gap));
        for (p, wt) in &c.panels {
            out.push_str(&format!("panel = {} {}", p.name(), wt));
            if let Some((_, r, m)) = sizes.iter().rev().find(|(sp, _, _)| sp == p) {
                if *r > 0.0 {
                    out.push_str(&format!(" ref {r}"));
                }
                if *m > 0.0 {
                    out.push_str(&format!(" min {m}"));
                }
            }
            out.push('\n');
        }
    }
}

/// The screen key recorded in a file's base ("screen = 1920x1080@27").
pub fn base_screen_of(base_text: &str) -> Option<(u32, u32, u32)> {
    for line in base_text.lines() {
        let t = line.split('#').next().unwrap_or("").trim();
        if let Some((k, v)) = t.split_once('=') {
            if k.trim() == "screen" {
                return parse_res_header(&format!("[{}]", v.trim()));
            }
        }
    }
    None
}

/// Serializes a FULL layout as the base of a .layaut file, recording
/// the screen it was created on.
pub fn serialize_base(spec: &LayoutSpec, key: (u32, u32, u32)) -> String {
    let mut out = String::from(
        "# nacelle-desktop layout saved from the grid editor.\n\
         # Format: <panel> = x y w h (percent of the window).\n",
    );
    out.push_str(&format!("screen = {}x{}@{}\n", key.0, key.1, key.2));
    for panel in Panel::all() {
        let ps = spec.p(panel);
        out.push_str(&format!(
            "{} = {:.2} {:.2} {:.2} {:.2}\n",
            panel.name(),
            ps.x,
            ps.y,
            ps.w,
            ps.h
        ));
    }
    out
}

pub fn serialize_sections(out: &mut String, sections: &[ResOverride]) {
    for sec in sections {
        out.push('\n');
        out.push_str(&format!("[{}x{}@{}]\n", sec.w, sec.h, sec.diag));
        for (panel, ps) in &sec.panels {
            out.push_str(&format!(
                "{} = {:.2} {:.2} {:.2} {:.2}\n",
                panel.name(),
                ps.x,
                ps.y,
                ps.w,
                ps.h
            ));
        }
    }
}

/// SAVE AS: writes ALL panels into the MAIN section (the base) of a new
/// .layaut file, recording the screen it was created on. Any previous
/// content of the file is replaced.
pub fn parse_screen_key(s: &str) -> Option<(u32, u32, u32)> {
    parse_res_header(&format!("[{}]", s.trim()))
}

/// The effective layout — built-in or file — in .layaut syntax, so a
/// user can see what he is starting from and fork it (--print-layaut).
pub fn print(def: &LayoutDef) -> String {
    let mut out = String::new();
    match &def.base {
        LayoutMode::Flex => {
            out.push_str("# The built-in default layout, written out.\n");
            let builtin;
            let sizes: &[(Panel, f32, f32)] = if def.sizes.is_empty() {
                builtin = crate::flex::builtin_sizes();
                &builtin
            } else {
                &def.sizes
            };
            serialize_flex(&mut out, &crate::flex::default_flex(), sizes);
        }
        LayoutMode::Custom(fl) => serialize_flex(&mut out, fl, &def.sizes),
        LayoutMode::Fixed(spec) => {
            out.push_str(
                "# Legacy fixed layout: <panel> = x y w h, percent of the window at 16:9.\n",
            );
            for panel in Panel::all() {
                let ps = spec.p(panel);
                if ps.x >= 100.0 {
                    continue;
                }
                out.push_str(&format!(
                    "{} = {:.2} {:.2} {:.2} {:.2}\n",
                    panel.name(),
                    ps.x,
                    ps.y,
                    ps.w,
                    ps.h
                ));
            }
        }
    }
    serialize_sections(&mut out, &def.overrides);
    serialize_boards(&mut out, &def.boards);
    out
}

/// Screen diagonal in inches of the monitor with the given connector
pub fn parse_fixed(src: &str) -> LayoutSpec {
    let mut spec = LayoutSpec::default();
    for line in src.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else { continue };
        let nums: Vec<f32> = v
            .split_whitespace()
            .filter_map(|t| {
                t.trim_end_matches("vw")
                    .trim_end_matches("vh")
                    .parse::<f32>()
                    .ok()
                    .filter(|x| x.is_finite())
            })
            .collect();
        if nums.len() != 4 {
            continue;
        }
        let p = PanelSpec {
            x: nums[0],
            y: nums[1],
            w: nums[2],
            h: nums[3],
        };
        match Panel::from_name(k.trim()) {
            Some(panel) => spec.set(panel, p),
            None => {
                // "screen" is base metadata (the screen the base was
                // created on), not a panel.
                if k.trim() != "screen" {
                    eprintln!("nacelle: unknown panel in .layaut: {}", k.trim());
                }
            }
        }
    }
    spec
}
