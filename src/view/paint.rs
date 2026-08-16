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
use crate::draw::CornerStyle;
use crate::theme::parse::State;
use crate::theme::Color;
use crate::ui::{sev_of, Align, BadgeStyle, Sev, SEVERITY_ROLES};
use crate::Rect;

// ------------------------------------------------------------- severity

/// The severity a word outside the closed set resolves to:
/// `script.severity_fallback`, which §5.10 forbids ever being `ok`.
pub fn sev_fallback(sf: &mut impl Surface) -> Sev {
    let word = sf.word("script.severity_fallback");
    // The same answer, from the same place, as [`crate::ui::sev_fallback`]
    // — a plugin's table and the host's must not judge one reading two
    // ways, which is what two copies of the rule always end up doing.
    sev_of(&word).unwrap_or_else(|| crate::ui::unnamed_severity(&word))
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
    /// Whether this role sets its figures on a fixed advance (§5.16's
    /// `tabular`). Carried as the ROLE's bool and handed to
    /// [`Surface::text_tab`], which measures the box from the face —
    /// this side of the library owns tokens, not faces.
    pub tabular: bool,
    /// The slot `type.<role>.face` names — the family AND the weight this
    /// role is set in, since the master declares both on the face block.
    ///
    /// This field is why the struct exists in the shape it does: a look
    /// read once per draw is only worth having if it carries everything
    /// the row loop needs, and the row loop was writing `FONT_UI` because
    /// the face was the one thing it could not get from here.
    pub face: u8,
    /// The role's own ink: `fg` at its constant alpha.
    pub color: Color,
}

/// The look of a role the master does not declare. There is no spare
/// role and there must not be one: a role is twelve tokens, so a single
/// spare word hides a whole ladder behind a name nobody wrote, and `body`
/// — the obvious candidate — is a REAL role of plausible size, which
/// renders a broken theme as a nearly-right interface and lets it ship.
///
/// Nothing is drawn in it: zero px, no leading, no ink. The same ruling
/// [`crate::ui::role`] makes for the objects that draw against `Ctx`,
/// made once more here because this is the resolver every view, every
/// script table and the whole ABI side goes through.
pub const NO_ROLE: RoleLook = RoleLook {
    px: 0.0,
    track: 0.0,
    leading: 0.0,
    tabular: false,
    // The interface slot, which is where an undesigned run has always
    // landed. Nothing is drawn in this look anyway — px is zero — so the
    // face is the one field here that cannot decide anything.
    face: crate::font::FONT_UI,
    color: Color::TRANSPARENT,
};

/// Resolves a type role by name. A name no `type.*` block declares warns
/// once and answers [`NO_ROLE`]: naming a role the theme does not have is
/// a defect to report, never a decision about how the text should look.
pub fn role_look(sf: &mut impl Surface, name: &str, shrink: f32) -> RoleLook {
    if !sf.has_token(&format!("type.{name}.size")) {
        // Said once, exactly as `ui::role` says it: a typo in a theme or
        // a script is worth one line and not sixty a second.
        crate::ui::warn_once(
            &format!("role:{name}"),
            &format!("unknown type role \"{name}\" — nothing is drawn in it"),
        );
        return NO_ROLE;
    }
    let raw = sf.px(&format!("type.{name}.size")) * sf.scale() * shrink;
    // The role's own ceiling and floor, the global floor beneath a role
    // whose theme states none of its own — and the arithmetic itself in
    // [`crate::theme::role_px`], which `ui::Role::px` calls too. Two
    // resolvers answering one question have to answer it identically, and
    // the only way to be sure of that is for there to be one answer.
    //
    // A floor at all only because the role EXISTS: the absent case has
    // already returned, since a floor on a role that does not exist would
    // put the hole back on screen at legible size, which is the failure
    // this whole rule was written to stop.
    let px = crate::theme::role_px(
        raw,
        sf.px(&format!("type.{name}.min_px")),
        sf.px("type.min_px"),
        sf.px(&format!("type.{name}.max_px")),
    );
    // Tracking tokens are em — a fraction of the run's own size.
    let track = px * sf.px(&format!("type.{name}.tracking"));
    // A role whose master states no `leading` measures zero: an unstated
    // line height is a broken role, and the height of a broken role is
    // not this file's to invent.
    let leading = sf.px(&format!("type.{name}.leading"));
    // §5.16's `tabular`, read here so that every view, every script table
    // and the whole ABI side gets it from ONE resolver.
    let tabular = sf.flag(&format!("type.{name}.tabular"));
    // §5.16's `face`, read through the same one resolver and by the same
    // word→slot rule `ui::Role::font` uses. Reading it here is what stops
    // a view and an object disagreeing about which family one role is in.
    let face = crate::font::face_slot(&sf.word(&format!("type.{name}.face")));
    let mut color = sf.color(&format!("type.{name}.fg"));
    let alpha = sf.px(&format!("type.{name}.alpha"));
    color.a *= if alpha > 0.0 { alpha.min(1.0) } else { 1.0 };
    RoleLook { px, track, leading, tabular, face, color }
}

