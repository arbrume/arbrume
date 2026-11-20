//! Tier B junctions: one closed bark surface, each child axis stitched into a hole cut
//! in the surface of its parent (`docs/junctions.md` has the measurements).
//!
//! With [`Junctions::TierB`](crate::Junctions) every lateral axis is joined to its
//! parent as follows. Everything is planned from the graph before any buffer is filled,
//! so the counts are known in advance and the fill stays one pure task per chain.
//!
//! * **Footprint.** Near its base the child is a surface of revolution about the line
//!   leaving the attachment point along its first internode, of radius `rho(s)`: its
//!   base radius widened by the collar of the species. Near the attachment point the
//!   parent is a cylinder about its tangent there, whose radius varies linearly along
//!   it. For each of the `n` generators of the child (one per side), the arc length `s`
//!   at which it leaves the parent is the larger root of a quadratic (see `exit`). The
//!   `n` points are the footprint: a closed loop on the parent, where the child meets it.
//! * **Collar.** The child surface starts at the footprint: `K` skewed rings go from the
//!   footprint (each vertex at its own arc length) to the first round ring of the child,
//!   placed a margin beyond the farthest footprint point. Along a generator the collar
//!   follows the profile `rho`, so it keeps the widening of the Tier A collar. Everything
//!   of the child before the footprint, inside the parent, is left out.
//! * **Hole.** The parent gets a ring at each end of the footprint, a margin beyond it
//!   along the parent; all rings between those two get the same number of sides, so that
//!   the surface there is a grid of quads. The quads of that grid whose columns meet the
//!   footprint, widened by the same margin around the parent, are left out. A rectangle
//!   of grid cells always has one simple loop as its border (the rim). The margin is half
//!   the base radius of the child, and at least the sagitta of the sides of the parent,
//!   the most its faceted surface departs from the smooth one the footprint lies on.
//! * **Stitch.** The rim and the footprint are two nested loops on the parent surface.
//!   They are joined by triangles in the order of their angles about the centre of the
//!   footprint, in a plane unrolled from the parent and stretched along its axis so that
//!   the bounding box of the footprint is square: the same zip as two rings of different
//!   side counts, giving `m + n` triangles.
//!
//! The rim and the footprint are written twice: once in the frame of the parent for the
//! stitch, once in the frame of the child for the collar. The copies have exactly the
//! same positions (computed by the same code), so the surface is closed once vertices of
//! equal position are welded, as texture seams always require; the copies carry the
//! texture coordinates, normals and colours of their side. The wind data of the rim and
//! of the footprint is that of the parent where they lie, so nothing tears when the plant
//! moves. All of it is written by the child: the stitch and collar are labelled with the
//! child axis.
//!
//! **Conflicts.** The hole of a child stays within the round rings of its parent, after
//! the collar of the parent; a hole that would pass the tip prolongs the parent straight
//! beyond it. Where two holes of one parent would share cells, the rings under them get
//! twice as many sides, up to [`MAX_SIDES`], at most [`REFINEMENTS`] times; what still
//! overlaps is trimmed from the later hole. A footprint point that falls outside its
//! hole is moved onto the parent surface as meshed, inside it, and the collar carries
//! that shift, fading it out towards the first round ring. A child whose first round
//! ring would lie beyond its own tip is entirely inside its parent and is not meshed.

use crate::MeshOptions;
use crate::bark::BarkPlan;
use crate::ring::{Axis, BaseProfile, MIN_SPAN, Ring, Sweep, seam_direction};
use arbrume_core::graph::{ChainId, PlantGraph};
use arbrume_core::species::Geometry;
use glam::Vec3;
use std::f32::consts::{PI, TAU};
use std::ops::Range;
use std::sync::OnceLock;

/// Most sides a parent ring gets to make room for the holes of its children.
const MAX_SIDES: u32 = 64;

/// Most times the rings under conflicting holes are refined.
const REFINEMENTS: u32 = 3;

/// Times the exit of a generator is solved again with the collar radius found.
const FIXED_POINT_STEPS: u32 = 4;

/// Below this, `1 - (axis . tangent)^2` is taken as 0: the child runs along its parent.
const PARALLEL: f32 = 1e-6;

/// Margin around a footprint, in base radii of the child, when that is more than the
/// sagitta of the parent.
const MARGIN: f32 = 0.5;

/// Inset of a moved footprint point from the border of its hole, as a fraction of the
/// margin.
const INSET: f32 = 0.25;

/// One footprint point: the arc length along the child at which its generator leaves the
/// parent, and the shift that moves it into the hole when it had to be moved.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Foot {
    /// Arc length along the child.
    pub arc: f32,
    /// Displacement added at the footprint, fading to 0 at the first round ring.
    pub shift: Vec3,
    /// Unwrapped column coordinate and height of the point in its hole, once placed.
    pub param: [f32; 2],
}

/// The base of a child axis as a surface of revolution about a straight line.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Collar {
    /// Start of the line: the attachment point, on the centerline of the parent.
    pub origin: Vec3,
    /// Unit direction of the line.
    pub axis: Vec3,
    /// Direction of angle 0 around the line (the seam of the child tube).
    pub seam: Vec3,
    /// `axis x seam`, the direction of a quarter turn.
    pub binormal: Vec3,
    /// Radius of the child before widening.
    pub radius: f32,
    /// Widening of the child near its base.
    pub profile: BaseProfile,
    /// Arc length of the first round ring.
    pub end: f32,
    /// Margin left around the footprint in the hole, and between the footprint and the
    /// first round ring: half the base radius, and at least the sagitta of the sides of
    /// the parent (the most its faceted surface departs from the smooth one).
    pub margin: f32,
}

impl Collar {
    /// Radius of the collar at arc length `s`.
    pub(crate) fn rho(&self, geometry: &Geometry, s: f32) -> f32 {
        self.radius * (1.0 + self.profile.widening(geometry, s).0)
    }

    /// Derivative of the collar radius at arc length `s`. The widening of a collar is
    /// `gain (1 - x)^2` with `x = (s - start) / length` clamped to `[0, 1]`.
    fn slope(&self, s: f32) -> f32 {
        match self.profile {
            BaseProfile::Collar {
                start,
                length,
                gain,
                ..
            } if s > start && s < start + length => {
                -2.0 * self.radius * gain * (1.0 - (s - start) / length) / length
            }
            _ => 0.0,
        }
    }

