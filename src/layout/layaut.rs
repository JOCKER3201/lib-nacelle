//! The `.layaut` FORMAT (u3 §3.2 `layout::layaut`): parse and
//! serialise, nothing else — no filesystem, no configuration.
//!
//! # Version 2: placements carry an identity
//!
//! Version 1 wrote one rectangle per widget — `clock = 1.5 2 16 7` —
//! which said, without meaning to, that a widget can only ever be in
//! one place. Version 2 writes `clock#3 = 1.5 2 16 7`: the same line
//! plus the instance it belongs to, so the file can hold `shell#4` and
//! `shell#9` side by side and the two of them stay two things across a
//! save and a reload. A flexbox column names its entries the same way
//! (`panel = shell#4 60`), and so does a per-screen section, which now
//! moves an instance rather than a widget (`shell#4 = 20 5 60 60`).
//!
//! Two more lines carry the bookkeeping: `version = 2`, which is how a
//! file says which grammar it is written in, and `next_instance = N`,
//! the promise that no identity is ever handed out twice — without it,
//! deleting the last instance and adding another would resurrect its
//! id and with it every stale reference to it.
//!
//! # Version 1 still reads
//!
//! A file with no `version` line is version 1 and is read by the same
//! [`parse`]: every widget it names becomes exactly one instance, in
//! the order the file names them, so the arrangement that comes out is
//! the arrangement that went in. Ids are minted from
//! the file's own order, so an un-migrated file (one installed in a
//! system directory, which the program must not rewrite) reads the
//! same way on every start. Turning one into a version 2 file on disk
//! — once, with a backup — is [`super::store::LayautStore::migrate`].

use super::def::{BoardDef, BoardId, LayoutDef, ResOverride, ScreenKey};
use super::instance::{Instance, InstanceId, InstanceList};
use crate::base::{ColumnItem, FlexColumn, FlexLayaut, LayoutMode, Panel, PanelSpec};

/// The grammar this module writes. A file without a `version` line is
/// version 1 — the format before placements had identities.
pub const FORMAT_VERSION: u32 = 2;

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

/// A line with its comment stripped and its edges trimmed.
fn clean(line: &str) -> &str {
    line.split('#').next().unwrap_or("").trim()
}

/// The same, for a line that may legitimately contain a `#` because a
/// placement carries one. Only a `#` that starts a WORD opens a
/// comment; `clock#3` is a placement, ` # note` is a comment.
fn clean_placement(line: &str) -> &str {
    let bytes = line.as_bytes();
    let mut cut = line.len();
    for (i, b) in bytes.iter().enumerate() {
        if *b == b'#' && (i == 0 || bytes[i - 1].is_ascii_whitespace()) {
            cut = i;
            break;
        }
    }
    line[..cut].trim()
}

/// A placement token: `<widget>` in a version 1 file, `<widget>#<id>`
/// in a version 2 one. None when this installation has no such widget.
fn parse_ref(tok: &str) -> Option<(Panel, Option<InstanceId>)> {
    let (name, id) = match tok.split_once('#') {
        Some((n, i)) => (
            n.trim(),
            i.trim().parse::<u32>().ok().filter(|n| *n > 0).map(InstanceId::new),
        ),
        None => (tok.trim(), None),
    };
    Panel::from_name(name).map(|p| (p, id))
}

/// Four finite numbers as a rectangle in vw/vh.
fn parse_spec(v: &str) -> Option<PanelSpec> {
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
    (nums.len() == 4).then(|| PanelSpec { x: nums[0], y: nums[1], w: nums[2], h: nums[3] })
}

