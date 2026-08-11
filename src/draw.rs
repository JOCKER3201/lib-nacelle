//! Draw list — everything as triangles. Most of them sample the glyph
//! atlas (text by its glyphs, solid shapes by the atlas's white
//! pixel); a run may instead sample an application-registered image.

use crate::base::Rect;
use crate::font::{FontSystem, Glyph};
use crate::theme::Color;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Vertex {
    pub pos: [f32; 2],
    pub uv: [f32; 2],
    pub color: [f32; 4],
}

/// A handle to pixels the RENDERER owns. The list only records which
/// image a run samples; registering the pixels, uploading them and
/// mapping the handle to a texture is the renderer's job — the same
/// split as with everything else here: the toolkit describes, the
/// application draws.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ImageId(pub u32);

/// The reserved image handle for frosted glass: a run tagged with it
/// samples a BLURRED copy of everything drawn before the first such
/// run this frame, by screen position. The renderer owns the blurring;
/// the list only marks where the glass lies.
pub const BLUR_IMAGE: ImageId = ImageId(u32::MAX);

/// The reserved handle band (r1). Everything at or above RESERVED_IMAGE_MIN
/// is a renderer instruction, not a texture: the glass ranks, the additive
/// pipeline, and BLUR_IMAGE itself (which aliases rank 2 — exactly what the
/// renderer drew for it before ranks existed). `create_texture` must never
/// hand these out.
pub const RESERVED_IMAGE_MIN: ImageId = ImageId(u32::MAX - 15);
/// Glass at pyramid rank 1..3: lightest to deepest blur. A rank the frame's
/// blur depth did not write resolves to the deepest one that exists.
pub const GLASS_RANK_1: ImageId = ImageId(u32::MAX - 1);
pub const GLASS_RANK_2: ImageId = ImageId(u32::MAX - 2);
pub const GLASS_RANK_3: ImageId = ImageId(u32::MAX - 3);
/// Additive blending over the glyph atlas: the run renders through fs_main
/// with SRC_ALPHA/ONE colour, ZERO/ONE alpha — glow and bloom compose with
/// light instead of milk (Appendix B, R1).
pub const ADD_ATLAS: ImageId = ImageId(u32::MAX - 8);

/// Whether a handle is one of the reserved instructions rather than a
/// registered texture.
pub fn is_reserved(id: ImageId) -> bool {
    id.0 >= RESERVED_IMAGE_MIN.0
}

/// Treatment of one rect corner — the vocabulary of the one tessellated
/// ring generator (r1 §3). There is no arc primitive and no mask-based
/// corner: nothing in this pipeline is antialiased except text, and a
/// smooth corner alone would be the only soft silhouette on screen.
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CornerStyle {
    Square = 0,
    Round = 1,
    Chamfer = 2,
}

/// One corner: the style plus its size — the cut length for Chamfer, the
/// radius for Round, ignored by Square. The size is a design value and
/// therefore always arrives as a parameter from a token; nothing here
/// defaults it.
#[derive(Clone, Copy, Debug)]
pub struct Corner {
    pub style: CornerStyle,
    pub size: f32,
}

impl Corner {
    pub const SQUARE: Corner = Corner { style: CornerStyle::Square, size: 0.0 };

    pub const fn round(r: f32) -> Corner {
        Corner { style: CornerStyle::Round, size: r }
    }

    pub const fn chamfer(len: f32) -> Corner {
        Corner { style: CornerStyle::Chamfer, size: len }
    }

    /// The same corner on a boundary moved inward by `d` (outward when `d`
    /// is negative), keeping the moved face parallel to the original.
    /// Round offsets to a concentric arc: exactly `r − d`. Chamfer: moving
    /// the 45° face `x + y = k` inward by `d` shifts its constant by `d·√2`
    /// while the corner it is measured from shifts by `2d`, so the cut
    /// shrinks by `(2 − √2)·d` — the derivation behind `chamfer_frame`'s
    /// 0.293·t (there `d = t/2`). Square has nothing to move.
    pub fn inset(self, d: f32) -> Corner {
        let size = match self.style {
            CornerStyle::Square => 0.0,
            CornerStyle::Round => (self.size - d).max(0.0),
            CornerStyle::Chamfer => {
                (self.size - (2.0 - std::f32::consts::SQRT_2) * d).max(0.0)
            }
        };
        Corner { style: self.style, size }
    }
}

/// The smallest segment count whose chord error stays under `tol` px at
/// radius `r`, capped by `ceiling` — the sagitta of a quarter-arc chord is
/// `r·(1 − cos(45°/S))`. The caller passes the theme's `corner.segments`
/// as the ceiling and the tolerance it can live with; at 0.25 px the
/// shipped corner ladder answers 3/3/4 where a flat 6 would spend 40 %
/// more vertices for error already below the pixel grid (r1 §3.4).
pub fn ring_segments(r: f32, tol: f32, ceiling: u8) -> u8 {
    let ceiling = ceiling.clamp(3, 16);
    let r = r.max(0.0);
    for s in 3..=ceiling {
        let half = std::f32::consts::FRAC_PI_4 / s as f32;
        if r * (1.0 - half.cos()) <= tol {
            return s;
        }
    }
    ceiling
}

/// Boundary of `r` under the four corner treatments, tl → tr → br → bl,
/// clockwise in screen coordinates (y down). Square contributes 1 point,
/// Chamfer 2, Round `segments + 1` — the counts depend only on style and
/// segments, never on size, so two parallel rings always correspond
/// index-to-index and a stroke between them is watertight. Sizes are
/// clamped to half the short side, segments to 3..=16 (geometry clamps,
/// not design defaults). One `sin_cos` per call; the arc itself is adds
/// and multiplies via incremental rotation, endpoints pinned exactly onto
/// the edges so a flush test can compare bitwise-close.
fn ring_points(r: Rect, c: &[Corner; 4], segments: u8, out: &mut Vec<[f32; 2]>) {
    out.clear();
    let seg = segments.clamp(3, 16) as u32;
    let cap = (r.w.min(r.h) * 0.5).max(0.0);
    let (sin_t, cos_t) = (std::f32::consts::FRAC_PI_2 / seg as f32).sin_cos();
    // Corner point plus the unit directions back along the two edges it
    // joins: the ring enters at `p + sz·e_in` and leaves at `p + sz·e_out`.
    let corners: [([f32; 2], [f32; 2], [f32; 2]); 4] = [
        ([r.x, r.y], [0.0, 1.0], [1.0, 0.0]),
        ([r.x + r.w, r.y], [-1.0, 0.0], [0.0, 1.0]),
        ([r.x + r.w, r.y + r.h], [0.0, -1.0], [-1.0, 0.0]),
        ([r.x, r.y + r.h], [1.0, 0.0], [0.0, -1.0]),
    ];
    for (i, &(p, ein, eout)) in corners.iter().enumerate() {
        let sz = c[i].size.clamp(0.0, cap);
        match c[i].style {
            CornerStyle::Square => out.push(p),
            CornerStyle::Chamfer => {
                out.push([p[0] + sz * ein[0], p[1] + sz * ein[1]]);
                out.push([p[0] + sz * eout[0], p[1] + sz * eout[1]]);
            }
            CornerStyle::Round => {
                let cx = p[0] + sz * (ein[0] + eout[0]);
                let cy = p[1] + sz * (ein[1] + eout[1]);
                out.push([p[0] + sz * ein[0], p[1] + sz * ein[1]]);
                let (mut vx, mut vy) = (-sz * eout[0], -sz * eout[1]);
                for _ in 1..seg {
                    let (nx, ny) = (vx * cos_t - vy * sin_t, vx * sin_t + vy * cos_t);
                    vx = nx;
                    vy = ny;
                    out.push([cx + vx, cy + vy]);
                }
                out.push([p[0] + sz * eout[0], p[1] + sz * eout[1]]);
            }
        }
    }
}

/// Two colours mixed in OUTPUT space — the space the rasteriser
/// interpolates in, which is what makes the two-stop gradient exact
/// (r1 §6.1). The `a·(1−u) + b·u` form returns the stops bit-for-bit at
/// u = 0 and u = 1; the endpoint-exactness test relies on that.
fn lerp(a: Color, b: Color, u: f32) -> Color {
    let k = 1.0 - u;
    Color {
        r: a.r * k + b.r * u,
        g: a.g * k + b.g * u,
        b: a.b * k + b.b * u,
        a: a.a * k + b.a * u,
    }
}