/// The role a `*_role` binding token names — `script.table_head_role`,
/// `list.label_role`. A binding resolving to nothing answers [`NO_ROLE`].
pub fn bound_role(sf: &mut impl Surface, binding: &str, shrink: f32) -> RoleLook {
    let word = sf.word(binding);
    if word.is_empty() {
        // The BINDING is what a reader has to go and fix, and it is the
        // one thing the role-side warning cannot name: an empty word
        // means either that this key is absent from the master or that a
        // consumer asked for a key nobody declares, and both are the
        // binding's story.
        crate::ui::warn_once(
            &format!("binding:{binding}"),
            &format!("\"{binding}\" names no type role — nothing is drawn in it"),
        );
        return NO_ROLE;
    }
    role_look(sf, &word, shrink)
}

// -------------------------------------------------------------- corners

/// The cut a shape word asks for, in the three [`Surface`] can draw.
///
/// Compared as a WORD, not as an enum index, for two reasons that both
/// bite here: the ABI can only ship words, and a preset's style slot
/// (`shape.badge.corners[0]`) has no `enum:` list in the master, so its
/// word table grows out of the values actually loaded — an index
/// memoised before a variant is read would freeze at the wrong answer.
///
/// `chevron` and `hexagon` are in the presets' vocabulary and in no
/// surface's: they degrade to the square a surface with no ring
/// primitive degrades to, for the same reason — it is the shape that can
/// be drawn honestly, and one a theme can already ask for.
pub fn corner_style(sf: &mut impl Surface, name: &str) -> CornerStyle {
    match sf.word(name).as_str() {
        "round" => CornerStyle::Round,
        "chamfer" => CornerStyle::Chamfer,
        _ => CornerStyle::Square,
    }
}

/// The radius a `*.corner` token states, for the rect that wears it.
///
/// A LENGTH IS NOT A SHAPE (§5.4d): this is the radius half of the pair
/// only, and [`corner_style`] carries the cut. `pill` is not a length at
/// all — §5.0 bakes the word to a negative sentinel — and names the
/// capsule: the radius at which both ends of the rect close over, which
/// is half its shorter side, and which is also the ceiling any stated
/// radius meets before two corners would cross.
///
/// The translation itself lives in [`crate::theme::corner_radius`] and is
/// called from here rather than repeated: a capsule written four times is
/// a capsule that stops being one somewhere.
pub fn corner_radius(sf: &mut impl Surface, name: &str, r: Rect, shrink: f32) -> f32 {
    let stated = sf.px(name);
    // A stated LENGTH meets the panel's shrink before it meets the box.
    // Every sentinel is a word ABOUT the box and has nothing to scale.
    let scaled = if stated > 0.0 { stated * shrink } else { stated };
    let radius = crate::theme::corner_radius(scaled, r.w, r.h);
    if radius == 0.0 && stated < 0.0 {
        // `auto` and `same_as_parent` are the rest of §5.0's table, and
        // neither is a radius. Named out loud rather than quietly cut to
        // nothing: the key is a theme's mistake to fix, not this file's
        // to paper over.
        crate::ui::warn_once(
            &format!("corner:{name}"),
            &format!("\"{name}\" holds a sentinel that is neither a length nor `pill`"),
        );
    }
    radius.min(r.w.min(r.h).max(0.0) / 2.0)
}

// --------------------------------------------------------------- text

/// Trims text with a trailing ellipsis so it fits `max_w`, measured at
/// the SAME letter tracking the caller draws with.
///
/// Measuring at a different tracking is how a content-measured table
/// column came to ellipsise the very cell it was sized from.
pub fn fit_end(
    sf: &mut impl Surface,
    face: u8,
    px: f32,
    text: &str,
    max_w: f32,
    track: f32,
) -> String {
    fit_end_tab(sf, face, px, text, max_w, track, false)
}