/// Puts a placement into the list: at the identity the file gave it, or
/// at a fresh one when the file is version 1 (or lost the id). Returns
/// the identity the instance ended up with.
fn place(
    insts: &mut InstanceList,
    widget: Panel,
    id: Option<InstanceId>,
    board: BoardId,
    rect: Option<PanelSpec>,
) -> InstanceId {
    match id {
        Some(id) if insts.restore(Instance { id, widget, board, rect }) => id,
        // A duplicated id in a hand-edited file is not a reason to lose
        // the placement: it gets a fresh identity and stays on the
        // board, and the next save writes the file back consistent.
        _ => insts.add(widget, board, rect),
    }
}

/// A raw split of the file: the base text, the per-screen sections and
/// the board sections, each still as text. What the store keeps when it
/// rewrites one part of a file and must not disturb the others.
pub fn split_raw(text: &str) -> (String, Vec<(ScreenKey, String)>, Vec<(BoardId, String)>) {
    let mut base = String::new();
    let mut screens: Vec<(ScreenKey, String)> = Vec::new();
    let mut boards: Vec<(BoardId, String)> = Vec::new();
    // Which section the lines are currently falling into.
    enum Cur {
        Base,
        Screen(usize),
        Board(usize),
    }
    let mut cur = Cur::Base;
    for line in text.lines() {
        let trimmed = clean(line);
        if let Some(k) = parse_res_header(trimmed) {
            screens.push((k, String::new()));
            cur = Cur::Screen(screens.len() - 1);
            continue;
        }
        if let Some(k) = parse_board_header(trimmed) {
            // The header alone is a board: an empty one, but a place.
            boards.push((k, String::new()));
            cur = Cur::Board(boards.len() - 1);
            continue;
        }
        let sink = match cur {
            Cur::Base => &mut base,
            Cur::Screen(i) => &mut screens[i].1,
            Cur::Board(i) => &mut boards[i].1,
        };
        sink.push_str(line);
        sink.push('\n');
    }
    (base, screens, boards)
}

/// The value of a bare `key = <number>` in a section's text, ignoring
/// anything inside a `[column]`. `version` and `next_instance` are
/// written in the base and nowhere else.
fn base_number(src: &str, key: &str) -> Option<u32> {
    let mut in_column = false;
    for line in src.lines() {
        let t = clean(line);
        if t.eq_ignore_ascii_case("[column]") {
            in_column = true;
            continue;
        }
        if in_column {
            continue;
        }
        if let Some((k, v)) = t.split_once('=') {
            if k.trim().eq_ignore_ascii_case(key) {
                return v.trim().parse::<u32>().ok();
            }
        }
    }
    None
}

/// Which grammar a file is written in. No `version` line = version 1,
/// the format before placements had identities.
pub fn version_of(text: &str) -> u32 {
    base_number(&split_raw(text).0, "version").unwrap_or(1)
}

/// Whether this text still has to be migrated to the current grammar.
pub fn is_legacy(text: &str) -> bool {
    version_of(text) < FORMAT_VERSION
}