    /// Position of the vertex at `[cos, sin]` of a collar ring at `blend`, as
    /// [`Collar::vertex`] gives it.
    pub(crate) fn position(
        &self,
        geometry: &Geometry,
        corner: [f32; 2],
        foot: &Foot,
        blend: f32,
    ) -> Vec3 {
        let s = foot.arc + (self.end - foot.arc) * blend;
        let radial = self.seam * corner[0] + self.binormal * corner[1];
        self.origin + self.axis * s + radial * self.rho(geometry, s) + foot.shift * (1.0 - blend)
    }

    /// Point at arc length `s` of the generator at `[cos, sin]`.
    fn generator(&self, geometry: &Geometry, [cos, sin]: [f32; 2], s: f32) -> Vec3 {
        let radial = self.seam * cos + self.binormal * sin;
        self.origin + self.axis * s + radial * self.rho(geometry, s)
    }

    /// Position, unit normal, arc length and radius of the vertex at `[cos, sin]` of a
    /// collar ring at `blend` (0 at the footprint, 1 at the first round ring).
    pub(crate) fn vertex(
        &self,
        geometry: &Geometry,
        corner: [f32; 2],
        foot: &Foot,
        blend: f32,
    ) -> (Vec3, Vec3, f32, f32) {
        let s = foot.arc + (self.end - foot.arc) * blend;
        let rho = self.rho(geometry, s);
        let radial = self.seam * corner[0] + self.binormal * corner[1];
        let position = self.origin + self.axis * s + radial * rho + foot.shift * (1.0 - blend);
        let normal = (radial - self.axis * self.slope(s)).normalize_or(radial);
        (position, normal, s, rho)
    }
}

/// The parent near an attachment point: a cylinder about the tangent there.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Host {
    /// Attachment point, on the centerline.
    pub origin: Vec3,
    /// Unit tangent of the parent.
    pub tangent: Vec3,
    /// Arc length of the attachment point along the parent.
    pub arc: f32,
    /// Radius at the attachment point.
    pub radius: f32,
    /// Change of the radius per meter along the parent.
    pub slope: f32,
}

impl Host {
    /// Height along the parent and squared distance from its axis of `point`.
    fn place(&self, point: Vec3) -> (f32, f32) {
        let offset = point - self.origin;
        let height = offset.dot(self.tangent);
        (height, (offset - self.tangent * height).length_squared())
    }
}

/// Where the hole of a junction is in the grid of its parent, and how the stitch is
/// unrolled.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Hole {
    /// Index of the parent chain.
    pub parent: u32,
    /// Rings of the parent bounding the hole, relative to its first ring: the cells of
    /// rows `rows[0]..rows[1]` are left out.
    pub rows: [u32; 2],
    /// First column left out, in `0..sides`.
    pub column: u32,
    /// Number of columns left out, less than `sides`.
    pub columns: u32,
    /// Sides of the parent rings over the hole.
    pub sides: u32,
    /// Direction of angle 0 of the parent around its tangent at the attachment point.
    pub normal: Vec3,
}

impl Hole {
    /// Whether the cell of row `row` and column `column` is left out.
    pub(crate) fn covers(&self, row: u32, column: u32) -> bool {
        row >= self.rows[0]
            && row < self.rows[1]
            && (column + self.sides - self.column) % self.sides < self.columns
    }

    /// Number of vertices of the rim.
    pub(crate) const fn rim_len(&self) -> usize {
        2 * (self.columns + self.rows[1] - self.rows[0]) as usize
    }

    /// Ring (relative to the parent's first) and unwrapped column of rim vertex `k`,
    /// counter-clockwise seen from outside: along the lower ring, up the last column,
    /// back along the upper ring and down the first column.
    pub(crate) fn rim(&self, k: usize) -> (u32, u32) {
        let (width, height) = (
            self.columns as usize,
            (self.rows[1] - self.rows[0]) as usize,
        );
        let (row, column) = if k < width {
            (0, k)
        } else if k < width + height {
            (k - width, width)
        } else if k < 2 * width + height {
            (height, 2 * width + height - k)
        } else {
            (2 * (width + height) - k, 0)
        };
        (self.rows[0] + row as u32, self.column + column as u32)
    }
}

/// Everything about one junction, planned before the fill.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Junction {
    /// The base of the child.
    pub collar: Collar,
    /// The parent near the attachment point.
    pub host: Host,
    /// Footprint points in the shared buffer, one per side of the child.
    pub feet: Range<usize>,
    /// Sides of the collar rings.
    pub sides: u32,
    /// Number of collar rings, the footprint included.
    pub rings: u32,
    /// Where it sits in the parent.
    pub hole: Hole,
    /// Triangles of its stitch in the shared buffer, numbering the rim vertices from 0
    /// and the footprint vertices after them.
    pub stitch: Range<usize>,
}

/// `[cos, sin]` of corner `corner` (taken modulo `sides`) of a ring of `sides` sides, up
/// to 64 sides, read from a table computed once as the tube tables are.
pub(crate) fn corner(sides: u32, corner: u32) -> [f32; 2] {
    static TABLE: OnceLock<Vec<[f32; 2]>> = OnceLock::new();
    let table = TABLE.get_or_init(|| {
        (0..=MAX_SIDES)
            .flat_map(|n| {
                (0..n).map(move |k| {
                    let (sin, cos) = (TAU * k as f32 / n as f32).sin_cos();
                    [cos, sin]
                })
            })
            .collect()
    });
    let start = (sides * sides.saturating_sub(1) / 2) as usize;
    table[start + (corner % sides) as usize]
}

