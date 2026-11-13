//! Tube surface: vertices and triangles stretched over the rings of one axis.
//!
//! **Frame.** Each ring needs a direction from which angles around it are counted. It is
//! carried from ring to ring by parallel transport: the direction of the previous ring is
//! turned by the smallest rotation that takes the previous tangent onto the new one. This
//! is the frame of Prusinkiewicz et al. (2001, eq. 10) with no rotation about the heading,
//! the rotation-minimizing frame: the surface does not twist however the axis bends, and
//! unlike the Frenet frame it is defined on straight parts and does not flip at
//! inflection points. The first direction points as far down as the tangent allows, which
//! puts the texture seam on the underside of a branch.
//!
//! **Vertices.** A ring of `n` sides has `n + 1` vertices, the first and the last at the
//! same place with texture coordinates one repeat apart. Normals are those of the smooth
//! surface: radial, tilted by the slope of the radius along the axis (and around it for a
//! lobed ring), so collars and flares shade as curved surfaces.
//!
//! **Triangles.** Two consecutive rings are joined by a band. When they have the same
//! number of sides the band is a strip of quads; otherwise the two rings are zipped,
//! advancing on the ring whose next side has its middle at the smaller angle, so each
//! vertex of the coarser ring fans out to the sides of the finer ring nearest to it.
//! Both cases give `m + n` triangles. The tip is closed by a fan to one apex vertex slightly beyond the last ring.
//! Front faces are counter-clockwise seen from outside.
//!
//! **Texture coordinates.** `u` runs around the ring and covers a whole number of
//! repeats, `v` is proportional to arc length; see [`Style`].

use crate::ring::Ring;
use glam::Vec3;
use std::f32::consts::TAU;

/// Height of the tip cap, in radii of the last ring.
const CAP_HEIGHT: f32 = 0.5;

/// Unit circles sampled for every number of sides up to a maximum, so that ring vertices
/// cost no trigonometry.
#[derive(Debug, Clone, Default)]
pub(crate) struct Circles {
    points: Vec<[f32; 2]>,
    /// Largest number of sides the tables cover.
    max_sides: u32,
}

impl Circles {
    /// Tables for rings of up to `max_sides` sides.
    #[cfg(test)]
    pub(crate) fn new(max_sides: u32) -> Self {
        let mut circles = Self::default();
        circles.ensure(max_sides);
        circles
    }

    /// Extends the tables to rings of up to `max_sides` sides.
    pub(crate) fn ensure(&mut self, max_sides: u32) {
        if max_sides <= self.max_sides && !self.points.is_empty() {
            return;
        }
        self.points.clear();
        self.points.reserve(Self::start(max_sides + 1));
        for sides in 0..=max_sides {
            for corner in 0..sides {
                let (sin, cos) = (TAU * corner as f32 / sides as f32).sin_cos();
                self.points.push([cos, sin]);
            }
            self.points.push([1.0, 0.0]);
        }
        self.max_sides = max_sides;
    }

    /// Index of the first point of the table for `sides` sides.
    const fn start(sides: u32) -> usize {
        (sides as usize * (sides as usize + 1)) / 2
    }

    /// `[cos, sin]` of the `sides + 1` vertices of a ring; the last equals the first.
    fn of(&self, sides: u32) -> &[[f32; 2]] {
        &self.points[Self::start(sides)..Self::start(sides + 1)]
    }
}

/// Per-axis constants of the surface.
///
/// `u` covers `u_repeats` repeats of the texture around the axis, a whole number chosen
/// from the circumference at the base and the texture size of the species. `v` is
/// `v_offset + arc * v_scale`, with `v_scale` such that a texel is as long as it is wide
/// at the base of the axis. Texel density is therefore that of the species wherever the
/// circumference is at least one texture repeat, and higher on thinner axes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Style {
    /// Number of buttress lobes of lobed rings.
    pub lobes: u32,
    /// Texture repeats around the axis.
    pub u_repeats: f32,
    /// Texture repeats per meter along the axis.
    pub v_scale: f32,
    /// `v` at the start of the axis.
    pub v_offset: f32,
    /// Close the base of the axis with a fan (the foot of a stem).
    pub closed_base: bool,
    /// Vertex colours of the axis, `None` for a bark without them.
    pub tint: Option<Tint>,
}