/// The rectangle placements of one section, in file order, each put
/// into the list on the given board.
fn read_rects(src: &str, board: BoardId, insts: &mut InstanceList) -> usize {
    let mut n = 0;
    for line in src.lines() {
        let line = clean_placement(line);
        if line.is_empty() {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else { continue };
        let key = k.trim();
        let Some(spec) = parse_spec(v) else {
            // Four finite numbers or nothing: "nan inf 3" is not a
            // rectangle, and must never reach the layout engine.
            if parse_ref(key).is_some() {
                eprintln!("nacelle: malformed rectangle in .layaut: {key}");
            }
            continue;
        };
        match parse_ref(key) {
            Some((p, id)) => {
                place(insts, p, id, board, Some(spec));
                n += 1;
            }
            None => {
                // The bookkeeping keys are not placements.
                if !matches!(
                    key.to_ascii_lowercase().as_str(),
                    "screen" | "version" | "next_instance" | "units" | "pad_x"
                ) {
                    eprintln!("nacelle: unknown panel in .layaut: {key}");
                }
            }
        }
    }
    n
}

/// Parses the flexbox `.layaut` format of one section, putting every
/// `panel =` line into the list as an instance of the given board.
pub fn parse_flex_into(
    src: &str,
    board: BoardId,
    insts: &mut InstanceList,
) -> Option<(FlexLayaut, Vec<(Panel, f32, f32)>)> {
    let mut columns: Vec<FlexColumn> = Vec::new();
    let mut cur: Option<FlexColumn> = None;
    let mut sizes: Vec<(Panel, f32, f32)> = Vec::new();
    let mut units_px = false;
    let mut pad_x: Option<f32> = None;
    for line in src.lines() {
        let line = clean_placement(line);
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
                    // "screen" is base metadata of the legacy format and
                    // "version"/"next_instance" are the bookkeeping;
                    // anything else outside a column is not ours.
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
                // "<widget>[#<id>] <weight> [ref <vh>] [min <vh>]" — the
                // sizes are the layout's business, not the widget's, so
                // they are written here next to the placement.
                let mut it = v.split_whitespace();
                let tok = it.next().unwrap_or("");
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
                match parse_ref(tok) {
                    Some((p, id)) => {
                        let id = place(insts, p, id, board, None);
                        c.panels.push(ColumnItem { id, widget: p, weight: weight.max(1.0) });
                        if ref_h > 0.0 || min_h > 0.0 {
                            sizes.push((p, ref_h, min_h));
                        }
                    }
                    None => eprintln!("nacelle: unknown panel in .layaut: {tok}"),
                }
            }
            other => eprintln!("nacelle: unknown option in .layaut: {other}"),
        }
    }
    if let Some(c) = cur.take() {
        columns.push(c);
    }
    // A column that lost every one of its panels — every widget it named
    // is uninstalled — is not a column any more.
    columns.retain(|c| !c.panels.is_empty());
    if columns.is_empty() {
        None
    } else {
        Some((FlexLayaut { columns, units_px, pad_x }, sizes))
    }
}

/// A board section's text as a definition plus its instances: flexbox
/// when it has columns, rectangles otherwise. A header with nothing
/// under it is an empty rectangle board — a place.
fn parse_board_def(text: &str, k: BoardId, insts: &mut InstanceList) -> BoardDef {
    if text.contains("[column]") {
        if let Some((fl, sizes)) = parse_flex_into(text, k, insts) {
            return BoardDef { base: LayoutMode::Custom(fl), sizes };
        }
    }
    read_rects(text, k, insts);
    BoardDef { base: LayoutMode::Rects, sizes: Vec::new() }
}