/// The parent of a junction at the node of polyline index `index` of `axis`, whose node
/// arcs are `arcs` (index 0 being the origin).
pub(crate) fn host(axis: &Axis, geometry: &Geometry, arcs: &[f32], index: usize) -> Host {
    let point = |i: usize| {
        if i == 0 {
            axis.origin
        } else {
            axis.positions[i - 1]
        }
    };
    let last = axis.positions.len();
    let incoming = (point(index) - point(index.saturating_sub(1))).normalize_or_zero();
    let outgoing = (point((index + 1).min(last)) - point(index)).normalize_or_zero();
    let tangent = (incoming + outgoing).normalize_or(incoming.normalize_or(Vec3::Y));
    let radius_at = |arc: f32| {
        let i = arcs.partition_point(|&a| a <= arc).clamp(1, last);
        let span = (arcs[i] - arcs[i - 1]).max(MIN_SPAN);
        let t = ((arc - arcs[i - 1]) / span).clamp(0.0, 1.0);
        let (a, b) = (axis.radius(i - 1), axis.radius(i));
        (a + (b - a) * t) * (1.0 + axis.profile.widening(geometry, arc).0)
    };
    let arc = arcs[index];
    let radius = radius_at(arc);
    let step = radius.max(MIN_SPAN);
    Host {
        origin: point(index),
        tangent,
        arc,
        radius,
        slope: (radius_at(arc + step) - radius_at((arc - step).max(0.0)))
            / (arc + step - (arc - step).max(0.0)),
    }
}

/// Arc length at which the generator at `corner` of `collar` leaves `host`, or `None`
/// when it does not.
///
/// For a collar radius `rho`, the generator `axis s + radial rho` (from the attachment
/// point) is at height `h = a s + b` along the parent, `a = axis . tangent` and
/// `b = rho radial . tangent`, and at the squared distance `s^2 + rho^2 - h^2` from its
/// axis. Setting this to the square of the parent radius `R + k h` gives a quadratic in
/// `s` whose larger root is where the generator leaves. The collar radius depends on
/// `s`, so the root is taken again with the radius at the previous root, a few times.
fn exit(collar: &Collar, host: &Host, geometry: &Geometry, [cos, sin]: [f32; 2]) -> Option<f32> {
    let radial = collar.seam * cos + collar.binormal * sin;
    let a = collar.axis.dot(host.tangent);
    let (k, mut s) = (host.slope, 0.0);
    let mut rho = collar.rho(geometry, 0.0);
    for _ in 0..FIXED_POINT_STEPS {
        let b = radial.dot(host.tangent) * rho;
        let base = host.radius + k * b;
        let qa = 1.0 - a * a * (1.0 + k * k);
        let qb = -2.0 * a * (b + k * base);
        let qc = rho * rho - b * b - base * base;
        let discriminant = qb * qb - 4.0 * qa * qc;
        if qa <= PARALLEL || discriminant < 0.0 {
            return None;
        }
        s = ((-qb + discriminant.sqrt()) / (2.0 * qa)).max(0.0);
        let next = collar.rho(geometry, s);
        let settled = (next - rho).abs() <= f32::EPSILON * rho;
        rho = next;
        if settled {
            break;
        }
    }
    Some(s)
}

/// Footprint of `collar` on `host` with `sides` points, appended to `feet`. Returns the
/// lowest and highest height of the footprint along the parent, or `None` when some
/// generator never leaves the parent.
pub(crate) fn footprint(
    collar: &Collar,
    host: &Host,
    geometry: &Geometry,
    sides: u32,
    feet: &mut Vec<Foot>,
) -> Option<(f32, f32)> {
    let (mut low, mut high) = (f32::INFINITY, f32::NEG_INFINITY);
    for k in 0..sides {
        let at = corner(sides, k);
        let arc = exit(collar, host, geometry, at)?;
        let (height, _) = host.place(collar.generator(geometry, at, arc));
        (low, high) = (low.min(height), high.max(height));
        feet.push(Foot {
            arc,
            shift: Vec3::ZERO,
            param: [0.0; 2],
        });
    }
    Some((low, high))
}

/// Half-width of the margin around a footprint, in meters.
pub(crate) const fn margin(collar: &Collar) -> f32 {
    collar.margin
}

/// Angle of `point` about the parent at the attachment point, from `normal`.
fn angle(host: &Host, normal: Vec3, point: Vec3) -> f32 {
    let offset = (point - host.origin).reject_from_normalized(host.tangent);
    let binormal = host.tangent.cross(normal);
    offset.dot(binormal).atan2(offset.dot(normal))
}

/// Angle of the footprint centre and the half-widths of its angular extent below and
/// above it, margin included, about the parent.
pub(crate) fn angular_extent(
    junction: &Junction,
    feet: &[Foot],
    geometry: &Geometry,
    normal: Vec3,
) -> (f32, f32, f32) {
    let collar = &junction.collar;
    let centre = angle(&junction.host, normal, junction.host.origin + collar.axis);
    let (mut below, mut above) = (0.0_f32, 0.0_f32);
    for (k, foot) in feet.iter().enumerate() {
        let point =
            collar.generator(geometry, corner(junction.sides, k as u32), foot.arc) + foot.shift;
        let turn = (angle(&junction.host, normal, point) - centre + PI).rem_euclid(TAU) - PI;
        (below, above) = (below.min(turn), above.max(turn));
    }
    let extra = (margin(collar) / junction.host.radius).min(1.0).asin();
    (centre, extra - below, above + extra)
}

/// Columns of a hole for the angular extent `(centre, below, above)` on rings of
/// `sides` sides: the first column in `0..sides` and the count, at most `sides - 1`.
pub(crate) fn columns(extent: (f32, f32, f32), sides: u32) -> (u32, u32) {
    let step = TAU / sides as f32;
    let first = ((extent.0 - extent.1) / step).floor();
    let last = ((extent.0 + extent.2) / step).ceil();
    let count = ((last - first) as u32).clamp(1, sides - 1);
    (first.rem_euclid(sides as f32) as u32 % sides, count)
}

