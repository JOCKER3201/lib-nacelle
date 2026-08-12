//! The virtualised row list — and, with the tree affordances turned on,
//! the tree as well.
//!
//! A row is `[chip] label ………… [status]`, with a hairline bar under the
//! label when the row carries a fraction. Every metric comes from the
//! `[list]` section the master has declared since the theme engine
//! landed and which, until now, nothing drew.
//!
//! It is written against [`Surface`] and [`RowModel`], so the same code
//! draws a script's `list`, a script's `tree` (a [`super::tree::FlatTree`]
//! IS a row model) and, across the ABI, a plugin's own list. There is no
//! second implementation to keep in step.

use super::hits::{Hit, Hits};
use super::model::{RowBuf, RowModel};
use super::paint::{self, RoleLook};
use super::scroll::{ScrollPhysics, ScrollView, ScrollbarEdge, ScrollbarLook};
use super::surface::Surface;
use super::table::Extent;
use super::virt;
use crate::draw::CornerStyle;
use crate::theme::parse::State;
use crate::theme::Color;
use crate::ui::Align;
use crate::Rect;

/// Everything one list remembers between frames.
///
/// The table's state minus the sort and the columns: a list has no
/// headings to click, so what is left is the selection, the offset and
/// what the last draw put on screen.
#[derive(Debug, Default)]
pub struct ListState {
    /// The KEY of the selected row — never an index, because the model
    /// is rebuilt every snapshot.
    pub selected: Option<String>,
    pub scroll: ScrollView,
    /// What the last draw put on screen, for the input that arrives
    /// between frames with no geometry of its own.
    pub extent: Extent,
    /// Bumped by every change the user made. A script's answer is cached
    /// per frame and a click has to invalidate that cache within the
    /// frame it lands in.
    pub interact_epoch: u64,
}

impl ListState {
    pub fn new() -> ListState {
        ListState::default()
    }

    fn touch(&mut self) {
        self.interact_epoch = self.interact_epoch.wrapping_add(1);
    }

    /// Selects a row by key, or clears the selection with `None`.
    pub fn select(&mut self, key: Option<String>) {
        if self.selected != key {
            self.selected = key;
            self.touch();
        }
    }

    pub fn is_selected(&self, key: &str) -> bool {
        self.selected.as_deref() == Some(key)
    }
}

/// The caller's arrangement choices, plus the stack's shrink factor.
/// Everything visual is read from the theme inside.
#[derive(Clone, Copy, Debug)]
pub struct ListStyle {
    /// The stack's shrink-to-fit factor — runtime state, never a look
    /// decision.
    pub shrink: f32,
}

impl Default for ListStyle {
    fn default() -> ListStyle {
        ListStyle { shrink: 1.0 }
    }
}

/// The view riding on a list: what it remembers, where it records the
/// rectangles it drew, and which of its interactions are on.
///
/// Every option is OFF in a view built with `Default`, and a list drawn
/// with all of them off draws exactly what a list with no view draws —
/// one implementation, so that is a property rather than a promise.
pub struct ListView<'a> {
    pub state: &'a mut ListState,
    pub hits: &'a mut Hits,
    /// Which view recorded a rectangle: one [`Hits`] may serve every
    /// view in a widget.
    pub id: u32,
    /// Rows answer the pointer and one of them may be selected.
    pub select: bool,
    /// Scroll the body instead of truncating it at the bottom edge.
    pub scroll: bool,
    /// Draw the tree affordances: the depth indent and an expander on
    /// every row that has children.
    pub tree: bool,
    /// A row name the ellipsis cut short explains itself when the
    /// pointer rests on it (F2 §8.1). Only what was TRIMMED asks, and
    /// only through the view path — a list drawn without one has nowhere
    /// to file a request from, which is the table's arrangement too.
    pub tooltip: bool,
}

