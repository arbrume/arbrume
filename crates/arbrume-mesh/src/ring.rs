//! Ring placement: where the cross-sections of one axis go.
//!
//! An axis is the polyline `origin, positions[0], positions[1], ...` with a radius at
//! each point. [`sample_axis`] turns it into a list of [`Ring`]s, the cross-sections the
//! tube is stretched over, in one walk from base to tip:
//!
//! * **Smoothing.** Between two consecutive nodes the centerline is a cubic Hermite
//!   segment. The tangent at a node is the bisector of the two chords that meet there and
//!   its magnitude is the length of the chord being drawn, which keeps the curve close to
//!   the polyline: it cannot loop or swing wide the way a uniform Catmull-Rom spline does
//!   when internode lengths differ. The curve passes through every node.
//! * **Ring spacing from curvature.** A segment of length `L` that turns by the angle
//!   `a` deviates from its chord by about `L a / 8`, so it gets
//!   `ceil(sqrt(L a / (8 tolerance)))` rings.
//! * **Node merging.** A node gets no ring when the nodes skipped since the last ring
//!   stay within the tolerance of a straight line and their radii within the tolerance of
//!   the radius at that ring. Straight twigs then cost two rings however many internodes
//!   they have.
//! * **Base profile.** Near its base an axis is widened: a root axis by the flare of the
//!   species, a lateral axis by a collar ([`BaseProfile`]). Rings are added there so that
//!   the profile is followed within the tolerance.
//!
//! The radius varies linearly between rings. The radius at node `i` is that of the
//! internode ending there, so the surface at a node is exactly as wide as the graph says,
//! which is what leaves and child axes are placed against.

use arbrume_core::species::Geometry;
use glam::Vec3;
use std::f32::consts::PI;

/// Chords and spans shorter than this, in meters, are treated as empty.
const MIN_SPAN: f32 = 1e-6;

/// Smallest radius of a ring, in meters, so that no cross-section collapses to a point.
const MIN_RADIUS: f32 = 1e-5;

/// Most rings one curved segment is cut into.
const MAX_SPAN_RINGS: f32 = 64.0;

/// Most rings used to follow a flare or a collar.
const MAX_BASE_RINGS: f32 = 32.0;

/// Fraction of the axis length over which `Geometry::flare_factor` differs from 1.
const FLARE_ZONE: f32 = 0.125;

/// One cross-section of an axis.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Ring {
    /// Point of the centerline.
    pub center: Vec3,
    /// Unit tangent of the centerline; the ring lies in the plane orthogonal to it.
    pub tangent: Vec3,
    /// Arc length from the start of the axis, measured along the polyline.
    pub arc: f32,
    /// Mean radius.
    pub radius: f32,
    /// Amplitude of the buttress lobes: the radius at the angle `a` around the ring is
    /// `radius + lobe * sin(lobes * a)`. Zero when the lobes are below the tolerance.
    pub lobe: f32,
    /// Number of sides.
    pub sides: u32,
}

/// Species geometry and tolerance shared by all axes of one mesh.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Sweep<'a> {
    /// Geometry layer of the species.
    pub geometry: &'a Geometry,
    /// Largest deviation from the smooth surface, in meters.
    pub tolerance: f32,
}

impl Sweep<'_> {
    /// Number of sides of a ring: a regular polygon of `n` sides deviates from its
    /// circle of radius `r` by about `r pi^2 / (2 n^2)`, which gives
    /// `n = pi sqrt(r / (2 tolerance))`, kept within the bounds of the species. A lobed
    /// ring gets at least four sides per lobe.
    pub(crate) fn sides(&self, radius: f32, lobe: f32) -> u32 {
        let [fewest, most] = self.geometry.radial_segments;
        let ideal = PI * (radius / (2.0 * self.tolerance)).sqrt();
        let sides = (ideal.ceil() as u32).clamp(fewest, most);
        if lobe > 0.0 {
            sides.max(4 * self.geometry.buttress_lobes)
        } else {
            sides
        }
    }

    /// Largest value [`Sweep::sides`] can return.
    pub(crate) fn max_sides(&self) -> u32 {
        self.geometry.radial_segments[1].max(4 * self.geometry.buttress_lobes)
    }
}

