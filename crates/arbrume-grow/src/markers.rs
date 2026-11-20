//! Markers of free space: the space colonization of Palubicki et al. (2009, section
//! 4.1), after Runions et al. (2007).
//!
//! The space a crown may fill is a set of marker points. Each cycle:
//!
//! 1. every marker inside the *occupancy zone* of a bud (a sphere of radius `rho`) is
//!    removed;
//! 2. every other marker inside the *perception volume* of one or more buds (a cone of
//!    full angle `theta` and length `r` about the direction of the bud) is given to the
//!    closest of them;
//! 3. a bud with markers has space (`Q = 1`) and prefers the direction
//!    `V = n / |n|`, `n` being the sum of the unit vectors from the bud to its markers
//!    (Runions et al. 2007, equation 2). A bud without markers has `Q = 0`.
//!
//! # Determinism
//!
//! Step 2 buckets the markers in a [`HashGrid`] and visits, bud by bud, only the cells
//! within reach of the bud, skipping those outside its occupancy zone and perception
//! cone. When the buds are not fewer than the markers it buckets the buds instead and
//! visits them marker by marker, the cheaper way round. Either way each marker keeps
//! the smallest pair (squared distance, bud) of the buds that see it: a minimum over
//! the same pairs and the same distances, which depends neither on the way round nor on
//! the order of the visits, so this step may run on the `rayon` pool. The sums of step
//! 3 are then made on one thread in marker order, so the floating-point result does not
//! depend on the number of threads. Ties between buds at the same distance go to the
//! bud given first.

use crate::surroundings::Shape;
use arbrume_core::grid::HashGrid;
use arbrume_core::rng::{Rng, Stream};
use glam::Vec3;
use rayon::prelude::*;
use std::sync::atomic::{AtomicU64, Ordering};

/// Marker given to no bud.
const FREE: u32 = u32::MAX;
/// Marker inside an occupancy zone.
const TAKEN: u32 = u32::MAX - 1;
/// Buds per task of the parallel association, bud by bud.
const BUD_CHUNK: usize = 16;
/// Markers per task of the parallel association, marker by marker.
const MARKER_CHUNK: usize = 512;
/// Claim of a marker inside an occupancy zone; it beats every claim of a seeing bud.
const CLAIM_TAKEN: u64 = 0;
/// Claim of a marker no bud sees.
const CLAIM_FREE: u64 = u64::MAX;

/// Sizes of the zones around a bud, in meters and radians.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Perception {
    /// Radius `rho` of the occupancy zone.
    pub occupancy: f32,
    /// Length `r` of the perception cone.
    pub reach: f32,
    /// Full opening angle `theta` of the perception cone, in radians.
    pub angle: f32,
}

/// The tests of the zones of a bud, with their constants.
#[derive(Debug, Clone, Copy)]
struct Zones {
    occupancy: f32,
    occupancy_sq: f32,
    reach: f32,
    /// Radius of the queries: the larger of `occupancy` and `reach`.
    radius: f32,
    /// Cosine and sine of the half-angle of the cone.
    cos_limit: f32,
    sin_limit: f32,
    /// Whether the cone is at least a half-space, which leaves no cell to skip by angle.
    wide: bool,
}

impl Zones {
    fn new(perception: &Perception) -> Self {
        let half = 0.5 * perception.angle;
        Self {
            occupancy: perception.occupancy,
            occupancy_sq: perception.occupancy * perception.occupancy,
            reach: perception.reach,
            radius: perception.reach.max(perception.occupancy),
            cos_limit: half.cos(),
            sin_limit: half.sin(),
            wide: half >= std::f32::consts::FRAC_PI_2,
        }
    }

