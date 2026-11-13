//! Triangle rasterization into a visibility buffer.
//!
//! The rasterizer writes, per sample, the depth of the nearest triangle and its id. It
//! interpolates nothing else: attributes are reconstructed afterwards, once per visible
//! sample, from the id and the sample position (see [`crate::shade`]). Plants have a lot
//! of overdraw in their crowns, so deferring the attribute work to visible samples keeps
//! the per-fragment cost to one depth test.
//!
//! Triangles arrive in clip space (`glam` convention: visible when `0 <= z <= w` and
//! `|x|, |y| <= w`). A triangle entirely inside a guard band of [`GUARD_BAND`] times the
//! viewport and in front of the near plane goes straight to the rasterizer; one crossing
//! the near plane or the guard band is clipped (Sutherland-Hodgman, on the stack) and
//! fanned into triangles. Vertices are snapped to 1/256 pixel and the edge functions are
//! evaluated exactly in 64-bit integers, stepped incrementally over the bounding box of
//! the triangle, with a tie rule that gives each pixel on a shared edge to exactly one of
//! the two triangles. Samples sit at pixel centres. Depth is the clip-space `z / w`,
//! interpolated linearly in screen space, and passes the test when strictly smaller than
//! the stored value.

use glam::Vec4;

/// Id stored in samples that no triangle covers.
pub const NO_TRIANGLE: u32 = u32::MAX;

/// Half-width of the clipping guard band in units of the half viewport. Triangles inside
/// it are rasterized without clipping; it bounds the fixed-point coordinates.
pub const GUARD_BAND: f32 = 2.0;

/// Sub-pixel resolution of snapped vertex coordinates, as a power of two.
const SUBPIXEL_BITS: u32 = 8;
const SUBPIXEL: i64 = 1 << SUBPIXEL_BITS;
const HALF_SUBPIXEL: i64 = SUBPIXEL / 2;

/// Largest polygon a triangle can become after clipping against five planes.
const MAX_CLIPPED: usize = 8;

/// Depth and triangle id per sample.
#[derive(Debug, Clone, PartialEq)]
pub struct VisibilityBuffer {
    /// Width in samples.
    pub width: u32,
    /// Height in samples.
    pub height: u32,
    /// Clip-space `z / w` of the nearest surface, 1 where nothing was drawn. Row-major,
    /// first row at the top of the image.
    pub depth: Vec<f32>,
    /// Id of the nearest triangle (see [`DrawCall::first_id`]), [`NO_TRIANGLE`] where
    /// nothing was drawn.
    pub id: Vec<u32>,
}

impl VisibilityBuffer {
    /// An empty buffer of `width` by `height` samples.
    #[must_use]
    pub fn new(width: u32, height: u32) -> Self {
        let mut buffer = Self {
            width: 0,
            height: 0,
            depth: Vec::new(),
            id: Vec::new(),
        };
        buffer.reset(width, height);
        buffer
    }

    /// Resizes the buffer and clears every sample, reusing the allocation.
    pub fn reset(&mut self, width: u32, height: u32) {
        let count = width as usize * height as usize;
        self.width = width;
        self.height = height;
        self.depth.clear();
        self.depth.resize(count, 1.0);
        self.id.clear();
        self.id.resize(count, NO_TRIANGLE);
    }
}

/// One batch of indexed triangles.
#[derive(Debug, Clone, Copy)]
pub struct DrawCall<'a> {
    /// Clip-space vertex positions.
    pub clip: &'a [Vec4],
    /// Triangle list indexing `clip`; triangles with an index out of range are skipped.
    pub indices: &'a [u32],
    /// Skip triangles whose front face (counter-clockwise in normalized device
    /// coordinates) points away from the viewer.
    pub cull_back: bool,
    /// Id written for the first triangle; triangle `i` writes `first_id + i`.
    pub first_id: u32,
}

/// Draws every triangle of `call` into `target`.
pub fn rasterize(target: &mut VisibilityBuffer, call: &DrawCall<'_>) {
    let viewport = Viewport::new(target.width, target.height);
    for (index, tri) in call.indices.as_chunks::<3>().0.iter().enumerate() {
        let fetch = |i: u32| call.clip.get(i as usize).copied();
        let (Some(a), Some(b), Some(c)) = (fetch(tri[0]), fetch(tri[1]), fetch(tri[2])) else {
            continue;
        };
        let id = call.first_id.wrapping_add(index as u32);
        draw_clip_triangle(target, &viewport, [a, b, c], call.cull_back, id);
    }
}