/// Widening of an axis near its base.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum BaseProfile {
    /// No widening.
    Plain,
    /// Trunk flare over the first eighth of an axis of the given length, followed with
    /// `rings` rings.
    Flare {
        /// Arc length of the whole axis.
        length: f32,
        /// Number of rings over the flared part.
        rings: f32,
    },
    /// Collar of a lateral axis: the radius is multiplied by `1 + gain` up to the arc
    /// length `start`, where the axis leaves its parent, and falls back to the plain
    /// radius over `length` as `gain * (1 - x)^2`.
    Collar {
        /// Arc length at which the axis crosses the surface of its parent.
        start: f32,
        /// Arc length of the collar.
        length: f32,
        /// Relative widening at the start of the collar.
        gain: f32,
        /// Number of rings over the collar.
        rings: f32,
    },
}

impl BaseProfile {
    /// Flare of a root axis of arc length `length` and base radius `radius`, or
    /// [`BaseProfile::Plain`] when the widening is below the tolerance.
    ///
    /// The ring count comes from the second derivative of the flare profile at the
    /// ground, `flare * radius * (8 ln 100 / length)^2`: `rings` equal steps over the
    /// flared eighth keep the chords within the tolerance.
    pub(crate) fn flare(sweep: &Sweep, length: f32, radius: f32) -> Self {
        let geometry = sweep.geometry;
        let lobes = if geometry.buttress_lobes > 0 {
            geometry.buttress_depth
        } else {
            0.0
        };
        let widening = geometry.flare * radius;
        if widening * (1.0 + lobes) < sweep.tolerance || length <= MIN_SPAN {
            return Self::Plain;
        }
        let rings = 100.0_f32.ln() * (widening / (8.0 * sweep.tolerance)).sqrt();
        Self::Flare {
            length,
            rings: rings.ceil().clamp(1.0, MAX_BASE_RINGS),
        }
    }

    /// Collar of a lateral axis of base radius `radius` on a parent of radius
    /// `parent_radius`, `sine` being the sine of the angle between the two axes; or
    /// [`BaseProfile::Plain`] when the widening is below the tolerance.
    ///
    /// The collar is never wider than the parent, so it cannot come out on the far side.
    /// Its profile `gain * radius * (1 - x)^2` has the constant second derivative
    /// `2 gain radius / length^2`, hence `sqrt(gain radius / (4 tolerance))` rings.
    pub(crate) fn collar(sweep: &Sweep, radius: f32, parent_radius: f32, sine: f32) -> Self {
        let geometry = sweep.geometry;
        let widest = (radius * (1.0 + geometry.collar_flare)).min(parent_radius.max(radius));
        let widening = widest - radius;
        if widening < sweep.tolerance {
            return Self::Plain;
        }
        let rings = (widening / (4.0 * sweep.tolerance)).sqrt();
        Self::Collar {
            start: parent_radius / sine.clamp(0.5, 1.0),
            length: geometry.collar_length * radius,
            gain: widening / radius,
            rings: rings.ceil().clamp(1.0, MAX_BASE_RINGS),
        }
    }

    /// Arc lengths where the widened part starts and ends, and its ring count.
    fn zone(&self) -> (f32, f32, f32) {
        match *self {
            Self::Plain => (f32::INFINITY, f32::INFINITY, 0.0),
            Self::Flare { length, rings } => (0.0, length * FLARE_ZONE, rings),
            Self::Collar {
                start,
                length,
                rings,
                ..
            } => (start, start + length, rings),
        }
    }

    /// Relative widening of the radius at `arc`, and the depth of the lobes as a
    /// fraction of that widening.
    fn widening(&self, geometry: &Geometry, arc: f32) -> (f32, f32) {
        match *self {
            Self::Plain => (0.0, 0.0),
            Self::Flare { length, .. } => {
                let height = arc / length;
                if height >= FLARE_ZONE {
                    return (0.0, 0.0);
                }
                let lobed = geometry.buttress_lobes > 0;
                let depth = if lobed { geometry.buttress_depth } else { 0.0 };
                (geometry.flare_factor(height) - 1.0, depth)
            }
            Self::Collar {
                start,
                length,
                gain,
                ..
            } => {
                let rest = 1.0 - ((arc - start) / length).clamp(0.0, 1.0);
                (gain * rest * rest, 0.0)
            }
        }
    }
}