/// Trims the columns of `hole` so that it shares no cell with `other`, keeping the
/// column `keep`; returns `false` when that is impossible.
pub(crate) fn trim(hole: &mut Hole, other: &Hole, keep: u32) -> bool {
    let rows_meet = hole.rows[0] < other.rows[1] && other.rows[0] < hole.rows[1];
    if !rows_meet || other.sides != hole.sides {
        return true;
    }
    let n = hole.sides;
    let local = |column: u32| (column + n - hole.column) % n;
    let kept = local(keep);
    let (mut low, mut high) = (0, hole.columns);
    for column in (0..other.columns).map(|c| local(other.column + c)) {
        if column == kept {
            return false;
        }
        if column < kept {
            low = low.max(column + 1);
        } else if column < hole.columns {
            high = high.min(column);
        }
    }
    hole.column = (hole.column + low) % n;
    hole.columns = high - low;
    true
}

/// The parent surface as it is meshed over a hole: the bilinear patches of its grid.
struct Grid<'a> {
    host: &'a Host,
    rings: &'a [Ring],
    /// `[cos, sin]` of the corners of the rings, which all have the same side count.
    circle: &'a [[f32; 2]],
    rows: [usize; 2],
    normal: Vec3,
    binormal: Vec3,
}

impl Grid<'_> {
    /// Point of the polygonal ring `ring` in the direction `radial` from its centre: on
    /// the side of the ring crossed by that direction, as the tube writes it.
    fn ring_point(&self, ring: &Ring, radial: Vec3) -> Vec3 {
        let binormal = ring.tangent.cross(ring.normal);
        let turn = radial
            .dot(binormal)
            .atan2(radial.dot(ring.normal))
            .rem_euclid(TAU);
        let step = TAU / ring.sides as f32;
        let side = ((turn / step).floor() as u32).min(ring.sides - 1);
        let fraction = turn / step - side as f32;
        let at = |k: u32| {
            let [cos, sin] = self.circle[(k % ring.sides) as usize];
            ring.center + (ring.normal * cos + binormal * sin) * ring.radius
        };
        at(side).lerp(at(side + 1), fraction)
    }

    /// Point of the meshed surface at `turn` (from the hole normal) and `height`.
    fn point(&self, turn: f32, height: f32) -> Vec3 {
        let (sin, cos) = turn.sin_cos();
        let radial = self.normal * cos + self.binormal * sin;
        let arc = self.host.arc + height;
        let [low, high] = self.rows;
        let row = (low + self.rings[low..high].partition_point(|ring| ring.arc <= arc))
            .clamp(low + 1, high);
        let (below, above) = (&self.rings[row - 1], &self.rings[row]);
        let blend = ((arc - below.arc) / (above.arc - below.arc).max(MIN_SPAN)).clamp(0.0, 1.0);
        self.ring_point(below, radial)
            .lerp(self.ring_point(above, radial), blend)
    }
}

/// Moves the footprint points of `junction` that fall outside its hole (after trimming,
/// or when the hole reaches the end of the round rings of the parent) into it, onto the
/// parent surface as it is meshed. `rings` are the rings of the parent, `circle` the
/// unit circle of their side count.
pub(crate) fn fit(
    junction: &Junction,
    feet: &mut [Foot],
    rings: &[Ring],
    geometry: &Geometry,
    circle: &[[f32; 2]],
) {
    let hole = &junction.hole;
    let host = &junction.host;
    let step = TAU / hole.sides as f32;
    let [low_row, high_row] = hole.rows.map(|row| row as usize);
    let low_arc = rings[low_row].arc - host.arc;
    let high_arc = rings[high_row].arc - host.arc;
    let inset_rows = (INSET * margin(&junction.collar)).min(0.25 * (high_arc - low_arc));
    let first = hole.column as f32 * step;
    let span = hole.columns as f32 * step;
    let inset = (INSET * margin(&junction.collar) / host.radius).min(0.25 * span);
    let grid = Grid {
        host,
        rings,
        circle,
        rows: [low_row, high_row],
        normal: hole.normal,
        binormal: host.tangent.cross(hole.normal),
    };
    for (k, foot) in feet.iter_mut().enumerate() {
        let at = corner(junction.sides, k as u32);
        let collar = &junction.collar;
        let point = collar.generator(geometry, at, foot.arc);
        let (height, _) = host.place(point);
        let turn = (angle(host, hole.normal, point) - first).rem_euclid(TAU);
        let (turn_low, turn_high) = (inset, span - inset);
        let (height_low, height_high) = (low_arc + inset_rows, high_arc - inset_rows);
        let outside = turn < turn_low || turn > turn_high;
        if outside || height < height_low || height > height_high {
            let clamped_turn = if turn > span + 0.5 * (TAU - span) {
                turn_low
            } else {
                turn.clamp(turn_low, turn_high)
            };
            let clamped_height = height.clamp(height_low, height_high);
            foot.shift = grid.point(first + clamped_turn, clamped_height) - point;
        }
    }
}

/// Unrolled plane of a stitch: `x` around the parent, `y` along it, both in meters.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Unroll {
    host: Host,
    normal: Vec3,
    /// Angle of the middle of the hole.
    middle: f32,
}

impl Unroll {
    /// The plane of `junction`.
    pub(crate) fn new(junction: &Junction) -> Self {
        let hole = &junction.hole;
        Self {
            host: junction.host,
            normal: hole.normal,
            middle: (hole.column as f32 + 0.5 * hole.columns as f32) * TAU / hole.sides as f32,
        }
    }

    /// Column coordinate, unwrapped about the middle of the hole, and height of `point`.
    pub(crate) fn param(&self, point: Vec3, sides: u32) -> (f32, f32) {
        let turn = (angle(&self.host, self.normal, point) - self.middle + PI).rem_euclid(TAU) - PI;
        let (height, _) = self.host.place(point);
        ((self.middle + turn) * sides as f32 / TAU, height)
    }

    /// Plane coordinates of the column coordinate `column` and the height `height`.
    pub(crate) fn plane(&self, column: f32, height: f32, sides: u32) -> [f32; 2] {
        [column * TAU / sides as f32 * self.host.radius, height]
    }
}

/// One turn in the units of [`pseudo_angle`].
const TURN: f32 = 4.0;

/// An increasing function of the angle of `(x, y)` from the `x` axis, in `[0, 4)`, that
/// costs no trigonometry: the position along the perimeter of the unit diamond.
fn pseudo_angle(x: f32, y: f32) -> f32 {
    let sum = x.abs() + y.abs();
    if sum == 0.0 {
        return 0.0;
    }
    let along = y / sum;
    if x >= 0.0 {
        if y >= 0.0 { along } else { TURN + along }
    } else {
        2.0 - along
    }
}

