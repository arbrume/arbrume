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
//! Step 2 looks buds up in a read-only [`HashGrid`], marker by marker, and may run on
//! the `rayon` pool: it only records the bud of each marker. The sums of step 3 are
//! then made on one thread in marker order, so the floating-point result does not
//! depend on the number of threads. Ties between buds at the same distance go to the
//! bud given first.

use crate::surroundings::Shape;
use arbrume_core::grid::HashGrid;
use arbrume_core::rng::{Rng, Stream};
use glam::Vec3;
use rayon::prelude::*;

/// Marker given to no bud.
const FREE: u32 = u32::MAX;
/// Marker inside an occupancy zone.
const TAKEN: u32 = u32::MAX - 1;
/// Markers per task of the parallel association.
const CHUNK: usize = 512;

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

/// The markers left and the working memory of their association to buds.
#[derive(Debug, Clone, Default)]
pub struct Markers {
    points: Vec<Vec3>,
    grid: HashGrid,
    owner: Vec<u32>,
    sums: Vec<Vec3>,
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

    /// Owner of `marker` among the buds of the grid: a bud index, [`FREE`] or [`TAKEN`].
    fn owner_of(
        grid: &HashGrid,
        marker: Vec3,
        directions: &[Vec3],
        perception: &Perception,
    ) -> u32 {
        let occupancy_sq = perception.occupancy * perception.occupancy;
        let cos_limit = (0.5 * perception.angle).cos();
        let reach = perception.reach.max(perception.occupancy);
        let mut best: Option<(u32, f32)> = None;
        let mut taken = false;
        grid.for_each_in_radius(marker, reach, |bud, position, distance_sq| {
            if distance_sq <= occupancy_sq {
                taken = true;
                return;
            }
            let distance = distance_sq.sqrt();
            let along = (marker - position).dot(directions[bud as usize]);
            let seen = distance <= perception.reach && along >= cos_limit * distance;
            let closer = best.is_none_or(|(best_bud, best_sq)| {
                distance_sq < best_sq || (distance_sq <= best_sq && bud < best_bud)
            });
            if seen && closer {
                best = Some((bud, distance_sq));
            }
        });
        if taken {
            TAKEN
        } else {
            best.map_or(FREE, |(bud, _)| bud)
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
        let cell = perception.reach.max(perception.occupancy);
        self.grid.rebuild(positions, cell);
        self.owner.clear();
        self.owner.resize(self.points.len(), FREE);
        let grid = &self.grid;
        let owner = |marker: &Vec3| Self::owner_of(grid, *marker, directions, perception);
        if parallel {
            self.owner
                .par_chunks_mut(CHUNK)
                .zip(self.points.par_chunks(CHUNK))
                .for_each(|(owners, markers)| {
                    for (slot, marker) in owners.iter_mut().zip(markers) {
                        *slot = owner(marker);
                    }
                });
        } else {
            for (slot, marker) in self.owner.iter_mut().zip(&self.points) {
                *slot = owner(marker);
            }
        }
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
    use super::{Markers, Perception};
    use crate::surroundings::Shape;
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
}