/// Sutherland–Hodgman against one gradient-space bound. `t` is affine in
/// position, so interpolating the crossing by `t` is exact: both bands
/// sharing the boundary compute the identical point and the seam cannot
/// crack. Returns the vertex count written into `out`.
fn clip_t(
    input: &[([f32; 2], f32)],
    bound: f32,
    keep_ge: bool,
    out: &mut [([f32; 2], f32); 8],
) -> usize {
    let inside = |t: f32| if keep_ge { t >= bound } else { t <= bound };
    let mut m = 0;
    let n = input.len();
    for i in 0..n {
        let (p0, t0) = input[i];
        let (p1, t1) = input[(i + 1) % n];
        let (in0, in1) = (inside(t0), inside(t1));
        if in0 {
            out[m] = (p0, t0);
            m += 1;
        }
        if in0 != in1 {
            let u = (bound - t0) / (t1 - t0);
            out[m] = (
                [p0[0] + (p1[0] - p0[0]) * u, p0[1] + (p1[1] - p0[1]) * u],
                bound,
            );
            m += 1;
        }
    }
    m
}

/// One contiguous run of vertices sampling one texture: the glyph
/// atlas (`None`) or a registered image. Runs partition the vertex
/// list in emission order, which is what keeps images correctly
/// layered between the things drawn before and after them.
#[derive(Clone, Copy)]
pub struct DrawRun {
    pub image: Option<ImageId>,
    /// One past the run's last vertex; the run starts where the
    /// previous one ended.
    pub end: u32,
    /// Scissor for this run, in device px, already intersected down the
    /// clip stack (r1's R2). None = the whole target. The renderer maps it
    /// to `cmd_set_scissor`, which is already dynamic state — clipping a
    /// ribbon, a scrolling list or the terminal costs nothing per frame.
    pub clip: Option<[f32; 4]>,
}

pub struct DrawList {
    pub verts: Vec<Vertex>,
    pub runs: Vec<DrawRun>,
    /// The clip stack: pushes intersect, pops restore. The TOP is stamped
    /// onto every run the moment it is opened.
    clips: Vec<[f32; 4]>,
    /// Reused ring-point buffers (r1 §5.3): the generators borrow them via
    /// mem::take so a ring costs no allocation after the first frame.
    scratch_a: Vec<[f32; 2]>,
    scratch_b: Vec<[f32; 2]>,
}

impl DrawList {
    pub fn new() -> Self {
        DrawList {
            verts: Vec::with_capacity(1 << 16),
            runs: Vec::new(),
            clips: Vec::new(),
            scratch_a: Vec::new(),
            scratch_b: Vec::new(),
        }
    }

    pub fn clear(&mut self) {
        self.verts.clear();
        self.runs.clear();
        self.clips.clear();
    }

    /// Clip everything drawn until the matching pop to this rect,
    /// intersected with whatever is already clipping. Unbalanced pushes are
    /// forgiven at clear() — a widget that early-returns must not wedge the
    /// whole frame.
    pub fn push_clip(&mut self, x: f32, y: f32, w: f32, h: f32) {
        let new = match self.clips.last() {
            Some(&[cx, cy, cw, ch]) => {
                let x0 = x.max(cx);
                let y0 = y.max(cy);
                let x1 = (x + w).min(cx + cw);
                let y1 = (y + h).min(cy + ch);
                [x0, y0, (x1 - x0).max(0.0), (y1 - y0).max(0.0)]
            }
            None => [x, y, w.max(0.0), h.max(0.0)],
        };
        self.clips.push(new);
        // A clip change is a run boundary even for the same texture.
        self.runs.push(DrawRun {
            image: self.runs.last().and_then(|r| r.image),
            end: self.verts.len() as u32,
            clip: Some(new),
        });
    }

    pub fn pop_clip(&mut self) {
        self.clips.pop();
        let clip = self.clips.last().copied();
        self.runs.push(DrawRun {
            image: self.runs.last().and_then(|r| r.image),
            end: self.verts.len() as u32,
            clip,
        });
    }

    /// Makes sure the vertices about to be pushed extend a run that
    /// samples `image`, starting a new run when the texture changes.
    fn run_for(&mut self, image: Option<ImageId>) {
        let clip = self.clips.last().copied();
        match self.runs.last_mut() {
            Some(run) if run.image == image && run.clip == clip => {}
            _ => self.runs.push(DrawRun { image, end: self.verts.len() as u32, clip }),
        }
    }

    fn seal(&mut self) {
        if let Some(run) = self.runs.last_mut() {
            run.end = self.verts.len() as u32;
        }
    }

    /// One quad, any binding, a colour per vertex — every shape above the
    /// glyph level funnels into here. `Vertex.color` was always interpolated
    /// by the rasteriser; the list simply never exposed it (r1 §0, fact 1).
    fn push_quad4(
        &mut self,
        image: Option<ImageId>,
        p: [[f32; 2]; 4],
        uv: [[f32; 2]; 4],
        c: [[f32; 4]; 4],
    ) {
        self.run_for(image);
        let v = |i: usize| Vertex { pos: p[i], uv: uv[i], color: c[i] };
        self.verts.extend_from_slice(&[v(0), v(1), v(2), v(0), v(2), v(3)]);
        self.seal();
    }

    /// One triangle over the atlas white pixel, a colour per vertex — the
    /// fan primitives' unit.
    fn push_tri_c(&mut self, image: Option<ImageId>, p: [[f32; 2]; 3], c: [[f32; 4]; 3]) {
        self.run_for(image);
        let (u, v) = FontSystem::white_uv();
        let vx = |i: usize| Vertex { pos: p[i], uv: [u, v], color: c[i] };
        self.verts.extend_from_slice(&[vx(0), vx(1), vx(2)]);
        self.seal();
    }

    fn push_quad(&mut self, p: [[f32; 2]; 4], uv: [[f32; 2]; 4], color: Color) {
        let c = color.to_array();
        self.push_quad4(None, p, uv, [c; 4]);
    }

    /// A rectangle filled with a registered image, whole. The color
    /// multiplies the image — white leaves it as it is, the alpha
    /// fades it.
    pub fn image(&mut self, x: f32, y: f32, w: f32, h: f32, id: ImageId, tint: Color) {
        self.run_for(Some(id));
        let c = tint.to_array();
        let p = [[x, y], [x + w, y], [x + w, y + h], [x, y + h]];
        let uv = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
        let v = |i: usize| Vertex { pos: p[i], uv: uv[i], color: c };
        self.verts.extend_from_slice(&[v(0), v(1), v(2), v(0), v(2), v(3)]);
        self.seal();
    }

    /// Frosted glass over the given rectangle: what was drawn before
    /// the first glass quad this frame shows through it blurred,
    /// tinted by `tint` (white leaves the blur as it is). The renderer
    /// samples by SCREEN position, so these vertices may be translated
    /// afterwards — an animation can carry the glass around and the
    /// frost stays put on the picture beneath.
    pub fn blur(&mut self, x: f32, y: f32, w: f32, h: f32, tint: Color) {
        self.run_for(Some(BLUR_IMAGE));
        let c = tint.to_array();
        let (u, v) = FontSystem::white_uv();
        let p = [[x, y], [x + w, y], [x + w, y + h], [x, y + h]];
        let vx = |i: usize| Vertex { pos: p[i], uv: [u, v], color: c };
        self.verts
            .extend_from_slice(&[vx(0), vx(1), vx(2), vx(0), vx(2), vx(3)]);
        self.seal();
    }

