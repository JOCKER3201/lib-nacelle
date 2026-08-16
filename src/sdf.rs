//! The CPU referee of the vector core (f3 §6, level E): the distance
//! formulas `fs_shape` computes in WGSL (§2.2), written once in Rust so
//! the mathematics is provable without a GPU. The shader is the
//! implementation, this file is the specification — the two must read
//! line for line alike, and a change to one without the other is wrong
//! by definition.
//!
//! K2's scope is `kind = Box` alone. `p` is the fragment's position in
//! local pixels relative to the shape's centre — exactly what a shape
//! vertex carries in its `uv` slot — and `b` the half sizes; screen
//! convention throughout, y grows downward.

use crate::draw::{Corner, CornerStyle};

/// cos 45°, the chamfer plane's normalisation — WGSL's SQRT1_2.
pub const SQRT1_2: f32 = std::f32::consts::FRAC_1_SQRT_2;

/// Exact signed distance to the axis-aligned box of half sizes `b`.
pub fn d_box(p: [f32; 2], b: [f32; 2]) -> f32 {
    let q = [p[0].abs() - b[0], p[1].abs() - b[1]];
    let o = [q[0].max(0.0), q[1].max(0.0)];
    q[0].max(q[1]).min(0.0) + (o[0] * o[0] + o[1] * o[1]).sqrt()
}

/// The rounded corner of radius `k` — the exact rounded-box distance.
pub fn d_round(p: [f32; 2], b: [f32; 2], k: f32) -> f32 {
    let q = [p[0].abs() - b[0] + k, p[1].abs() - b[1] + k];
    let o = [q[0].max(0.0), q[1].max(0.0)];
    q[0].max(q[1]).min(0.0) + (o[0] * o[0] + o[1] * o[1]).sqrt() - k
}

/// The chamfered corner of cut `k`: the box intersected with the 45°
/// half-plane `|x| + |y| = b.x + b.y − k`. The `max` of two fields is
/// the exact distance only near the boundary — and only there does
/// coverage read it; inside, the underestimate saturates anyway
/// (§2.2's honesty note).
pub fn d_chamfer(p: [f32; 2], b: [f32; 2], k: f32) -> f32 {
    let cut = (p[0].abs() + p[1].abs() - (b[0] + b[1] - k)) * SQRT1_2;
    d_box(p, b).max(cut)
}

/// Which corner rules `p`: 0 tl, 1 tr, 2 br, 3 bl — `ring_points`'
/// order, y down. The quadrant boundary sits mid-edge, where |d| is at
/// least `min(b) − max(k)` and the treatment switch cannot reach the
/// coverage ramp (§2.2).
pub fn corner_index(p: [f32; 2]) -> usize {
    match (p[1] >= 0.0, p[0] >= 0.0) {
        (false, false) => 0,
        (false, true) => 1,
        (true, true) => 2,
        (true, false) => 3,
    }
}

/// The Box-family distance under four per-corner treatments — what one
/// `fs_shape` fragment computes for a Box record.
pub fn d_shape(p: [f32; 2], b: [f32; 2], corners: &[Corner; 4]) -> f32 {
    let c = corners[corner_index(p)];
    match c.style {
        CornerStyle::Square => d_box(p, b),
        CornerStyle::Round => d_round(p, b, c.size),
        CornerStyle::Chamfer => d_chamfer(p, b, c.size),
    }
}

/// The stroke band, INWARD from the boundary (§2.2, the project's
/// convention): zero on the silhouette, zero again at depth `stroke`,
/// negative between.
pub fn d_band(d: f32, stroke: f32) -> f32 {
    d.max(-d - stroke)
}

/// Box-filter coverage of the half-plane at signed distance `d` under
/// AA width `w` (§2.3): exact for a straight edge, first-order correct
/// for curvature well above the pixel. In the shader `w` is
/// `length(vec2(dpdx(d), dpdy(d)))` — never `fwidth`, which over-reads
/// √2 on a 45° slope; the reference takes it as a parameter.
pub fn coverage(d: f32, w: f32) -> f32 {
    (0.5 - d / w.max(1e-6)).clamp(0.0, 1.0)
}