/// One axis to sample: a polyline with radii and its base profile.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Axis<'a> {
    /// Start of the axis.
    pub origin: Vec3,
    /// Node positions after the origin, base to tip.
    pub positions: &'a [Vec3],
    /// Radius of the internode ending at each position.
    pub radii: &'a [f32],
    /// Widening near the base.
    pub profile: BaseProfile,
}

impl Axis<'_> {
    /// Point `index` of the polyline, 0 being the origin.
    fn point(&self, index: usize) -> Vec3 {
        if index == 0 {
            self.origin
        } else {
            self.positions[index - 1]
        }
    }

    /// Radius at point `index` of the polyline.
    fn radius(&self, index: usize) -> f32 {
        self.radii[index.max(1) - 1].max(MIN_RADIUS)
    }

    /// Unit direction from the origin to the first node that is not on it.
    pub(crate) fn first_direction(&self) -> Option<Vec3> {
        let offsets = self.positions.iter().map(|&point| point - self.origin);
        offsets
            .map(|offset| (offset, offset.length()))
            .find(|&(_, length)| length > MIN_SPAN)
            .map(|(offset, length)| offset / length)
    }

    /// Arc length of the polyline.
    pub(crate) fn length(&self) -> f32 {
        let mut previous = self.origin;
        let mut total = 0.0;
        for &position in self.positions {
            total += position.distance(previous);
            previous = position;
        }
        total
    }
}

/// A point of the centerline where a ring is placed.
#[derive(Debug, Clone, Copy)]
struct Station {
    point: Vec3,
    tangent: Vec3,
    arc: f32,
    radius: f32,
}

/// The last station that got a ring, and what was skipped since.
#[derive(Debug, Clone, Copy)]
struct Anchor {
    station: Station,
    /// Polyline index of the station.
    index: usize,
    /// Unit direction of the chord leaving the station.
    ray: Vec3,
    /// Largest distance from the ray of the nodes skipped so far.
    deviation: f32,
}

impl Anchor {
    /// Decides whether the node at `point` can go without a ring, `next` being the node
    /// after it, and records its deviation when it can.
    ///
    /// The node is at most `deviation(point) + deviation(next)` away from the straight
    /// span that would replace it, both measured from the ray of the anchor.
    fn absorbs(&mut self, point: Vec3, next: Vec3, radius: f32, tolerance: f32) -> bool {
        if (radius - self.station.radius).abs() > tolerance {
            return false;
        }
        let next_offset = next - self.station.point;
        let next_along = next_offset.dot(self.ray);
        let next_deviation = (next_offset - self.ray * next_along).length_squared();
        if next_deviation > tolerance * tolerance {
            return false;
        }
        let offset = point - self.station.point;
        let along = offset.dot(self.ray);
        let deviation = (offset - self.ray * along).length().max(self.deviation);
        let straight = next_along > along && along >= 0.0;
        let absorbed = straight && deviation + next_deviation.sqrt() <= tolerance;
        if absorbed {
            self.deviation = deviation;
        }
        absorbed
    }
}

/// Angle between two unit vectors, accurate for small angles. Below about 45 degrees it
/// is the first two terms of the arcsine series of the sine, within one percent.
fn angle(a: Vec3, b: Vec3) -> f32 {
    let sine_squared = a.cross(b).length_squared();
    if a.dot(b) > 0.7 {
        sine_squared.sqrt() * (1.0 + sine_squared / 6.0)
    } else {
        sine_squared.sqrt().atan2(a.dot(b))
    }
}

/// State of one walk along an axis.
struct Walker<'a, 'b> {
    axis: &'a Axis<'a>,
    sweep: &'a Sweep<'a>,
    out: &'b mut Vec<Ring>,
}