/// Vertex colours of one axis, as factors on the base colour of the bark material.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Tint {
    /// Factor where the radius is 0.
    pub young: [f32; 3],
    /// Factor from `young_radius` on.
    pub old: [f32; 3],
    /// Radius at which the bark has its old colour.
    pub young_radius: f32,
}

impl Tint {
    /// Vertex colour at `radius`.
    fn at(&self, radius: f32) -> [f32; 4] {
        let t = (radius / self.young_radius).clamp(0.0, 1.0);
        let t = t * t * (3.0 - 2.0 * t);
        let mix =
            |channel: usize| self.young[channel] + (self.old[channel] - self.young[channel]) * t;
        [mix(0), mix(1), mix(2), 1.0]
    }
}

/// The part of the bark buffers owned by one axis.
#[derive(Debug)]
pub(crate) struct TubeOut<'a> {
    /// Vertex positions.
    pub positions: &'a mut [[f32; 3]],
    /// Vertex normals.
    pub normals: &'a mut [[f32; 3]],
    /// Texture coordinates.
    pub uvs: &'a mut [[f32; 2]],
    /// Vertex colours; empty when the style has no tint.
    pub colors: &'a mut [[f32; 4]],
    /// Triangle indices.
    pub indices: &'a mut [u32],
    /// Index of the first vertex of the axis in the whole buffer.
    pub base: u32,
}

/// Number of vertices and of indices of the tube over `rings`, with a fan closing its
/// base when `closed_base`.
pub(crate) fn tube_counts(rings: &[Ring], closed_base: bool) -> (usize, usize) {
    let (Some(first), Some(last)) = (rings.first(), rings.last()) else {
        return (0, 0);
    };
    let foot = if closed_base { first.sides as usize } else { 0 };
    let vertices: usize = rings.iter().map(|ring| ring.sides as usize + 1).sum();
    let bands: usize = rings
        .windows(2)
        .map(|pair| (pair[0].sides + pair[1].sides) as usize)
        .sum();
    let extra = if closed_base { foot + 2 } else { 1 };
    (vertices + extra, 3 * (bands + last.sides as usize + foot))
}

/// Direction of the first vertex of the first ring: as far down as possible while
/// orthogonal to `tangent`, or along X for a vertical axis.
fn seam_direction(tangent: Vec3) -> Vec3 {
    let down = Vec3::NEG_Y.reject_from_normalized(tangent);
    if down.length_squared() > 1e-4 {
        down.normalize()
    } else {
        Vec3::X.reject_from_normalized(tangent).normalize()
    }
}

/// Parallel transport of the unit vector `normal`, orthogonal to `from`, to the tangent
/// `to`: the rotation about `from x to` that takes `from` onto `to`, applied to `normal`.
pub(crate) fn transport(normal: Vec3, from: Vec3, to: Vec3) -> Vec3 {
    let cosine = from.dot(to);
    if cosine < -0.999 {
        return normal
            .reject_from_normalized(to)
            .normalize_or(to.any_orthonormal_vector());
    }
    let moved = normal - (from + to) * (normal.dot(to) / (1.0 + cosine));
    moved.reject_from_normalized(to).normalize_or(normal)
}

/// Slopes along the axis of the radius and of the lobe amplitude at ring `index`, by
/// central differences (one-sided at the ends).
fn slopes(rings: &[Ring], index: usize) -> (f32, f32) {
    let before = &rings[index.saturating_sub(1)];
    let after = &rings[(index + 1).min(rings.len() - 1)];
    let span = after.arc - before.arc;
    if span > 0.0 {
        (
            (after.radius - before.radius) / span,
            (after.lobe - before.lobe) / span,
        )
    } else {
        (0.0, 0.0)
    }
}