    /// Claim of the bud `bud` at `position`, growing towards `direction`, on the marker
    /// at `marker`, `distance_sq` away from it.
    ///
    /// The claim is [`CLAIM_TAKEN`] for a marker in the occupancy zone, the squared
    /// distance (high bits) and bud index (low bits) for a marker in the perception
    /// cone, and [`CLAIM_FREE`] otherwise: the smallest claim on a marker names its
    /// owner, closest bud first.
    fn claim(&self, bud: u32, (position, direction): Bud, marker: Vec3, distance_sq: f32) -> u64 {
        if distance_sq <= self.occupancy_sq {
            return CLAIM_TAKEN;
        }
        let distance = distance_sq.sqrt();
        let along = (marker - position).dot(direction);
        if distance <= self.reach && along >= self.cos_limit * distance {
            (u64::from(distance_sq.to_bits()) << 32) | u64::from(bud)
        } else {
            CLAIM_FREE
        }
    }

    /// Whether a cell of edge `size` centered on `center` may hold a marker in a zone of
    /// the bud at `position` growing towards `direction`.
    ///
    /// The cell is bounded by the ball of radius `size` about its center, wider than the
    /// cell, which leaves room for rounding: a cell rejected here holds no marker that
    /// [`Self::claim`] would accept. The ball meets the cone when the angle between the
    /// axis and the center is at most the half-angle plus `asin(size / distance)`.
    fn may_reach(&self, (position, direction): Bud, center: Vec3, size: f32) -> bool {
        let offset = center - position;
        let distance = offset.length();
        if distance <= size + self.occupancy || self.wide {
            return distance <= size + self.radius;
        }
        if distance > size + self.reach {
            return false;
        }
        let sin_extra = size / distance;
        let cos_extra = (1.0 - sin_extra * sin_extra).sqrt();
        let cos_widest = self.cos_limit * cos_extra - self.sin_limit * sin_extra;
        offset.dot(direction) >= cos_widest * distance
    }
}

/// Position and unit direction of a bud.
type Bud = (Vec3, Vec3);

/// The markers left and the working memory of their association to buds.
#[derive(Debug, Clone, Default)]
pub struct Markers {
    points: Vec<Vec3>,
    grid: HashGrid,
    owner: Vec<u32>,
    sums: Vec<Vec3>,
    buds: Vec<Bud>,
}

impl Markers {
    /// Markers at the given points.
    #[must_use]
    pub fn from_points(points: Vec<Vec3>) -> Self {
        Self {
            points,
            ..Self::default()
        }
    }

    /// Markers scattered uniformly in `envelope`: about `density` per cubic meter, and
    /// at most `limit`.
    ///
    /// Candidate `i` is drawn in the bounding box of the envelope from `(seed, i)` and
    /// kept when it is inside, so the markers depend on the seed and the envelope only.
    #[must_use]
    pub fn scatter(envelope: &Shape, seed: u64, density: f32, limit: u32) -> Self {
        let bounds = envelope.bounds();
        let size = bounds.max - bounds.min;
        let volume = f64::from(size.x) * f64::from(size.y) * f64::from(size.z);
        let candidates = (volume * f64::from(density)).min(f64::from(limit)) as u32;
        let points = (0..candidates)
            .filter_map(|index| {
                let mut rng = Rng::new(seed, index, Stream::ENVIRONMENT);
                let unit = Vec3::new(rng.f32(), rng.f32(), rng.f32());
                let point = bounds.min + unit * size;
                envelope.contains(point).then_some(point)
            })
            .collect();
        Self::from_points(points)
    }

    /// The markers left.
    #[must_use]
    pub fn points(&self) -> &[Vec3] {
        &self.points
    }

    /// Owner of a marker from its smallest claim: a bud index, [`FREE`] or [`TAKEN`].
    const fn owner_of(claim: u64) -> u32 {
        match claim {
            CLAIM_TAKEN => TAKEN,
            CLAIM_FREE => FREE,
            _ => claim as u32,
        }
    }

    /// Calls `claim(marker, key)` for every marker of `grid` the bud `bud` claims.
    fn claims_of(
        grid: &HashGrid,
        zones: &Zones,
        bud: u32,
        zone: Bud,
        mut claim: impl FnMut(usize, u64),
    ) {
        let keep = |center, size| zones.may_reach(zone, center, size);
        grid.for_each_in_kept_cells(zone.0, zones.radius, keep, |marker, point, distance_sq| {
            let key = zones.claim(bud, zone, point, distance_sq);
            if key != CLAIM_FREE {
                claim(marker as usize, key);
            }
        });
    }