/// §2.10's one mix: bed and edge composed in a single record so their
/// shared outer silhouette blends ONCE. `a_out` is the silhouette's
/// coverage, `a_band` the band's; straight-alpha RGBA out. Split into
/// two records the same silhouette would compose `1 − (1 − a)²` where
/// this returns `a` — the dark rim on a translucent panel over glass.
pub fn compose(fill: [f32; 4], stroke_c: [f32; 4], a_out: f32, a_band: f32) -> [f32; 4] {
    let t = a_band * stroke_c[3];
    [
        fill[0] + (stroke_c[0] - fill[0]) * t,
        fill[1] + (stroke_c[1] - fill[1]) * t,
        fill[2] + (stroke_c[2] - fill[2]) * t,
        a_out * (fill[3] + (1.0 - fill[3]) * t),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    const B: [f32; 2] = [40.0, 25.0];

    fn mixed() -> [Corner; 4] {
        [
            Corner::round(8.0),
            Corner::chamfer(10.0),
            Corner::SQUARE,
            Corner::round(3.0),
        ]
    }

    /// Deep inside the coverage saturates to one, deep outside to zero
    /// — across all three corner treatments at once.
    #[test]
    fn coverage_saturates_inside_and_vanishes_outside() {
        let c = mixed();
        assert_eq!(coverage(d_shape([0.0, 0.0], B, &c), 1.0), 1.0);
        assert_eq!(
            coverage(d_shape([B[0] - 2.0, 0.0], B, &c), 1.0),
            1.0,
            "two px inside is still fully covered"
        );
        for p in [[80.0, 0.0], [0.0, -60.0], [70.0, 55.0], [-90.0, -70.0]] {
            assert_eq!(coverage(d_shape(p, B, &c), 1.0), 0.0, "{p:?}");
        }
    }

    /// 64 directions from the centre: where the sign of d flips, the
    /// coverage reads one half within the stated tolerance — including
    /// the rays that cross a quadrant seam or a treatment switch.
    #[test]
    fn the_boundary_reads_half_in_64_directions() {
        let c = mixed();
        for i in 0..64 {
            let a = i as f32 / 64.0 * std::f32::consts::TAU;
            let (s, co) = a.sin_cos();
            // Bisect d = 0 along the ray: the centre is inside, 200 px
            // out is outside for this box, and the shape is convex, so
            // the sign flips exactly once.
            let (mut lo, mut hi) = (0.0f32, 200.0f32);
            for _ in 0..48 {
                let mid = 0.5 * (lo + hi);
                if d_shape([co * mid, s * mid], B, &c) < 0.0 {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            let t = 0.5 * (lo + hi);
            let cov = coverage(d_shape([co * t, s * t], B, &c), 1.0);
            assert!((cov - 0.5).abs() <= 0.02, "direction {i}: coverage {cov}");
        }
    }

    /// The round corner runs on its own arc: d = 0 at both tangent
    /// points and at the arc's 45° point, and the square corner it
    /// replaced lies outside by exactly k(√2 − 1).
    #[test]
    fn a_round_corner_runs_on_its_arc() {
        let k = 8.0f32;
        let e = 1e-3;
        assert!(d_round([B[0], B[1] - k], B, k).abs() <= e);
        assert!(d_round([B[0] - k, B[1]], B, k).abs() <= e);
        let c45 = [B[0] - k + k * SQRT1_2, B[1] - k + k * SQRT1_2];
        assert!(d_round(c45, B, k).abs() <= e);
        let d = d_round(B, B, k);
        assert!((d - k * (std::f32::consts::SQRT_2 - 1.0)).abs() <= e, "{d}");
    }

    /// The chamfer passes through (b.x − c, b.y) and (b.x, b.y − c) —
    /// the same condition `chamfer_frame_stroke_never_leaves_the_rect`
    /// pins on the tessellated path — its midpoint lies ON the cut, and
    /// the old square corner sits outside it by k·cos 45°.
    #[test]
    fn a_chamfer_runs_on_its_cut() {
        let k = 10.0f32;
        let e = 1e-3;
        assert!(d_chamfer([B[0] - k, B[1]], B, k).abs() <= e);
        assert!(d_chamfer([B[0], B[1] - k], B, k).abs() <= e);
        assert!(d_chamfer([B[0] - k * 0.5, B[1] - k * 0.5], B, k).abs() <= e);
        let d = d_chamfer(B, B, k);
        assert!((d - k * SQRT1_2).abs() <= e, "{d}");
    }

    /// The band is the inward stroke: zero on the silhouette, zero at
    /// depth `stroke`, deepest in the middle — and one px past either
    /// side it is outside the band by that px.
    #[test]
    fn the_band_runs_inward_from_the_boundary() {
        let t = 4.0f32;
        assert_eq!(d_band(0.0, t), 0.0);
        assert_eq!(d_band(-t, t), 0.0);
        assert_eq!(d_band(-t * 0.5, t), -t * 0.5);
        assert_eq!(d_band(1.0, t), 1.0);
        assert_eq!(d_band(-t - 1.0, t), 1.0);
    }

    /// §2.10: on the shared silhouette the composed alpha is the
    /// STROKE's own, not 1 − (1 − a)² — the double blend a split
    /// fill+ring pair produces there.
    #[test]
    fn fill_and_stroke_share_one_edge_not_two() {
        let fill = [0.2, 0.4, 0.6, 1.0];
        let stroke = [1.0, 1.0, 1.0, 1.0];
        // On the silhouette, deep in the band: the edge blends once.
        let px = compose(fill, stroke, 0.5, 1.0);
        assert_eq!(px[3], 0.5);
        let double = 1.0 - (1.0 - 0.5f32) * (1.0 - 0.5);
        assert_ne!(px[3], double, "the split-record double blend");
        // Inside, past the band: the fill alone, bit for bit.
        assert_eq!(compose(fill, stroke, 1.0, 0.0), fill);
        // In the band's heart: the stroke's own colour at full alpha.
        assert_eq!(compose(fill, stroke, 1.0, 1.0), stroke);
    }

    /// d_box is the true Euclidean distance wherever that is checkable
    /// by hand: past an edge the offset, past a corner the diagonal,
    /// at the centre minus the short half side.
    #[test]
    fn the_box_distance_is_euclidean() {
        assert_eq!(d_box([B[0] + 3.0, 0.0], B), 3.0);
        assert_eq!(d_box([0.0, -(B[1] + 7.0)], B), 7.0);
        let d = d_box([B[0] + 3.0, B[1] + 4.0], B);
        assert!((d - 5.0).abs() <= 1e-5, "{d}");
        assert_eq!(d_box([0.0, 0.0], B), -B[1]);
    }
}
