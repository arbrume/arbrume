//! Tube surface: vertices and triangles stretched over the rings of one axis.
//!
//! **Frame.** Each ring needs a direction from which angles around it are counted. It is
//! carried from ring to ring by parallel transport (`crate::ring::frame_rings`, run when
//! the rings are planned): the direction of the previous ring is turned by the smallest
//! rotation that takes the previous tangent onto the new one. This is the frame of
//! Prusinkiewicz et al. (2001, eq. 10) with no rotation about the heading, the
//! rotation-minimizing frame: the surface does not twist however the axis bends, and
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
//! Both cases give `m + n` triangles. The tip is closed by a fan to one apex vertex
//! slightly beyond the last ring. Front faces are counter-clockwise seen from outside.
//!
//! **Junctions.** With Tier B junctions (`crate::junction`) the quads of a parent over
//! the hole of a child are left out, the first rings of a child are the skewed rings of
//! its collar, and the child writes, after its tube, the rim of its hole and its
//! footprint in the frame of the parent, and the triangles between them.
//!
//! **Texture coordinates.** `u` runs around the ring and covers a whole number of
//! repeats, `v` is proportional to arc length; see [`Style`].

use crate::junction::{Foot, Junction};
use crate::ring::Ring;
use arbrume_core::species::Geometry;
use glam::Vec3;
use std::f32::consts::TAU;

/// Most sides of a collar ring, whose side count is that of a plain ring of its radius
/// (`geometry::MAX_RADIAL_SEGMENTS` of the species schema).
const RING_LIMIT: usize = 64;

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

/// The junctions an axis takes part in: the holes cut in it for its children, and its
/// own junction with its parent. All empty with Tier A junctions.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Joins<'a> {
    /// Junctions of the children of the axis.
    pub holes: &'a [Junction],
    /// Junction of the axis with its parent.
    pub own: Option<&'a Junction>,
    /// Footprint of the own junction.
    pub feet: &'a [Foot],
    /// Triangles of the stitches of all junctions, see `Junction::stitch`.
    pub stitches: &'a [[u32; 3]],
    /// Rings and style of the parent, when the axis has its own junction.
    pub parent: Option<(&'a [Ring], &'a Style)>,
    /// Geometry of the species, for the collar profile.
    pub geometry: &'a Geometry,
}