    /// Fills `owner` with the owner of each of the `count` markers of `grid`, visiting
    /// the cells around each bud of `buds`.
    fn owners_from_buds(
        grid: &HashGrid,
        zones: &Zones,
        buds: &[Bud],
        count: usize,
        parallel: bool,
        owner: &mut Vec<u32>,
    ) {
        if parallel {
            let claims: Vec<AtomicU64> = (0..count).map(|_| AtomicU64::new(CLAIM_FREE)).collect();
            buds.par_iter()
                .enumerate()
                .with_min_len(BUD_CHUNK)
                .for_each(|(bud, &zone)| {
                    Self::claims_of(grid, zones, bud as u32, zone, |marker, key| {
                        claims[marker].fetch_min(key, Ordering::Relaxed);
                    });
                });
            owner.extend(
                claims
                    .into_iter()
                    .map(|claim| Self::owner_of(claim.into_inner())),
            );
        } else {
            let mut claims = vec![CLAIM_FREE; count];
            for (bud, &zone) in buds.iter().enumerate() {
                Self::claims_of(grid, zones, bud as u32, zone, |marker, key| {
                    claims[marker] = claims[marker].min(key);
                });
            }
            owner.extend(claims.into_iter().map(Self::owner_of));
        }
    }

    /// Fills `owner` with the owner of each marker of `markers`, visiting the buds of
    /// `grid` around it.
    fn owners_from_markers(
        grid: &HashGrid,
        zones: &Zones,
        buds: &[Bud],
        markers: &[Vec3],
        parallel: bool,
        owner: &mut Vec<u32>,
    ) {
        let owner_of = |marker: &Vec3| {
            let mut best = CLAIM_FREE;
            grid.for_each_in_radius(*marker, zones.radius, |bud, _, distance_sq| {
                let claim = zones.claim(bud, buds[bud as usize], *marker, distance_sq);
                best = best.min(claim);
            });
            Self::owner_of(best)
        };
        owner.resize(markers.len(), FREE);
        if parallel {
            owner
                .par_chunks_mut(MARKER_CHUNK)
                .zip(markers.par_chunks(MARKER_CHUNK))
                .for_each(|(owners, markers)| {
                    for (slot, marker) in owners.iter_mut().zip(markers) {
                        *slot = owner_of(marker);
                    }
                });
        } else {
            for (slot, marker) in owner.iter_mut().zip(markers) {
                *slot = owner_of(marker);
            }
        }
    }

    /// Fills `owner` with the owner of every marker for the buds at `positions`,
    /// bucketing the larger of the two sets.
    fn associate(
        &mut self,
        positions: &[Vec3],
        directions: &[Vec3],
        perception: &Perception,
        parallel: bool,
    ) {
        let zones = Zones::new(perception);
        self.buds.clear();
        self.buds
            .extend(positions.iter().copied().zip(directions.iter().copied()));
        self.owner.clear();
        let (buds, owner) = (&self.buds, &mut self.owner);
        if self.points.len() > positions.len() {
            self.grid.rebuild(&self.points, zones.radius);
            let count = self.points.len();
            Self::owners_from_buds(&self.grid, &zones, buds, count, parallel, owner);
        } else {
            self.grid.rebuild(positions, zones.radius);
            Self::owners_from_markers(&self.grid, &zones, buds, &self.points, parallel, owner);
        }
    }

