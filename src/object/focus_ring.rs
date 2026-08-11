//! The keyboard focus ring — the one overlay every control draws
//! identically, so two objects can never disagree about what focus
//! looks like.
//!
//! Focus is not a state-ladder rung (§5.21): the ring sits AROUND the
//! control, outside its own edge, drawn only while the chain answers
//! `ring = true` — keyboard navigation has happened and no pointer
//! press has hidden it since. At boot neither has happened, so the boot
//! frame keeps its pixels.
//!
//! Every token is read per frame: `focus.ring.enabled` is
//! a11y-protected and the hc variant may thicken `focus.ring.width`
//! mid-run, so only `TokenId`s are cached (the `OnceLock` idiom the
//! rest of the objects use), never resolved pixels.

use crate::draw::{Corner, CornerStyle};
use crate::font::FontSystem;
use crate::theme::{self, Color, TokenId};
use crate::{Ctx, Rect};
use std::sync::OnceLock;

fn tok(cell: &'static OnceLock<TokenId>, name: &'static str) -> TokenId {
    *cell.get_or_init(|| theme::id(name).unwrap_or(TokenId::MISSING))
}

/// The engine's colour, in the draw list's clothes.
fn col(c: theme::ThemeColor) -> Color {
    Color { r: c.r, g: c.g, b: c.b, a: c.a }
}

/// The resolved ring treatment, read fresh each call: (width, offset,
/// colour), or None while `focus.ring.enabled` is off or the width
/// degrades to nothing.
fn treatment() -> Option<(f32, f32, Color)> {
    static ENABLED: OnceLock<TokenId> = OnceLock::new();
    static WIDTH: OnceLock<TokenId> = OnceLock::new();
    static OFFSET: OnceLock<TokenId> = OnceLock::new();
    static COLOR: OnceLock<TokenId> = OnceLock::new();
    let t = theme::resolved();
    if !t.flag(tok(&ENABLED, "focus.ring.enabled")) {
        return None;
    }
    let w = t.px(tok(&WIDTH, "focus.ring.width")).max(0.0);
    if w <= 0.0 {
        return None;
    }
    let off = t.px(tok(&OFFSET, "focus.ring.offset")).max(0.0);
    Some((w, off, col(t.color(tok(&COLOR, "focus.ring.color")))))
}

/// Draws the keyboard focus ring AROUND `r`, outside the control's own
/// edge: a gap of `focus.ring.offset` between the control and the
/// band's inner face, `focus.ring.width` thick, in `focus.ring.color`,
/// plus the `glow.focus_ring` halo when a theme enables that class.
/// No-op when `focus.ring.enabled` is false.
pub fn draw(ctx: &mut Ctx, r: Rect) {
    let Some((w, off, color)) = treatment() else {
        return;
    };
    // rect_outline strokes INSIDE its rect, so the ring rect grows by
    // offset + width on every side and the band lands wholly outside
    // the control: [offset, offset + width] past its edge.
    let d = off + w;
    let ring = Rect::new(r.x - d, r.y - d, r.w + 2.0 * d, r.h + 2.0 * d);
    ctx.dl.rect_outline(ring.x, ring.y, ring.w, ring.h, w, color);
    glow(ctx, ring, color);
}

/// The parallelogram variant — a button's slanted quad. Same treatment,
/// stroked as a closed polyline centred on the outward-offset outline.
pub fn draw_quad(ctx: &mut Ctx, q: [[f32; 2]; 4]) {
    let Some((w, off, color)) = treatment() else {
        return;
    };
    // polyline centres its stroke on the path, so the path runs through
    // the band's middle: offset + width/2 out from the control's edge.
    let outer = offset_convex_quad(q, off + w * 0.5);
    ctx.dl.polyline(&outer, w, color, true);
    // No halo here yet: glow_ring speaks rects only. Default ships the
    // glow class disabled; a theme that enables it halos the
    // rectangular controls, and the parallelograms join when the glow
    // primitives grow a quad form.
}

/// The `glow.focus_ring` halo around the ring band — `element` tint
/// rule, exactly as `panel_edge_glow`: the halo wears the ring's own
/// resolved colour, at the class's alpha scaled by the one global knob.
fn glow(ctx: &mut Ctx, ring: Rect, tint: Color) {
    static ON: OnceLock<TokenId> = OnceLock::new();
    static RADIUS: OnceLock<TokenId> = OnceLock::new();
    static ALPHA: OnceLock<TokenId> = OnceLock::new();
    static SCALE: OnceLock<TokenId> = OnceLock::new();
    let t = theme::resolved();
    if !t.flag(tok(&ON, "glow.focus_ring.enabled")) {
        return;
    }
    let radius = t.px(tok(&RADIUS, "glow.focus_ring.radius")).max(0.0);
    let alpha = (t.px(tok(&ALPHA, "glow.focus_ring.alpha"))
        * t.px(tok(&SCALE, "glow.alpha_scale")))
    .clamp(0.0, 1.0);
    if radius <= 0.0 || alpha <= 0.0 {
        return;
    }
    let c = [Corner { style: CornerStyle::Square, size: 0.0 }; 4];
    ctx.dl.glow_ring(ring, &c, 1, radius, tint.alpha(alpha), FontSystem::mask_soft_uv());
}