impl Walker<'_, '_> {
    /// Walks the polyline and emits the rings.
    fn run(&mut self) {
        let axis = self.axis;
        let count = axis.positions.len();
        let Some(start) = axis.first_direction() else {
            return;
        };
        let station = Station {
            point: axis.origin,
            tangent: start,
            arc: 0.0,
            radius: axis.radius(0),
        };
        self.push(&station);
        let mut anchor = Anchor {
            station,
            index: 0,
            ray: start,
            deviation: 0.0,
        };
        let (mut incoming, mut arc) = (start, 0.0);
        for index in 1..=count {
            let point = axis.point(index);
            let chord = point - axis.point(index - 1);
            let length = chord.length();
            arc += length;
            if length > MIN_SPAN {
                incoming = chord / length;
            }
            let radius = axis.radius(index);
            let next = (index < count).then(|| axis.point(index + 1));
            let skipped =
                next.is_some_and(|next| anchor.absorbs(point, next, radius, self.sweep.tolerance));
            if skipped || point.distance(anchor.station.point) <= MIN_SPAN {
                continue;
            }
            let outgoing = next.map_or(incoming, |next| (next - point).normalize_or(incoming));
            let station = Station {
                point,
                tangent: (incoming + outgoing).normalize_or(outgoing),
                arc,
                radius,
            };
            self.span(&anchor.station, &station, anchor.index + 1 == index);
            anchor = Anchor {
                station,
                index,
                ray: outgoing,
                deviation: 0.0,
            };
        }
    }

    /// Emits the rings after `a` up to and including `b`. A `curved` span is a Hermite
    /// segment between two consecutive nodes; otherwise it is straight.
    fn span(&mut self, a: &Station, b: &Station, curved: bool) {
        let chord = b.point - a.point;
        let chord_length = chord.length();
        let direction = chord / chord_length;
        let length = b.arc - a.arc;
        let turn = if curved {
            angle(a.tangent, direction) + angle(direction, b.tangent)
        } else {
            0.0
        };
        let bend = (length * turn / (8.0 * self.sweep.tolerance)).sqrt();
        let bend_rings = bend.ceil().clamp(1.0, MAX_SPAN_RINGS);
        let (zone_start, zone_end, zone_rings) = self.axis.profile.zone();
        let mut from = a.arc;
        for stop in [zone_start, zone_end, b.arc] {
            let to = if stop >= b.arc - MIN_SPAN {
                b.arc
            } else {
                stop
            };
            if to - from <= MIN_SPAN {
                continue;
            }
            let share = to - from;
            let middle = from + 0.5 * share;
            let mut rings = (bend_rings * share / length).ceil();
            if middle > zone_start && middle < zone_end {
                rings = rings.max((zone_rings * share / (zone_end - zone_start)).ceil());
            }
            let steps = rings.max(1.0) as u32;
            for step in 1..=steps {
                if step == steps && to >= b.arc {
                    break;
                }
                let arc = from + share * step as f32 / steps as f32;
                let station = between(a, b, (arc - a.arc) / length, curved);
                self.push(&station);
            }
            from = to;
        }
        self.push(b);
    }

    /// Appends the ring of a station, widened by the base profile.
    fn push(&mut self, station: &Station) {
        let geometry = self.sweep.geometry;
        let (gain, lobe_depth) = self.axis.profile.widening(geometry, station.arc);
        let radius = station.radius * (1.0 + gain);
        let lobe = station.radius * gain * lobe_depth;
        let lobe = if lobe >= self.sweep.tolerance {
            lobe
        } else {
            0.0
        };
        self.out.push(Ring {
            center: station.point,
            tangent: station.tangent,
            arc: station.arc,
            radius,
            lobe,
            sides: self.sweep.sides(radius, lobe),
        });
    }
}

/// Station at the parameter `u` in `(0, 1)` between `a` and `b`: on the Hermite segment
/// whose end tangents are those of the stations scaled by the chord length when
/// `curved`, on the chord otherwise. Arc length and radius are interpolated linearly.
fn between(a: &Station, b: &Station, u: f32, curved: bool) -> Station {
    let chord = b.point - a.point;
    let (point, tangent) = if curved {
        let scale = chord.length();
        let (u2, u3) = (u * u, u * u * u);
        let point = a.point * (2.0 * u3 - 3.0 * u2 + 1.0)
            + a.tangent * (scale * (u3 - 2.0 * u2 + u))
            + b.point * (3.0 * u2 - 2.0 * u3)
            + b.tangent * (scale * (u3 - u2));
        let velocity = chord * (6.0 * (u - u2))
            + a.tangent * (scale * (3.0 * u2 - 4.0 * u + 1.0))
            + b.tangent * (scale * (3.0 * u2 - 2.0 * u));
        (point, velocity.normalize_or(chord.normalize_or(a.tangent)))
    } else {
        (a.point.lerp(b.point, u), chord.normalize_or(a.tangent))
    };
    Station {
        point,
        tangent,
        arc: a.arc + (b.arc - a.arc) * u,
        radius: a.radius + (b.radius - a.radius) * u,
    }
}