/// Orientation of one ring.
#[derive(Debug, Clone, Copy)]
struct Frame {
    /// Direction of angle 0.
    normal: Vec3,
    /// Direction of a quarter turn: `tangent x normal`.
    binormal: Vec3,
}

/// Write position in the buffers of one axis.
struct Writer<'a, 'b> {
    out: &'b mut TubeOut<'a>,
    style: &'b Style,
    circles: &'b Circles,
    vertex: usize,
    index: usize,
}

impl Writer<'_, '_> {
    /// Writes the vertices of a round ring.
    fn round_ring(&mut self, ring: &Ring, frame: &Frame, slope: f32) {
        let unit = self.circles.of(ring.sides);
        let range = self.vertex..self.vertex + unit.len();
        let shrink = (1.0 + slope * slope).sqrt().recip();
        let axial = ring.tangent * (-slope * shrink);
        let u_step = self.style.u_repeats / ring.sides as f32;
        let v = self.style.v_offset + ring.arc * self.style.v_scale;
        let positions = self.out.positions[range.clone()].iter_mut();
        let normals = self.out.normals[range.clone()].iter_mut();
        let uvs = self.out.uvs[range].iter_mut();
        for (corner, (((position, normal), uv), &[cos, sin])) in
            positions.zip(normals).zip(uvs).zip(unit).enumerate()
        {
            let radial = frame.normal * cos + frame.binormal * sin;
            *position = (ring.center + radial * ring.radius).to_array();
            *normal = (radial * shrink + axial).to_array();
            *uv = [corner as f32 * u_step, v];
        }
        self.paint(self.vertex..self.vertex + unit.len(), ring.radius);
        self.vertex += unit.len();
    }

    /// Writes the colour of the vertices `range`, which lie on an axis of `radius`.
    fn paint(&mut self, range: std::ops::Range<usize>, radius: f32) {
        if let Some(tint) = &self.style.tint {
            self.out.colors[range].fill(tint.at(radius));
        }
    }

    /// Writes the vertices of a ring with buttress lobes. With the radius
    /// `rho(a, s) = radius(s) + lobe(s) sin(lobes a)`, the normal of the surface is
    /// `radial - (rho_a / rho) tangential - rho_s tangent`, normalized.
    fn lobed_ring(&mut self, ring: &Ring, frame: &Frame, slope: (f32, f32)) {
        let unit = self.circles.of(ring.sides);
        let lobes = self.style.lobes as f32;
        let u_step = self.style.u_repeats / ring.sides as f32;
        let v = self.style.v_offset + ring.arc * self.style.v_scale;
        for (corner, &[cos, sin]) in unit.iter().enumerate() {
            let phase = lobes * TAU * (corner % ring.sides as usize) as f32 / ring.sides as f32;
            let (wave_sin, wave_cos) = phase.sin_cos();
            let rho = ring.radius + ring.lobe * wave_sin;
            let around = ring.lobe * lobes * wave_cos / rho;
            let along = slope.0 + slope.1 * wave_sin;
            let radial = frame.normal * cos + frame.binormal * sin;
            let tangential = frame.binormal * cos - frame.normal * sin;
            let normal = (radial - tangential * around - ring.tangent * along).normalize();
            let at = self.vertex + corner;
            self.out.positions[at] = (ring.center + radial * rho).to_array();
            self.out.normals[at] = normal.to_array();
            self.out.uvs[at] = [corner as f32 * u_step, v];
        }
        self.paint(self.vertex..self.vertex + unit.len(), ring.radius);
        self.vertex += unit.len();
    }

    /// Writes one triangle from vertex numbers local to the axis.
    fn triangle(&mut self, a: usize, b: usize, c: usize) {
        let base = self.out.base;
        let corners = [a, b, c].map(|local| base + local as u32);
        self.out.indices[self.index..self.index + 3].copy_from_slice(&corners);
        self.index += 3;
    }