/// Sets the parameters of the footprint points of `junction` in its hole and appends
/// the triangles of its stitch to `stitches`; returns their range. `rings` are the rings
/// of the parent.
fn plan_stitch(
    junction: &Junction,
    feet: &mut [Foot],
    rings: &[Ring],
    geometry: &Geometry,
    loops: &mut Loops,
    stitches: &mut Vec<[u32; 3]>,
) -> Range<usize> {
    let hole = &junction.hole;
    let unroll = Unroll::new(junction);
    loops.outer.clear();
    loops.outer.extend((0..hole.rim_len()).map(|k| {
        let (row, column) = hole.rim(k);
        let height = rings[row as usize].arc - junction.host.arc;
        unroll.plane(column as f32, height, hole.sides)
    }));
    loops.inner.clear();
    loops
        .inner
        .extend(feet.iter_mut().enumerate().map(|(k, foot)| {
            let unit = corner(junction.sides, k as u32);
            let (position, ..) = junction.collar.vertex(geometry, unit, foot, 0.0);
            let (column, height) = unroll.param(position, hole.sides);
            foot.param = [column, height];
            unroll.plane(column, height, hole.sides)
        }));
    let first = stitches.len();
    stitch(&loops.outer, &loops.inner, &mut loops.angles, |a, b, c| {
        stitches.push([a, b, c].map(|k| k as u32));
    });
    first..stitches.len()
}

/// Emits the stitch between the rim (`outer`, counter-clockwise) and the footprint
/// (`inner`, counter-clockwise), given their plane coordinates, as triangles of vertex
/// numbers `i` for the rim and `outer.len() + j` for the footprint. `angles` is working
/// memory. The loops are merged by
/// their angles about the centre of the bounding box of the footprint, in a plane
/// stretched along `y` so that this box is square.
pub(crate) fn stitch(
    outer: &[[f32; 2]],
    inner: &[[f32; 2]],
    angles: &mut [Vec<f32>; 2],
    mut triangle: impl FnMut(usize, usize, usize),
) {
    let low = inner
        .iter()
        .fold([f32::INFINITY; 2], |m, p| [m[0].min(p[0]), m[1].min(p[1])]);
    let high = inner.iter().fold([f32::NEG_INFINITY; 2], |m, p| {
        [m[0].max(p[0]), m[1].max(p[1])]
    });
    let centre = [
        f32::midpoint(low[0], high[0]),
        f32::midpoint(low[1], high[1]),
    ];
    let stretch = ((high[0] - low[0]) / (high[1] - low[1]).max(f32::MIN_POSITIVE)).clamp(1e-3, 1e3);
    let angle_of = |p: &[f32; 2]| pseudo_angle(p[0] - centre[0], (p[1] - centre[1]) * stretch);
    let mut first = [0; 2];
    for (side, (loop_, out)) in [outer, inner]
        .into_iter()
        .zip(angles.iter_mut())
        .enumerate()
    {
        out.clear();
        out.extend(loop_.iter().map(angle_of));
        let from = (0..out.len())
            .min_by(|&a, &b| out[a].total_cmp(&out[b]))
            .unwrap_or(0);
        let base = out[from];
        out.rotate_left(from);
        for value in out.iter_mut().skip(1) {
            *value = base + (*value - base).rem_euclid(TURN);
        }
        out.push(base + TURN);
        first[side] = from;
    }
    let lengths = [outer.len(), inner.len()];
    let vertex =
        |side: usize, step: usize| side * lengths[0] + (first[side] + step) % lengths[side];
    let mut steps = [0, 0];
    while steps[0] < lengths[0] || steps[1] < lengths[1] {
        let next = |side: usize| {
            if steps[side] < lengths[side] {
                angles[side][steps[side] + 1]
            } else {
                f32::INFINITY
            }
        };
        let side = usize::from(next(0) > next(1));
        let (rim, foot) = (vertex(0, steps[0]), vertex(1, steps[1]));
        triangle(rim, vertex(side, steps[side] + 1), foot);
        steps[side] += 1;
    }
}

/// Number of collar rings over a collar of `length` whose profile needs `zone_rings`
/// rings over its whole length `zone`.
pub(crate) fn collar_rings(length: f32, zone: (f32, f32, f32)) -> u32 {
    let (start, end, rings) = zone;
    if !end.is_finite() || end <= start {
        return 1;
    }
    ((rings * length / (end - start)).ceil() as u32).clamp(1, 32)
}

/// Marks a chain without a junction in [`Scratch`].
const NONE: u32 = u32::MAX;

/// Working memory of the junction planning, reused from one mesh to the next.
#[derive(Debug, Clone, Default)]
pub(crate) struct Scratch {
    /// Junction of each chain, by chain id, or [`NONE`].
    own: Vec<u32>,
    /// The children of chain `c` are `children[first_child[c]..first_child[c + 1]]`.
    first_child: Vec<u32>,
    children: Vec<u32>,
    /// Arc lengths at which the chain being planned needs a ring.
    pub stops: Vec<f32>,
    /// Length by which the chain being planned is prolonged beyond its tip.
    pub extend: f32,
    /// Arc length of each point of the polyline of the chain being planned.
    arcs: Vec<f32>,
    /// Arc lengths along its parent of the ends of the hole of each junction.
    spans: Vec<[f32; 2]>,
    /// Unrolled rim and footprint of the stitch being planned.
    loops: Loops,
    /// Normal of the parent and angular extent of each hole of the chain being planned.
    extents: Vec<(Vec3, (f32, f32, f32))>,
    /// `[cos, sin]` of the corners of the rings over the hole being placed.
    circle: Vec<[f32; 2]>,
    /// Rings of the chain being planned after its collar.
    tail: Vec<Ring>,
    /// Holes of the chain being planned, while their columns are refined.
    placed: Vec<Hole>,
}

/// Plane coordinates of the rim and of the footprint of a stitch.
#[derive(Debug, Clone, Default)]
struct Loops {
    outer: Vec<[f32; 2]>,
    inner: Vec<[f32; 2]>,
    angles: [Vec<f32>; 2],
}