/// Screen mapping of normalized device coordinates, in sub-pixel units.
#[derive(Debug, Clone, Copy)]
struct Viewport {
    width: u32,
    height: u32,
    scale_x: f32,
    scale_y: f32,
}

impl Viewport {
    fn new(width: u32, height: u32) -> Self {
        let sub = SUBPIXEL as f32;
        Self {
            width,
            height,
            scale_x: 0.5 * width as f32 * sub,
            scale_y: 0.5 * height as f32 * sub,
        }
    }

    /// Snapped screen position (y down) and depth of a clip-space vertex with `w > 0`.
    fn project(&self, v: Vec4) -> (i64, i64, f32) {
        let inv = 1.0 / v.w;
        let x = (v.x * inv + 1.0) * self.scale_x;
        let y = (1.0 - v.y * inv) * self.scale_y;
        (x.round() as i64, y.round() as i64, v.z * inv)
    }
}

/// Which of the guard and near planes a vertex is outside of, as a bit set.
fn outcode(v: Vec4) -> u32 {
    let g = GUARD_BAND * v.w;
    u32::from(v.z < 0.0 || v.w <= 0.0)
        | u32::from(v.x > g) << 1
        | u32::from(v.x < -g) << 2
        | u32::from(v.y > g) << 3
        | u32::from(v.y < -g) << 4
}

/// Whether the triangle lies entirely outside the view frustum.
fn outside_frustum(v: &[Vec4; 3]) -> bool {
    let all = |test: fn(Vec4) -> bool| v.iter().all(|&p| test(p));
    all(|p| p.x > p.w)
        || all(|p| p.x < -p.w)
        || all(|p| p.y > p.w)
        || all(|p| p.y < -p.w)
        || all(|p| p.z < 0.0)
        || all(|p| p.z > p.w)
}

fn draw_clip_triangle(
    target: &mut VisibilityBuffer,
    viewport: &Viewport,
    v: [Vec4; 3],
    cull_back: bool,
    id: u32,
) {
    if !v.iter().all(|p| p.is_finite()) || outside_frustum(&v) {
        return;
    }
    if outcode(v[0]) | outcode(v[1]) | outcode(v[2]) == 0 {
        let screen = v.map(|p| viewport.project(p));
        draw_screen_triangle(target, viewport, screen, cull_back, id);
        return;
    }
    let mut polygon = [Vec4::ZERO; MAX_CLIPPED];
    let count = clip_polygon(&v, &mut polygon);
    for i in 1..count.saturating_sub(1) {
        let fan = [polygon[0], polygon[i], polygon[i + 1]];
        if fan.iter().all(|p| p.w > 0.0 && p.is_finite()) {
            let screen = fan.map(|p| viewport.project(p));
            draw_screen_triangle(target, viewport, screen, cull_back, id);
        }
    }
}

/// Clips a triangle against the near plane and the guard band; returns the number of
/// polygon vertices written to `out`.
fn clip_polygon(v: &[Vec4; 3], out: &mut [Vec4; MAX_CLIPPED]) -> usize {
    let g = GUARD_BAND;
    let planes = [
        Vec4::new(0.0, 0.0, 1.0, 0.0),
        Vec4::new(-1.0, 0.0, 0.0, g),
        Vec4::new(1.0, 0.0, 0.0, g),
        Vec4::new(0.0, -1.0, 0.0, g),
        Vec4::new(0.0, 1.0, 0.0, g),
    ];
    let mut current = [Vec4::ZERO; MAX_CLIPPED];
    current[..3].copy_from_slice(v);
    let mut count = 3;
    for plane in planes {
        let mut next = [Vec4::ZERO; MAX_CLIPPED];
        let mut written = 0;
        for i in 0..count {
            let (a, b) = (current[i], current[(i + 1) % count]);
            let (da, db) = (plane.dot(a), plane.dot(b));
            if da >= 0.0 && written < MAX_CLIPPED {
                next[written] = a;
                written += 1;
            }
            if (da >= 0.0) != (db >= 0.0) && written < MAX_CLIPPED {
                next[written] = a + (b - a) * (da / (da - db));
                written += 1;
            }
        }
        current = next;
        count = written;
        if count < 3 {
            return 0;
        }
    }
    out.copy_from_slice(&current);
    count
}