/// Appends the rings of `axis` to `out`. Nothing is appended when the axis has no
/// extent (fewer than two rings).
pub(crate) fn sample_axis(axis: &Axis, sweep: &Sweep, out: &mut Vec<Ring>) {
    let first = out.len();
    Walker { axis, sweep, out }.run();
    if out.len() - first < 2 {
        out.truncate(first);
    }
}

#[cfg(test)]
mod tests {
    use super::{Axis, BaseProfile, Ring, Sweep, sample_axis};
    use arbrume_core::species::Geometry;
    use glam::Vec3;

    fn rings(origin: Vec3, positions: &[Vec3], radii: &[f32], profile: BaseProfile) -> Vec<Ring> {
        let geometry = Geometry::default();
        let sweep = Sweep {
            geometry: &geometry,
            tolerance: 0.002,
        };
        let axis = Axis {
            origin,
            positions,
            radii,
            profile,
        };
        let mut out = Vec::new();
        sample_axis(&axis, &sweep, &mut out);
        out
    }

    #[test]
    fn straight_even_axis_needs_two_rings() {
        let positions: Vec<Vec3> = (1..=20).map(|i| Vec3::Y * i as f32 * 0.1).collect();
        let out = rings(Vec3::ZERO, &positions, &[0.01; 20], BaseProfile::Plain);
        assert_eq!(out.len(), 2);
        assert!((out[1].arc - 2.0).abs() < 1e-5);
        assert_eq!(out[1].center, positions[19]);
    }

    #[test]
    fn tapering_axis_keeps_rings_where_the_radius_changes() {
        let positions: Vec<Vec3> = (1..=20).map(|i| Vec3::Y * i as f32 * 0.1).collect();
        let radii: Vec<f32> = (0..20).map(|i| 0.1 - i as f32 * 0.004).collect();
        let out = rings(Vec3::ZERO, &positions, &radii, BaseProfile::Plain);
        assert!(out.len() > 10 && out.len() <= 21, "{}", out.len());
        assert!(out.windows(2).all(|pair| pair[1].radius <= pair[0].radius));
    }

    #[test]
    fn bends_get_more_rings_and_pass_through_the_nodes() {
        let positions = [
            Vec3::new(0.0, 0.5, 0.0),
            Vec3::new(0.3, 0.9, 0.0),
            Vec3::new(0.8, 1.0, 0.0),
        ];
        let out = rings(Vec3::ZERO, &positions, &[0.02; 3], BaseProfile::Plain);
        assert!(out.len() > 4, "{}", out.len());
        for node in positions {
            assert!(out.iter().any(|ring| ring.center.distance(node) < 1e-6));
        }
        assert!(out.windows(2).all(|pair| pair[1].arc > pair[0].arc));
        assert!(
            out.iter()
                .all(|ring| (ring.tangent.length() - 1.0).abs() < 1e-5)
        );
    }

    #[test]
    fn smoothed_curve_stays_near_the_polyline() {
        let positions = [
            Vec3::new(0.0, 0.05, 0.0),
            Vec3::new(1.0, 0.1, 0.0),
            Vec3::new(1.05, 0.1, 0.5),
        ];
        let out = rings(Vec3::ZERO, &positions, &[0.01; 3], BaseProfile::Plain);
        let points = [Vec3::ZERO, positions[0], positions[1], positions[2]];
        for ring in &out {
            let nearest = points
                .windows(2)
                .map(|pair| {
                    let chord = pair[1] - pair[0];
                    let u = ((ring.center - pair[0]).dot(chord) / chord.length_squared())
                        .clamp(0.0, 1.0);
                    ring.center.distance(pair[0] + chord * u)
                })
                .fold(f32::INFINITY, f32::min);
            assert!(nearest < 0.15, "{nearest}");
        }
    }

