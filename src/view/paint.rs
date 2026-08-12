//! The drawing vocabulary the views share, written once against
//! [`Surface`].
//!
//! Everything here used to live in `ui.rs` against `Ctx` and is now
//! reached from both sides of the plugin boundary. The port is
//! mechanical and deliberately so: `t.px(tok(&CELL, "table.cell_pad"))`
//! became `sf.px("table.cell_pad")`, which resolves the same token
//! through the same engine, so the host draws the same pixels it drew
//! before. `ui::meter` and `ui::badge` are now one-line wrappers around
//! [`meter`] and [`badge`] — there is one implementation of each, which
//! is the point.
//!
//! The cost of naming tokens by string is paid ONCE per draw: a view
//! reads its metrics into a `Look` struct before its row loop, never
//! inside it.

use super::surface::{StateInk, Surface};
use crate::theme::parse::State;
use crate::theme::Color;
use crate::ui::{sev_of, Align, BadgeStyle, Sev, SEVERITY_ROLES};
use crate::Rect;

// ------------------------------------------------------------- severity

/// The severity a word outside the closed set resolves to:
/// `script.severity_fallback`, which §5.10 forbids ever being `ok`.
pub fn sev_fallback(sf: &mut impl Surface) -> Sev {
    let word = sf.word("script.severity_fallback");
    sev_of(&word).unwrap_or(Sev(6))
}

fn sev_role(s: Sev) -> &'static str {
    SEVERITY_ROLES[(s.0 as usize).min(SEVERITY_ROLES.len() - 1)]
}

/// The ink a severity writes in — the label, the value, the status word.
pub fn sev_text(sf: &mut impl Surface, s: Sev) -> Color {
    sf.color(&format!("severity.{}.text", sev_role(s)))
}

/// The hairline a severity draws around a hollow pill.
pub fn sev_edge(sf: &mut impl Surface, s: Sev) -> Color {
    sf.color(&format!("severity.{}.edge", sev_role(s)))
}

/// The bed a severity fills a hollow pill with.
pub fn sev_fill(sf: &mut impl Surface, s: Sev) -> Color {
    sf.bed(&format!("severity.{}.fill", sev_role(s)))
}

/// The ink that reads ON a severity's solid fill.
pub fn sev_on(sf: &mut impl Surface, s: Sev) -> Color {
    sf.color(&format!("severity.{}.on", sev_role(s)))
}

// ----------------------------------------------------------- type roles

/// One `type.*` role, resolved for the panel being drawn.
///
/// Read once per draw and carried into the row loop, exactly as the file
/// panel's `Look::read` does: the role is four token lookups and a row
/// loop must not repeat them.
#[derive(Clone, Copy, Debug)]
pub struct RoleLook {
    /// Size in device px, at the panel scale and the stack's shrink.
    pub px: f32,
    /// Letter spacing in px for a run at that size.
    pub track: f32,
    /// Line height as a multiple of `px`.
    pub leading: f32,
    /// The role's own ink: `fg` at its constant alpha.
    pub color: Color,
}

/// Resolves a type role by name. An unknown role falls back to `body`,
/// the rule `script.text_role` has always followed: a typo must stay
/// readable rather than vanish.
pub fn role_look(sf: &mut impl Surface, name: &str, shrink: f32) -> RoleLook {
    let name = if sf.has_token(&format!("type.{name}.size")) {
        name
    } else {
        // Said once, exactly as `ui::role` says it: a typo in a theme or
        // a script is worth one line and not sixty a second.
        crate::ui::warn_once(
            &format!("role:{name}"),
            &format!("unknown type role \"{name}\" — falling back to body"),
        );
        "body"
    };
    let px = (sf.px(&format!("type.{name}.size")) * sf.scale() * shrink)
        .max(sf.px("type.min_px"));
    // Tracking tokens are em — a fraction of the run's own size.
    let track = px * sf.px(&format!("type.{name}.tracking"));
    let leading = sf.px(&format!("type.{name}.leading"));
    let mut color = sf.color(&format!("type.{name}.fg"));
    let alpha = sf.px(&format!("type.{name}.alpha"));
    color.a *= if alpha > 0.0 { alpha.min(1.0) } else { 1.0 };
    RoleLook { px, track, leading: if leading > 0.0 { leading } else { 1.0 }, color }
}