/// Edge function `a * x + b * y + c` of one triangle edge, in sub-pixel units.
#[derive(Debug, Clone, Copy)]
struct Edge {
    a: i64,
    b: i64,
    c: i64,
}

impl Edge {
    /// The edge from `p` to `q`, biased so that pixels exactly on it are owned by one of
    /// the two triangles sharing it.
    fn new(from: (i64, i64), to: (i64, i64)) -> Self {
        let a = from.1 - to.1;
        let b = to.0 - from.0;
        let owns = a > 0 || (a == 0 && b < 0);
        Self {
            a,
            b,
            c: -(a * from.0 + b * from.1) - i64::from(!owns),
        }
    }

    fn at(&self, x: i64, y: i64) -> i64 {
        self.a * x + self.b * y + self.c
    }
}

/// Pixel range `[first, last]` whose centres may lie in `[lo, hi]` sub-pixel units.
fn pixel_span(lo: i64, hi: i64, size: u32) -> Option<(i64, i64)> {
    let first = (lo - HALF_SUBPIXEL + SUBPIXEL - 1)
        .div_euclid(SUBPIXEL)
        .max(0);
    let last = (hi - HALF_SUBPIXEL)
        .div_euclid(SUBPIXEL)
        .min(i64::from(size) - 1);
    (first <= last).then_some((first, last))
}

fn draw_screen_triangle(
    target: &mut VisibilityBuffer,
    viewport: &Viewport,
    mut s: [(i64, i64, f32); 3],
    cull_back: bool,
    id: u32,
) {
    let area = (s[1].0 - s[0].0) * (s[2].1 - s[0].1) - (s[2].0 - s[0].0) * (s[1].1 - s[0].1);
    if area == 0 || (cull_back && area > 0) {
        return;
    }
    if area < 0 {
        s.swap(1, 2);
    }
    let xs = [s[0].0, s[1].0, s[2].0];
    let ys = [s[0].1, s[1].1, s[2].1];
    let lo_x = xs.iter().min().copied().unwrap_or(0);
    let hi_x = xs.iter().max().copied().unwrap_or(0);
    let lo_y = ys.iter().min().copied().unwrap_or(0);
    let hi_y = ys.iter().max().copied().unwrap_or(0);
    let Some((x0, x1)) = pixel_span(lo_x, hi_x, viewport.width) else {
        return;
    };
    let Some((y0, y1)) = pixel_span(lo_y, hi_y, viewport.height) else {
        return;
    };
    let edges = [
        Edge::new((s[1].0, s[1].1), (s[2].0, s[2].1)),
        Edge::new((s[2].0, s[2].1), (s[0].0, s[0].1)),
        Edge::new((s[0].0, s[0].1), (s[1].0, s[1].1)),
    ];
    let plane = DepthPlane::new(&s);
    let span = Span { x0, x1, y0, y1 };
    fill(target, &edges, &plane, span, id);
}

/// Depth as an affine function of the sub-pixel position.
#[derive(Debug, Clone, Copy)]
struct DepthPlane {
    origin: (f64, f64, f64),
    dx: f64,
    dy: f64,
}

impl DepthPlane {
    fn new(s: &[(i64, i64, f32); 3]) -> Self {
        let p = s.map(|(x, y, z)| (x as f64, y as f64, f64::from(z)));
        let (e1x, e1y, e1z) = (p[1].0 - p[0].0, p[1].1 - p[0].1, p[1].2 - p[0].2);
        let (e2x, e2y, e2z) = (p[2].0 - p[0].0, p[2].1 - p[0].1, p[2].2 - p[0].2);
        let det = e1x * e2y - e2x * e1y;
        Self {
            origin: p[0],
            dx: (e1z * e2y - e2z * e1y) / det,
            dy: (e2z * e1x - e1z * e2x) / det,
        }
    }