/// The `[list]` and `[tree]` metrics, read ONCE per draw.
///
/// The `Look::read` pattern, and the reason [`Surface`] may name tokens
/// by string at all: forty rows a frame would otherwise be forty hash
/// lookups per token.
struct Look {
    row_h: f32,
    pad_x: f32,
    gap: f32,
    rule_w: f32,
    rule_every: usize,
    glyph: f32,
    glyph_gap: f32,
    status_gap: f32,
    /// `list.scroll_gutter` — the lane a row keeps clear at the scrolling
    /// edge. An overlay bar costs the content nothing, so without this it
    /// draws ON TOP of a row's trailing status; the master's 0u is the
    /// drawing as it stands, and a theme that wants the bar beside the
    /// text rather than over it says so here.
    scroll_gutter: f32,
    bar_h: f32,
    bar_gap: f32,
    label: RoleLook,
    status: RoleLook,
    rule_c: Color,
    /// `tree.indent`, which the master derives from `list.indent`.
    indent: f32,
    disclosure: f32,
    disclosure_gap: f32,
    /// `tree.guides` as a stroke width; 0 (the master's `none`) means
    /// no guides are drawn — the same shape `list.rule` has.
    guide_w: f32,
    guide_c: Color,
    /// `list.corner_style` and `list.corner` — the cut and the radius of
    /// the hover/selection plate under a row, which is the one thing the
    /// master's `list.corner` was ever written for.
    plate_cut: CornerStyle,
    plate_radius: f32,
}

impl Look {
    fn read(sf: &mut impl Surface, shrink: f32, tree: bool, row_w: f32) -> Look {
        let guide_w = if tree { sf.px("tree.guides") } else { 0.0 };
        let row_h = sf.px("list.row_h") * shrink;
        Look {
            row_h,
            pad_x: sf.px("list.pad_x") * shrink,
            gap: sf.px("list.gap") * shrink,
            // A hairline is a hairline at every scale — the rule of
            // `ui::rule`, and the reason this one is not shrunk.
            rule_w: sf.px("list.rule"),
            rule_every: sf.px("list.rule_every").max(0.0) as usize,
            glyph: sf.px("list.glyph") * shrink,
            glyph_gap: sf.px("list.glyph_gap") * shrink,
            status_gap: sf.px("list.status_gap") * shrink,
            scroll_gutter: sf.px("list.scroll_gutter") * shrink,
            bar_h: sf.px("list.bar_h") * shrink,
            bar_gap: sf.px("list.bar_gap") * shrink,
            label: paint::bound_role(sf, "list.label_role", shrink),
            status: paint::bound_role(sf, "list.status_role", shrink),
            // A list has no rule colour of its own and needs none: this
            // is the hairline a script widget draws, which the master
            // already names.
            rule_c: sf.color("component.script.rule"),
            indent: if tree { sf.px("tree.indent") * shrink } else { 0.0 },
            disclosure: if tree { sf.px("tree.disclosure") * shrink } else { 0.0 },
            disclosure_gap: if tree { sf.px("tree.disclosure_gap") * shrink } else { 0.0 },
            guide_w,
            guide_c: if guide_w > 0.0 {
                sf.color("component.tree.guide")
            } else {
                Color { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }
            },
            plate_cut: paint::corner_style(sf, "list.corner_style"),
            // Settled here and not per row: every row of a list wears
            // the same rectangle, and a `pill` radius is read off it.
            plate_radius: paint::corner_radius(
                sf,
                "list.corner",
                Rect::new(0.0, 0.0, row_w, row_h),
                shrink,
            ),
        }
    }

    /// The pitch of one row: its height plus the gap under it. What the
    /// window arithmetic and the scroll content are measured in.
    fn pitch(&self) -> f32 {
        (self.row_h + self.gap).max(1.0)
    }
}

/// The height `n` rows occupy: `n` rows and the `n - 1` gaps between
/// them. Pure, because the caller that needs it most — a stack measuring
/// itself a frame before it draws — has no surface yet and reads the two
/// tokens itself.
pub fn height(row_h: f32, gap: f32, n: usize) -> f32 {
    if n == 0 {
        return 0.0;
    }
    row_h * n as f32 + gap * n.saturating_sub(1) as f32
}

/// [`height`] for a caller that does have a surface.
pub fn natural_h(sf: &mut impl Surface, n: usize, shrink: f32) -> f32 {
    height(sf.px("list.row_h") * shrink, sf.px("list.gap") * shrink, n)
}