    /// Runs one cycle for the buds at `positions` growing towards the unit vectors
    /// `directions`: removes the markers they occupy and returns, for each bud, the
    /// direction `V` to its markers, or zero for a bud that has none.
    ///
    /// `parallel` runs the association on the current `rayon` pool; the result is the
    /// same either way.
    pub fn perceive(
        &mut self,
        positions: &[Vec3],
        directions: &[Vec3],
        perception: &Perception,
        parallel: bool,
    ) -> &[Vec3] {
        self.sums.clear();
        self.sums.resize(positions.len(), Vec3::ZERO);
        if positions.is_empty() || self.points.is_empty() {
            return &self.sums;
        }
        self.associate(positions, directions, perception, parallel);
        for (marker, &bud) in self.points.iter().zip(&self.owner) {
            if bud < TAKEN {
                let towards = *marker - positions[bud as usize];
                self.sums[bud as usize] += towards.normalize_or_zero();
            }
        }
        let mut owners = self.owner.iter();
        self.points
            .retain(|_| owners.next().is_some_and(|&owner| owner != TAKEN));
        for sum in &mut self.sums {
            *sum = sum.normalize_or_zero();
        }
        &self.sums
    }
}

#[cfg(test)]
mod tests {
    use super::{CLAIM_FREE, Markers, Perception, TAKEN, Zones};
    use crate::surroundings::Shape;
    use arbrume_core::rng::{Rng, Stream};
    use glam::Vec3;

    const PERCEPTION: Perception = Perception {
        occupancy: 1.0,
        reach: 3.0,
        angle: std::f32::consts::FRAC_PI_2,
    };

    #[test]
    fn markers_in_the_occupancy_zone_are_removed() {
        let points = vec![
            Vec3::new(0.0, 0.5, 0.0),
            Vec3::new(0.0, 0.99, 0.0),
            Vec3::new(0.0, 1.5, 0.0),
            Vec3::new(0.9, 0.0, 0.0),
            Vec3::new(5.0, 5.0, 5.0),
        ];
        let mut markers = Markers::from_points(points);
        let seen = markers.perceive(&[Vec3::ZERO], &[Vec3::Y], &PERCEPTION, false)[0];
        assert!((seen - Vec3::Y).length() < 1e-6);
        assert_eq!(
            markers.points(),
            &[Vec3::new(0.0, 1.5, 0.0), Vec3::new(5.0, 5.0, 5.0)]
        );
    }

    #[test]
    fn only_markers_inside_the_cone_are_perceived() {
        let inside = Vec3::new(1.0, 2.0, 0.0);
        let outside_angle = Vec3::new(2.0, 1.0, 0.0);
        let behind = Vec3::new(0.0, -2.0, 0.0);
        let too_far = Vec3::new(0.0, 3.5, 0.0);
        let points = vec![inside, outside_angle, behind, too_far];
        let mut markers = Markers::from_points(points);
        let seen = markers.perceive(&[Vec3::ZERO], &[Vec3::Y], &PERCEPTION, false)[0];
        assert!((seen - inside.normalize()).length() < 1e-6);
        assert_eq!(markers.points().len(), 4);
        let mut blind = Markers::from_points(vec![outside_angle, behind, too_far]);
        let none = blind.perceive(&[Vec3::ZERO], &[Vec3::Y], &PERCEPTION, false)[0];
        assert_eq!(none, Vec3::ZERO);
    }