/// Offsets a convex quad outward by `d`: each edge's line moves `d`
/// along its outward normal (away from the centroid, whatever the
/// winding) and neighbouring lines re-intersect — the mitre join, exact
/// for a convex quad. Near-parallel neighbours (a degenerate quad) fall
/// back to pushing the vertex along the edge normal.
fn offset_convex_quad(q: [[f32; 2]; 4], d: f32) -> [[f32; 2]; 4] {
    if d <= 0.0 {
        return q;
    }
    let cx = (q[0][0] + q[1][0] + q[2][0] + q[3][0]) * 0.25;
    let cy = (q[0][1] + q[1][1] + q[2][1] + q[3][1]) * 0.25;
    // Each edge as its offset line n·p = c, |n| = 1, n pointing outward.
    let mut lines = [[0.0f32; 3]; 4];
    for i in 0..4 {
        let p = q[i];
        let r = q[(i + 1) % 4];
        let (dx, dy) = (r[0] - p[0], r[1] - p[1]);
        let len = (dx * dx + dy * dy).sqrt().max(1e-4);
        let (mut nx, mut ny) = (dy / len, -dx / len);
        let (mx, my) = ((p[0] + r[0]) * 0.5 - cx, (p[1] + r[1]) * 0.5 - cy);
        if nx * mx + ny * my < 0.0 {
            nx = -nx;
            ny = -ny;
        }
        lines[i] = [nx, ny, nx * p[0] + ny * p[1] + d];
    }
    let mut out = q;
    for i in 0..4 {
        // Vertex (i+1) joins edge i and edge i+1.
        let a = lines[i];
        let b = lines[(i + 1) % 4];
        let v = (i + 1) % 4;
        let det = a[0] * b[1] - a[1] * b[0];
        if det.abs() < 1e-6 {
            out[v] = [q[v][0] + a[0] * d, q[v][1] + a[1] * d];
        } else {
            out[v] = [
                (a[2] * b[1] - a[1] * b[2]) / det,
                (a[0] * b[2] - a[2] * b[0]) / det,
            ];
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::offset_convex_quad;

    fn close(a: [f32; 2], b: [f32; 2]) -> bool {
        (a[0] - b[0]).abs() < 1e-3 && (a[1] - b[1]).abs() < 1e-3
    }

    #[test]
    fn rect_quad_grows_by_d_on_every_side() {
        let q = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
        let o = offset_convex_quad(q, 2.0);
        assert!(close(o[0], [-2.0, -2.0]), "{:?}", o[0]);
        assert!(close(o[1], [12.0, -2.0]), "{:?}", o[1]);
        assert!(close(o[2], [12.0, 12.0]), "{:?}", o[2]);
        assert!(close(o[3], [-2.0, 12.0]), "{:?}", o[3]);
    }

    #[test]
    fn winding_does_not_matter() {
        // The same square, wound the other way round.
        let q = [[0.0, 10.0], [10.0, 10.0], [10.0, 0.0], [0.0, 0.0]];
        let o = offset_convex_quad(q, 2.0);
        assert!(close(o[0], [-2.0, 12.0]), "{:?}", o[0]);
        assert!(close(o[2], [12.0, -2.0]), "{:?}", o[2]);
    }

    #[test]
    fn parallelogram_edges_stay_parallel() {
        // A button quad: skew 3 on a 20x10 rect.
        let q = [[3.0, 0.0], [20.0, 0.0], [17.0, 10.0], [0.0, 10.0]];
        let o = offset_convex_quad(q, 1.5);
        // Top and bottom edges stay horizontal, moved out by d.
        assert!((o[0][1] - -1.5).abs() < 1e-3 && (o[1][1] - -1.5).abs() < 1e-3);
        assert!((o[2][1] - 11.5).abs() < 1e-3 && (o[3][1] - 11.5).abs() < 1e-3);
        // The slanted sides keep their direction.
        let s0 = (q[3][0] - q[0][0], q[3][1] - q[0][1]);
        let s1 = (o[3][0] - o[0][0], o[3][1] - o[0][1]);
        let cross = s0.0 * s1.1 - s0.1 * s1.0;
        assert!(cross.abs() < 1e-2, "slant changed: {cross}");
    }

    #[test]
    fn zero_offset_is_identity() {
        let q = [[3.0, 0.0], [20.0, 0.0], [17.0, 10.0], [0.0, 10.0]];
        assert_eq!(offset_convex_quad(q, 0.0), q);
    }
}