    /// Joins ring `a` (first vertex `a_start`, `a_sides` sides) to the next ring `b`.
    fn band(&mut self, a_start: usize, a_sides: usize, b_start: usize, b_sides: usize) {
        if a_sides == b_sides {
            for side in 0..a_sides {
                let (a, b) = (a_start + side, b_start + side);
                self.triangle(a, a + 1, b);
                self.triangle(a + 1, b + 1, b);
            }
            return;
        }
        let (mut i, mut j) = (0, 0);
        while i < a_sides || j < b_sides {
            let (a, b) = (a_start + i, b_start + j);
            if (2 * i + 1) * b_sides <= (2 * j + 1) * a_sides {
                self.triangle(a, a + 1, b);
                i += 1;
            } else {
                self.triangle(a, b + 1, b);
                j += 1;
            }
        }
    }

    /// Closes the tip with a fan from the last ring to an apex vertex.
    fn cap(&mut self, ring: &Ring, start: usize) {
        let apex = self.vertex;
        let height = ring.radius * CAP_HEIGHT;
        self.out.positions[apex] = (ring.center + ring.tangent * height).to_array();
        self.out.normals[apex] = ring.tangent.to_array();
        self.out.uvs[apex] = [
            0.5 * self.style.u_repeats,
            self.style.v_offset + (ring.arc + height) * self.style.v_scale,
        ];
        self.paint(apex..apex + 1, ring.radius);
        self.vertex += 1;
        for side in 0..ring.sides as usize {
            self.triangle(start + side, start + side + 1, apex);
        }
    }

    /// Closes the base with a fan facing backwards: a copy of the first ring, which is
    /// the first vertices of the axis, and a vertex at its centre.
    fn foot(&mut self, ring: &Ring) {
        let (sides, start) = (ring.sides as usize, self.vertex);
        let center = start + sides;
        let back = (-ring.tangent).to_array();
        self.out.positions.copy_within(0..sides, start);
        self.out.uvs.copy_within(0..sides, start);
        self.out.positions[center] = ring.center.to_array();
        self.out.uvs[center] = [0.5 * self.style.u_repeats, self.style.v_offset];
        self.out.normals[start..=center].fill(back);
        self.paint(start..center + 1, ring.radius);
        self.vertex += sides + 1;
        for side in 0..sides {
            self.triangle(start + (side + 1) % sides, start + side, center);
        }
    }
}

/// Writes the tube over `rings` into `out`, whose slices must have exactly the lengths
/// given by [`tube_counts`].
///
/// # Panics
///
/// Panics when the slices of `out` do not have those lengths.
pub(crate) fn write_tube(rings: &[Ring], style: &Style, circles: &Circles, out: &mut TubeOut) {
    let Some(first) = rings.first() else {
        return;
    };
    let mut writer = Writer {
        out,
        style,
        circles,
        vertex: 0,
        index: 0,
    };
    let mut normal = seam_direction(first.tangent);
    let mut previous = (0, 0, first.tangent);
    for (index, ring) in rings.iter().enumerate() {
        normal = transport(normal, previous.2, ring.tangent);
        let frame = Frame {
            normal,
            binormal: ring.tangent.cross(normal),
        };
        let start = writer.vertex;
        let slope = slopes(rings, index);
        if ring.lobe > 0.0 {
            writer.lobed_ring(ring, &frame, slope);
        } else {
            writer.round_ring(ring, &frame, slope.0);
        }
        if index > 0 {
            writer.band(previous.0, previous.1, start, ring.sides as usize);
        }
        previous = (start, ring.sides as usize, ring.tangent);
    }
    writer.cap(&rings[rings.len() - 1], previous.0);
    if style.closed_base {
        writer.foot(first);
    }
    assert_eq!(writer.vertex, writer.out.positions.len());
    assert_eq!(writer.index, writer.out.indices.len());
}

#[cfg(test)]
mod tests {
    use super::{Circles, Style, TubeOut, transport, tube_counts, write_tube};
    use crate::ring::Ring;
    use glam::Vec3;

    const STYLE: Style = Style {
        lobes: 0,
        u_repeats: 2.0,
        v_scale: 1.0,
        v_offset: 0.0,
        closed_base: false,
        tint: None,
    };