    /// Arbitrary quadrilateral (vertices along the perimeter).
    pub fn quad(&mut self, p: [[f32; 2]; 4], color: Color) {
        let (u, v) = FontSystem::white_uv();
        self.push_quad(p, [[u, v]; 4], color);
    }

    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: Color) {
        self.quad([[x, y], [x + w, y], [x + w, y + h], [x, y + h]], color);
    }

    pub fn rect_outline(&mut self, x: f32, y: f32, w: f32, h: f32, t: f32, color: Color) {
        self.rect(x, y, w, t, color);
        self.rect(x, y + h - t, w, t, color);
        self.rect(x, y + t, t, h - 2.0 * t, color);
        self.rect(x + w - t, y + t, t, h - 2.0 * t, color);
    }

    pub fn line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, t: f32, color: Color) {
        let dx = x1 - x0;
        let dy = y1 - y0;
        let len = (dx * dx + dy * dy).sqrt().max(0.0001);
        let nx = -dy / len * t * 0.5;
        let ny = dx / len * t * 0.5;
        self.quad(
            [
                [x0 + nx, y0 + ny],
                [x1 + nx, y1 + ny],
                [x1 - nx, y1 - ny],
                [x0 - nx, y0 - ny],
            ],
            color,
        );
    }

    pub fn polyline(&mut self, pts: &[[f32; 2]], t: f32, color: Color, closed: bool) {
        if pts.len() < 2 {
            return;
        }
        for w in pts.windows(2) {
            self.line(w[0][0], w[0][1], w[1][0], w[1][1], t, color);
        }
        if closed {
            let a = pts[pts.len() - 1];
            let b = pts[0];
            self.line(a[0], a[1], b[0], b[1], t, color);
        }
    }

    /// Frame with clipped corners in the augmented-ui style (eDEX panels).
    ///
    /// The stroke is drawn INSIDE the rect. The rect comes from layout and the
    /// width from a theme token, and only this convention keeps the theme out
    /// of panel geometry: a heavier border under one theme must never grow the
    /// thing it borders (r1's ruling on the centred-vs-inside split —
    /// `rect_outline` was already inside, this path was centred, and the two
    /// disagreed by half a stroke).
    ///
    /// The polyline is centred on its own path, so the path is inset by t/2 —
    /// and the 45° face needs more than that: offsetting the line x + y = k
    /// inward by d moves its CONSTANT by d·√2, so the cut length measured
    /// along the axes changes by d·(√2−1) each side, i.e. the effective cut
    /// shrinks by (2−√2)·t/2 ≈ 0.293·t. The earlier t/2 guess left the face
    /// 0.44 px outside the rect at stroke.regular (r1's derivation).
    pub fn chamfer_frame(&mut self, x: f32, y: f32, w: f32, h: f32, cut: f32, t: f32, color: Color) {
        // A wrapper over the one ring generator (r1 §3.3): identical band —
        // outer face on the rect, inner face `t` further in, the 45° face
        // shortened per Corner::inset — at the same 48 vertices, but
        // watertight where the polyline overlapped and notched at joints.
        self.ring(Rect::new(x, y, w, h), &[Corner::chamfer(cut); 4], 3, t, color);
    }

    /// The filled counterpart of `chamfer_frame`: the very octagon the
    /// frame outlines, as three quads. A background drawn with this
    /// stays inside the border instead of poking past the cut corners.
    pub fn chamfer_fill(&mut self, x: f32, y: f32, w: f32, h: f32, cut: f32, color: Color) {
        let cut = cut.min(w * 0.5).min(h * 0.5).max(0.0);
        self.quad(
            [[x + cut, y], [x + w - cut, y], [x + w - cut, y + h], [x + cut, y + h]],
            color,
        );
        self.quad(
            [[x, y + cut], [x + cut, y], [x + cut, y + h], [x, y + h - cut]],
            color,
        );
        self.quad(
            [[x + w - cut, y], [x + w, y + cut], [x + w, y + h - cut], [x + w - cut, y + h]],
            color,
        );
    }

    /// Stroked ring over `r` — the one tessellated generator (r1 §3): a
    /// Square, Chamfer or Round treatment per corner, `stroke` px wide,
    /// drawn INSIDE the rect. The rect is layout's and the width is the
    /// theme's, and only this alignment keeps the theme's knob out of the
    /// layout's geometry: a heavier border must never grow the thing it
    /// borders. Emitted as one quad per boundary segment between the rect's
    /// own boundary and the boundary inset by `stroke` (corners via
    /// Corner::inset, which carries chamfer_frame's 0.293·t derivation at
    /// full width), so the outer face is exactly flush with `r`, nothing
    /// leaks, and nothing overlaps — which additive runs care about.
    /// Cost: all-square 24 verts (rect_outline's price), all-chamfer 48
    /// (chamfer_frame's), round 6·(S+1) per corner; `segments` from
    /// ring_segments() with the theme's ceiling.
    pub fn ring(&mut self, r: Rect, c: &[Corner; 4], segments: u8, stroke: f32, color: Color) {
        if r.w <= 0.0 || r.h <= 0.0 {
            return;
        }
        let t = stroke.max(0.0).min(r.w.min(r.h) * 0.5);
        if t <= 0.0 {
            return;
        }
        let inner_r = Rect::new(
            r.x + t,
            r.y + t,
            (r.w - 2.0 * t).max(0.0),
            (r.h - 2.0 * t).max(0.0),
        );
        let ci = [c[0].inset(t), c[1].inset(t), c[2].inset(t), c[3].inset(t)];
        let mut outer = std::mem::take(&mut self.scratch_a);
        let mut inner = std::mem::take(&mut self.scratch_b);
        ring_points(r, c, segments, &mut outer);
        ring_points(inner_r, &ci, segments, &mut inner);
        debug_assert_eq!(outer.len(), inner.len());
        let (u, v) = FontSystem::white_uv();
        let col = color.to_array();
        let n = outer.len();
        for i in 0..n {
            let j = (i + 1) % n;
            self.push_quad4(
                None,
                [outer[i], outer[j], inner[j], inner[i]],
                [[u, v]; 4],
                [col; 4],
            );
        }
        self.scratch_a = outer;
        self.scratch_b = inner;
    }

    /// Filled interior of the same ring, the fill counterpart of ring().
    /// Fast paths keep the shapes the program already draws at their old
    /// price: all-Square is one quad (6 verts — exactly rect()), all-Chamfer
    /// at one cut is chamfer_fill's three quads (18). Everything else fans
    /// from the centroid at 3 verts per boundary point. Drawn on the
    /// ORIGINAL rect: the fill must reach the rect edge under the border,
    /// and the z-order puts the ring above it.
    pub fn ring_fill(&mut self, r: Rect, c: &[Corner; 4], segments: u8, color: Color) {
        if r.w <= 0.0 || r.h <= 0.0 {
            return;
        }
        if c.iter().all(|k| k.style == CornerStyle::Square) {
            self.rect(r.x, r.y, r.w, r.h, color);
            return;
        }
        if c.iter().all(|k| k.style == CornerStyle::Chamfer && k.size == c[0].size) {
            self.chamfer_fill(r.x, r.y, r.w, r.h, c[0].size, color);
            return;
        }
        let mut pts = std::mem::take(&mut self.scratch_a);
        ring_points(r, c, segments, &mut pts);
        let n = pts.len();
        if n >= 3 {
            let (mut cx, mut cy) = (0.0f32, 0.0f32);
            for p in &pts {
                cx += p[0];
                cy += p[1];
            }
            let inv = 1.0 / n as f32;
            let (cx, cy) = (cx * inv, cy * inv);
            let col = color.to_array();
            for i in 0..n {
                let j = (i + 1) % n;
                self.push_tri_c(None, [[cx, cy], pts[i], pts[j]], [col; 3]);
            }
        }
        self.scratch_a = pts;
    }

    /// Quadrilateral with a colour per vertex — the entry point every
    /// gradient is built on (r1 §6). A two-stop gradient interpolated in
    /// output space is affine in (x, y), and Gouraud reproduces an affine
    /// function exactly on any triangulation: no diagonal seam, no bands,
    /// 6 verts at any angle.
    pub fn quad_c(&mut self, p: [[f32; 2]; 4], c: [Color; 4]) {
        let (u, v) = FontSystem::white_uv();
        self.push_quad4(
            None,
            p,
            [[u, v]; 4],
            [c[0].to_array(), c[1].to_array(), c[2].to_array(), c[3].to_array()],
        );
    }

    /// Rect under a linear gradient, banded only where it must be. `stops`
    /// are (position 0..1 along the axis, colour), positions clamped and
    /// forced non-decreasing; `angle` is radians, 0 = left→right,
    /// π/2 = top→bottom (y down), t = 0 at the least-projected corner.
    /// Two stops spanning 0..1 are EXACTLY free at any angle — one quad,
    /// corner colours evaluated in output space (see quad_c). Anything
    /// else is piecewise affine, so it becomes one band per stop interval:
    /// the rect clipped to the slab between two stops, each band exact,
    /// seams sharing bitwise-identical vertices — 8 stops = 7 bands =
    /// 42 verts on an axis-aligned angle. Multi-stop or OKLab-space
    /// gradients arrive here already sampled: the resolver did that, the
    /// list never reads tokens.
    pub fn rect_grad(&mut self, r: Rect, stops: &[(f32, Color)], angle: f32) {
        if r.w <= 0.0 || r.h <= 0.0 || stops.is_empty() {
            return;
        }
        if stops.len() == 1 {
            self.rect(r.x, r.y, r.w, r.h, stops[0].1);
            return;
        }
        // Corners tl,tr,br,bl with their normalised projection onto the
        // axis. Normalising by the observed min/max makes the extreme
        // corners land on t = 0 and t = 1 exactly, at any angle.
        let (sin_a, cos_a) = angle.sin_cos();
        let p = [
            [r.x, r.y],
            [r.x + r.w, r.y],
            [r.x + r.w, r.y + r.h],
            [r.x, r.y + r.h],
        ];
        let proj = [
            p[0][0] * cos_a + p[0][1] * sin_a,
            p[1][0] * cos_a + p[1][1] * sin_a,
            p[2][0] * cos_a + p[2][1] * sin_a,
            p[3][0] * cos_a + p[3][1] * sin_a,
        ];
        let (mut lo, mut hi) = (proj[0], proj[0]);
        for &v in &proj[1..] {
            lo = lo.min(v);
            hi = hi.max(v);
        }
        let span = (hi - lo).max(1e-6);
        let s0 = stops[0].0.clamp(0.0, 1.0);
        let s_last = stops[stops.len() - 1].0.clamp(0.0, 1.0);
        if stops.len() == 2 && s0 == 0.0 && s_last == 1.0 {
            let t = |i: usize| (proj[i] - lo) / span;
            let c = |i: usize| lerp(stops[0].1, stops[1].1, t(i)).to_array();
            let (u, v) = FontSystem::white_uv();
            self.push_quad4(None, p, [[u, v]; 4], [c(0), c(1), c(2), c(3)]);
            return;
        }
        // Band edges: 0, every stop, 1 — the flat caps fall out as bands
        // between two equal colours, zero-width bands are skipped. Within a
        // band the stop function is affine by construction.
        let corners: [([f32; 2], f32); 4] = [
            (p[0], (proj[0] - lo) / span),
            (p[1], (proj[1] - lo) / span),
            (p[2], (proj[2] - lo) / span),
            (p[3], (proj[3] - lo) / span),
        ];
        let band = |a: (f32, Color), b: (f32, Color), list: &mut Self| {
            if b.0 - a.0 <= 1e-6 {
                return;
            }
            let mut buf1 = [([0.0f32; 2], 0.0f32); 8];
            let mut buf2 = [([0.0f32; 2], 0.0f32); 8];
            let n1 = clip_t(&corners, a.0, true, &mut buf1);
            let n2 = clip_t(&buf1[..n1], b.0, false, &mut buf2);
            if n2 < 3 {
                return;
            }
            let colour = |t: f32| lerp(a.1, b.1, (t - a.0) / (b.0 - a.0)).to_array();
            let (v0, t0) = buf2[0];
            for i in 1..n2 - 1 {
                let (v1, t1) = buf2[i];
                let (v2, t2) = buf2[i + 1];
                list.push_tri_c(None, [v0, v1, v2], [colour(t0), colour(t1), colour(t2)]);
            }
        };
        let mut prev = (0.0f32, stops[0].1);
        let mut running = 0.0f32;
        for &(pos, col) in stops {
            running = running.max(pos.clamp(0.0, 1.0));
            band(prev, (running, col), self);
            prev = (running, col);
        }
        band(prev, (1.0, prev.1), self);
    }

    /// Triangle fan around `centre`, one colour per rim point, the rim
    /// CLOSED — rim[k−1] joins rim[0]: hexagons, reticles, radial washes
    /// (r1 §6.3, 3·k verts). An open wedge is quad_c's job; closing is what
    /// makes a k-gon k triangles. Fewer than 3 rim points draw nothing.
    pub fn fan_c(&mut self, centre: [f32; 2], rim: &[[f32; 2]], c_centre: Color, c_rim: &[Color]) {
        let n = rim.len().min(c_rim.len());
        if n < 3 {
            return;
        }
        let cc = c_centre.to_array();
        for i in 0..n {
            let j = (i + 1) % n;
            self.push_tri_c(
                None,
                [centre, rim[i], rim[j]],
                [cc, c_rim[i].to_array(), c_rim[j].to_array()],
            );
        }
    }

    /// An image quad with explicit texture coordinates, corner order
    /// tl,tr,br,bl: sub-rect sprites, tiled decoration under a REPEAT
    /// sampler, the scanline plate's drifting window (r1 §6.3). The tint
    /// multiplies as in image(); the UVs are the caller's business — a
    /// reserved handle here is deliberate, not policed, because the sprite
    /// glow endgame is exactly ADD_ATLAS with explicit UVs.
    pub fn image_uv(&mut self, r: Rect, uv: [[f32; 2]; 4], id: ImageId, tint: Color) {
        let p = [
            [r.x, r.y],
            [r.x + r.w, r.y],
            [r.x + r.w, r.y + r.h],
            [r.x, r.y + r.h],
        ];
        self.push_quad4(Some(id), p, uv, [tint.to_array(); 4]);
    }

    /// One quad over the soft-mask sprite, adding or covering — the
    /// "ADD_ATLAS with explicit UVs" endgame [`DrawList::image_uv`]'s
    /// comment names, but with the coordinates in the SPRITE's own 0..1
    /// space rather than the atlas's. Each uv is clamped to the unit
    /// square and mapped into `band` (`FontSystem::mask_soft_uv()`,
    /// passed by the caller — the list keeps no font-system state), so a
    /// caller can address the disk's profile and nothing else: glyph
    /// texels stay unreachable whatever numbers arrive, which is what
    /// lets the plugin ABI expose this without policing its input. An
    /// EMPTY band (u1 ≤ u0 or v1 ≤ v0) is the maskless degenerate
    /// case and falls back to the atlas's white pixel — a solid quad,
    /// raw but present, the same discipline as `soft_box`. `additive`
    /// picks light (the ADD_ATLAS run — glow) over cover (the normal
    /// atlas run — shadow).
    pub fn mask_quad(
        &mut self,
        p: [[f32; 2]; 4],
        uv: [[f32; 2]; 4],
        band: (f32, f32, f32, f32),
        color: Color,
        additive: bool,
    ) {
        if color.a <= 0.0 {
            return;
        }
        let (u0, v0, u1, v1) = band;
        let m: [[f32; 2]; 4] = if u1 <= u0 || v1 <= v0 {
            let (u, v) = FontSystem::white_uv();
            [[u, v]; 4]
        } else {
            std::array::from_fn(|i| {
                [
                    u0 + (u1 - u0) * uv[i][0].clamp(0.0, 1.0),
                    v0 + (v1 - v0) * uv[i][1].clamp(0.0, 1.0),
                ]
            })
        };
        let image = if additive { Some(ADD_ATLAS) } else { None };
        self.push_quad4(image, p, m, [color.to_array(); 4]);
    }

    /// Glow OUTSIDE the ring, in an additive run — through ADD_ATLAS the
    /// pipeline adds light instead of filming milk over a lit backdrop.
    ///
    /// The real technique (r1 §4.1/§4.3): the soft-disk mask from the R8
    /// band, laid along the ring's OWN path: the outline — every corner
    /// in its declared style — extruded outward by `radius`, with the
    /// disk's 2-texel cardinal strip across the extrusion. A rounded
    /// corner therefore glows on its own arc grown by the glow, a
    /// chamfered corner glows along its diagonal, and a square corner
    /// mitres — the glow always matches the shape it wraps. Nothing is
    /// emitted inside the path, so the glow never tints a translucent
    /// fill. Cost: one quad per outline segment — 48 verts around a
    /// chamfered panel, 24 square, 168 at round S=6 — still far under
    /// the shell fallback. `mask_uv` is `FontSystem::mask_soft_uv()`,
    /// passed by the caller (Ctx has it; the draw list stays free of the
    /// font system).
    ///
    /// An EMPTY `mask_uv` (u1 ≤ u0 or v1 ≤ v0) is the maskless
    /// degenerate case and falls back to the concentric-shell
    /// approximation below — a themeless run must still draw something
    /// raw. The renderer binds ADD_ATLAS since r1 P7, so both forms
    /// RENDER; only an actual texture miss still drops the run — the
    /// glow is then absent, never wrong.
    pub fn glow_ring(
        &mut self,
        r: Rect,
        c: &[Corner; 4],
        segments: u8,
        radius: f32,
        color: Color,
        mask_uv: (f32, f32, f32, f32),
    ) {
        if !(radius > 0.0) || color.a <= 0.0 || r.w <= 0.0 || r.h <= 0.0 {
            return;
        }
        let (u0, v0, u1, v1) = mask_uv;
        if u1 <= u0 || v1 <= v0 {
            self.glow_shell(r, c, segments, radius, color);
            return;
        }
        let mut inner = std::mem::take(&mut self.scratch_a);
        let mut outer = std::mem::take(&mut self.scratch_b);
        ring_points(r, c, segments, &mut inner);
        let grown = Rect::new(
            r.x - radius,
            r.y - radius,
            r.w + 2.0 * radius,
            r.h + 2.0 * radius,
        );
        let ck = [
            c[0].inset(-radius),
            c[1].inset(-radius),
            c[2].inset(-radius),
            c[3].inset(-radius),
        ];
        ring_points(grown, &ck, segments, &mut outer);
        // The strip: u pinned to the centre of the stretchable band, v
        // running from the disk's peak on the path to the sprite's zero
        // at the outer rim — the same profile the nine-slice edges
        // carried, now perpendicular to the path everywhere, corners
        // included. Point counts agree because counts depend only on
        // corner STYLE, which inset() preserves (glow_shell's invariant).
        let su = u0 + (u1 - u0) * (32.0 / 64.0);
        let vi = v0 + (v1 - v0) * (31.0 / 64.0);
        let col = color.to_array();
        let n = inner.len().min(outer.len());
        for i in 0..n {
            let j = (i + 1) % n;
            self.push_quad4(
                Some(ADD_ATLAS),
                [inner[i], inner[j], outer[j], outer[i]],
                [[su, vi], [su, vi], [su, v0], [su, v0]],
                [col; 4],
            );
        }
        self.scratch_a = inner;
        self.scratch_b = outer;
    }

    /// The nine-slice core every soft sprite shape shares (r1 §4.3): `r`
    /// cut at `cell` px from each side, the mask's corner cells pinned to
    /// the corners, its 2-texel middle band stretched along the edges and
    /// across the centre. `centre = false` drops the middle quad (8 quads,
    /// 48 verts — the glows); `true` keeps it (9 quads, 54 — the fills).
    /// The 31/64 · 33/64 split is the mask-band CONTRACT's geometry (a
    /// 64-texel sprite whose stretchable middle is texels 31..33, r1 §4.2
    /// via font::MASK_SOFT), not a design value.
    fn nine_slice(
        &mut self,
        image: Option<ImageId>,
        r: Rect,
        cell: f32,
        uv: (f32, f32, f32, f32),
        color: Color,
        centre: bool,
    ) {
        let (u0, v0, u1, v1) = uv;
        let cell = cell.clamp(0.0, r.w.min(r.h) * 0.5);
        let xs = [r.x, r.x + cell, r.x + r.w - cell, r.x + r.w];
        let ys = [r.y, r.y + cell, r.y + r.h - cell, r.y + r.h];
        let (su, sv) = (u1 - u0, v1 - v0);
        let us = [u0, u0 + su * (31.0 / 64.0), u0 + su * (33.0 / 64.0), u1];
        let vs = [v0, v0 + sv * (31.0 / 64.0), v0 + sv * (33.0 / 64.0), v1];
        let col = color.to_array();
        for j in 0..3 {
            for i in 0..3 {
                if i == 1 && j == 1 && !centre {
                    continue;
                }
                self.push_quad4(
                    image,
                    [
                        [xs[i], ys[j]],
                        [xs[i + 1], ys[j]],
                        [xs[i + 1], ys[j + 1]],
                        [xs[i], ys[j + 1]],
                    ],
                    [
                        [us[i], vs[j]],
                        [us[i + 1], vs[j]],
                        [us[i + 1], vs[j + 1]],
                        [us[i], vs[j + 1]],
                    ],
                    [col; 4],
                );
            }
        }
    }

    /// FILLED soft rectangle: the same nine-slice with the centre kept, in
    /// a normal-blend atlas run — the shadow bed under a panel, not light.
    /// `radius` is the feather: alpha is zero exactly on the rect boundary
    /// and ramps to the disk's peak over `radius` px, so the whole soft
    /// shape stays INSIDE `r` (the caller inflates when it wants the blur
    /// to reach past an edge — shadow() below does exactly that). 54 verts.
    /// An empty `mask_uv` degrades raw to a plain filled rect.
    pub fn soft_box(&mut self, r: Rect, radius: f32, color: Color, mask_uv: (f32, f32, f32, f32)) {
        if r.w <= 0.0 || r.h <= 0.0 || color.a <= 0.0 {
            return;
        }
        let (u0, v0, u1, v1) = mask_uv;
        if u1 <= u0 || v1 <= v0 {
            self.rect(r.x, r.y, r.w, r.h, color);
            return;
        }
        self.nine_slice(None, r, radius.max(0.0), mask_uv, color, true);
    }

    /// Drop shadow under a panel: soft_box over `r` translated by `offset`
    /// and inflated by `radius`, so the feather reaches `radius` px past
    /// every edge of the shifted rect and the plateau still covers the
    /// panel's own footprint. Normal blend — a shadow subtracts by
    /// covering, it is not light. Offset, radius and colour are the
    /// caller's tokens (shadow.dx/dy, shadow.radius, shadow.color);
    /// nothing here defaults them.
    pub fn shadow(&mut self, r: Rect, offset: [f32; 2], radius: f32, color: Color, mask_uv: (f32, f32, f32, f32)) {
        let radius = radius.max(0.0);
        self.soft_box(
            Rect::new(
                r.x + offset[0] - radius,
                r.y + offset[1] - radius,
                r.w + 2.0 * radius,
                r.h + 2.0 * radius,
            ),
            radius,
            color,
            mask_uv,
        );
    }

    /// r1 §4.1's shell approximation, kept as glow_ring's maskless
    /// degenerate case: concentric ring strokes whose alpha falls
    /// quadratically with distance. Cost: 3–5 shells at one ring stroke
    /// each — 144–240 verts around a chamfered panel, 504–840 at round
    /// S=6, ten times the sprite — which is why it is only the fallback.
    /// Shells share their boundary rings, so nothing overlaps and additive
    /// blending cannot double-brighten a seam. Corner sizes grow with each
    /// shell (round concentric exactly, chamfer by the parallel-face
    /// offset); a Square corner stays square — the shell technique's
    /// stated approximation.
    fn glow_shell(&mut self, r: Rect, c: &[Corner; 4], segments: u8, radius: f32, color: Color) {
        // Shells thinner than ~2 px stop reading as steps of the falloff —
        // a quality clamp on the approximation, not a design value; the
        // radius and the peak alpha both belong to the caller's tokens.
        let shells = ((radius * 0.5).ceil()).clamp(3.0, 5.0) as u32;
        let step = radius / shells as f32;
        let mut prev = std::mem::take(&mut self.scratch_a);
        let mut cur = std::mem::take(&mut self.scratch_b);
        ring_points(r, c, segments, &mut prev);
        let (u, v) = FontSystem::white_uv();
        for k in 1..=shells {
            let d = step * k as f32;
            let grown = Rect::new(r.x - d, r.y - d, r.w + 2.0 * d, r.h + 2.0 * d);
            let ck = [
                c[0].inset(-d),
                c[1].inset(-d),
                c[2].inset(-d),
                c[3].inset(-d),
            ];
            ring_points(grown, &ck, segments, &mut cur);
            // (1 − u)² sampled at the shell midline: the cheap honest
            // stand-in for a blur tail, scaled by the caller's alpha.
            let f = 1.0 - (k as f32 - 0.5) / shells as f32;
            let col = Color { a: color.a * f * f, ..color }.to_array();
            let n = prev.len();
            for i in 0..n {
                let j = (i + 1) % n;
                self.push_quad4(
                    Some(ADD_ATLAS),
                    [prev[i], prev[j], cur[j], cur[i]],
                    [[u, v]; 4],
                    [col; 4],
                );
            }
            std::mem::swap(&mut prev, &mut cur);
        }
        self.scratch_a = prev;
        self.scratch_b = cur;
    }

    fn glyph_quad(&mut self, g: &Glyph, pen_x: f32, baseline: f32, color: Color) {
        if g.w <= 0.0 {
            return;
        }
        let x0 = (pen_x + g.xmin).round();
        let y1 = (baseline - g.ymin).round(); // bitmap bottom
        let y0 = y1 - g.h;
        let x1 = x0 + g.w;
        self.push_quad(
            [[x0, y0], [x1, y0], [x1, y1], [x0, y1]],
            [
                [g.u0, g.v0],
                [g.u1, g.v0],
                [g.u1, g.v1],
                [g.u0, g.v1],
            ],
            color,
        );
    }

    /// Draws text; (x, y) is the top-left corner of the text box.
    pub fn text(
        &mut self,
        fs: &mut FontSystem,
        font: u8,
        px: f32,
        x: f32,
        y: f32,
        text: &str,
        color: Color,
        letter_spacing: f32,
    ) {
        let (ascent, _) = fs.line_metrics(font, px);
        let baseline = y + ascent;
        let mut pen = x;
        for ch in text.chars() {
            if let Some(g) = fs.glyph(font, px, ch) {
                self.glyph_quad(&g, pen, baseline, color);
                pen += g.advance + letter_spacing;
            }
        }
    }

    /// Text horizontally centered on cx.
    #[allow(clippy::too_many_arguments)]
    pub fn text_center(
        &mut self,
        fs: &mut FontSystem,
        font: u8,
        px: f32,
        cx: f32,
        y: f32,
        text: &str,
        color: Color,
        letter_spacing: f32,
    ) {
        let w = fs.measure(font, px, text, letter_spacing);
        self.text(fs, font, px, cx - w / 2.0, y, text, color, letter_spacing);
    }

    /// Text right-aligned to the rx edge.
    #[allow(clippy::too_many_arguments)]
    pub fn text_right(
        &mut self,
        fs: &mut FontSystem,
        font: u8,
        px: f32,
        rx: f32,
        y: f32,
        text: &str,
        color: Color,
        letter_spacing: f32,
    ) {
        let w = fs.measure(font, px, text, letter_spacing);
        self.text(fs, font, px, rx - w, y, text, color, letter_spacing);
    }

    /// Module header: text on the left, optionally on the right, and an
    /// optional plain underline. Any part can be left out — empty text
    /// with the underline on gives just the line.
    #[allow(clippy::too_many_arguments)]
    pub fn module_title(
        &mut self,
        fs: &mut FontSystem,
        x: f32,
        y: f32,
        w: f32,
        px: f32,
        left: &str,
        right: &str,
        color: Color,
        underline: bool,
    ) {
        // The five constants that survived the first wave, tokened: this is
        // the one text path with no Ctx, so it reads the resolved theme
        // directly. em tokens bake to bare multipliers of the caller's px.
        use crate::theme::{self, TokenId};
        use std::sync::OnceLock;
        fn tokc(cell: &'static OnceLock<TokenId>, name: &'static str) -> TokenId {
            *cell.get_or_init(|| theme::id(name).unwrap_or(TokenId::MISSING))
        }
        static LEAD: OnceLock<TokenId> = OnceLock::new();
        static TRACK: OnceLock<TokenId> = OnceLock::new();
        static PAD: OnceLock<TokenId> = OnceLock::new();
        static GAP: OnceLock<TokenId> = OnceLock::new();
        static RULE: OnceLock<TokenId> = OnceLock::new();
        static RULE_COL: OnceLock<TokenId> = OnceLock::new();
        let t = theme::resolved();
        let h = px * t.px(tokc(&LEAD, "component.module.leading")).max(0.0);
        let font = crate::font::FONT_UI;
        let spacing = px * t.px(tokc(&TRACK, "component.module.tracking")).max(0.0);
        let pad = px * t.px(tokc(&PAD, "component.module.pad")).max(0.0);
        let gap = px * t.px(tokc(&GAP, "component.module.gap")).max(0.0);
        self.text(fs, font, px, x + pad, y, left, color, spacing);
        if !right.is_empty() {
            // The right-hand text is trimmed to whatever the left one
            // leaves. Without this the two simply overlapped in a narrow
            // panel — the CPU header wrote its model name straight
            // through its own title.
            let used = fs.measure(font, px, left, spacing) + gap;
            let room = (w - used).max(0.0);
            let shown = fit_tail(fs, font, px, right, spacing, room);
            if !shown.is_empty() {
                self.text_right(fs, font, px, x + w - pad, y, &shown, color, spacing);
            }
        }
        if underline {
            let rw = t.px(tokc(&RULE, "component.module.rule")).max(0.0);
            let rc = t.color(tokc(&RULE_COL, "component.module.rule_color"));
            self.line(
                x,
                y + h,
                x + w,
                y + h,
                rw,
                Color { r: rc.r, g: rc.g, b: rc.b, a: rc.a },
            );
        }
    }
}