/// Number of vertices and of indices of the tube over `rings`, with a fan closing its
/// base when `closed_base`, the cells of the holes of `joins` left out and the stitch of
/// its own junction.
pub(crate) fn tube_counts(rings: &[Ring], closed_base: bool, joins: &Joins) -> (usize, usize) {
    let (Some(first), Some(last)) = (rings.first(), rings.last()) else {
        return (0, 0);
    };
    let foot = if closed_base { first.sides as usize } else { 0 };
    let vertices: usize = rings.iter().map(|ring| ring.sides as usize + 1).sum();
    let bands: usize = rings
        .windows(2)
        .map(|pair| (pair[0].sides + pair[1].sides) as usize)
        .sum();
    let cut: usize = joins
        .holes
        .iter()
        .map(|junction| {
            let hole = &junction.hole;
            2 * ((hole.rows[1] - hole.rows[0]) * hole.columns) as usize
        })
        .sum();
    let extra = if closed_base { foot + 2 } else { 1 };
    let stitch = joins.own.map_or(0, |junction| {
        junction.hole.rim_len() + junction.sides as usize
    });
    (
        vertices + extra + stitch,
        3 * (bands - cut + last.sides as usize + foot + stitch),
    )
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

/// Position and unit normal of corner `corner` of ring `index` of `rings`, whose unit
/// circle point is `[cos, sin]`.
///
/// Every vertex of a round or lobed ring is computed here, so that a copy of a ring
/// vertex written elsewhere (the rim of a junction) has exactly the same position. With
/// the radius `rho(a, s) = radius(s) + lobe(s) sin(lobes a)`, the normal of a lobed ring
/// is `radial - (rho_a / rho) tangential - rho_s tangent`, normalized.
fn ring_corner(
    rings: &[Ring],
    index: usize,
    [cos, sin]: [f32; 2],
    corner: usize,
    lobes: u32,
) -> (Vec3, Vec3) {
    let ring = &rings[index];
    let (slope, lobe_slope) = slopes(rings, index);
    let binormal = ring.tangent.cross(ring.normal);
    let radial = ring.normal * cos + binormal * sin;
    if ring.lobe > 0.0 {
        let count = lobes as f32;
        let phase = count * TAU * (corner % ring.sides as usize) as f32 / ring.sides as f32;
        let (wave_sin, wave_cos) = phase.sin_cos();
        let rho = ring.radius + ring.lobe * wave_sin;
        let around = ring.lobe * count * wave_cos / rho;
        let along = slope + lobe_slope * wave_sin;
        let tangential = binormal * cos - ring.normal * sin;
        let normal = (radial - tangential * around - ring.tangent * along).normalize();
        (ring.center + radial * rho, normal)
    } else {
        let shrink = (1.0 + slope * slope).sqrt().recip();
        let axial = ring.tangent * (-slope * shrink);
        (ring.center + radial * ring.radius, radial * shrink + axial)
    }
}

/// Write position in the buffers of one axis.
struct Writer<'a, 'b> {
    out: &'b mut TubeOut<'a>,
    style: &'b Style,
    circles: &'b Circles,
    joins: &'b Joins<'b>,
    vertex: usize,
    index: usize,
}

impl Writer<'_, '_> {
    /// Writes the vertices of ring `index` of `rings`, round or lobed.
    fn ring(&mut self, rings: &[Ring], index: usize) {
        let ring = &rings[index];
        let unit = self.circles.of(ring.sides);
        let u_step = self.style.u_repeats / ring.sides as f32;
        let v = self.style.v_offset + ring.arc * self.style.v_scale;
        for (corner, &point) in unit.iter().enumerate() {
            let (position, normal) = ring_corner(rings, index, point, corner, self.style.lobes);
            let at = self.vertex + corner;
            self.out.positions[at] = position.to_array();
            self.out.normals[at] = normal.to_array();
            self.out.uvs[at] = [corner as f32 * u_step, v];
        }
        self.paint(self.vertex..self.vertex + unit.len(), ring.radius);
        self.vertex += unit.len();
    }

    /// Writes the vertices of a collar ring at `blend`, 0 at the footprint and 1 at the
    /// first round ring. The normal of a vertex is that of the collar as meshed: the
    /// cross product of the chord between its neighbours around the ring and the chord
    /// between its neighbours on the rings before and after, so that it agrees with the
    /// skewed quads around it.
    fn collar_ring(&mut self, sides: u32, junction: &Junction, blend: f32) {
        let u_step = self.style.u_repeats / sides as f32;
        let step = 1.0 / junction.rings as f32;
        let (geometry, feet) = (self.joins.geometry, self.joins.feet);
        let unit = self.circles.of(sides);
        let n = sides as usize;
        let ring = |blend: f32| {
            let mut points = [Vec3::ZERO; RING_LIMIT];
            for (k, point) in points.iter_mut().enumerate().take(n) {
                *point = junction.collar.position(geometry, unit[k], &feet[k], blend);
            }
            points
        };
        let (before, after) = (ring((blend - step).max(0.0)), ring((blend + step).min(1.0)));
        let here = ring(blend);
        for (corner, &point) in unit.iter().enumerate() {
            let foot = &feet[corner % n];
            let (position, smooth, arc, radius) =
                junction.collar.vertex(geometry, point, foot, blend);
            let around = here[(corner + 1) % n] - here[(corner + n - 1) % n];
            let along = after[corner % n] - before[corner % n];
            let normal = around.cross(along).normalize_or(smooth);
            let at = self.vertex + corner;
            self.out.positions[at] = position.to_array();
            self.out.normals[at] = normal.to_array();
            let v = self.style.v_offset + arc * self.style.v_scale;
            self.out.uvs[at] = [corner as f32 * u_step, v];
            self.paint(at..at + 1, radius);
        }
        self.vertex += n + 1;
    }

    /// Writes the colour of the vertices `range`, which lie on an axis of `radius`.
    fn paint(&mut self, range: std::ops::Range<usize>, radius: f32) {
        if let Some(tint) = &self.style.tint {
            self.out.colors[range].fill(tint.at(radius));
        }
    }

    /// Writes one triangle from vertex numbers local to the axis.
    fn triangle(&mut self, a: usize, b: usize, c: usize) {
        let base = self.out.base;
        let corners = [a, b, c].map(|local| base + local as u32);
        self.out.indices[self.index..self.index + 3].copy_from_slice(&corners);
        self.index += 3;
    }

    /// Joins ring `a` (first vertex, sides) to the next ring `b`, leaving out the cells
    /// of row `row` that a hole covers.
    fn band(
        &mut self,
        (a_start, a_sides): (usize, usize),
        (b_start, b_sides): (usize, usize),
        row: u32,
    ) {
        if a_sides == b_sides {
            let holes = self.joins.holes;
            for side in 0..a_sides {
                if holes
                    .iter()
                    .any(|junction| junction.hole.covers(row, side as u32))
                {
                    continue;
                }
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

    /// Writes the rim and the footprint of the own junction in the frame of the parent,
    /// and the triangles between them.
    fn stitch(&mut self, junction: &Junction, parent: &[Ring], parent_style: &Style) {
        let hole = &junction.hole;
        let u_step = parent_style.u_repeats / hole.sides as f32;
        let rim_start = self.vertex;
        for k in 0..hole.rim_len() {
            let (row, column) = hole.rim(k);
            let ring = &parent[row as usize];
            let local = (column % hole.sides) as usize;
            let point = self.circles.of(hole.sides)[local];
            let (position, normal) =
                ring_corner(parent, row as usize, point, local, parent_style.lobes);
            let v = parent_style.v_offset + ring.arc * parent_style.v_scale;
            let uv = [column as f32 * u_step, v];
            self.copy(position, normal, uv, parent_style, ring.radius);
        }
        let host = &junction.host;
        for (k, foot) in self.joins.feet.iter().enumerate() {
            let unit = self.circles.of(junction.sides)[k];
            let (position, ..) = junction.collar.vertex(self.joins.geometry, unit, foot, 0.0);
            let radial = (position - host.origin).reject_from_normalized(host.tangent);
            let [column, height] = foot.param;
            let v = parent_style.v_offset + (host.arc + height) * parent_style.v_scale;
            let normal = radial.normalize_or(hole.normal);
            self.copy(
                position,
                normal,
                [column * u_step, v],
                parent_style,
                host.radius,
            );
        }
        for &[a, b, c] in &self.joins.stitches[junction.stitch.clone()] {
            let local = |k: u32| rim_start + k as usize;
            self.triangle(local(a), local(b), local(c));
        }
    }

    /// Writes one vertex of the stitch with the colour `style` has at `radius`.
    fn copy(&mut self, position: Vec3, normal: Vec3, uv: [f32; 2], style: &Style, radius: f32) {
        let at = self.vertex;
        self.out.positions[at] = position.to_array();
        self.out.normals[at] = normal.to_array();
        self.out.uvs[at] = uv;
        if let Some(tint) = &style.tint {
            self.out.colors[at] = tint.at(radius);
        }
        self.vertex += 1;
    }
}

/// Writes the tube over `rings` into `out`, whose slices must have exactly the lengths
/// given by [`tube_counts`] for the same `joins`.
///
/// # Panics
///
/// Panics when the slices of `out` do not have those lengths.
pub(crate) fn write_tube(
    rings: &[Ring],
    style: &Style,
    circles: &Circles,
    joins: &Joins,
    out: &mut TubeOut,
) {
    let Some(first) = rings.first() else {
        return;
    };
    let mut writer = Writer {
        out,
        style,
        circles,
        joins,
        vertex: 0,
        index: 0,
    };
    let mut previous = (0, 0);
    for (index, ring) in rings.iter().enumerate() {
        let start = writer.vertex;
        match (ring.skew, joins.own) {
            (Some((_, blend)), Some(junction)) => writer.collar_ring(ring.sides, junction, blend),
            _ => writer.ring(rings, index),
        }
        if index > 0 {
            writer.band(previous, (start, ring.sides as usize), index as u32 - 1);
        }
        previous = (start, ring.sides as usize);
    }
    writer.cap(&rings[rings.len() - 1], previous.0);
    if style.closed_base {
        writer.foot(first);
    }
    if let (Some(junction), Some((parent, parent_style))) = (joins.own, joins.parent) {
        writer.stitch(junction, parent, parent_style);
    }
    assert_eq!(writer.vertex, writer.out.positions.len());
    assert_eq!(writer.index, writer.out.indices.len());
}

#[cfg(test)]
mod tests {
    use super::{Circles, Joins, Style, TubeOut, tube_counts, write_tube};
    use crate::ring::{Ring, frame_rings, transport};
    use arbrume_core::species::Geometry;
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
        let geometry = Geometry::default();
        let joins = Joins {
            holes: &[],
            own: None,
            feet: &[],
            stitches: &[],
            parent: None,
            geometry: &geometry,
        };
        let mut rings = rings.to_vec();
        frame_rings(&mut rings);
        let rings = &rings[..];
        let (vertices, indices) = tube_counts(rings, style.closed_base, &joins);
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
        write_tube(rings, style, &Circles::new(64), &joins, &mut out);
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
                    normal: Vec3::ZERO,
                    skew: None,
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
        let geometry = Geometry::default();
        let joins = Joins {
            holes: &[],
            own: None,
            feet: &[],
            stitches: &[],
            parent: None,
            geometry: &geometry,
        };
        let (vertices, indices) = tube_counts(&rings, false, &joins);
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