    fn at(&self, x: i64, y: i64) -> f64 {
        self.origin.2 + self.dx * (x as f64 - self.origin.0) + self.dy * (y as f64 - self.origin.1)
    }
}

/// Inclusive pixel rectangle.
#[derive(Debug, Clone, Copy)]
struct Span {
    x0: i64,
    x1: i64,
    y0: i64,
    y1: i64,
}

/// Bounding boxes at least this many pixels wide have their covered run solved per row
/// instead of testing every pixel of the box.
const WIDE_SPAN: i64 = 8;

fn fill(target: &mut VisibilityBuffer, edges: &[Edge; 3], plane: &DepthPlane, span: Span, id: u32) {
    let width = target.width as usize;
    let step_x = edges.map(|e| e.a * SUBPIXEL);
    let step_z = (plane.dx * SUBPIXEL as f64) as f32;
    for py in span.y0..=span.y1 {
        let sy = py * SUBPIXEL + HALF_SUBPIXEL;
        let (mut first, mut last) = (span.x0, span.x1);
        if span.x1 - span.x0 >= WIDE_SPAN {
            let start = span.x0 * SUBPIXEL + HALF_SUBPIXEL;
            let Some((lo, hi)) = covered_run(edges.map(|edge| edge.at(start, sy)), step_x) else {
                continue;
            };
            first = span.x0 + lo;
            last = last.min(span.x0 + hi);
        }
        let sx = first * SUBPIXEL + HALF_SUBPIXEL;
        let mut e = edges.map(|edge| edge.at(sx, sy));
        let mut z = plane.at(sx, sy) as f32;
        let row = py as usize * width;
        for px in first..=last {
            if (e[0] | e[1] | e[2]) >= 0 {
                let index = row + px as usize;
                if z < target.depth[index] && z >= 0.0 {
                    target.depth[index] = z;
                    target.id[index] = id;
                }
            }
            e[0] += step_x[0];
            e[1] += step_x[1];
            e[2] += step_x[2];
            z += step_z;
        }
    }
}