/// Parses a complete `.layaut` file of either grammar: version 2 as it
/// is written, version 1 by minting one instance per placement in file
/// order (see the module header).
pub fn parse(text: &str, name: &str) -> LayoutDef {
    let (base_text, screens, board_texts) = split_raw(text);
    let mut insts = InstanceList::new();
    // The counter the file recorded comes FIRST: every id it hands out
    // afterwards has to clear the highest one the file ever used, not
    // just the highest one still in it.
    if let Some(n) = base_number(&base_text, "next_instance") {
        insts.reserve_up_to(n);
    }

    // The base = the HOME board.
    let mut sizes: Vec<(Panel, f32, f32)> = Vec::new();
    let base = if base_text.contains("[column]") {
        match parse_flex_into(&base_text, (0, 0), &mut insts) {
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
    } else if read_rects(&base_text, (0, 0), &mut insts) > 0 {
        LayoutMode::Rects
    } else {
        // Overrides only, or boards only: the generated arrangement is
        // the base.
        LayoutMode::Flex
    };

    // The extra boards. Positions are normalised to be contiguous
    // around the centre: a hand-edited [board 5] with nothing between
    // it and home is simply the first board on the right. Switching
    // walks one neighbour at a time, so gaps would be unreachable.
    let raw_boards: Vec<(BoardId, BoardDef)> = board_texts
        .iter()
        .map(|(k, t)| (*k, parse_board_def(t, *k, &mut insts)))
        .collect();
    let boards = normalize_boards(raw_boards, &mut insts);

    // A generated base has no placements in the file: it is composed
    // out of what is installed, one instance per addon, with identities
    // from the reserved range so that composing it on every load
    // neither collides with a saved id nor moves the file's counter.
    if matches!(base, LayoutMode::Flex) {
        for p in crate::flex::default_instances().iter() {
            insts.add_generated(p.widget, (0, 0), p.widget.idx() as u32);
        }
    }

    let overrides = screens.iter().map(|(k, t)| read_screen(*k, t, &insts)).collect();

    LayoutDef {
        base,
        overrides,
        sizes,
        boards,
        instances: insts,
        base_screen: base_screen_of(&base_text),
    }
}

/// One `[WxH@D]` section. Its lines name a placement exactly as the
/// base does — `shell#4 = 20 5 60 60`. A version 1 line has no id and
/// named a widget, which could only ever have meant the one instance of
/// it on home.
fn read_screen(k: ScreenKey, src: &str, insts: &InstanceList) -> ResOverride {
    let mut rects: Vec<(InstanceId, PanelSpec)> = Vec::new();
    for line in src.lines() {
        let line = clean_placement(line);
        let Some((key, v)) = line.split_once('=') else { continue };
        let Some(spec) = parse_spec(v) else { continue };
        let Some((p, id)) = parse_ref(key.trim()) else { continue };
        let id = match id {
            Some(id) => id,
            None => {
                let Some(i) = insts.on_board((0, 0)).into_iter().find(|i| i.widget == p) else {
                    continue;
                };
                i.id
            }
        };
        rects.retain(|(i, _)| *i != id);
        rects.push((id, spec));
    }
    ResOverride { w: k.0, h: k.1, diag: k.2, rects }
}

/// Renumbers the boards so the arms are contiguous from the centre out,
/// and drags every instance along with the board it stands on.
pub fn normalize_boards(
    boards: Vec<(BoardId, BoardDef)>,
    insts: &mut InstanceList,
) -> Vec<(BoardId, BoardDef)> {
    // Four arms, each renumbered contiguously from the centre out; a
    // diagonal position has no gesture that reaches it and is dropped.
    // The vertical arms hold one board each — the top and the bottom
    // board are fixtures like home, so anything beyond the nearest
    // section is dropped.
    type Sel = dyn Fn(BoardId) -> Option<i32>;
    type Place = dyn Fn(i32) -> BoardId;
    let mut out: Vec<(BoardId, BoardDef)> = Vec::new();
    let mut moved: Vec<(BoardId, BoardId)> = Vec::new();
    let arms: [(&Sel, &Place, usize); 4] = [
        (&|(x, y)| (y == 0 && x < 0).then_some(-x), &|d| (-d, 0), usize::MAX),
        (&|(x, y)| (y == 0 && x > 0).then_some(x), &|d| (d, 0), usize::MAX),
        (&|(x, y)| (x == 0 && y < 0).then_some(-y), &|d| (0, -d), 1),
        (&|(x, y)| (x == 0 && y > 0).then_some(y), &|d| (0, d), 1),
    ];
    for (sel, place, cap) in arms {
        let mut on_arm: Vec<(i32, BoardId, &BoardDef)> = boards
            .iter()
            .filter_map(|(id, def)| sel(*id).map(|d| (d, *id, def)))
            .collect();
        on_arm.sort_by_key(|(d, _, _)| *d);
        for (i, (_, was, def)) in on_arm.into_iter().take(cap).enumerate() {
            let now = place(i as i32 + 1);
            moved.push((was, now));
            out.push((now, def.clone()));
        }
    }
    // Instances follow their board; one whose board was dropped (a
    // diagonal, a second top board) goes with it — the place it stood
    // on does not exist.
    let kept: Vec<BoardId> = moved.iter().map(|(w, _)| *w).collect();
    for k in insts.boards() {
        if k != (0, 0) && !kept.contains(&k) {
            insts.remove_board(k);
        }
    }
    // Two passes through a scratch position, because renumbering can
    // send board 5 to 1 while 1 is still occupied.
    for (was, _) in &moved {
        for i in insts.iter_mut_on(*was) {
            i.board = (was.0, was.1 + 1000);
        }
    }
    for (was, now) in &moved {
        for i in insts.iter_mut_on((was.0, was.1 + 1000)) {
            i.board = *now;
        }
    }
    out.sort_by_key(|(k, _)| *k);
    out
}

/// The `version` and `next_instance` lines a version 2 file opens with.
fn serialize_header(out: &mut String, insts: &InstanceList) {
    out.push_str(&format!("version = {FORMAT_VERSION}\n"));
    out.push_str(&format!("next_instance = {}\n", insts.next_free()));
}

/// Rewrites (or adds) the bookkeeping lines of a preserved base text,
/// so that splicing new boards into an old file cannot leave a stale
/// promise about which ids are still free.
pub fn with_header(base: &str, insts: &InstanceList) -> String {
    let mut out = String::new();
    let mut seen_version = false;
    let mut seen_next = false;
    for line in base.lines() {
        match clean(line).split_once('=').map(|(k, _)| k.trim().to_ascii_lowercase()) {
            Some(k) if k == "version" => {
                seen_version = true;
                out.push_str(&format!("version = {FORMAT_VERSION}\n"));
            }
            Some(k) if k == "next_instance" => {
                seen_next = true;
                out.push_str(&format!("next_instance = {}\n", insts.next_free()));
            }
            _ => {
                out.push_str(line);
                out.push('\n');
            }
        }
    }
    if seen_version && seen_next {
        return out;
    }
    // A missing line is added under the file's opening comment, where a
    // reader looks for it, rather than above the title.
    let mut head = String::new();
    let mut rest = out.as_str();
    while let Some((line, tail)) = rest.split_once('\n') {
        if !line.trim().is_empty() && !line.trim_start().starts_with('#') {
            break;
        }
        head.push_str(line);
        head.push('\n');
        rest = tail;
    }
    if !seen_version {
        head.push_str(&format!("version = {FORMAT_VERSION}\n"));
    }
    if !seen_next {
        head.push_str(&format!("next_instance = {}\n", insts.next_free()));
    }
    head.push_str(rest);
    head
}

/// How a placement names itself in a file: `<widget>#<id>` for a saved
/// instance, and the bare `<widget>` for a COMPOSED one.
///
/// A generated identity is the widget's position in the registry and
/// belongs to no file — writing it down would pin this installation's
/// order into a file that is meant to follow whatever is installed. So
/// the arrangement the program composes is written the way it is known:
/// by name, exactly as version 1 wrote everything.
fn token(widget: Panel, id: InstanceId) -> String {
    if id.is_generated() {
        widget.name().to_string()
    } else {
        format!("{}#{}", widget.name(), id)
    }
}

/// One rectangle line: `<widget>#<id> = x y w h`.
fn serialize_rect(out: &mut String, i: &Instance) {
    let Some(ps) = i.rect else { return };
    out.push_str(&format!(
        "{} = {:.2} {:.2} {:.2} {:.2}\n",
        token(i.widget, i.id),
        ps.x,
        ps.y,
        ps.w,
        ps.h
    ));
}

pub fn serialize_boards(out: &mut String, boards: &[(BoardId, BoardDef)], insts: &InstanceList) {
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
            LayoutMode::Rects => {
                for i in insts.on_board((*x, *y)) {
                    if i.hidden() {
                        continue;
                    }
                    serialize_rect(out, &i);
                }
            }
        }
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
        for it in &c.panels {
            out.push_str(&format!("panel = {} {}", token(it.widget, it.id), it.weight));
            if let Some((_, r, m)) = sizes.iter().rev().find(|(sp, _, _)| *sp == it.widget) {
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
        let t = clean(line);
        if let Some((k, v)) = t.split_once('=') {
            if k.trim() == "screen" {
                return parse_res_header(&format!("[{}]", v.trim()));
            }
        }
    }
    None
}

/// Serializes the HOME board as the base of a `.layaut` file, recording
/// the screen it was created on. The instances of the other boards ride
/// in their own sections, so only home's are written here.
pub fn serialize_base(insts: &InstanceList, key: (u32, u32, u32)) -> String {
    let mut out = String::from(
        "# nacelle-desktop layout saved from the grid editor.\n\
         # Format: <widget>#<instance> = x y w h (percent of the window).\n",
    );
    serialize_header(&mut out, insts);
    out.push_str(&format!("screen = {}x{}@{}\n", key.0, key.1, key.2));
    for i in insts.on_board((0, 0)) {
        serialize_rect(&mut out, &i);
    }
    out
}

pub fn parse_screen_key(s: &str) -> Option<(u32, u32, u32)> {
    parse_res_header(&format!("[{}]", s.trim()))
}

pub fn serialize_sections(out: &mut String, sections: &[ResOverride], insts: &InstanceList) {
    for sec in sections {
        out.push('\n');
        out.push_str(&format!("[{}x{}@{}]\n", sec.w, sec.h, sec.diag));
        for (id, ps) in &sec.rects {
            // An override for an instance the layout no longer places
            // is dropped rather than written back nameless: the widget
            // it moved is gone, and so is the reason for the line.
            let Some(i) = insts.get(*id) else { continue };
            out.push_str(&format!(
                "{} = {:.2} {:.2} {:.2} {:.2}\n",
                token(i.widget, *id),
                ps.x,
                ps.y,
                ps.w,
                ps.h
            ));
        }
    }
}

/// The effective layout — built-in or file — in `.layaut` syntax, so a
/// user can see what he is starting from and fork it (--print-layaut).
pub fn print(def: &LayoutDef) -> String {
    let mut out = String::new();
    serialize_header(&mut out, &def.instances);
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
            serialize_flex(&mut out, &crate::flex::compose(&def.board_instances((0, 0))), sizes);
        }
        LayoutMode::Custom(fl) => serialize_flex(&mut out, fl, &def.sizes),
        LayoutMode::Rects => {
            out.push_str(
                "# Rectangle board: <widget>#<instance> = x y w h, percent of the window.\n",
            );
            for i in def.board_instances((0, 0)) {
                if i.hidden() {
                    continue;
                }
                serialize_rect(&mut out, &i);
            }
        }
    }
    serialize_sections(&mut out, &def.overrides, &def.instances);
    serialize_boards(&mut out, &def.boards, &def.instances);
    out
}

/// The canonical version 2 text of a layout — what the store writes.
///
/// Unlike [`print`], a GENERATED base stays generated: it writes no
/// placements for home, so the file keeps following whatever addons
/// the machine has installed instead of freezing today's list into it.
pub fn write_file(def: &LayoutDef) -> String {
    let mut out = String::from("# nacelle layout.\n");
    serialize_header(&mut out, &def.instances);
    if let Some(k) = def.base_screen {
        out.push_str(&format!("screen = {}x{}@{}\n", k.0, k.1, k.2));
    }
    match &def.base {
        LayoutMode::Flex => {}
        LayoutMode::Custom(fl) => serialize_flex(&mut out, fl, &def.sizes),
        LayoutMode::Rects => {
            for i in def.board_instances((0, 0)) {
                if i.hidden() {
                    continue;
                }
                serialize_rect(&mut out, &i);
            }
        }
    }
    serialize_sections(&mut out, &def.overrides, &def.instances);
    serialize_boards(&mut out, &def.boards, &def.instances);
    out
}