impl Scratch {
    /// Resets the memory for `graph`.
    pub(crate) fn start(&mut self, graph: &PlantGraph) {
        let count = graph.chain_count();
        self.own.clear();
        self.own.resize(count, NONE);
        self.spans.clear();
        self.first_child.clear();
        self.first_child.resize(count + 1, 0);
        for chain in graph.chain_ids() {
            if let Some(parent) = graph.parent_chain(chain) {
                self.first_child[parent.index() + 1] += 1;
            }
        }
        for c in 0..count {
            self.first_child[c + 1] += self.first_child[c];
        }
        self.children.clear();
        self.children.resize(self.first_child[count] as usize, 0);
        let mut next = self.first_child.clone();
        for chain in graph.chain_ids() {
            if let Some(parent) = graph.parent_chain(chain) {
                self.children[next[parent.index()] as usize] = chain.0;
                next[parent.index()] += 1;
            }
        }
    }

    /// The junction of chain `chain` with its parent, if it was planned.
    pub(crate) fn own(&self, chain: usize) -> Option<usize> {
        let own = self.own[chain];
        (own != NONE).then_some(own as usize)
    }
}

/// What the planning of junctions reads besides the plan.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Context<'a> {
    /// The plant.
    pub graph: &'a PlantGraph,
    /// Species geometry and tolerance.
    pub sweep: &'a Sweep<'a>,
    /// Options of the mesh.
    pub options: &'a MeshOptions,
}

/// Plans the junctions of the kept children of `chain`, sampled as `axis`, whose own
/// junction is `own`: their footprints, collars and the arc lengths of the ends of their
/// holes, which go to `plan.scratch.stops` with the start of the round rings of `chain`.
pub(crate) fn plan_children(
    plan: &mut BarkPlan,
    context: &Context,
    axis: &Axis,
    chain: ChainId,
    own: Option<usize>,
) {
    let (graph, sweep) = (context.graph, context.sweep);
    let geometry = sweep.geometry;
    plan.scratch.stops.clear();
    plan.scratch.extend = 0.0;
    let first_arc = own.map_or(0.0, |own| plan.junctions[own].collar.end);
    if own.is_some() {
        plan.scratch.stops.push(first_arc);
    }
    let arcs = &mut plan.scratch.arcs;
    arcs.clear();
    arcs.push(0.0);
    let mut previous = axis.origin;
    for &point in axis.positions {
        arcs.push(arcs[arcs.len() - 1] + point.distance(previous));
        previous = point;
    }
    let last_arc = arcs[arcs.len() - 1];
    let Some(first_node) = graph.chain_view(chain).map(|view| view.first.index()) else {
        return;
    };
    let range = plan.scratch.first_child[chain.index()] as usize
        ..plan.scratch.first_child[chain.index() + 1] as usize;
    for slot in range {
        let child = ChainId(plan.scratch.children[slot]);
        let Some(view) = graph.chain_view(child) else {
            continue;
        };
        let nodes = context
            .options
            .max_cycle
            .map_or(view.len(), |cycle| view.born_by(cycle));
        let index = view.attach.index() + 1 - first_node;
        if index > axis.positions.len()
            || !crate::bark::kept(&view, nodes, context.options, plan, true)
        {
            continue;
        }
        let child_axis = Axis {
            origin: view.origin,
            positions: &view.positions[..nodes],
            radii: &view.radii[..nodes],
            profile: BaseProfile::Plain,
            stops: &[],
            extend: 0.0,
        };
        let Some(direction) = child_axis.first_direction() else {
            continue;
        };
        let seam = seam_direction(direction);
        let collar = Collar {
            origin: view.origin,
            axis: direction,
            seam,
            binormal: direction.cross(seam),
            radius: child_axis.radius(0),
            profile: crate::bark::base_profile(graph, &view, &child_axis, sweep),
            end: 0.0,
            margin: 0.0,
        };
        let host = host(axis, geometry, &plan.scratch.arcs, index);
        let sagitta = host.radius * (1.0 - (PI / sweep.sides(host.radius, 0.0) as f32).cos());
        let collar = Collar {
            margin: (MARGIN * collar.radius).max(sagitta),
            ..collar
        };
        let planned = Planned {
            collar,
            host,
            axis: child_axis,
            span: [first_arc, last_arc],
        };
        if let Some(junction) = plan_one(plan, sweep, &planned, chain) {
            plan.scratch.own[child.index()] = junction;
        }
    }
    plan.scratch.stops.sort_by(f32::total_cmp);
}

/// One child being planned: its collar, the parent near its base, its length and the
/// arc lengths of its parent between which a hole can go.
struct Planned<'a> {
    collar: Collar,
    host: Host,
    axis: Axis<'a>,
    span: [f32; 2],
}

/// Whether the polyline of `axis` is longer than `arc`.
fn reaches(axis: &Axis, arc: f32) -> bool {
    let mut previous = axis.origin;
    let mut total = 0.0;
    for &point in axis.positions {
        total += point.distance(previous);
        if total > arc {
            return true;
        }
        previous = point;
    }
    false
}

/// Plans the junction of one child; returns its index in `plan.junctions`, or `None`
/// when the child does not come out of its parent.
fn plan_one(plan: &mut BarkPlan, sweep: &Sweep, planned: &Planned, parent: ChainId) -> Option<u32> {
    let geometry = sweep.geometry;
    let mut collar = planned.collar;
    let sides = sweep.sides(collar.rho(geometry, 0.0), 0.0);
    let feet_start = plan.feet.len();
    let Some((low, high)) = footprint(&collar, &planned.host, geometry, sides, &mut plan.feet)
    else {
        plan.feet.truncate(feet_start);
        return None;
    };
    let feet = &plan.feet[feet_start..];
    let near = feet
        .iter()
        .map(|foot| foot.arc)
        .fold(f32::INFINITY, f32::min);
    let far = feet.iter().map(|foot| foot.arc).fold(0.0, f32::max);
    collar.end = far + margin(&collar);
    if !reaches(&planned.axis, collar.end + MIN_SPAN) {
        plan.feet.truncate(feet_start);
        return None;
    }
    let margin = margin(&collar);
    let [first, last] = planned.span;
    let arc = planned.host.arc;
    let span = [
        (arc + low - margin).clamp(first, last),
        (arc + high + margin).max(first),
    ];
    plan.scratch.extend = plan.scratch.extend.max(span[1] - last);
    plan.scratch.stops.extend(span);
    plan.scratch.spans.push(span);
    plan.junctions.push(Junction {
        collar,
        host: planned.host,
        feet: feet_start..plan.feet.len(),
        sides,
        rings: collar_rings(collar.end - near, collar.profile.zone()),
        hole: Hole {
            parent: parent.0,
            rows: [0, 0],
            column: 0,
            columns: 0,
            sides,
            normal: Vec3::X,
        },
        stitch: 0..0,
    });
    Some((plan.junctions.len() - 1) as u32)
}