/// Range `[lo, hi]` of pixel steps `k >= 0` along a row where every edge value
/// `start + k * step` is non-negative, `None` when it is empty.
fn covered_run(start: [i64; 3], step: [i64; 3]) -> Option<(i64, i64)> {
    let (mut lo, mut hi) = (0_i64, i64::MAX);
    for (e, s) in start.into_iter().zip(step) {
        match s.signum() {
            1 => lo = lo.max(-(e.div_euclid(s))),
            -1 => hi = hi.min(e.div_euclid(-s)),
            _ if e < 0 => return None,
            _ => {}
        }
    }
    (lo <= hi).then_some((lo, hi))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ndc(x: f32, y: f32, z: f32) -> Vec4 {
        Vec4::new(x, y, z, 1.0)
    }

    fn draw(target: &mut VisibilityBuffer, clip: &[Vec4], cull_back: bool, first_id: u32) {
        let indices: Vec<u32> = (0..clip.len() as u32).collect();
        let call = DrawCall {
            clip,
            indices: &indices,
            cull_back,
            first_id,
        };
        rasterize(target, &call);
    }

    #[test]
    fn half_screen_triangle_covers_lower_left_pixels() {
        let mut target = VisibilityBuffer::new(8, 8);
        let tri = [
            ndc(-1.0, -1.0, 0.5),
            ndc(1.0, -1.0, 0.5),
            ndc(-1.0, 1.0, 0.5),
        ];
        draw(&mut target, &tri, true, 0);
        for y in 0..8_usize {
            for x in (0..8_usize).filter(|&x| x != y) {
                let covered = target.id[y * 8 + x] == 0;
                assert_eq!(covered, x < y, "pixel {x},{y}");
            }
        }
    }

    #[test]
    fn two_triangles_sharing_an_edge_cover_each_pixel_once() {
        let mut first = VisibilityBuffer::new(16, 16);
        let mut second = VisibilityBuffer::new(16, 16);
        let (a, b, c, d) = (
            ndc(-0.9, -0.7, 0.5),
            ndc(0.8, -0.95, 0.5),
            ndc(0.7, 0.85, 0.5),
            ndc(-0.75, 0.9, 0.5),
        );
        draw(&mut first, &[a, b, c], false, 0);
        draw(&mut second, &[a, c, d], false, 0);
        let overlap = (0..256).filter(|&i| first.id[i] == 0 && second.id[i] == 0);
        assert_eq!(overlap.count(), 0);
        let mut whole = VisibilityBuffer::new(16, 16);
        draw(&mut whole, &[a, b, c, a, c, d], false, 0);
        let union = (0..256)
            .filter(|&i| first.id[i] == 0 || second.id[i] == 0)
            .count();
        assert_eq!(
            union,
            whole.id.iter().filter(|&&id| id != NO_TRIANGLE).count()
        );
    }

    #[test]
    fn nearer_triangle_wins_whatever_the_draw_order() {
        let far = [
            ndc(-1.0, -1.0, 0.8),
            ndc(1.0, -1.0, 0.8),
            ndc(0.0, 1.0, 0.8),
        ];
        let near = [
            ndc(-1.0, -1.0, 0.2),
            ndc(1.0, -1.0, 0.2),
            ndc(0.0, 1.0, 0.2),
        ];
        for order in [[far, near], [near, far]] {
            let mut target = VisibilityBuffer::new(8, 8);
            draw(
                &mut target,
                &order[0],
                true,
                if order[0] == near { 1 } else { 2 },
            );
            draw(
                &mut target,
                &order[1],
                true,
                if order[1] == near { 1 } else { 2 },
            );
            let centre = 4 * 8 + 4;
            assert_eq!(target.id[centre], 1);
            assert!((target.depth[centre] - 0.2).abs() < 1e-5);
        }
    }

    #[test]
    fn covered_run_matches_a_pixel_by_pixel_test() {
        let mut state = 0x2545_f491_u64;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state % 2001).cast_signed() - 1000
        };
        for _ in 0..2000 {
            let start = [next() * 37, next() * 37, next() * 37];
            let step = [next(), next(), next() / 7];
            let inside: Vec<i64> = (0..64)
                .filter(|&k| (0..3).all(|i| start[i] + k * step[i] >= 0))
                .collect();
            let run = covered_run(start, step).map(|(lo, hi)| (lo, hi.min(63)));
            let expected = inside.first().map(|&lo| (lo, *inside.last().unwrap()));
            match (run, expected) {
                (Some((lo, hi)), Some(e)) => assert_eq!((lo, hi), e),
                (Some((lo, hi)), None) => assert!(lo > hi || lo > 63),
                (None, e) => assert_eq!(e, None),
            }
        }
    }

    #[test]
    fn back_faces_are_culled_only_when_asked() {
        let clockwise = [
            ndc(-1.0, -1.0, 0.5),
            ndc(0.0, 1.0, 0.5),
            ndc(1.0, -1.0, 0.5),
        ];
        let mut culled = VisibilityBuffer::new(8, 8);
        draw(&mut culled, &clockwise, true, 0);
        assert!(culled.id.iter().all(|&id| id == NO_TRIANGLE));
        let mut kept = VisibilityBuffer::new(8, 8);
        draw(&mut kept, &clockwise, false, 0);
        assert!(kept.id.contains(&0));
    }

    #[test]
    fn triangles_crossing_the_near_plane_and_guard_band_are_clipped() {
        let mut target = VisibilityBuffer::new(32, 32);
        let crossing = [
            Vec4::new(-0.5, -0.5, -2.0, 0.01),
            Vec4::new(1e6, -0.5, 0.5, 1.0),
            Vec4::new(0.0, 0.5, 0.5, 1.0),
            Vec4::new(f32::NAN, 0.0, 0.0, 1.0),
            Vec4::new(0.0, 0.0, 0.5, 1.0),
            Vec4::new(1.0, 1.0, 0.5, 0.0),
            Vec4::new(-1e30, -1e30, 0.3, 1e-30),
            Vec4::new(1e30, -1e30, 0.3, 1e-30),
            Vec4::new(0.0, 1e30, 0.3, 1.0),
        ];
        draw(&mut target, &crossing, false, 0);
        assert!(target.id.contains(&0));
        assert!(target.depth.iter().all(|&d| (0.0..=1.0).contains(&d)));
    }
}