    struct Tube {
        positions: Vec<[f32; 3]>,
        normals: Vec<[f32; 3]>,
        uvs: Vec<[f32; 2]>,
        colors: Vec<[f32; 4]>,
        indices: Vec<u32>,
    }

    fn tube(rings: &[Ring], style: &Style) -> Tube {
        let (vertices, indices) = tube_counts(rings, style.closed_base);
        let colored = if style.tint.is_some() { vertices } else { 0 };
        let mut tube = Tube {
            positions: vec![[0.0; 3]; vertices],
            normals: vec![[0.0; 3]; vertices],
            uvs: vec![[0.0; 2]; vertices],
            colors: vec![[0.0; 4]; colored],
            indices: vec![0; indices],
        };
        let mut out = TubeOut {
            positions: &mut tube.positions,
            normals: &mut tube.normals,
            uvs: &mut tube.uvs,
            colors: &mut tube.colors,
            indices: &mut tube.indices,
            base: 0,
        };
        write_tube(rings, style, &Circles::new(64), &mut out);
        tube
    }

    /// Rings along a quarter circle of radius 1 in the XY plane, with varying sides.
    fn arc_rings(sides: impl Fn(usize) -> u32) -> Vec<Ring> {
        (0..=8)
            .map(|step| {
                let angle = step as f32 * std::f32::consts::FRAC_PI_2 / 8.0;
                Ring {
                    center: Vec3::new(angle.sin(), 1.0 - angle.cos(), 0.0),
                    tangent: Vec3::new(angle.cos(), angle.sin(), 0.0),
                    arc: angle,
                    radius: 0.1,
                    lobe: 0.0,
                    sides: sides(step),
                }
            })
            .collect()
    }

    fn outward_and_counter_clockwise(tube: &Tube, rings: &[Ring]) {
        let nearest = |point: Vec3| {
            let by_distance = |a: &&Ring, b: &&Ring| {
                a.center
                    .distance(point)
                    .total_cmp(&b.center.distance(point))
            };
            rings.iter().min_by(by_distance).unwrap().center
        };
        for triangle in tube.indices.as_chunks::<3>().0 {
            let [a, b, c] = triangle.map(|index| Vec3::from(tube.positions[index as usize]));
            let face = (b - a).cross(c - a);
            assert!(face.length() > 1e-9, "degenerate triangle {triangle:?}");
            let centroid = (a + b + c) / 3.0;
            assert!(face.dot(centroid - nearest(centroid)) > 0.0, "{triangle:?}");
            for &vertex in triangle {
                let normal = Vec3::from(tube.normals[vertex as usize]);
                assert!((normal.length() - 1.0).abs() < 1e-5);
                assert!(normal.dot(face) > 0.0, "normal against the face");
            }
        }
    }

    #[test]
    fn circles_are_closed_unit_polygons() {
        let circles = Circles::new(16);
        for sides in 3..=16 {
            let unit = circles.of(sides);
            assert_eq!(unit.len(), sides as usize + 1);
            assert_eq!(unit[0], unit[sides as usize]);
            assert!(
                unit.iter()
                    .all(|[cos, sin]| (cos * cos + sin * sin - 1.0).abs() < 1e-6)
            );
        }
    }

    #[test]
    fn uniform_tube_is_closed_at_the_tip_and_faces_outward() {
        let rings = arc_rings(|_| 8);
        let tube = tube(&rings, &STYLE);
        assert_eq!(tube.positions.len(), 9 * 9 + 1);
        assert_eq!(tube.indices.len(), 3 * (8 * 16 + 8));
        outward_and_counter_clockwise(&tube, &rings);
    }