/// Draws `model`'s rows into `r`.
///
/// Without a view it is what it has always been able to be: as many rows
/// as fit, from the top, truncated at the bottom edge. With one it
/// scrolls, answers the pointer and remembers a selection.
pub fn list<S: Surface, M: RowModel>(
    sf: &mut S,
    r: Rect,
    model: &M,
    st: &ListStyle,
    view: Option<ListView>,
) {
    let (mut state, mut hits, view_id, select, scroll, tree, explain) = match view {
        Some(v) => {
            (Some(v.state), Some(v.hits), v.id, v.select, v.scroll, v.tree, v.tooltip)
        }
        None => (None, None, 0, false, false, false, false),
    };
    let look = Look::read(sf, st.shrink, tree, r.w);
    let pitch = look.pitch();
    let total = model.len();
    if r.h <= 0.0 || total == 0 {
        if let Some(s) = state.as_deref_mut() {
            s.extent = Extent {
                scrollable: false,
                viewport: r.h.max(0.0),
                content: virt::content_h(pitch, total),
                bar: None,
            };
        }
        return;
    }
    let content = virt::content_h(pitch, total);
    // An offset has to live somewhere: without a view there is no state
    // to hold one, and a list that cannot remember where it was scrolled
    // to has not been scrolled.
    let scrolling = scroll && state.is_some();
    let can_clip = sf.can_clip();

    // The window. Without scrolling it is the top of the list, cut at
    // the last WHOLE row that fits — a half-drawn row with no offset
    // behind it is a fault, not a view.
    let mut window = virt::RowWindow {
        first: 0,
        count: ((r.h / pitch).floor() as usize).min(total),
        y0: 0.0,
    };
    let mut geom = None;
    let mut bar_look = None;
    if let Some(s) = state.as_deref_mut() {
        s.extent = Extent { scrollable: scrolling, viewport: r.h, content, bar: None };
    }
    if scrolling {
        let phys = ScrollPhysics::read(sf);
        let look_bar = ScrollbarLook::read(sf);
        let now = sf.now();
        let mouse = sf.mouse();
        if let Some(s) = state.as_deref_mut() {
            let snap = if can_clip {
                super::Snap::None
            } else {
                super::Snap::Row(pitch)
            };
            s.scroll.tick(now, r.h, content, snap, &phys);
            window = virt::row_window(s.scroll.offset(), r.h, pitch, total);
            // The band the bar could occupy at its WIDEST: a bar that
            // grows under the pointer must not shrink out from under it.
            let reach = look_bar.w_hover.max(look_bar.w) + look_bar.margin;
            let band = match look_bar.edge {
                ScrollbarEdge::Left => Rect::new(r.x, r.y, reach, r.h),
                ScrollbarEdge::Right => Rect::new(r.right() - reach, r.y, reach, r.h),
            };
            let hovered = band.contains(mouse.0, mouse.1);
            geom = super::scroll::scrollbar(
                r,
                &look_bar,
                s.scroll.offset(),
                r.h,
                content,
                hovered || s.scroll.dragging(),
            );
            s.extent.bar = geom.as_ref().map(|g| (g.track, g.thumb));
            bar_look = Some((look_bar, hovered));
        }
    }

    let selected_key: Option<&str> = state.as_deref().and_then(|s| s.selected.as_deref());
    let dragging = state.as_deref().is_some_and(|s| s.scroll.dragging());
    let now = sf.now();
    let bar_alpha = match (state.as_deref(), &bar_look) {
        (Some(s), Some((look_bar, hovered))) => {
            if *hovered || dragging {
                1.0
            } else {
                s.scroll.fade_alpha(now, look_bar.auto_hide, look_bar.fade_ms)
            }
        }
        _ => 1.0,
    };
    // The lane `list.scroll_gutter` asks a row to keep clear, charged to
    // the side the bar is actually on and only while one is drawn. The
    // master's 0u leaves every row exactly where it was.
    let (gutter_l, gutter_r) = match (&geom, &bar_look) {
        (Some(_), Some((look_bar, _))) if look.scroll_gutter > 0.0 => match look_bar.edge {
            ScrollbarEdge::Left => (look.scroll_gutter, 0.0),
            ScrollbarEdge::Right => (0.0, look.scroll_gutter),
        },
        _ => (0.0, 0.0),
    };
    let mouse = sf.mouse();

    // A window that starts part-way down a row needs the body clipped,
    // or the first row paints over whatever sits above the list.
    let clipped = scrolling && sf.clip(r);
    let mut buf = RowBuf::new();
    for d in window.rows() {
        model.row(d, &mut buf);
        let y = r.y + window.y_of(d, pitch);
        let row_r = Rect::new(r.x, y, r.w, look.row_h);
        if select {
            let hovered = row_r.contains(mouse.0, mouse.1)
                && mouse.1 >= r.y
                && mouse.1 < r.bottom();
            let chosen = selected_key == Some(buf.key.as_str());
            let rung = match (chosen, hovered) {
                (true, true) => Some(State::SelectedHover),
                (true, false) => Some(State::Selected),
                (false, true) => Some(State::Hover),
                _ => None,
            };
            if let Some(rung) = rung {
                // `list.item` — the class the master already declares
                // for exactly this. No new selection colour exists, or
                // needs to.
                let style = sf.class_state("list.item", rung);
                if style.fill.a > 0.0 {
                    sf.ring_fill(row_r, look.plate_cut, look.plate_radius, style.fill);
                }
            }
        }
        // Recorded whatever `select` says: a row rectangle is also how
        // the wheel finds out WHICH view the pointer is over.
        if let Some(h) = hits.as_deref_mut() {
            h.push(row_r, Hit::Row { id: view_id, key: buf.key.clone() });
        }

        let mut x = r.x + gutter_l + look.pad_x + buf.depth as f32 * look.indent;
        // Optional indent guides: one hairline per ancestor level, in
        // the column its expander would occupy.
        if look.guide_w > 0.0 && buf.depth > 0 {
            for k in 0..buf.depth {
                let gx =
                    r.x + gutter_l + look.pad_x + k as f32 * look.indent + look.disclosure / 2.0;
                sf.line(gx, y, gx, y + pitch, look.guide_w, look.guide_c);
            }
        }
        if tree && look.disclosure > 0.0 {
            if buf.has_children {
                paint::disclosure(
                    sf,
                    x,
                    y,
                    look.disclosure,
                    look.row_h,
                    buf.expanded,
                    look.label.color,
                );
                if let Some(h) = hits.as_deref_mut() {
                    // After the row, so the last rectangle drawn is the
                    // one that takes the click.
                    h.push(
                        Rect::new(x, y, look.disclosure + look.disclosure_gap, look.row_h),
                        Hit::Disclosure { id: view_id, key: buf.key.clone() },
                    );
                }
            }
            // Reserved even for a leaf, so labels at one depth line up
            // whether or not their neighbours can be opened.
            x += look.disclosure + look.disclosure_gap;
        }
        // The chip: the row's severity, read a second time as a colour —
        // the same relationship the table's bar has with its number.
        if let Some(sev) = buf.severity {
            if look.glyph > 0.0 {
                let c = paint::sev_text(sf, sev);
                let h = look.glyph.min(look.row_h);
                sf.rect(Rect::new(x, y + (look.row_h - h) / 2.0, look.glyph, h), c);
                x += look.glyph + look.glyph_gap;
            }
        }
        // The status is measured from the right edge; the label gets
        // what is left.
        let mut right = r.right() - gutter_r - look.pad_x;
        if !buf.status.is_empty() {
            let sw = sf.measure(look.status.px, &buf.status, look.status.track);
            let sy = paint::center_line_y(sf, y, look.row_h, look.status.px, look.status.leading);
            sf.text(
                look.status.px,
                right,
                sy,
                &buf.status,
                look.status.color,
                look.status.track,
                Align::Right,
            );
            right -= sw + look.status_gap;
        }
        let label_w = (right - x).max(1.0);
        // A row carrying a bar gives it the bottom of the row, and the
        // label is centred in what is left.
        let text_h = match buf.bar {
            Some(_) => (look.row_h - look.bar_h - look.bar_gap).max(1.0),
            None => look.row_h,
        };
        let ly = paint::center_line_y(sf, y, text_h, look.label.px, look.label.leading);
        let shown = paint::fit_end(sf, look.label.px, &buf.label, label_w, look.label.track);
        sf.text(
            look.label.px,
            x,
            ly,
            &shown,
            look.label.color,
            look.label.track,
            Align::Left,
        );
        // A row name the ellipsis cut short finishes itself when the
        // pointer rests on it (F2 §8.1). The anchor is the LABEL's own
        // rectangle, not the row's: the status beside it is drawn whole
        // or not at all, so it has nothing to explain and must not
        // answer for its neighbour. The vertical test is the hover
        // test's — a row half outside a scrolled window is half not
        // there.
        if explain && mouse.1 >= r.y && mouse.1 < r.bottom() {
            paint::explain_trim(
                sf,
                crate::object::tooltip::cell_key(view_id, 0, &buf.key),
                Rect::new(x, y, label_w, look.row_h),
                &shown,
                &buf.label,
            );
        }
        if let Some(frac) = buf.bar {
            let bar = Rect::new(x, y + text_h + look.bar_gap, label_w, look.bar_h);
            paint::meter(sf, bar, frac, buf.severity, true);
        }
        if look.rule_every > 0 && look.rule_w > 0.0 && (d + 1) % look.rule_every == 0 {
            let ry = y + look.row_h + look.gap / 2.0;
            sf.line(r.x, ry, r.right(), ry, look.rule_w, look.rule_c);
        }
    }
    if clipped {
        sf.unclip();
    }
    // The bar last, over the rows it covers — which is why its
    // rectangles are recorded last too: the pointer points at what it
    // can see.
    if let (Some(g), Some((_, hovered))) = (geom, bar_look) {
        paint::scrollbar(sf, &g, bar_alpha, hovered, dragging);
        if let Some(h) = hits.as_deref_mut() {
            let mid = g.thumb.y + g.thumb.h / 2.0;
            h.push(
                Rect::new(g.track.x, g.track.y, g.track.w, mid - g.track.y),
                Hit::Track { id: view_id, toward_end: false },
            );
            h.push(
                Rect::new(g.track.x, mid, g.track.w, g.track.bottom() - mid),
                Hit::Track { id: view_id, toward_end: true },
            );
            h.push(g.thumb, Hit::Thumb { id: view_id });
        }
    }
}