    #[test]
    fn coincident_nodes_and_empty_axes_are_harmless() {
        let same = [Vec3::ZERO, Vec3::ZERO];
        assert_eq!(rings(Vec3::ZERO, &same, &[0.01; 2], BaseProfile::Plain), []);
        let positions = [Vec3::ZERO, Vec3::X, Vec3::X, Vec3::X];
        let out = rings(Vec3::ZERO, &positions, &[0.01; 4], BaseProfile::Plain);
        assert_eq!(out.len(), 2);
        assert!(out.iter().all(|ring| ring.center.is_finite()));
    }

    #[test]
    fn collar_widens_the_base_only() {
        let geometry = Geometry::default();
        let sweep = Sweep {
            geometry: &geometry,
            tolerance: 0.0005,
        };
        let profile = BaseProfile::collar(&sweep, 0.02, 0.1, 1.0);
        let BaseProfile::Collar { start, length, .. } = profile else {
            panic!("expected a collar, got {profile:?}");
        };
        let positions = [Vec3::X * 0.5, Vec3::X];
        let out = rings(Vec3::ZERO, &positions, &[0.02; 2], profile);
        let widest = 0.02 * (1.0 + geometry.collar_flare);
        assert!((out[0].radius - widest).abs() < 1e-6);
        let in_collar = out
            .iter()
            .filter(|ring| ring.arc > start - 1e-5 && ring.arc < start + length + 1e-5);
        assert!(in_collar.count() >= 3);
        let after = out.iter().filter(|ring| ring.arc >= start + length);
        assert!(
            after
                .into_iter()
                .all(|ring| (ring.radius - 0.02).abs() < 1e-6)
        );
        assert!(out.windows(2).all(|pair| pair[1].radius <= pair[0].radius));
    }

    #[test]
    fn collar_is_never_wider_than_the_parent() {
        let geometry = Geometry::default();
        let sweep = Sweep {
            geometry: &geometry,
            tolerance: 0.0005,
        };
        let profile = BaseProfile::collar(&sweep, 0.09, 0.1, 1.0);
        let BaseProfile::Collar { gain, .. } = profile else {
            panic!("expected a collar, got {profile:?}");
        };
        assert!((0.09 * (1.0 + gain) - 0.1).abs() < 1e-6);
        assert_eq!(
            BaseProfile::collar(&sweep, 0.1, 0.1, 1.0),
            BaseProfile::Plain
        );
        assert_eq!(
            BaseProfile::collar(&sweep, 0.001, 0.1, 1.0),
            BaseProfile::Plain
        );
    }

    #[test]
    fn flare_follows_the_species_profile() {
        let geometry = Geometry::default();
        let sweep = Sweep {
            geometry: &geometry,
            tolerance: 0.002,
        };
        let profile = BaseProfile::flare(&sweep, 8.0, 0.2);
        let positions: Vec<Vec3> = (1..=16).map(|i| Vec3::Y * i as f32 * 0.5).collect();
        let out = rings(Vec3::ZERO, &positions, &[0.2; 16], profile);
        assert!((out[0].radius - 0.2 * geometry.flare_factor(0.0)).abs() < 1e-6);
        let flared = out.iter().filter(|ring| ring.arc < 1.0).count();
        assert!(flared >= 4, "{flared}");
        for ring in &out {
            let expected = 0.2 * geometry.flare_factor(ring.arc / 8.0);
            assert!((ring.radius - expected).abs() < 1e-5);
        }
    }

    #[test]
    fn sides_follow_radius_within_species_bounds() {
        let geometry = Geometry::default();
        let sweep = Sweep {
            geometry: &geometry,
            tolerance: 0.002,
        };
        let [fewest, most] = geometry.radial_segments;
        assert_eq!(sweep.sides(0.0005, 0.0), fewest);
        assert_eq!(sweep.sides(5.0, 0.0), most);
        let counts: Vec<u32> = (1..200)
            .map(|i| sweep.sides(i as f32 * 0.002, 0.0))
            .collect();
        assert!(counts.windows(2).all(|pair| pair[0] <= pair[1]));
        let coarse = Sweep {
            geometry: &geometry,
            tolerance: 0.008,
        };
        assert!(coarse.sides(0.05, 0.0) < sweep.sides(0.05, 0.0));
    }
}