/// Replaces the rings of a chain, which start at `start` in `plan.rings`, that lie before
/// the first round ring of its junction `own` with the collar rings of that junction.
pub(crate) fn collar_rings_into(plan: &mut BarkPlan, own: usize, start: usize) {
    let junction = plan.junctions[own].clone();
    let feet = &plan.feet[junction.feet.clone()];
    let collar = &junction.collar;
    let first_round = plan.rings[start..]
        .iter()
        .position(|ring| ring.arc >= collar.end - MIN_SPAN)
        .map_or(plan.rings.len(), |at| start + at);
    let count = junction.rings;
    let geometry = &plan.geometry;
    let rings = (0..count).map(|k| {
        let blend = k as f32 / count as f32;
        let total: f32 = feet
            .iter()
            .map(|foot| foot.arc + (collar.end - foot.arc) * blend)
            .sum();
        let arc = total / feet.len() as f32;
        Ring {
            center: collar.origin + collar.axis * arc,
            tangent: collar.axis,
            arc,
            radius: collar.rho(geometry, arc),
            lobe: 0.0,
            sides: junction.sides,
            normal: Vec3::ZERO,
            skew: Some((own as u32, blend)),
        }
    });
    let tail = &mut plan.scratch.tail;
    tail.clear();
    tail.extend(plan.rings.drain(first_round..));
    plan.rings.truncate(start);
    plan.rings.extend(rings);
    plan.rings.extend_from_slice(tail);
}

/// Index of the ring of `rings[from..]` whose arc length is nearest `arc`.
fn nearest(rings: &[Ring], from: usize, arc: f32) -> usize {
    let after = from + rings[from..].partition_point(|ring| ring.arc < arc);
    let before = after.saturating_sub(1).max(from);
    let after = after.min(rings.len() - 1);
    if (rings[after].arc - arc).abs() < (arc - rings[before].arc).abs() {
        after
    } else {
        before
    }
}

/// Places the holes of the junctions `holes_start..` in the rings of their parent, which
/// start at `start` in `plan.rings` and whose own junction is `own`: rows, equal side
/// counts over each hole, columns, trimming against the earlier holes, and the footprint
/// points moved into their holes. Where two holes would share cells, the rings under
/// them get twice as many sides (up to 64) so that their columns are narrower, before
/// any hole is trimmed. A junction that finds no room is dropped, and its child is not
/// meshed.
pub(crate) fn place_holes(
    plan: &mut BarkPlan,
    start: usize,
    holes_start: usize,
    own: Option<usize>,
) {
    assign_rows(plan, start, holes_start, own);
    measure_extents(plan, start, holes_start);
    let refinements = if plan.junctions.len() > holes_start + 1 {
        REFINEMENTS
    } else {
        0
    };
    for _ in 0..refinements {
        equalize(plan, start, holes_start);
        if !refine(plan, start, holes_start) {
            break;
        }
    }
    equalize(plan, start, holes_start);
    for index in holes_start..plan.junctions.len() {
        if !place_one(plan, start, holes_start, index) {
            plan.junctions[index].hole.columns = 0;
            plan.junctions[index].hole.rows = [0, 0];
            if let Some(child) = plan
                .scratch
                .own
                .iter()
                .position(|&own| own as usize == index)
            {
                plan.scratch.own[child] = NONE;
            }
        }
    }
    let widest = plan.rings[start..].iter().map(|ring| ring.sides).max();
    plan.max_sides = plan.max_sides.max(widest.unwrap_or(0));
}

/// Normal of the parent and angular extent of each of the holes `holes_start..`, kept
/// in the scratch memory: they do not depend on the side counts.
fn measure_extents(plan: &mut BarkPlan, start: usize, holes_start: usize) {
    let rings = &plan.rings[start..];
    plan.scratch.extents.clear();
    for junction in &plan.junctions[holes_start..] {
        let [low, high] = junction.hole.rows;
        let at = nearest(rings, low as usize, junction.host.arc).min(high as usize);
        let normal = rings.get(at).map_or(Vec3::X, |ring| ring.normal);
        let feet = &plan.feet[junction.feet.clone()];
        let extent = angular_extent(junction, feet, &plan.geometry, normal);
        plan.scratch.extents.push((normal, extent));
    }
}

/// Rows of the holes `holes_start..`: the rings at or beyond the ends of their spans,
/// within the round rings of the parent.
fn assign_rows(plan: &mut BarkPlan, start: usize, holes_start: usize, own: Option<usize>) {
    let first_row = own.map_or(0, |own| plan.junctions[own].rings as usize);
    let rings = &plan.rings[start..];
    let last_row = rings.len().saturating_sub(1);
    for (index, junction) in plan.junctions.iter_mut().enumerate().skip(holes_start) {
        let span = plan.scratch.spans[index];
        if first_row >= last_row {
            continue;
        }
        let below =
            |arc: f32| first_row + rings[first_row..].partition_point(|r| r.arc <= arc + MIN_SPAN);
        let low = below(span[0])
            .saturating_sub(1)
            .clamp(first_row, last_row - 1);
        let high = below(span[1] - 2.0 * MIN_SPAN).clamp(low + 1, last_row);
        junction.hole.rows = [low as u32, high as u32];
    }
}