// ---------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn px(name: &str) -> f32 {
        crate::theme::resolved().px(crate::theme::id(name).unwrap())
    }

    fn word(name: &str) -> String {
        crate::theme::enum_word_of(crate::theme::id(name).unwrap()).unwrap_or_default()
    }

    #[test]
    fn the_height_of_a_list_counts_the_gaps_between_rows_and_not_after_them() {
        assert_eq!(height(20.0, 4.0, 0), 0.0);
        assert_eq!(height(20.0, 4.0, 1), 20.0);
        assert_eq!(height(20.0, 4.0, 3), 68.0);
        // The master's own gap is zero, so a list is exactly its rows —
        // which is what keeps a `list` element measurable the way `rows`
        // has always been.
        assert_eq!(height(20.0, 0.0, 3), 60.0);
    }

    #[test]
    fn the_master_declares_the_metrics_this_file_draws_from() {
        assert!(px("list.row_h") > 0.0);
        assert!(px("list.glyph") > 0.0 && px("list.glyph_gap") > 0.0);
        assert!(px("list.indent") > 0.0 && px("list.status_gap") > 0.0);
        assert!(px("list.bar_h") > 0.0 && px("list.bar_gap") > 0.0);
        assert_eq!(word("list.label_role"), "body");
        assert_eq!(word("list.status_role"), "caption");
        // The tree's own three, and the neutral default that keeps the
        // master's render where it was: no guides.
        assert_eq!(px("tree.indent"), px("list.indent"), "one indent, two names");
        assert!(px("tree.disclosure") > 0.0 && px("tree.disclosure_gap") > 0.0);
        assert_eq!(px("tree.guides"), 0.0, "`none` is a stroke of nothing");
        // The master's gap and rule are both off, so a list drawn today
        // is rows and nothing between them.
        assert_eq!(px("list.gap"), 0.0);
        assert_eq!(px("list.rule"), 0.0);
        assert_eq!(px("list.rule_every"), 0.0);
    }

    #[test]
    fn a_row_names_its_own_corner_wheel_and_gutter_instead_of_borrowing_a_tiles() {
        // The three the plugins reported missing. corner and wheel_px are
        // declared at the values they were borrowing from `filetile.*`, so
        // a row that switches names draws and scrolls exactly as before.
        assert_eq!(px("list.corner"), px("filetile.corner"));
        assert_eq!(px("list.wheel_px"), px("filetile.wheel_px"));
        // scrollbar.mode = overlay costs the content nothing, so the bar
        // may sit over a row's trailing status. The gutter is the theme's
        // answer to that, and the master's answer is "leave it as it was".
        assert_eq!(px("list.scroll_gutter"), 0.0);
        assert_eq!(word("scrollbar.mode"), "overlay");
    }

    #[test]
    fn the_empty_state_line_answers_to_its_own_name() {
        // Both keys used to hang off the tail of [boot] with no header,
        // so `emptystate.*` resolved to nothing at all. `px` first: it is
        // what loads the master in a test that runs on its own.
        assert!(px("emptystate.y_frac") > 0.0);
        assert_eq!(word("emptystate.role"), "value");
        // And the names that only existed because the header was missing
        // are gone: a boot screen has no "nothing here" line.
        assert!(crate::theme::id("boot.role").is_none());
        assert!(crate::theme::id("boot.y_frac").is_none());
    }

    // ---- the trimmed name explains itself ----

    use crate::view::model::Rows;
    use crate::view::surface::tests::FakeSurface;

    const LONG: &str = "org.freedesktop.NetworkManager";

    /// A surface with just enough theme to draw a row that can be
    /// pointed at: a row 20 px tall, 4 px of padding, and a body role
    /// whose characters are 5 px wide (the fake measures half an em).
    ///
    /// The two role BINDINGS are stated as well. They have to be: a
    /// binding standing at no word names no role, and no role draws
    /// nothing — which is the whole point of the rule, and would leave
    /// these tests measuring an empty row.
    fn dressed() -> FakeSurface {
        FakeSurface::new()
            .token("list.row_h", 20.0)
            .token("list.pad_x", 4.0)
            .token("type.body.size", 10.0)
            .token("type.body.leading", 1.0)
            .word_at("list.label_role", "body")
            .word_at("list.status_role", "body")
    }

    fn one_row(label: &str) -> Rows {
        let mut row = RowBuf::new();
        row.key = "nm".to_string();
        row.label = label.to_string();
        Rows::new(vec![row])
    }

    fn drawn(mut sf: FakeSurface, model: &Rows, tooltip: bool) -> FakeSurface {
        let mut state = ListState::new();
        let mut hits = super::super::hits::Hits::new();
        list(
            &mut sf,
            Rect::new(0.0, 0.0, 100.0, 60.0),
            model,
            &ListStyle::default(),
            Some(ListView {
                state: &mut state,
                hits: &mut hits,
                id: 3,
                select: false,
                scroll: false,
                tree: false,
                tooltip,
            }),
        );
        sf
    }

    #[test]
    fn a_row_name_the_ellipsis_cut_short_explains_itself() {
        // 30 characters at 5 px is 150 px of name in 92 px of room.
        let model = one_row(LONG);
        let sf = drawn(dressed().at(20.0, 10.0), &model, true);
        assert_eq!(sf.tips.len(), 1, "one row under the pointer, one request");
        let (id, r, text) = &sf.tips[0];
        assert_eq!(text, LONG);
        // The anchor is the label's box, not the row's: it starts after
        // the padding and stops where the row's own width does.
        assert_eq!((r.x, r.y, r.h), (4.0, 0.0, 20.0));
        assert!(r.w < 100.0, "the label's room, not the whole row");
        // The identity is the PLACE — this view, the label column, this
        // row's key — so it survives the model being rebuilt around it.
        assert_eq!(*id, crate::object::tooltip::cell_key(3, 0, "nm"));
    }

    #[test]
    fn a_name_that_fits_and_a_list_that_was_not_asked_say_nothing() {
        // Short enough to be drawn whole: there is nothing to add.
        let short = one_row("nm");
        assert!(drawn(dressed().at(20.0, 10.0), &short, true).tips.is_empty());
        // Trimmed, but the caller did not ask for tooltips: a list that
        // was drawn before this phase draws exactly as it did.
        let model = one_row(LONG);
        assert!(drawn(dressed().at(20.0, 10.0), &model, false).tips.is_empty());
        // Trimmed and asked for, but the pointer is on the row BELOW the
        // only one there is.
        assert!(drawn(dressed().at(20.0, 45.0), &model, true).tips.is_empty());
    }

    // ---- the plate under a row ----

    /// One row, 20 px tall, with the pointer resting on it and a plate
    /// colour to draw — the only frame in which a hover plate exists.
    fn hovered(corner: f32, cut: &str) -> FakeSurface {
        let mut sf = dressed()
            .token("list.corner", corner)
            .word_at("list.corner_style", cut)
            .plate(Color { r: 1.0, g: 1.0, b: 1.0, a: 1.0 })
            .at(20.0, 10.0);
        let model = one_row("nm");
        let mut state = ListState::new();
        let mut hits = super::super::hits::Hits::new();
        list(
            &mut sf,
            Rect::new(0.0, 0.0, 100.0, 60.0),
            &model,
            &ListStyle::default(),
            Some(ListView {
                state: &mut state,
                hits: &mut hits,
                id: 3,
                select: true,
                scroll: false,
                tree: false,
                tooltip: false,
            }),
        );
        sf
    }

    #[test]
    fn the_plate_under_a_row_wears_the_corner_the_theme_states() {
        let sf = hovered(4.0, "round");
        assert_eq!(sf.rings.len(), 1, "one row under the pointer, one plate");
        let (r, style, radius) = sf.rings[0];
        assert_eq!((r.x, r.y, r.w, r.h), (0.0, 0.0, 100.0, 20.0), "the row's own box");
        assert_eq!(style, CornerStyle::Round);
        assert_eq!(radius, 4.0);
        // Both halves of the pair move the plate, which is the whole
        // point: `list.corner` used to be read by nothing at all.
        let sf = hovered(9.0, "round");
        assert_eq!(sf.rings[0].2, 9.0);
        let sf = hovered(4.0, "chamfer");
        assert_eq!(sf.rings[0].1, CornerStyle::Chamfer);
        // A row 20 px tall cannot wear a 40 px radius: two corners would
        // cross before they met.
        let sf = hovered(40.0, "round");
        assert_eq!(sf.rings[0].2, 10.0);
    }

    #[test]
    fn the_master_states_a_rows_corner_and_how_it_is_cut() {
        // What the drawing test stands on: the shipped values, so the
        // rounded plate above is the plate the user sees.
        assert!(px("list.corner") > 0.0);
        // Asked the way `CtxSurface::word` asks it: the answer the
        // drawing gets, not a second reading of the same file.
        let id = crate::theme::id("list.corner_style").expect("declared in the master");
        assert_eq!(crate::ui::theme_word(id), "round");
    }

    #[test]
    fn a_selection_by_key_survives_the_row_moving() {
        let mut s = ListState::new();
        s.select(Some("beta".into()));
        assert!(s.is_selected("beta"));
        assert!(!s.is_selected("alpha"));
        let e = s.interact_epoch;
        // The same selection again is not a change, and must not make
        // the per-frame cache think one happened.
        s.select(Some("beta".into()));
        assert_eq!(s.interact_epoch, e);
        s.select(None);
        assert!(!s.is_selected("beta"));
        assert_ne!(s.interact_epoch, e);
    }
}