    #[test]
    fn closed_base_adds_a_backward_fan_and_tint_follows_the_radius() {
        let mut rings = arc_rings(|_| 6);
        for (step, ring) in rings.iter_mut().enumerate() {
            ring.radius = 0.2 - 0.02 * step as f32;
        }
        let open = tube(&rings, &STYLE);
        let style = Style {
            closed_base: true,
            tint: Some(super::Tint {
                young: [0.5, 0.25, 0.25],
                old: [1.0; 3],
                young_radius: 0.1,
            }),
            ..STYLE
        };
        let closed = tube(&rings, &style);
        assert_eq!(closed.positions.len(), open.positions.len() + 6 + 1);
        assert_eq!(closed.indices.len(), open.indices.len() + 3 * 6);
        let foot = &closed.indices[closed.indices.len() - 3..];
        let [a, b, c] = [0, 1, 2].map(|i| Vec3::from(closed.positions[foot[i] as usize]));
        assert!((b - a).cross(c - a).dot(rings[0].tangent) < 0.0);
        assert_eq!(closed.colors[0], [1.0; 4]);
        let tip = closed.colors[open.positions.len() - 1];
        assert!(tip[0] < 0.9 && tip[1] < tip[0]);
        assert_eq!(open.colors.len(), 0);
    }

    #[test]
    fn rings_of_different_sides_are_zipped() {
        let rings = arc_rings(|step| [12, 12, 9, 7, 7, 4, 3, 5, 8][step]);
        let tube = tube(&rings, &STYLE);
        let (vertices, indices) = tube_counts(&rings, false);
        assert_eq!(
            (tube.positions.len(), tube.indices.len()),
            (vertices, indices)
        );
        outward_and_counter_clockwise(&tube, &rings);
        let mut used = vec![false; vertices];
        for &index in &tube.indices {
            used[index as usize] = true;
        }
        assert!(used.iter().all(|&used| used));
    }

    #[test]
    fn planar_bend_does_not_twist_the_seam() {
        let rings = arc_rings(|_| 6);
        let tube = tube(&rings, &STYLE);
        for ring in 0..rings.len() {
            let seam = tube.positions[ring * 7];
            assert!(seam[2].abs() < 1e-6, "seam left the plane of the bend");
            assert_eq!(tube.uvs[ring * 7][0], 0.0);
            assert!((tube.uvs[ring * 7 + 6][0] - 2.0).abs() < 1e-6);
            assert_eq!(tube.positions[ring * 7], tube.positions[ring * 7 + 6]);
        }
    }

    #[test]
    fn transport_keeps_the_frame_orthonormal_and_has_no_twist() {
        let mut tangent = Vec3::Y;
        let mut normal = Vec3::X;
        for step in 1..=400 {
            let t = step as f32 * 0.05;
            let next = Vec3::new(t.cos() * 0.6, 1.0, t.sin() * 0.6).normalize();
            let moved = transport(normal, tangent, next);
            assert!((moved.length() - 1.0).abs() < 1e-5);
            assert!(moved.dot(next).abs() < 1e-5);
            let axis = tangent.cross(next).normalize_or_zero();
            let turned = (moved - normal).dot(axis);
            assert!(turned.abs() < 1e-3, "rotation about a foreign axis");
            (tangent, normal) = (next, moved);
        }
        let back = transport(Vec3::X, Vec3::Y, Vec3::NEG_Y);
        assert!((back.length() - 1.0).abs() < 1e-5 && back.dot(Vec3::NEG_Y).abs() < 1e-5);
    }

    #[test]
    fn lobed_rings_have_unit_outward_normals() {
        let mut rings = arc_rings(|_| 24);
        for (step, ring) in rings.iter_mut().enumerate() {
            ring.lobe = 0.03 * (8 - step) as f32 / 8.0;
        }
        rings[8].lobe = 0.0;
        let style = Style { lobes: 5, ..STYLE };
        let tube = tube(&rings, &style);
        outward_and_counter_clockwise(&tube, &rings);
        let radii: Vec<f32> = (0..24)
            .map(|corner| Vec3::from(tube.positions[corner]).distance(rings[0].center))
            .collect();
        let widest = radii.iter().copied().fold(0.0, f32::max);
        let narrowest = radii.iter().copied().fold(f32::INFINITY, f32::min);
        assert!(widest > 0.125 && narrowest < 0.075);
    }
}