/// The role a `*_role` binding token names — `script.table_head_role`,
/// `list.label_role`. A binding resolving to nothing is `body`.
pub fn bound_role(sf: &mut impl Surface, binding: &str, shrink: f32) -> RoleLook {
    let word = sf.word(binding);
    role_look(sf, if word.is_empty() { "body" } else { &word }, shrink)
}

// --------------------------------------------------------------- text

/// Trims text with a trailing ellipsis so it fits `max_w`, measured at
/// the SAME letter tracking the caller draws with.
///
/// Measuring at a different tracking is how a content-measured table
/// column came to ellipsise the very cell it was sized from.
pub fn fit_end(sf: &mut impl Surface, px: f32, text: &str, max_w: f32, track: f32) -> String {
    if sf.measure(px, text, track) <= max_w {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let mut n = chars.len().saturating_sub(1);
    while n > 1 {
        let cand: String = chars[..n].iter().collect::<String>() + "\u{2026}";
        if sf.measure(px, &cand, track) <= max_w {
            return cand;
        }
        n -= 1;
    }
    "\u{2026}".to_string()
}

/// The tooltip a TRIMMED label files (F2 §8.1): `shown` is what reached
/// the screen, `full` is what there was, and the difference between them
/// is the whole reason to say anything.
///
/// Nothing happens when the two are equal — a tooltip repeating what is
/// already legible is noise — and nothing happens when the pointer is
/// somewhere else, which is checked HERE so the string comparison is the
/// only work a row the pointer is nowhere near ever does.
///
/// One sentence, one place. A tab, a segment, a table heading and a
/// table cell each wrote it out, and the list was about to be the fifth;
/// the rule they were all writing is this function.
pub fn explain_trim(sf: &mut impl Surface, id: u64, anchor: Rect, shown: &str, full: &str) {
    if shown == full {
        return;
    }
    let (mx, my) = sf.mouse();
    if !anchor.contains(mx, my) {
        return;
    }
    sf.tooltip(id, anchor, full);
}

/// Breaks text into lines no wider than `max_w`, measured at the SAME
/// letter tracking the caller draws with.
///
/// The first text breaking in the toolkit — everything before it either
/// fitted or ellipsised ([`fit_end`]). Greedy by words, which is what a
/// tooltip and a label want: a word starts a new line when it no longer
/// fits, and a single word wider than the whole box is broken by
/// characters rather than allowed to overflow. Explicit newlines in the
/// text are kept; an empty `max_w` (or one narrower than a character)
/// answers one line per source line, unbroken, so a nonsense width
/// degrades to "no wrapping" instead of to an endless loop.
pub fn wrap(sf: &mut impl Surface, px: f32, text: &str, max_w: f32, track: f32) -> Vec<String> {
    let mut out = Vec::new();
    for para in text.split('\n') {
        if max_w <= 0.0 {
            out.push(para.to_string());
            continue;
        }
        let mut line = String::new();
        for word in para.split_whitespace() {
            let cand = if line.is_empty() {
                word.to_string()
            } else {
                format!("{line} {word}")
            };
            if sf.measure(px, &cand, track) <= max_w {
                line = cand;
                continue;
            }
            if !line.is_empty() {
                out.push(std::mem::take(&mut line));
            }
            // The word alone on its line: kept whole when it fits,
            // broken by characters when nothing else can be done.
            if sf.measure(px, word, track) <= max_w {
                line = word.to_string();
                continue;
            }
            let mut piece = String::new();
            for ch in word.chars() {
                let mut cand = piece.clone();
                cand.push(ch);
                if !piece.is_empty() && sf.measure(px, &cand, track) > max_w {
                    out.push(std::mem::take(&mut piece));
                    piece.push(ch);
                } else {
                    piece = cand;
                }
            }
            line = piece;
        }
        out.push(line);
    }
    out
}

/// Top of a single line centred in a box of `box_h`. The line occupies
/// its role's leading; in optical mode the cap-height bias nudges it.
pub fn center_line_y(sf: &mut impl Surface, y: f32, box_h: f32, px: f32, leading: f32) -> f32 {
    let mut ty = y + (box_h - px * leading) / 2.0;
    if sf.enum_is("rhythm.center_mode", "optical") {
        ty += px * sf.px("rhythm.cap_center_bias");
    }
    ty
}

/// One aligned run inside a cell of width `w` starting at `x`.
#[allow(clippy::too_many_arguments)]
pub fn cell_text(
    sf: &mut impl Surface,
    x: f32,
    y: f32,
    w: f32,
    align: Align,
    px: f32,
    text: &str,
    color: Color,
    track: f32,
) {
    let tx = match align {
        Align::Left => x,
        Align::Center => x + w / 2.0,
        Align::Right => x + w,
    };
    sf.text(px, tx, y, text, color, track, align);
}

/// The number at the front of a formatted cell (`"41.2%"` → 41.2), for a
/// bar reading the value it also prints. `None` when the text does not
/// start with one — a bar of nothing is drawn empty, never invented.
pub fn leading_number(text: &str) -> Option<f32> {
    let end = text
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-'))
        .unwrap_or(text.len());
    text[..end].parse::<f32>().ok().filter(|v| v.is_finite())
}

// -------------------------------------------------------------- shapes

/// The framed bar: a recessed track and a fill to `frac`.
///
/// A severity is the model's judgement of the DATA (an index into the
/// closed set, not a colour) and tints the fill; `track = false` says the
/// value has no meaningful whole, so no outline claims one.
pub fn meter(sf: &mut impl Surface, r: Rect, frac: f32, sev: Option<Sev>, track: bool) {
    let frac = if frac.is_finite() { frac.clamp(0.0, 1.0) } else { 0.0 };
    let bw = sf.px("progress.border");
    // The fill sits `progress.inset` behind the ring, so it never
    // touches it.
    let inset = bw + sf.px("progress.inset");
    if track {
        let c = sf.color("component.bar.track");
        sf.rect_outline(r, bw, c);
    }
    let inner = (r.w - 2.0 * inset).max(0.0);
    let fill = match sev {
        Some(s) => sev_text(sf, s),
        None => sf.color("component.bar.fill"),
    };
    sf.rect(
        Rect::new(r.x + inset, r.y + inset, inner * frac, (r.h - 2.0 * inset).max(0.0)),
        fill,
    );
}

/// The CRITICAL / CONTAINED pill: a filled, ringed capsule around a
/// short text, its four colours from the severity at draw time. Returns
/// the pill's width.
///
/// The corner honours `badge.corner` as far as the surface can: a
/// positive radius cuts a chamfer where there is one to cut, and
/// degrades to square where there is not (the ABI has no chamfer) —
/// which is the look `badge.corner = 0` already asks for.
pub fn badge(
    sf: &mut impl Surface,
    r: Rect,
    text: &str,
    sev: Option<Sev>,
    style: BadgeStyle,
    align: Align,
    shrink: f32,
) -> f32 {
    let role = bound_role(sf, "script.badge_role", shrink);
    let tw = sf.measure(role.px, text, role.track);
    let pad = sf.px("badge.pad_x") * shrink;
    let h = (sf.px("badge.h") * shrink).min(r.h).max(1.0);
    let w = (tw + 2.0 * pad).min(r.w).max(1.0);
    let x = match align {
        Align::Left => r.x,
        Align::Center => r.x + (r.w - w) / 2.0,
        Align::Right => r.right() - w,
    };
    let y = r.y + (r.h - h) / 2.0;
    let solid = match style {
        BadgeStyle::Solid => true,
        BadgeStyle::Hollow => false,
        BadgeStyle::FromTheme => match sev {
            Some(s) if sf.flag("badge.style_from_severity") => {
                sf.word(&format!("severity.{}.badge_style", sev_role(s))) == "solid"
            }
            _ => false,
        },
    };
    let (fill, edge, ink) = match (sev, solid) {
        (Some(s), true) => (sev_text(sf, s), sev_text(sf, s), sev_on(sf, s)),
        (Some(s), false) => (sev_fill(sf, s), sev_edge(sf, s), sev_text(sf, s)),
        (None, true) => (
            sf.bed("component.badge.solid_fill"),
            sf.bed("component.badge.solid_fill"),
            sf.color("component.badge.solid_text"),
        ),
        (None, false) => (
            sf.bed("component.badge.fill"),
            sf.color("component.badge.edge"),
            sf.color("component.badge.text"),
        ),
    };
    let pill = Rect::new(x, y, w, h);
    let corner = sf.px("badge.corner");
    let bw = sf.px("badge.border");
    if corner > 0.0 {
        let cut = corner.min(h / 2.0) * shrink;
        sf.chamfer_fill(pill, cut, fill);
        if bw > 0.0 && !solid {
            sf.chamfer_frame(pill, cut, bw, edge);
        }
    } else {
        // `pill` is a negative sentinel until R5 lands: square it is.
        sf.rect(pill, fill);
        if bw > 0.0 && !solid {
            sf.rect_outline(pill, bw, edge);
        }
    }
    let ty = center_line_y(sf, y, h, role.px, role.leading);
    sf.text(role.px, x + w / 2.0, ty, text, ink, role.track, Align::Center);
    w
}

/// The little triangle beside a sorted heading: point up for ascending,
/// down for descending. An outline, drawn with the polyline every icon in
/// this project is drawn with, so it inherits the same hairline.
pub fn sort_marker(
    sf: &mut impl Surface,
    x: f32,
    y: f32,
    size: f32,
    line_px: f32,
    dir: super::SortDir,
    color: Color,
) {
    if size <= 0.0 {
        return;
    }
    // Centred on the heading's own type size — the same optical guess the
    // rest of this vocabulary makes until a shared centring primitive
    // exists.
    let top = y + (line_px - size).max(0.0) / 2.0;
    let half = size / 2.0;
    let pts = match dir {
        super::SortDir::Asc => [[x + half, top], [x + size, top + size], [x, top + size]],
        super::SortDir::Desc => [[x, top], [x + size, top], [x + half, top + size]],
    };
    let hair = sf.px("stroke.hair");
    sf.polyline(&pts, hair, color, true);
}

/// The expander beside a tree row: a triangle pointing right when the
/// node is closed and down when it is open.
///
/// The state turns the GLYPH, not its colour: rotation is geometry, and
/// geometry is the one thing a theme does not have to say twice.
pub fn disclosure(
    sf: &mut impl Surface,
    x: f32,
    y: f32,
    size: f32,
    line_px: f32,
    expanded: bool,
    color: Color,
) {
    if size <= 0.0 {
        return;
    }
    let top = y + (line_px - size).max(0.0) / 2.0;
    let half = size / 2.0;
    let pts = if expanded {
        [[x, top], [x + size, top], [x + half, top + size]]
    } else {
        [[x, top], [x + size, top + half], [x, top + size]]
    };
    let hair = sf.px("stroke.hair");
    sf.polyline(&pts, hair, color, true);
}

/// The scrollbar, from the geometry [`super::scroll::scrollbar`] worked
/// out: the groove when the theme asks for one, then the thumb on the
/// `scrollbar.thumb` class's ladder.
pub fn scrollbar(
    sf: &mut impl Surface,
    geom: &super::scroll::ScrollbarGeom,
    alpha: f32,
    hovered: bool,
    dragging: bool,
) {
    if alpha <= 0.0 {
        return;
    }
    if sf.enum_is("scrollbar.track", "on") {
        let mut c = sf.bed("component.scrollbar.track");
        c.a *= alpha;
        sf.rect(geom.track, c);
    }
    let rung = if dragging {
        State::Dragging
    } else if hovered {
        State::Hover
    } else {
        State::Idle
    };
    let style: StateInk = sf.class_state("scrollbar.thumb", rung);
    let mut fill = style.fill;
    fill.a *= alpha;
    if fill.a > 0.0 {
        sf.rect(geom.thumb, fill);
    }
    let mut edge = style.edge;
    edge.a *= alpha;
    if style.edge_width > 0.0 && edge.a > 0.0 {
        sf.rect_outline(geom.thumb, style.edge_width, edge);
    }
}

// ---------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// A surface that only measures: half an em a character, which is
    /// wrong about fonts and right about monotonicity — all the breaking
    /// arithmetic asks of it. Nothing here draws.
    struct Ruler;

    impl Surface for Ruler {
        fn rect(&mut self, _r: Rect, _c: Color) {}
        fn rect_outline(&mut self, _r: Rect, _w: f32, _c: Color) {}
        fn line(&mut self, _x0: f32, _y0: f32, _x1: f32, _y1: f32, _w: f32, _c: Color) {}
        fn polyline(&mut self, _p: &[[f32; 2]], _w: f32, _c: Color, _closed: bool) {}
        fn text(&mut self, _px: f32, _x: f32, _y: f32, _s: &str, _c: Color, _t: f32, _a: Align) {}
        fn measure(&mut self, px: f32, s: &str, _track: f32) -> f32 {
            s.chars().count() as f32 * px * 0.5
        }
        fn clip(&mut self, _r: Rect) -> bool {
            false
        }
        fn unclip(&mut self) {}
        fn has_token(&mut self, _name: &str) -> bool {
            false
        }
        fn px(&mut self, _name: &str) -> f32 {
            0.0
        }
        fn color(&mut self, _name: &str) -> Color {
            Color { r: 0.0, g: 0.0, b: 0.0, a: 1.0 }
        }
        fn bed(&mut self, _name: &str) -> Color {
            Color { r: 0.0, g: 0.0, b: 0.0, a: 1.0 }
        }
        fn flag(&mut self, _name: &str) -> bool {
            false
        }
        fn word(&mut self, _name: &str) -> String {
            String::new()
        }
        fn class_state(&mut self, _class: &str, _state: State) -> StateInk {
            StateInk::raw()
        }
        fn epoch(&mut self) -> u32 {
            0
        }
        fn now(&self) -> f64 {
            0.0
        }
        fn mouse(&self) -> (f32, f32) {
            (0.0, 0.0)
        }
        fn scale(&self) -> f32 {
            1.0
        }
    }

    // ---- wrapping ----

    #[test]
    fn text_that_fits_is_one_line_and_keeps_its_spacing_rules() {
        // 10 characters at 10 px = 50 px wide.
        assert_eq!(wrap(&mut Ruler, 10.0, "abcdefghij", 50.0, 0.0), ["abcdefghij"]);
        assert_eq!(wrap(&mut Ruler, 10.0, "", 50.0, 0.0), [""]);
    }

    #[test]
    fn a_line_breaks_at_the_last_word_that_fits() {
        // At 10 px a character is 5 px wide: 35 px holds "one two".
        assert_eq!(wrap(&mut Ruler, 10.0, "one two three", 35.0, 0.0), ["one two", "three"]);
        // 30 px does not, so every word gets its own line.
        assert_eq!(
            wrap(&mut Ruler, 10.0, "one two three", 30.0, 0.0),
            ["one", "two", "three"]
        );
    }

    #[test]
    fn a_word_wider_than_the_box_is_broken_rather_than_left_hanging() {
        assert_eq!(wrap(&mut Ruler, 10.0, "abcdefghij", 25.0, 0.0), ["abcde", "fghij"]);
        // A short word before it still gets its own line first.
        assert_eq!(wrap(&mut Ruler, 10.0, "x abcdef", 25.0, 0.0), ["x", "abcde", "f"]);
    }

    #[test]
    fn explicit_newlines_are_kept_and_a_nonsense_width_stops_wrapping() {
        assert_eq!(wrap(&mut Ruler, 10.0, "one\ntwo", 500.0, 0.0), ["one", "two"]);
        // Zero width would otherwise break every character forever.
        assert_eq!(wrap(&mut Ruler, 10.0, "one two", 0.0, 0.0), ["one two"]);
    }

    #[test]
    fn the_leading_number_is_read_and_never_invented() {
        assert_eq!(leading_number("41.2%"), Some(41.2));
        assert_eq!(leading_number("-3 of 4"), Some(-3.0));
        assert_eq!(leading_number("firefox"), None);
        assert_eq!(leading_number(""), None);
        assert_eq!(leading_number("..."), None);
    }

    // ---- the rule every trimmed label follows ----

    use crate::view::surface::tests::FakeSurface;

    const FULL: &str = "org.freedesktop.NetworkManager";
    const CUT: &str = "org.freedesk\u{2026}";

    fn anchor() -> Rect {
        Rect::new(10.0, 10.0, 100.0, 20.0)
    }

    #[test]
    fn a_label_the_ellipsis_cut_short_asks_for_the_whole_of_itself() {
        let mut sf = FakeSurface::new().at(20.0, 15.0);
        explain_trim(&mut sf, 7, anchor(), CUT, FULL);
        assert_eq!(sf.tips.len(), 1);
        let (id, r, text) = &sf.tips[0];
        assert_eq!(*id, 7, "the caller's identity, untouched");
        assert_eq!(text, FULL, "the whole of it — the stump is already on screen");
        // The anchor comes back as it went in: the box is placed against
        // what was pointed at, not against the pointer alone.
        assert_eq!((r.x, r.y, r.w, r.h), (10.0, 10.0, 100.0, 20.0));
    }

    #[test]
    fn a_label_that_arrived_whole_says_nothing() {
        // The pointer is resting on it, and there is still nothing to
        // add: a tooltip repeating what is already legible is noise.
        let mut sf = FakeSurface::new().at(20.0, 15.0);
        explain_trim(&mut sf, 7, anchor(), FULL, FULL);
        assert!(sf.tips.is_empty());
        // The empty label is the same case and not a special one.
        explain_trim(&mut sf, 7, anchor(), "", "");
        assert!(sf.tips.is_empty());
    }

    #[test]
    fn a_trimmed_label_the_pointer_left_asks_for_nothing() {
        // Off the anchor entirely: this is how a tooltip goes away —
        // the frame files no request, and the manager disarms.
        let mut sf = FakeSurface::new().at(200.0, 15.0);
        explain_trim(&mut sf, 7, anchor(), CUT, FULL);
        assert!(sf.tips.is_empty());
        // The far edges are the containment rule's, not a second one:
        // the top-left corner is inside, the bottom-right is not.
        let mut on = FakeSurface::new().at(10.0, 10.0);
        explain_trim(&mut on, 7, anchor(), CUT, FULL);
        assert_eq!(on.tips.len(), 1);
        let mut off = FakeSurface::new().at(110.0, 30.0);
        explain_trim(&mut off, 7, anchor(), CUT, FULL);
        assert!(off.tips.is_empty());
    }
}