/// Gives all rings over each hole the largest side count among them.
fn equalize(plan: &mut BarkPlan, start: usize, holes_start: usize) {
    let rings = &mut plan.rings[start..];
    let holes = &plan.junctions[holes_start..];
    let mut changed = true;
    while changed {
        changed = false;
        for rows in holes
            .iter()
            .map(|junction| junction.hole.rows)
            .filter(|r| r[1] > r[0])
        {
            let span = rows[0] as usize..=rows[1] as usize;
            let most = rings[span.clone()]
                .iter()
                .map(|ring| ring.sides)
                .max()
                .unwrap_or(3);
            for ring in &mut rings[span] {
                changed |= ring.sides != most;
                ring.sides = most;
            }
        }
    }
}

/// Doubles the side count of the rings under every two holes that would share cells;
/// returns whether a ring changed.
fn refine(plan: &mut BarkPlan, start: usize, holes_start: usize) -> bool {
    let mut placed = std::mem::take(&mut plan.scratch.placed);
    placed.clear();
    placed.extend(
        (holes_start..plan.junctions.len())
            .map(|index| columns_of(plan, start, holes_start, index)),
    );
    let mut changed = false;
    for (i, a) in placed.iter().enumerate() {
        for b in &placed[..i] {
            if a.columns == 0 || b.columns == 0 || !shares_cells(a, b) {
                continue;
            }
            let rows = a.rows[0].min(b.rows[0]) as usize..=a.rows[1].max(b.rows[1]) as usize;
            for ring in &mut plan.rings[start..][rows] {
                let finer = (ring.sides * 2).min(MAX_SIDES);
                changed |= finer != ring.sides;
                ring.sides = finer;
            }
        }
    }
    plan.scratch.placed = placed;
    changed
}

/// Whether two holes of one parent share a cell.
fn shares_cells(a: &Hole, b: &Hole) -> bool {
    let rows_meet = a.rows[0] < b.rows[1] && b.rows[0] < a.rows[1];
    rows_meet
        && (0..b.columns).any(|c| {
            let column = (b.column + c) % b.sides;
            a.sides == b.sides && (column + a.sides - a.column) % a.sides < a.columns
        })
}

/// Hole `index` with its normal, sides and columns for the current rings, before
/// trimming; no columns when it has no rows.
fn columns_of(plan: &BarkPlan, start: usize, holes_start: usize, index: usize) -> Hole {
    let junction = &plan.junctions[index];
    let mut hole = junction.hole;
    let [low, high] = hole.rows;
    if high <= low {
        hole.columns = 0;
        return hole;
    }
    let (normal, extent) = plan.scratch.extents[index - holes_start];
    hole.normal = normal;
    hole.sides = plan.rings[start + low as usize].sides;
    (hole.column, hole.columns) = columns(extent, hole.sides);
    hole
}

/// Columns of hole `index`, trimmed against the holes before it, and its footprint
/// moved into it; `false` when it finds no room.
fn place_one(plan: &mut BarkPlan, start: usize, holes_start: usize, index: usize) -> bool {
    let mut junction = plan.junctions[index].clone();
    junction.hole = columns_of(plan, start, holes_start, index);
    if junction.hole.columns == 0 {
        return false;
    }
    let (_, extent) = plan.scratch.extents[index - holes_start];
    let sides = junction.hole.sides;
    let step = TAU / sides as f32;
    let keep = (extent.0 / step).floor().rem_euclid(sides as f32) as u32 % sides;
    for other in &plan.junctions[holes_start..index] {
        if other.hole.columns > 0 && !trim(&mut junction.hole, &other.hole, keep) {
            return false;
        }
    }
    let rings = &plan.rings[start..];
    let feet = &mut plan.feet[junction.feet.clone()];
    let circle = &mut plan.scratch.circle;
    circle.clear();
    circle.extend((0..sides).map(|k| corner(sides, k)));
    fit(&junction, feet, rings, &plan.geometry, circle);
    let loops = &mut plan.scratch.loops;
    junction.stitch = plan_stitch(
        &junction,
        feet,
        rings,
        &plan.geometry,
        loops,
        &mut plan.stitches,
    );
    plan.junctions[index] = junction;
    true
}

#[cfg(test)]
mod tests {
    use super::{Hole, stitch};

    #[test]
    fn rim_runs_around_the_rectangle() {
        let hole = Hole {
            parent: 0,
            rows: [2, 4],
            column: 7,
            columns: 3,
            sides: 8,
            normal: glam::Vec3::X,
        };
        let rim: Vec<(u32, u32)> = (0..hole.rim_len()).map(|k| hole.rim(k)).collect();
        assert_eq!(rim.len(), 10);
        assert_eq!(rim[0], (2, 7));
        assert_eq!(rim[3], (2, 10));
        assert_eq!(rim[5], (4, 10));
        assert_eq!(rim[8], (4, 7));
        assert_eq!(rim[9], (3, 7));
        assert!(hole.covers(2, 7) && hole.covers(3, 1) && !hole.covers(3, 2) && !hole.covers(4, 0));
    }

    #[test]
    fn stitch_of_nested_loops_is_counter_clockwise() {
        let outer: Vec<[f32; 2]> = [
            [-2.0, -2.0],
            [0.0, -2.0],
            [2.0, -2.0],
            [2.0, 0.0],
            [2.0, 2.0],
            [0.0, 2.0],
            [-2.0, 2.0],
            [-2.0, 0.0],
        ]
        .to_vec();
        let inner: Vec<[f32; 2]> = (0..5)
            .map(|k| {
                let a = std::f32::consts::TAU * k as f32 / 5.0 + 0.3;
                [0.6 * a.cos() + 0.2, 0.9 * a.sin()]
            })
            .collect();
        let all: Vec<[f32; 2]> = outer.iter().chain(&inner).copied().collect();
        let mut count = 0;
        stitch(&outer, &inner, &mut [Vec::new(), Vec::new()], |a, b, c| {
            let (p, q, r) = (all[a], all[b], all[c]);
            let area = (q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0]);
            assert!(area > 0.0, "{a} {b} {c}");
            count += 1;
        });
        assert_eq!(count, 13);
    }
}