/// Shortens `text` with an ellipsis until it fits `max_w`; empty when
/// there is no room even for the ellipsis. The `ui` module has the same
/// thing built on `Ctx`; this one needs only the font system, because a
/// draw list has no context.
pub(crate) fn fit_tail(
    fs: &mut FontSystem,
    font: u8,
    px: f32,
    text: &str,
    spacing: f32,
    max_w: f32,
) -> String {
    if max_w <= 0.0 {
        return String::new();
    }
    if fs.measure(font, px, text, spacing) <= max_w {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let mut n = chars.len().saturating_sub(1);
    while n > 0 {
        let cand: String = chars[..n].iter().collect::<String>() + "\u{2026}";
        if fs.measure(font, px, &cand, spacing) <= max_w {
            return cand;
        }
        n -= 1;
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The frame's stroke stays INSIDE the rect it frames. The rect is
    /// layout's and the width is the theme's, so a theme thickening a border
    /// must never move a panel edge — which the old centred stroke did, by
    /// half the width on every side.
    #[test]
    fn chamfer_frame_stroke_never_leaves_the_rect() {
        let (x, y, w, h) = (10.0, 20.0, 200.0, 100.0);
        for t in [1.0f32, 2.0, 4.0, 8.0] {
            let mut dl = DrawList::new();
            dl.chamfer_frame(x, y, w, h, 16.0, t, Color::rgb8(255, 255, 255));
            let e = 0.01;
            for v in &dl.verts {
                let [px, py] = v.pos;
                assert!(
                    px >= x - e && px <= x + w + e && py >= y - e && py <= y + h + e,
                    "stroke t={t} leaks: ({px},{py}) outside ({x},{y},{w},{h})"
                );
            }
        }
    }

    /// The generator's counts are the contract every cost estimate in r1
    /// stands on: Square 1 point, Chamfer 2, Round S+1; the stroke is one
    /// quad per boundary segment, the fill 3 verts per point past the
    /// fast paths.
    #[test]
    fn ring_vertex_counts_per_corner_mode() {
        let r = Rect::new(10.0, 20.0, 200.0, 100.0);
        let cases: [(&[Corner; 4], usize, usize); 4] = [
            // all square: rect_outline's price / rect's price
            (&[Corner::SQUARE; 4], 24, 6),
            // all chamfer: chamfer_frame's / chamfer_fill's price
            (&[Corner::chamfer(12.0); 4], 48, 18),
            // all round S=6: 28 points -> 168 stroke, 84 fan
            (&[Corner::round(8.0); 4], 168, 84),
            // mixed chamfer tl+br, square tr+bl: 6 points
            (
                &[
                    Corner::chamfer(12.0),
                    Corner::SQUARE,
                    Corner::chamfer(12.0),
                    Corner::SQUARE,
                ],
                36,
                18,
            ),
        ];
        for (c, stroke_verts, fill_verts) in cases {
            let mut dl = DrawList::new();
            dl.ring(r, c, 6, 2.0, Color::rgb8(255, 255, 255));
            assert_eq!(dl.verts.len(), stroke_verts, "stroke {c:?}");
            let mut dl = DrawList::new();
            dl.ring_fill(r, c, 6, Color::rgb8(255, 255, 255));
            assert_eq!(dl.verts.len(), fill_verts, "fill {c:?}");
        }
    }

    /// The adaptive segment rule at the shipped corner ladder (r1 §3.4):
    /// 3/3/4 at a 0.25 px chord tolerance, and the theme's ceiling always
    /// wins on large radii.
    #[test]
    fn ring_segments_matches_the_shipped_ladder() {
        assert_eq!(ring_segments(4.3, 0.25, 6), 3);
        assert_eq!(ring_segments(6.5, 0.25, 6), 3);
        assert_eq!(ring_segments(11.9, 0.25, 6), 4);
        assert_eq!(ring_segments(1000.0, 0.25, 6), 6);
    }

    /// Every corner mix, every width: the stroke never leaves the rect —
    /// the rect is layout's, the width the theme's — AND it stays flush,
    /// touching all four edges. Inside but shrunken would be a different
    /// bug with the same containment signature.
    #[test]
    fn ring_stroke_stays_inside_and_flush() {
        let r = Rect::new(10.0, 20.0, 200.0, 100.0);
        let cases: [[Corner; 4]; 4] = [
            [Corner::SQUARE; 4],
            [Corner::chamfer(16.0); 4],
            [Corner::round(16.0); 4],
            [
                Corner::chamfer(40.0),
                Corner::SQUARE,
                Corner::round(12.0),
                Corner::chamfer(4.0),
            ],
        ];
        for c in &cases {
            for t in [1.0f32, 2.0, 4.0, 8.0] {
                let mut dl = DrawList::new();
                dl.ring(r, c, 6, t, Color::rgb8(255, 255, 255));
                let e = 1e-3;
                let (mut lo_x, mut hi_x) = (f32::MAX, f32::MIN);
                let (mut lo_y, mut hi_y) = (f32::MAX, f32::MIN);
                for v in &dl.verts {
                    let [px, py] = v.pos;
                    assert!(
                        px >= r.x - e
                            && px <= r.x + r.w + e
                            && py >= r.y - e
                            && py <= r.y + r.h + e,
                        "stroke t={t} leaks: ({px},{py}) outside {c:?}"
                    );
                    lo_x = lo_x.min(px);
                    hi_x = hi_x.max(px);
                    lo_y = lo_y.min(py);
                    hi_y = hi_y.max(py);
                }
                assert!((lo_x - r.x).abs() <= e, "left edge not flush, t={t} {c:?}");
                assert!((hi_x - r.right()).abs() <= e, "right edge not flush, t={t} {c:?}");
                assert!((lo_y - r.y).abs() <= e, "top edge not flush, t={t} {c:?}");
                assert!((hi_y - r.bottom()).abs() <= e, "bottom edge not flush, t={t} {c:?}");
            }
        }
    }

    /// The fill through the generator honours the same boundary the stroke
    /// does — the retargeted successor of the chamfer_fill test below.
    #[test]
    fn ring_fill_stays_inside_the_rect() {
        let r = Rect::new(10.0, 20.0, 200.0, 100.0);
        let c = [
            Corner::round(16.0),
            Corner::chamfer(24.0),
            Corner::SQUARE,
            Corner::round(8.0),
        ];
        let mut dl = DrawList::new();
        dl.ring_fill(r, &c, 6, Color::rgb8(255, 255, 255));
        assert!(!dl.verts.is_empty());
        let e = 1e-3;
        for v in &dl.verts {
            let [px, py] = v.pos;
            assert!(px >= r.x - e && px <= r.x + r.w + e && py >= r.y - e && py <= r.y + r.h + e);
        }
    }

    /// The two-stop fast path is one quad whose extreme corners carry the
    /// stop colours BIT-FOR-BIT — the a·(1−u) + b·u lerp guarantees it —
    /// and it stays one quad at any angle, because an output-space two-stop
    /// gradient is affine and Gouraud needs no bands for affine.
    #[test]
    fn gradient_two_stop_endpoints_exact_and_free() {
        let r = Rect::new(10.0, 20.0, 200.0, 100.0);
        let a = Color::rgb8(10, 200, 30).alpha(0.7);
        let b = Color::rgb8(250, 40, 90);
        let mut dl = DrawList::new();
        dl.rect_grad(r, &[(0.0, a), (1.0, b)], 0.0);
        assert_eq!(dl.verts.len(), 6, "two stops must not band");
        for v in &dl.verts {
            if v.pos[0] == r.x {
                assert_eq!(v.color, a.to_array(), "left endpoint drifted");
            }
            if v.pos[0] == r.x + r.w {
                assert_eq!(v.color, b.to_array(), "right endpoint drifted");
            }
        }
        // Any angle: still exactly one quad.
        let mut dl = DrawList::new();
        dl.rect_grad(r, &[(0.0, a), (1.0, b)], 0.6);
        assert_eq!(dl.verts.len(), 6, "the fast path is angle-independent");
    }

    /// Eight stops = seven bands = 42 verts on an axis-aligned angle
    /// (r1 §6.2): one band per stop interval, the caps zero-width, every
    /// band a single quad.
    #[test]
    fn gradient_eight_stops_band_count() {
        let r = Rect::new(0.0, 0.0, 700.0, 100.0);
        let stops: Vec<(f32, Color)> = (0..8)
            .map(|i| (i as f32 / 7.0, Color::rgb8(i as u8 * 30, 100, 200)))
            .collect();
        let mut dl = DrawList::new();
        dl.rect_grad(r, &stops, 0.0);
        assert_eq!(dl.verts.len(), 42, "7 bands of one quad each");
    }

    /// Pushed clips intersect down the stack and stamp the runs; popping
    /// restores the enclosing clip.
    #[test]
    fn clip_intersection_stamps_runs() {
        let mut dl = DrawList::new();
        let col = Color::rgb8(255, 255, 255);
        dl.push_clip(10.0, 10.0, 100.0, 100.0);
        dl.push_clip(50.0, 50.0, 100.0, 100.0);
        dl.rect(0.0, 0.0, 500.0, 500.0, col);
        assert_eq!(
            dl.runs.last().unwrap().clip,
            Some([50.0, 50.0, 60.0, 60.0]),
            "inner clip must be the intersection"
        );
        dl.pop_clip();
        dl.rect(0.0, 0.0, 500.0, 500.0, col);
        assert_eq!(dl.runs.last().unwrap().clip, Some([10.0, 10.0, 100.0, 100.0]));
        dl.pop_clip();
        dl.rect(0.0, 0.0, 500.0, 500.0, col);
        assert_eq!(dl.runs.last().unwrap().clip, None);
    }

    /// The glow lives OUTSIDE the rect, inside rect+radius, and in an
    /// ADD_ATLAS run — absent, never milky, until the renderer binds the
    /// additive pipeline. Both forms honour the same envelope: the
    /// nine-slice sprite (real mask uv) and the shell fallback (empty uv).
    #[test]
    fn glow_ring_additive_and_outside() {
        let r = Rect::new(50.0, 60.0, 200.0, 100.0);
        let radius = 8.0;
        let uvs: [(f32, f32, f32, f32); 2] =
            [FontSystem::mask_soft_uv(), (0.0, 0.0, 0.0, 0.0)];
        for uv in uvs {
            let mut dl = DrawList::new();
            dl.glow_ring(r, &[Corner::chamfer(16.0); 4], 6, radius, Color::rgb8(0, 255, 200), uv);
            assert!(!dl.verts.is_empty());
            assert!(
                dl.runs.iter().any(|run| run.image == Some(ADD_ATLAS)),
                "glow must be an additive run (uv {uv:?})"
            );
            let e = 1e-3;
            for v in &dl.verts {
                let [px, py] = v.pos;
                let inside = px > r.x + e
                    && px < r.right() - e
                    && py > r.y + e
                    && py < r.bottom() - e;
                assert!(!inside, "glow leaked into the rect: ({px},{py}) (uv {uv:?})");
                assert!(
                    px >= r.x - radius - e
                        && px <= r.right() + radius + e
                        && py >= r.y - radius - e
                        && py <= r.bottom() + radius + e,
                    "glow past its own radius: ({px},{py}) (uv {uv:?})"
                );
            }
        }
    }

    /// The sprite costs r1 §4.4 stands on: the glow is 8 quads = 48 verts
    /// with the centre dropped, soft_box keeps the centre at 9 quads = 54 —
    /// at ANY radius and panel size, which is the whole point over shells.
    #[test]
    fn nine_slice_vertex_counts() {
        let uv = FontSystem::mask_soft_uv();
        let col = Color::rgb8(0, 255, 200);
        for radius in [2.0f32, 8.0, 40.0] {
            for r in [Rect::new(50.0, 60.0, 200.0, 100.0), Rect::new(0.0, 0.0, 24.0, 24.0)] {
                // One quad per outline segment, at ANY radius and size:
                // 4 points square, 8 chamfered, 4·(S+1) at round S=6.
                let mut dl = DrawList::new();
                dl.glow_ring(r, &[Corner { style: CornerStyle::Square, size: 0.0 }; 4], 6, radius, col, uv);
                assert_eq!(dl.verts.len(), 24, "square glow r={radius} rect={:?}", (r.w, r.h));
                let mut dl = DrawList::new();
                dl.glow_ring(r, &[Corner::chamfer(8.0); 4], 6, radius, col, uv);
                assert_eq!(dl.verts.len(), 48, "chamfer glow r={radius} rect={:?}", (r.w, r.h));
                let mut dl = DrawList::new();
                dl.glow_ring(r, &[Corner::round(8.0); 4], 6, radius, col, uv);
                assert_eq!(dl.verts.len(), 168, "round glow r={radius} rect={:?}", (r.w, r.h));
                let mut dl = DrawList::new();
                dl.soft_box(r, radius, col, uv);
                assert_eq!(dl.verts.len(), 54, "soft_box r={radius} rect={:?}", (r.w, r.h));
            }
        }
    }

    /// The plugin-facing mask quad: sprite-space uv is clamped into the
    /// band, so whatever numbers cross the ABI the quad can sample the
    /// soft disk and nothing else; `additive` picks the ADD_ATLAS run
    /// over the normal one; an empty band degrades to the white pixel —
    /// solid, raw, still present.
    #[test]
    fn mask_quad_stays_inside_the_band() {
        let band = FontSystem::mask_soft_uv();
        let (u0, v0, u1, v1) = band;
        let p = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
        let wild = [[-3.0, 0.5], [7.0, -2.0], [0.5, 9.0], [0.25, 0.75]];
        let col = Color::rgb8(0, 255, 200);
        let e = 1e-6;

        let mut dl = DrawList::new();
        dl.mask_quad(p, wild, band, col, true);
        assert_eq!(dl.verts.len(), 6, "one quad, additive");
        assert!(dl.runs.iter().any(|r| r.image == Some(ADD_ATLAS)));
        for v in &dl.verts {
            assert!(v.uv[0] >= u0 - e && v.uv[0] <= u1 + e, "u escaped the band: {}", v.uv[0]);
            assert!(v.uv[1] >= v0 - e && v.uv[1] <= v1 + e, "v escaped the band: {}", v.uv[1]);
        }

        // Cover blend: the same geometry lands in the plain atlas run.
        let mut dl = DrawList::new();
        dl.mask_quad(p, wild, band, col, false);
        assert_eq!(dl.verts.len(), 6);
        assert!(dl.runs.iter().all(|r| r.image.is_none()));

        // The maskless degenerate case: every vertex on the white pixel.
        let mut dl = DrawList::new();
        dl.mask_quad(p, wild, (0.0, 0.0, 0.0, 0.0), col, true);
        let w = FontSystem::white_uv();
        assert!(dl.verts.iter().all(|v| v.uv == [w.0, w.1]));

        // A fully transparent colour draws nothing at all.
        let mut dl = DrawList::new();
        dl.mask_quad(p, wild, band, col.alpha(0.0), true);
        assert!(dl.verts.is_empty());
    }

    /// The glow follows the corner it wraps: around a Round corner every
    /// sprite vertex keeps the corner's own radius from the arc centre
    /// (the glow is an arc grown by the glow, not a square bloom), and
    /// some of them live INSIDE the bounding rect — in the notch beside
    /// the arc, which is outside the panel's rounded fill.
    #[test]
    fn glow_ring_follows_a_round_corner() {
        let r = Rect::new(100.0, 100.0, 200.0, 160.0);
        let (s, radius) = (24.0f32, 10.0);
        let mut dl = DrawList::new();
        dl.glow_ring(r, &[Corner::round(s); 4], 8, radius, Color::rgb8(0, 255, 200), FontSystem::mask_soft_uv());
        let centre = [r.x + s, r.y + s];
        let e = 1e-3;
        let mut in_notch = 0;
        for v in &dl.verts {
            let [px, py] = v.pos;
            if px < centre[0] && py < centre[1] {
                let d = ((px - centre[0]).powi(2) + (py - centre[1]).powi(2)).sqrt();
                assert!(d >= s - e, "glow entered the rounded fill: ({px},{py}) d={d}");
                if px > r.x + e && py > r.y + e {
                    in_notch += 1;
                }
            }
        }
        assert!(in_notch > 0, "no vertex hugs the arc — the glow is not corner-true");
    }

    /// Every nine-slice vertex samples INSIDE the mask sprite's uv rect —
    /// never a glyph, never the white pixel — and its interior slice
    /// edges sit on the sprite's 31/64 · 33/64 middle band, so the edges
    /// stretch only the 2-texel cardinal strips.
    #[test]
    fn nine_slice_samples_only_the_mask() {
        let uv = FontSystem::mask_soft_uv();
        let (u0, v0, u1, v1) = uv;
        let mut dl = DrawList::new();
        dl.glow_ring(
            Rect::new(50.0, 60.0, 200.0, 100.0),
            &[Corner::SQUARE; 4],
            3,
            12.0,
            Color::rgb8(255, 0, 255),
            uv,
        );
        dl.soft_box(Rect::new(10.0, 10.0, 80.0, 40.0), 6.0, Color::rgb8(0, 0, 0), uv);
        assert!(!dl.verts.is_empty());
        let e = 1e-6;
        for v in &dl.verts {
            let [u, w] = v.uv;
            assert!(
                u >= u0 - e && u <= u1 + e && w >= v0 - e && w <= v1 + e,
                "uv ({u},{w}) escapes the mask sprite"
            );
        }
    }

    /// soft_box stays inside its rect (the feather is inward); shadow is
    /// soft_box on the rect shifted by the offset and inflated by the
    /// radius, and stays inside THAT envelope. Both run normal-blend:
    /// no ADD_ATLAS run may appear — a shadow is not light.
    #[test]
    fn soft_box_and_shadow_containment() {
        let uv = FontSystem::mask_soft_uv();
        let r = Rect::new(30.0, 40.0, 120.0, 60.0);
        let e = 1e-3;
        let mut dl = DrawList::new();
        dl.soft_box(r, 10.0, Color::rgb8(0, 0, 0), uv);
        assert!(!dl.verts.is_empty());
        for v in &dl.verts {
            let [px, py] = v.pos;
            assert!(
                px >= r.x - e && px <= r.right() + e && py >= r.y - e && py <= r.bottom() + e,
                "soft_box leaks: ({px},{py})"
            );
        }
        let (dx, dy, radius) = (4.0, 6.0, 10.0);
        let mut dl = DrawList::new();
        dl.shadow(r, [dx, dy], radius, Color::rgb8(0, 0, 0), uv);
        assert!(!dl.verts.is_empty());
        assert!(
            dl.runs.iter().all(|run| run.image != Some(ADD_ATLAS)),
            "a shadow must not be additive"
        );
        let (x0, y0) = (r.x + dx - radius, r.y + dy - radius);
        let (x1, y1) = (r.right() + dx + radius, r.bottom() + dy + radius);
        for v in &dl.verts {
            let [px, py] = v.pos;
            assert!(
                px >= x0 - e && px <= x1 + e && py >= y0 - e && py <= y1 + e,
                "shadow past its envelope: ({px},{py})"
            );
        }
    }

    /// The raw degenerate cases the governing principle demands: an empty
    /// mask uv must still draw — glow_ring as shells, soft_box as a plain
    /// rect — never nothing, never a sample of unrelated atlas texels.
    #[test]
    fn empty_mask_uv_degrades_raw() {
        let empty = (0.0, 0.0, 0.0, 0.0);
        let r = Rect::new(0.0, 0.0, 100.0, 50.0);
        let mut dl = DrawList::new();
        dl.glow_ring(r, &[Corner::SQUARE; 4], 3, 8.0, Color::rgb8(255, 255, 255), empty);
        assert!(!dl.verts.is_empty(), "maskless glow must still draw shells");
        let mut dl = DrawList::new();
        dl.soft_box(r, 8.0, Color::rgb8(255, 255, 255), empty);
        assert_eq!(dl.verts.len(), 6, "maskless soft_box is a plain rect");
    }

    /// Every vertex of the fill must satisfy the eight half-planes of
    /// the chamfered octagon — a fill poking past a cut corner is the
    /// bug this shape exists to prevent.
    #[test]
    fn chamfer_fill_stays_inside_the_frame() {
        let (x, y, w, h, cut) = (10.0, 20.0, 200.0, 100.0, 16.0);
        let mut dl = DrawList::new();
        dl.chamfer_fill(x, y, w, h, cut, Color::rgb8(255, 255, 255));
        assert!(!dl.verts.is_empty());
        let e = 0.001;
        for v in &dl.verts {
            let [px, py] = v.pos;
            assert!(px >= x - e && px <= x + w + e && py >= y - e && py <= y + h + e);
            // The four corner diagonals, as x+y style half-planes.
            assert!(px + py >= x + y + cut - e, "top-left corner leaks");
            assert!((x + w - px) + py >= cut - e + y, "top-right corner leaks");
            assert!(px + (y + h - py) >= x + cut - e, "bottom-left corner leaks");
            assert!((x + w - px) + (y + h - py) >= cut - e, "bottom-right corner leaks");
        }
    }
}