    #[test]
    fn a_marker_seen_by_two_buds_goes_to_the_closest() {
        let marker = Vec3::new(0.4, 2.0, 0.0);
        let buds = [Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0)];
        let mut markers = Markers::from_points(vec![marker]);
        let seen = markers
            .perceive(&buds, &[Vec3::Y, Vec3::Y], &PERCEPTION, false)
            .to_vec();
        assert!((seen[0] - marker.normalize()).length() < 1e-6);
        assert_eq!(seen[1], Vec3::ZERO);
        let middle = Vec3::new(0.5, 2.0, 0.0);
        let mut tie = Markers::from_points(vec![middle]);
        let seen = tie.perceive(&buds, &[Vec3::Y, Vec3::Y], &PERCEPTION, false);
        assert!(seen[0] != Vec3::ZERO && seen[1] == Vec3::ZERO);
    }

    #[test]
    fn the_direction_is_the_normalized_sum_of_unit_vectors() {
        let points = vec![Vec3::new(0.0, 2.0, 0.0), Vec3::new(1.5, 1.5, 0.0)];
        let expected = (Vec3::Y + Vec3::new(1.0, 1.0, 0.0).normalize()).normalize();
        let mut markers = Markers::from_points(points);
        let seen = markers.perceive(&[Vec3::ZERO], &[Vec3::Y], &PERCEPTION, false)[0];
        assert!((seen - expected).length() < 1e-6);
    }

    #[test]
    fn scattering_fills_the_envelope_and_threads_do_not_change_the_result() {
        let envelope = Shape::Ellipsoid {
            center: Vec3::new(0.0, 4.0, 0.0),
            radii: Vec3::new(3.0, 4.0, 3.0),
        };
        let scattered = Markers::scatter(&envelope, 9, 20.0, 100_000);
        let count = scattered.points().len();
        assert!(count > 2000 && count < 3200, "{count}");
        assert!(scattered.points().iter().all(|&p| envelope.contains(p)));
        assert!(Markers::scatter(&envelope, 9, 20.0, 500).points().len() <= 500);
        let buds: Vec<Vec3> = (0..60)
            .map(|i| {
                Vec3::new(
                    (i as f32).sin() * 2.0,
                    0.1 * i as f32,
                    (i as f32).cos() * 2.0,
                )
            })
            .collect();
        let directions = vec![Vec3::Y; buds.len()];
        let (mut one, mut many) = (scattered.clone(), scattered);
        let sequential = one
            .perceive(&buds, &directions, &PERCEPTION, false)
            .to_vec();
        let parallel = many
            .perceive(&buds, &directions, &PERCEPTION, true)
            .to_vec();
        assert_eq!(sequential, parallel);
        assert_eq!(one.points(), many.points());
        assert!(one.points().len() < count);
    }

    /// Markers left and directions of a search that tests every bud against every marker.
    fn brute_force(
        markers: &[Vec3],
        buds: &[(Vec3, Vec3)],
        perception: &Perception,
    ) -> (Vec<Vec3>, Vec<Vec3>) {
        let zones = Zones::new(perception);
        let mut sums = vec![Vec3::ZERO; buds.len()];
        let mut left = Vec::new();
        for &marker in markers {
            let claim = (0..buds.len())
                .map(|bud| {
                    zones.claim(
                        bud as u32,
                        buds[bud],
                        marker,
                        marker.distance_squared(buds[bud].0),
                    )
                })
                .fold(CLAIM_FREE, u64::min);
            let owner = Markers::owner_of(claim);
            if owner < TAKEN {
                sums[owner as usize] += (marker - buds[owner as usize].0).normalize_or_zero();
            }
            if owner != TAKEN {
                left.push(marker);
            }
        }
        (
            left,
            sums.iter().map(|sum| sum.normalize_or_zero()).collect(),
        )
    }

    #[test]
    fn both_ways_round_match_a_search_of_every_pair() {
        let mut rng = Rng::new(5, 0, Stream::ENVIRONMENT);
        let mut cloud = |count: usize, extent: f32| -> Vec<Vec3> {
            (0..count)
                .map(|_| Vec3::new(rng.signed(), rng.signed(), rng.signed()) * extent)
                .collect()
        };
        let markers = cloud(3000, 6.0);
        let (few, many) = (cloud(80, 5.0), cloud(4000, 5.0));
        let mut rng = Rng::new(6, 0, Stream::ENVIRONMENT);
        for positions in [few, many] {
            let directions: Vec<Vec3> = positions.iter().map(|_| rng.unit_sphere()).collect();
            let buds: Vec<(Vec3, Vec3)> = positions
                .iter()
                .copied()
                .zip(directions.iter().copied())
                .collect();
            for angle in [0.6_f32, std::f32::consts::FRAC_PI_2, 4.0] {
                let perception = Perception {
                    angle,
                    ..PERCEPTION
                };
                let (left, expected) = brute_force(&markers, &buds, &perception);
                for parallel in [false, true] {
                    let mut grid = Markers::from_points(markers.clone());
                    let seen = grid.perceive(&positions, &directions, &perception, parallel);
                    assert_eq!(
                        seen,
                        expected.as_slice(),
                        "{} buds, angle {angle}",
                        buds.len()
                    );
                    assert_eq!(grid.points(), left.as_slice());
                }
            }
        }
    }
}