/// [`fit_end`] measured under the role's figure box. The same rule one
/// rung further: a tabular column trimmed against proportional widths
/// ellipsises a cell that fits, because every figure it holds is drawn
/// wider than it was measured.
pub fn fit_end_tab(
    sf: &mut impl Surface,
    face: u8,
    px: f32,
    text: &str,
    max_w: f32,
    track: f32,
    tabular: bool,
) -> String {
    // No room at all is not a width to abbreviate to: the ellipsis this
    // used to answer with is a glyph as wide as any other, so it went
    // over whatever squeezed the room shut. `draw::fit_tail` and the
    // panel band's `fit_lead` have both ruled it that way since they
    // were written, and `winframe::fit_title` had to re-state it locally
    // because THIS function did not — a trimming rule stated three times
    // and contradicted once. Stated here, it is the toolkit's answer.
    if max_w <= 0.0 {
        return String::new();
    }
    if sf.measure_tab(face, px, text, track, tabular) <= max_w {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let mut n = chars.len().saturating_sub(1);
    while n > 1 {
        let cand: String = chars[..n].iter().collect::<String>() + "\u{2026}";
        if sf.measure_tab(face, px, &cand, track, tabular) <= max_w {
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
pub fn wrap(
    sf: &mut impl Surface,
    face: u8,
    px: f32,
    text: &str,
    max_w: f32,
    track: f32,
) -> Vec<String> {
    wrap_tab(sf, face, px, text, max_w, track, false)
}

/// [`wrap`] measured under the role's figure box (§5.16 `tabular`), the
/// same rung [`fit_end_tab`] is to [`fit_end`].
///
/// A break is a MEASUREMENT, and a run that is drawn with a box and
/// broken without one is broken in the wrong places: every figure of the
/// candidate line is drawn wider than it was ruled, so the box overflows
/// on the right exactly as far as the digits it holds. That is the
/// mismatch `fit_end_tab` was written for, one line-breaking further on.
#[allow(clippy::too_many_arguments)]
pub fn wrap_tab(
    sf: &mut impl Surface,
    face: u8,
    px: f32,
    text: &str,
    max_w: f32,
    track: f32,
    tabular: bool,
) -> Vec<String> {
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
            if sf.measure_tab(face, px, &cand, track, tabular) <= max_w {
                line = cand;
                continue;
            }
            if !line.is_empty() {
                out.push(std::mem::take(&mut line));
            }
            // The word alone on its line: kept whole when it fits,
            // broken by characters when nothing else can be done.
            if sf.measure_tab(face, px, word, track, tabular) <= max_w {
                line = word.to_string();
                continue;
            }
            let mut piece = String::new();
            for ch in word.chars() {
                let mut cand = piece.clone();
                cand.push(ch);
                if !piece.is_empty() && sf.measure_tab(face, px, &cand, track, tabular) > max_w {
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
    face: u8,
    px: f32,
    text: &str,
    color: Color,
    track: f32,
) {
    cell_text_tab(sf, x, y, w, align, face, px, text, color, track, false);
}

/// [`cell_text`] under the role's figure box — the form a numeric column
/// takes. `PID 1471` and `PID 1888` then occupy the same width, which is
/// the difference between a column that stands still and one that moves
/// a pixel or two every time a process is replaced.
#[allow(clippy::too_many_arguments)]
pub fn cell_text_tab(
    sf: &mut impl Surface,
    x: f32,
    y: f32,
    w: f32,
    align: Align,
    face: u8,
    px: f32,
    text: &str,
    color: Color,
    track: f32,
    tabular: bool,
) {
    let tx = match align {
        Align::Left => x,
        Align::Center => x + w / 2.0,
        Align::Right => x + w,
    };
    sf.text_tab(face, px, tx, y, text, color, track, align, tabular);
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
    // `progress.corner` was declared and read by nothing, so a theme that
    // rounded its bars got squares. The fill wears the track's own corner
    // inset by the same distance the fill is inset — a square-ended fill
    // inside a rounded track hangs out past the cap, which is the bug the
    // slider's groove already had to fix.
    let cut = corner_style(sf, "progress.corner_style");
    let radius = corner_radius(sf, "progress.corner", r, 1.0);
    if track {
        let c = sf.color("component.bar.track");
        sf.ring(r, cut, radius, bw, c);
    }
    let inner = (r.w - 2.0 * inset).max(0.0);
    let fill = match sev {
        Some(s) => sev_text(sf, s),
        None => sf.color("component.bar.fill"),
    };
    let bar = Rect::new(r.x + inset, r.y + inset, inner * frac, (r.h - 2.0 * inset).max(0.0));
    sf.ring_fill(bar, cut, (radius - inset).max(0.0), fill);
}

/// The CRITICAL / CONTAINED pill: a filled, ringed capsule around a
/// short text, its four colours from the severity at draw time. Returns
/// the pill's width.
///
/// The corner is the theme's: `badge.corner` for the radius — `pill`
/// included, which is what the master ships and what makes the capsule
/// this thing is named after — and `shape.badge.corners`' style slot for
/// the cut.
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
    let tw = sf.measure_tab(role.face, role.px, text, role.track, role.tabular);
    let pad = sf.px("badge.pad_x") * shrink;
    // No floor under either: a `.max(1.0)` here is a one-pixel badge
    // nobody's theme asked for. `badge.h = 0` means the master wants no
    // badge, and that is a look it is entitled to state; the width is
    // the measured text plus the theme's padding, which is already a
    // length rather than a guess.
    let h = (sf.px("badge.h") * shrink).min(r.h);
    let w = (tw + 2.0 * pad).min(r.w);
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
    // A badge is the one element that states its shape in two places:
    // `badge.corner` is the radius, and the style half of the preset's
    // `shape.badge.corners` is the cut. Both are read, so a theme that
    // moves either one moves the badge.
    let cut = corner_style(sf, "shape.badge.corners[0]");
    let radius = corner_radius(sf, "badge.corner", pill, shrink);
    let bw = sf.px("badge.border");
    sf.ring_fill(pill, cut, radius, fill);
    if bw > 0.0 && !solid {
        sf.ring(pill, cut, radius, bw, edge);
    }
    let ty = center_line_y(sf, y, h, role.px, role.leading);
    sf.text_tab(
        role.face, role.px, x + w / 2.0, ty, text, ink, role.track, Align::Center,
        role.tabular,
    );
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

/// Which GRAMMAR a [`disclosure`] triangle is drawn in.
///
/// The two consumers draw the same three points and mean opposite things
/// by them, and that is the whole of the difference — so this is one
/// primitive with a parameter and not two triangles. Two copies would
/// let a theme's hairline, its centring rule and its winding drift apart
/// for no reason, and the caller would still have to choose between
/// them: the choice does not go away, it only stops being named.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Disclosure {
    /// A node in a TREE. Closed points along the row — "there is more
    /// inside this" — and open points down at the children it just
    /// revealed. Every file tree ever drawn reads this way and nothing
    /// here changes it.
    Tree,
    /// The caret on a DROP-DOWN's anchor. Closed points DOWN, at the
    /// direction the list will unfold, because a caret announces where
    /// the list goes and not the fact that it is currently shut — GTK,
    /// Qt and HTML's `select` all agree, and a `▷` here reads as "go
    /// into this row", which is the tree's sentence and not this one's.
    /// Open points back up, at the edge the list folds into.
    Drop,
}

/// The triangle that says a thing opens: the expander beside a tree row,
/// the caret on a drop-down's anchor. `kind` picks which of the two
/// sentences the shape is speaking ([`Disclosure`]); `expanded` is the
/// thing's own state.
///
/// The state turns the GLYPH, not its colour: rotation is geometry, and
/// geometry is the one thing a theme does not have to say twice.
pub fn disclosure(
    sf: &mut impl Surface,
    x: f32,
    y: f32,
    size: f32,
    line_px: f32,
    kind: Disclosure,
    expanded: bool,
    color: Color,
) {
    if size <= 0.0 {
        return;
    }
    let top = y + (line_px - size).max(0.0) / 2.0;
    let half = size / 2.0;
    // Named once and used by both grammars, so "down" is one shape in
    // this file and cannot end up two slightly different ones.
    let down = [[x, top], [x + size, top], [x + half, top + size]];
    let pts = match (kind, expanded) {
        // Along the row, toward what opening would reveal.
        (Disclosure::Tree, false) => [[x, top], [x + size, top + half], [x, top + size]],
        (Disclosure::Tree, true) => down,
        (Disclosure::Drop, false) => down,
        // Back at the anchor the open list folds into.
        (Disclosure::Drop, true) => [[x + half, top], [x + size, top + size], [x, top + size]],
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
    // The master asks for `@corner.pill` here and got a rectangle: the
    // radius was declared and read by nothing. The pair is the ordinary
    // one — the radius from `scrollbar.corner`, the cut from
    // `scrollbar.corner_style` — so a capsule thumb is a capsule.
    let cut = corner_style(sf, "scrollbar.corner_style");
    let radius = corner_radius(sf, "scrollbar.corner", geom.thumb, 1.0);
    let mut fill = style.fill;
    fill.a *= alpha;
    if fill.a > 0.0 {
        sf.ring_fill(geom.thumb, cut, radius, fill);
    }
    let mut edge = style.edge;
    edge.a *= alpha;
    if style.edge_width > 0.0 && edge.a > 0.0 {
        sf.ring(geom.thumb, cut, radius, style.edge_width, edge);
    }
}

// ---------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::font::FONT_UI;

    /// A surface that only measures: half an em a character, which is
    /// wrong about fonts and right about monotonicity — all the breaking
    /// arithmetic asks of it. Nothing here draws.
    struct Ruler;

    impl Surface for Ruler {
        fn rect(&mut self, _r: Rect, _c: Color) {}
        fn rect_outline(&mut self, _r: Rect, _w: f32, _c: Color) {}
        fn line(&mut self, _x0: f32, _y0: f32, _x1: f32, _y1: f32, _w: f32, _c: Color) {}
        fn polyline(&mut self, _p: &[[f32; 2]], _w: f32, _c: Color, _closed: bool) {}
        fn text(&mut self, _face: u8, _px: f32, _x: f32, _y: f32, _s: &str, _c: Color, _t: f32, _a: Align) {}
        fn measure(&mut self, _face: u8, px: f32, s: &str, _track: f32) -> f32 {
            s.chars().count() as f32 * px * 0.5
        }
        /// A box that is WIDER than the proportional run it replaces,
        /// which is the one property a caller can rely on: a real box is
        /// the widest figure of the face, so a boxed run never measures
        /// narrower. Doubling makes the difference impossible to miss —
        /// the default implementation of this method ignores `tabular`
        /// entirely, and a break that went through it would be silent.
        fn measure_tab(&mut self, face: u8, px: f32, s: &str, track: f32, tabular: bool) -> f32 {
            let w = self.measure(face, px, s, track);
            if tabular {
                w * 2.0
            } else {
                w
            }
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
        assert_eq!(wrap(&mut Ruler, FONT_UI, 10.0, "abcdefghij", 50.0, 0.0), ["abcdefghij"]);
        assert_eq!(wrap(&mut Ruler, FONT_UI, 10.0, "", 50.0, 0.0), [""]);
    }

    #[test]
    fn a_line_breaks_at_the_last_word_that_fits() {
        // At 10 px a character is 5 px wide: 35 px holds "one two".
        assert_eq!(wrap(&mut Ruler, FONT_UI, 10.0, "one two three", 35.0, 0.0), ["one two", "three"]);
        // 30 px does not, so every word gets its own line.
        assert_eq!(
            wrap(&mut Ruler, FONT_UI, 10.0, "one two three", 30.0, 0.0),
            ["one", "two", "three"]
        );
    }

    #[test]
    fn a_word_wider_than_the_box_is_broken_rather_than_left_hanging() {
        assert_eq!(wrap(&mut Ruler, FONT_UI, 10.0, "abcdefghij", 25.0, 0.0), ["abcde", "fghij"]);
        // A short word before it still gets its own line first.
        assert_eq!(wrap(&mut Ruler, FONT_UI, 10.0, "x abcdef", 25.0, 0.0), ["x", "abcde", "f"]);
    }

    #[test]
    fn explicit_newlines_are_kept_and_a_nonsense_width_stops_wrapping() {
        assert_eq!(wrap(&mut Ruler, FONT_UI, 10.0, "one\ntwo", 500.0, 0.0), ["one", "two"]);
        // Zero width would otherwise break every character forever.
        assert_eq!(wrap(&mut Ruler, FONT_UI, 10.0, "one two", 0.0, 0.0), ["one two"]);
    }

    /// A break is a measurement, so the role's figure box has to reach
    /// it. The pair is the proof: the same string, the same width, and
    /// the only difference between the two calls is the box.
    #[test]
    fn a_break_is_measured_under_the_box_the_run_will_be_drawn_with() {
        // 60 px holds "one two" proportionally; under a box every run is
        // twice as wide, so each word takes a line of its own.
        assert_eq!(
            wrap_tab(&mut Ruler, FONT_UI, 10.0, "one two three", 60.0, 0.0, false),
            ["one two", "three"]
        );
        assert_eq!(
            wrap_tab(&mut Ruler, FONT_UI, 10.0, "one two three", 60.0, 0.0, true),
            ["one", "two", "three"]
        );
        // The character break inside one long word answers the box too:
        // five characters fit at 25 px, two under the box.
        assert_eq!(
            wrap_tab(&mut Ruler, FONT_UI, 10.0, "abcdefghij", 25.0, 0.0, true),
            ["ab", "cd", "ef", "gh", "ij"]
        );
    }

    /// No room is not a width to abbreviate to. Three trimmers in this
    /// library answered that way and this one answered "…", so the
    /// objects that wanted the toolkit's answer had to write it out
    /// themselves; the rule is stated once now.
    #[test]
    fn a_trim_with_no_room_at_all_draws_nothing_rather_than_an_ellipsis() {
        for room in [0.0, -1.0, -400.0] {
            assert_eq!(
                fit_end(&mut Ruler, FONT_UI, 10.0, "SESSION", room, 0.0),
                "",
                "room {room} produced a glyph to draw"
            );
        }
        // A width that holds something still holds the ellipsis: the
        // rule above is about NO room, not about tight room.
        assert_eq!(fit_end(&mut Ruler, FONT_UI, 10.0, "SESSION", 20.0, 0.0), "SES\u{2026}");
    }

    #[test]
    fn the_leading_number_is_read_and_never_invented() {
        assert_eq!(leading_number("41.2%"), Some(41.2));
        assert_eq!(leading_number("-3 of 4"), Some(-3.0));
        assert_eq!(leading_number("firefox"), None);
        assert_eq!(leading_number(""), None);
        assert_eq!(leading_number("..."), None);
    }

    // ---- severity ----

    use crate::view::surface::tests::FakeSurface;

    /// §5.10's fallback is the master's word, and the one answer this
    /// file may not give is a number of its own. A key naming a severity
    /// the closed set does not hold lands on its LAST rung — counted off
    /// the set rather than written down, so a master that adds a rung
    /// cannot leave a stale index pointing at somebody else's colour.
    #[test]
    fn an_unnameable_severity_fallback_lands_on_the_last_rung_and_never_on_ok() {
        let mut sf = FakeSurface::new().word_at("script.severity_fallback", "chartreuse");
        let sev = sev_fallback(&mut sf);
        assert_eq!(
            sev,
            Sev(SEVERITY_ROLES.len() as u16 - 1),
            "the fallback must be the last rung of the set, not a number in this file"
        );
        assert_ne!(SEVERITY_ROLES[sev.0 as usize], "ok", "§5.10 forbids the fallback being `ok`");
        // A word the set DOES hold still wins outright — the fallback is
        // the exception, not the rule.
        let mut sf = FakeSurface::new().word_at("script.severity_fallback", "warning");
        assert_eq!(sev_fallback(&mut sf), sev_of("warning").unwrap());
    }

    // ---- corners ----

    /// A badge 12 px tall and wider than it is tall — three characters
    /// at 5 px and 4 px of padding a side — on a surface that can draw
    /// rings. Wider matters: the capsule closes over the SHORTER side.
    fn badged(sf: FakeSurface) -> FakeSurface {
        let mut sf = sf
            .token("badge.h", 12.0)
            .token("badge.pad_x", 4.0)
            .token("type.body.size", 10.0)
            .token("type.body.leading", 1.0)
            .word_at("script.badge_role", "body");
        badge(
            &mut sf,
            Rect::new(0.0, 0.0, 100.0, 20.0),
            "hot",
            None,
            BadgeStyle::Hollow,
            Align::Left,
            1.0,
        );
        sf
    }

    #[test]
    fn a_pill_is_the_capsule_its_name_says_and_a_length_is_a_length() {
        let r = Rect::new(0.0, 0.0, 40.0, 12.0);
        // `pill` is a word, not a number: the sentinel it bakes to means
        // half the shorter side, which closes both ends.
        let pill = crate::theme::expr::sentinel("pill").unwrap();
        let mut sf = FakeSurface::new().token("x.corner", pill);
        assert_eq!(corner_radius(&mut sf, "x.corner", r, 1.0), 6.0);
        // A stated radius is itself, shrunk with everything else...
        let mut sf = FakeSurface::new().token("x.corner", 3.0);
        assert_eq!(corner_radius(&mut sf, "x.corner", r, 1.0), 3.0);
        assert_eq!(corner_radius(&mut sf, "x.corner", r, 0.5), 1.5);
        // ...and never past the point where two corners would cross.
        let mut sf = FakeSurface::new().token("x.corner", 100.0);
        assert_eq!(corner_radius(&mut sf, "x.corner", r, 1.0), 6.0);
        // The rest of the sentinel table is not a radius at all, and
        // says so on the way to drawing nothing.
        let auto = crate::theme::expr::sentinel("auto").unwrap();
        let mut sf = FakeSurface::new().token("x.corner", auto);
        assert_eq!(corner_radius(&mut sf, "x.corner", r, 1.0), 0.0);
    }

    // ---- roles ----

    /// A theme with `body` fully stated and a readable global floor —
    /// everything a fallback to `body` would need to look plausible.
    fn typeset() -> FakeSurface {
        FakeSurface::new()
            .token("type.body.size", 10.0)
            .token("type.body.leading", 1.4)
            .token("type.min_px", 8.0)
    }

    #[test]
    fn a_binding_that_names_no_role_draws_nothing_at_all() {
        // A binding standing at no word: neither the master nor the theme
        // said what this text is, so nothing is what it looks like.
        let mut sf = typeset();
        let look = bound_role(&mut sf, "list.label_role", 1.0);
        assert_eq!(look.px, 0.0, "the global floor must not apply to a role that is absent");
        assert_eq!(look.leading, 0.0);
        assert_eq!(look.color.a, 0.0);

        // A binding standing at a name no `type.*` block declares is the
        // same hole reached through the other door — and `body` sitting
        // right there, fully stated, is exactly the trap: a fallback to
        // it renders a broken theme as a nearly-right interface.
        let mut sf = typeset().word_at("list.label_role", "no_such_role");
        let look = bound_role(&mut sf, "list.label_role", 1.0);
        assert_eq!(look.px, 0.0);
        assert_eq!(look.color.a, 0.0);
    }

    #[test]
    fn a_declared_role_obeys_its_own_floor_and_its_own_ceiling() {
        let mut sf = typeset().word_at("list.label_role", "body");
        assert_eq!(bound_role(&mut sf, "list.label_role", 1.0).px, 10.0);
        // Shrunk under the GLOBAL floor, a role the master declares still
        // stops there: `type.min_px` is the last defence against
        // unreadable type.
        assert_eq!(bound_role(&mut sf, "list.label_role", 0.1).px, 8.0);
        // The role's own floor wins over the global one when it is higher.
        let mut sf = typeset().word_at("list.label_role", "body").token("type.body.min_px", 12.0);
        assert_eq!(bound_role(&mut sf, "list.label_role", 1.0).px, 12.0);
        // A ceiling caps the size...
        let mut sf = typeset().word_at("list.label_role", "body").token("type.body.max_px", 9.0);
        assert_eq!(bound_role(&mut sf, "list.label_role", 1.0).px, 9.0);
        // ...and `0px` is how the master spells "uncapped", which must
        // never read as a ceiling of nothing.
        let mut sf = typeset().word_at("list.label_role", "body").token("type.body.max_px", 0.0);
        assert_eq!(bound_role(&mut sf, "list.label_role", 1.0).px, 10.0);
        // A ceiling under the floor is a theme contradicting itself, and
        // the floor is the one that wins.
        let mut sf = typeset()
            .word_at("list.label_role", "body")
            .token("type.body.min_px", 12.0)
            .token("type.body.max_px", 4.0);
        assert_eq!(bound_role(&mut sf, "list.label_role", 1.0).px, 12.0);
    }

    #[test]
    fn the_cut_is_the_word_the_theme_wrote_and_nothing_else() {
        let mut sf = FakeSurface::new().word_at("x.corner_style", "round");
        assert_eq!(corner_style(&mut sf, "x.corner_style"), CornerStyle::Round);
        let mut sf = FakeSurface::new().word_at("x.corner_style", "chamfer");
        assert_eq!(corner_style(&mut sf, "x.corner_style"), CornerStyle::Chamfer);
        // `chevron` is in the presets' vocabulary and in no surface's,
        // and a theme that says nothing has said nothing.
        let mut sf = FakeSurface::new().word_at("x.corner_style", "chevron");
        assert_eq!(corner_style(&mut sf, "x.corner_style"), CornerStyle::Square);
        let mut sf = FakeSurface::new();
        assert_eq!(corner_style(&mut sf, "x.corner_style"), CornerStyle::Square);
    }

    #[test]
    fn a_badge_wears_the_radius_and_the_cut_the_theme_states() {
        let pill = crate::theme::expr::sentinel("pill").unwrap();
        // The master's own pair: the pill sentinel and `round`.
        let sf = badged(
            FakeSurface::new()
                .token("badge.corner", pill)
                .word_at("shape.badge.corners[0]", "round"),
        );
        assert!(sf.rects.is_empty(), "a shaped badge is not a rectangle");
        assert_eq!(sf.rings.len(), 1);
        let (r, style, radius) = sf.rings[0];
        assert_eq!(r.h, 12.0);
        assert_eq!(style, CornerStyle::Round);
        assert_eq!(radius, 6.0, "half the 12 px height: the capsule");
        // Move the radius token, and the pill stops being one.
        let sf = badged(
            FakeSurface::new()
                .token("badge.corner", 2.0)
                .word_at("shape.badge.corners[0]", "round"),
        );
        assert_eq!(sf.rings[0].2, 2.0);
        // Move the style token, and the same radius is cut differently.
        let sf = badged(
            FakeSurface::new()
                .token("badge.corner", pill)
                .word_at("shape.badge.corners[0]", "chamfer"),
        );
        assert_eq!(sf.rings[0].1, CornerStyle::Chamfer);
        assert_eq!(sf.rings[0].2, 6.0);
    }

    #[test]
    fn a_hollow_badge_strokes_the_same_shape_it_filled() {
        // Two rings, one geometry: a ring drawn on a different radius
        // from its fill is the two-shapes bug in miniature.
        let pill = crate::theme::expr::sentinel("pill").unwrap();
        let sf = badged(
            FakeSurface::new()
                .token("badge.corner", pill)
                .token("badge.border", 1.0)
                .word_at("shape.badge.corners[0]", "chamfer"),
        );
        assert_eq!(sf.strokes.len(), 1);
        let (fill_r, fill_style, fill_radius) = sf.rings[0];
        let (edge_r, edge_style, edge_radius) = sf.strokes[0];
        assert_eq!((fill_r.x, fill_r.y, fill_r.w, fill_r.h), (edge_r.x, edge_r.y, edge_r.w, edge_r.h));
        assert_eq!(fill_style, edge_style);
        assert_eq!(fill_radius, edge_radius);
    }

    #[test]
    fn the_master_states_both_halves_of_a_badges_corner() {
        // The proof the drawing tests stand on: these are the values the
        // shipped theme really holds, so the pill above is the pill the
        // user sees.
        let t = crate::theme::resolved();
        let id = |n: &str| crate::theme::id(n).expect("declared in the master");
        assert_eq!(
            t.px(id("badge.corner")),
            crate::theme::expr::sentinel("pill").unwrap(),
            "the master asks for a capsule, which used to bake to a square"
        );
        // Asked the way `CtxSurface::word` asks it, so this is the
        // answer the drawing really gets and not a second reading of the
        // same file.
        assert_eq!(
            crate::ui::theme_word(id("shape.badge.corners[0]")),
            "round",
            "the style half of the preset, which the badge now reads"
        );
    }

    /// The thumb and the bar, whose corner tokens the audit found
    /// declared and read by NOTHING: the master writes `@corner.pill` on
    /// a scrollbar thumb and the drawing was `rect`, so the one token a
    /// theme has for the shape of a thumb moved nothing at all. Both
    /// halves of the pair are measured — the radius the token states and
    /// the cut its `*_corner_style` sibling names — because a radius
    /// with no cut is the same silence one step along.
    #[test]
    fn the_thumb_and_the_bar_wear_the_corner_their_own_tokens_state() {
        let pill = crate::theme::expr::sentinel("pill").unwrap();
        // A plate, because a thumb with no fill colour is never drawn
        // and a test about its SHAPE would then measure nothing.
        let thumbed = |radius: f32, cut: &str| -> FakeSurface {
            let geom = crate::view::scroll::ScrollbarGeom {
                track: Rect::new(90.0, 0.0, 10.0, 100.0),
                thumb: Rect::new(90.0, 20.0, 10.0, 40.0),
            };
            let mut sf = FakeSurface::new()
                .plate(Color { r: 1.0, g: 1.0, b: 1.0, a: 1.0 })
                .token("scrollbar.corner", radius)
                .word_at("scrollbar.corner_style", cut);
            scrollbar(&mut sf, &geom, 1.0, false, false);
            sf
        };

        let sf = thumbed(pill, "round");
        assert!(sf.rects.is_empty(), "a shaped thumb is not a rectangle");
        assert_eq!(sf.rings.len(), 1);
        assert_eq!(sf.rings[0].1, CornerStyle::Round);
        assert_eq!(sf.rings[0].2, 5.0, "half the thumb's 10 px width: the capsule");
        // A stated length is itself, and the cut is the sibling's word:
        // move either token and the thumb moves with it.
        assert_eq!(thumbed(2.0, "round").rings[0].2, 2.0);
        assert_eq!(thumbed(pill, "chamfer").rings[0].1, CornerStyle::Chamfer);
        // `@corner.none` is a literal zero and stays the slab it asks
        // for: reading a token is not deciding for the theme.
        assert_eq!(thumbed(0.0, "round").rings[0].2, 0.0);

        // The bar is the same pair on the other element, and its fill
        // wears the track's corner inset by its own inset — a
        // square-ended fill inside a rounded track hangs out past the cap.
        let r = Rect::new(0.0, 0.0, 100.0, 10.0);
        let mut sf = FakeSurface::new()
            .token("progress.corner", pill)
            .word_at("progress.corner_style", "round");
        meter(&mut sf, r, 1.0, None, true);
        assert_eq!(sf.strokes.len(), 1, "the track's ring");
        assert_eq!(sf.strokes[0].1, CornerStyle::Round);
        assert_eq!(sf.strokes[0].2, 5.0, "half the bar's 10 px height");
        assert_eq!(sf.rings.len(), 1, "the fill");
        assert_eq!(sf.rings[0].2, 5.0);
        // The shipped radius is zero, and the bar drawn from it is the
        // rectangle the master asked for.
        let mut sf = FakeSurface::new().word_at("progress.corner_style", "round");
        meter(&mut sf, r, 1.0, None, true);
        assert_eq!(sf.rings[0].2, 0.0);
    }

    /// The values behind the test above, in the shipped file: the tokens
    /// the audit found unread are read, and what they say is what the
    /// user sees.
    #[test]
    fn the_master_states_a_corner_for_the_thumb_and_for_the_bar() {
        let t = crate::theme::resolved();
        let id = |n: &str| crate::theme::id(n).expect("declared in the master");
        assert_eq!(
            t.px(id("scrollbar.corner")),
            crate::theme::expr::sentinel("pill").unwrap(),
            "the master asks for a capsule thumb, which used to bake to a slab"
        );
        // Asked the way `CtxSurface::word` asks it, so this is the answer
        // the drawing really gets and not a second reading of the file.
        assert_eq!(crate::ui::theme_word(id("scrollbar.corner_style")), "round");
        assert_eq!(t.px(id("progress.corner")), 0.0, "`@corner.none` is a length of zero");
        assert!(
            !crate::ui::theme_word(id("progress.corner_style")).is_empty(),
            "a radius with no cut is the same silence one step along"
        );
    }

    // ---- the triangle that says a thing opens ----

    /// The one triangle `disclosure` drew, as its three points.
    fn triangle(kind: Disclosure, expanded: bool) -> Vec<[f32; 2]> {
        let mut sf = FakeSurface::new();
        // A 10 px glyph in a 10 px line box: the box drops out of the
        // arithmetic, so what is left is the shape and only the shape.
        disclosure(&mut sf, 0.0, 0.0, 10.0, 10.0, kind, expanded, Color::TRANSPARENT);
        assert_eq!(sf.polylines.len(), 1, "a disclosure is one closed outline");
        sf.polylines.remove(0)
    }

    /// Where a three-point triangle points, read off its own geometry:
    /// the apex is the corner that shares no coordinate with the other
    /// two, and the direction is where it sits relative to them.
    fn points(pts: &[[f32; 2]]) -> &'static str {
        assert_eq!(pts.len(), 3);
        let same = |a: f32, b: f32| (a - b).abs() < 0.01;
        // The apex stands alone on both axes; the other two hold the
        // edge it points away from.
        let apex = *pts
            .iter()
            .find(|p| {
                pts.iter().filter(|q| same(q[0], p[0])).count() == 1
                    && pts.iter().filter(|q| same(q[1], p[1])).count() == 1
            })
            .expect("a triangle with no apex is not one of ours");
        let base: Vec<[f32; 2]> = pts.iter().copied().filter(|p| *p != apex).collect();
        if same(base[0][0], base[1][0]) {
            "right"
        } else if apex[1] > base[0][1] {
            "down"
        } else {
            "up"
        }
    }

    /// The two grammars, which are the whole reason the parameter
    /// exists. A tree says "there is more inside this row" and points
    /// ALONG it when shut; a drop-down's caret says "the list comes out
    /// downwards" and points DOWN when shut — the convention GTK, Qt and
    /// `select` share. Drawing a tree's `▷` on a list anchor tells the
    /// user to walk into the row, which is a different offer entirely.
    #[test]
    fn a_caret_points_where_its_own_convention_says_and_not_where_a_trees_does() {
        assert_eq!(points(&triangle(Disclosure::Tree, false)), "right");
        assert_eq!(points(&triangle(Disclosure::Tree, true)), "down");
        assert_eq!(points(&triangle(Disclosure::Drop, false)), "down");
        assert_eq!(points(&triangle(Disclosure::Drop, true)), "up");
        // Open and shut are different SHAPES in both grammars — the
        // state turns the glyph, and a caret that never turned would
        // leave the open list unannounced.
        assert_ne!(triangle(Disclosure::Tree, false), triangle(Disclosure::Tree, true));
        assert_ne!(triangle(Disclosure::Drop, false), triangle(Disclosure::Drop, true));
        // The shut drop and the open tree are the same "down" arrow, and
        // that is the point of one primitive: there is exactly one of it.
        assert_eq!(triangle(Disclosure::Drop, false), triangle(Disclosure::Tree, true));
    }

    #[test]
    fn a_disclosure_with_no_box_to_draw_in_draws_nothing() {
        let mut sf = FakeSurface::new();
        disclosure(&mut sf, 0.0, 0.0, 0.0, 10.0, Disclosure::Drop, false, Color::TRANSPARENT);
        assert!(sf.polylines.is_empty(), "a glyph the theme sized to nothing is not drawn");
    }

    // ---- the rule every trimmed label follows ----


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

