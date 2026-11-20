//! Uniform hash grid over a set of points, for neighbourhood queries.
//!
//! The growth cycle asks, for every bud, which markers or other buds lie within a sphere
//! (occupancy zone) or a cone (perception volume, Palubicki et al. 2009). The grid
//! answers both without scanning all points.
//!
//! # Structure
//!
//! Space is cut into cubic cells of edge `cell_size`. Each point is assigned to its cell,
//! the cell coordinates are hashed into a table of buckets (a power of two at least as
//! large as the point count), and the points are stored sorted by bucket: one counting
//! sort, three flat arrays, no allocation per cell. The grid is unbounded: no world
//! extent is needed, and memory is proportional to the number of points.
//!
//! The grid is a snapshot. Rebuild it with [`HashGrid::rebuild`], which reuses the
//! allocations, whenever the points change. Queries take `&self` and can run in parallel.
//!
//! # Choosing the cell size
//!
//! A query visits every cell overlapping the bounding box of its sphere, so a cell edge
//! close to the usual query radius is a good start: a query with
//! `radius <= cell_size` visits at most 27 cells. When a query would visit more cells
//! than there are points, it scans the points directly instead.
//!
//! Indices passed to query callbacks are positions in the slice given to
//! [`HashGrid::build`]. The order in which a query reports points is deterministic for a
//! given point slice and cell size.

use crate::rng::mix64;
use glam::{IVec3, Vec3};

/// Uniform hash grid over points. See the [module documentation](self).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct HashGrid {
    inverse_cell: f32,
    mask: u32,
    bucket_start: Vec<u32>,
    cells: Vec<IVec3>,
    points: Vec<Vec3>,
    indices: Vec<u32>,
}

impl HashGrid {
    /// Builds a grid over `points` with cubic cells of edge `cell_size` (positive).
    #[must_use]
    pub fn build(points: &[Vec3], cell_size: f32) -> Self {
        let mut grid = Self::default();
        grid.rebuild(points, cell_size);
        grid
    }

    /// Replaces the content of the grid, reusing its allocations.
    pub fn rebuild(&mut self, points: &[Vec3], cell_size: f32) {
        let count = points.len();
        let buckets = count.next_power_of_two().max(1);
        self.inverse_cell = cell_size.recip();
        self.mask = (buckets - 1) as u32;
        self.bucket_start.clear();
        self.bucket_start.resize(buckets + 1, 0);
        for &point in points {
            let bucket = self.bucket(self.cell_of(point));
            self.bucket_start[bucket + 1] += 1;
        }
        for bucket in 0..buckets {
            self.bucket_start[bucket + 1] += self.bucket_start[bucket];
        }
        self.cells.clear();
        self.cells.resize(count, IVec3::ZERO);
        self.points.clear();
        self.points.resize(count, Vec3::ZERO);
        self.indices.clear();
        self.indices.resize(count, 0);
        let mut cursor = self.bucket_start.clone();
        for (index, &point) in points.iter().enumerate() {
            let cell = self.cell_of(point);
            let bucket = self.bucket(cell);
            let slot = cursor[bucket] as usize;
            self.cells[slot] = cell;
            self.points[slot] = point;
            self.indices[slot] = index as u32;
            cursor[bucket] += 1;
        }
    }

    /// Number of points in the grid.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.points.len()
    }

    /// `true` when the grid holds no point.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    #[inline]
    fn cell_of(&self, point: Vec3) -> IVec3 {
        (point * self.inverse_cell).floor().as_ivec3()
    }

    /// Bucket of a cell: the three coordinates packed 21 bits each, then `mix64`.
    #[inline]
    fn bucket(&self, cell: IVec3) -> usize {
        let pack = |v: i32| u64::from(v.cast_unsigned()) & 0x1F_FFFF;
        let key = pack(cell.x) | (pack(cell.y) << 21) | (pack(cell.z) << 42);
        (mix64(key) as u32 & self.mask) as usize
    }

    /// Calls `visit(index, point, squared_distance)` for every point within `radius` of
    /// `center` (boundary included).
    pub fn for_each_in_radius(&self, center: Vec3, radius: f32, visit: impl FnMut(u32, Vec3, f32)) {
        self.for_each_in_kept_cells(center, radius, |_, _| true, visit);
    }

    /// Calls `visit(index, point, squared_distance)` for the points within `radius` of
    /// `center` (boundary included), skipping the cells for which `keep(cell_center,
    /// cell_size)` is `false`.
    ///
    /// `keep` only prunes work: when the query scans the points directly (see the
    /// [module documentation](self)), points of skipped cells may still be visited.
    pub fn for_each_in_kept_cells(
        &self,
        center: Vec3,
        radius: f32,
        mut keep: impl FnMut(Vec3, f32) -> bool,
        mut visit: impl FnMut(u32, Vec3, f32),
    ) {
        let radius_sq = radius * radius;
        let mut test = |slot: usize| {
            let distance_sq = self.points[slot].distance_squared(center);
            if distance_sq <= radius_sq {
                visit(self.indices[slot], self.points[slot], distance_sq);
            }
        };
        let low = self.cell_of(center - Vec3::splat(radius));
        let high = self.cell_of(center + Vec3::splat(radius));
        let span = |lo: i32, hi: i32| u64::from(hi.wrapping_sub(lo).cast_unsigned()) + 1;
        let cell_count = span(low.x, high.x)
            .saturating_mul(span(low.y, high.y))
            .saturating_mul(span(low.z, high.z));
        if cell_count > self.points.len() as u64 {
            (0..self.points.len()).for_each(test);
            return;
        }
        let size = self.inverse_cell.recip();
        let cells = (low.z..=high.z).flat_map(|z| {
            (low.y..=high.y).flat_map(move |y| (low.x..=high.x).map(move |x| IVec3::new(x, y, z)))
        });
        for cell in cells.filter(|cell| keep((cell.as_vec3() + 0.5) * size, size)) {
            let bucket = self.bucket(cell);
            let slots = self.bucket_start[bucket] as usize..self.bucket_start[bucket + 1] as usize;
            slots
                .filter(|&slot| self.cells[slot] == cell)
                .for_each(&mut test);
        }
    }

    /// Calls `visit(index, point, squared_distance)` for every point inside the cone of
    /// apex `apex`, unit axis `axis`, half-angle `half_angle` radians and length `range`:
    /// points at a distance in `(0, range]` from the apex whose direction from the apex
    /// makes an angle of at most `half_angle` with the axis. A point exactly at the apex
    /// is not reported.
    pub fn for_each_in_cone(
        &self,
        apex: Vec3,
        axis: Vec3,
        half_angle: f32,
        range: f32,
        mut visit: impl FnMut(u32, Vec3, f32),
    ) {
        let cos_limit = half_angle.cos();
        self.for_each_in_radius(apex, range, |index, point, distance_sq| {
            let along = (point - apex).dot(axis);
            if distance_sq > 0.0 && along >= cos_limit * distance_sq.sqrt() {
                visit(index, point, distance_sq);
            }
        });
    }

    /// Nearest point within `radius` of `center` as `(index, squared_distance)`. Ties go
    /// to the smallest index.
    #[must_use]
    pub fn nearest_in_radius(&self, center: Vec3, radius: f32) -> Option<(u32, f32)> {
        let mut best: Option<(u32, f32)> = None;
        self.for_each_in_radius(center, radius, |index, _, distance_sq| {
            let closer = best.is_none_or(|(best_index, best_sq)| {
                distance_sq < best_sq || (distance_sq <= best_sq && index < best_index)
            });
            if closer {
                best = Some((index, distance_sq));
            }
        });
        best
    }
}

#[cfg(test)]
mod tests {
    use super::HashGrid;
    use crate::rng::{Rng, Stream};
    use glam::Vec3;

    fn cloud(count: u32, extent: f32) -> Vec<Vec3> {
        let mut rng = Rng::new(7, 0, Stream::ENVIRONMENT);
        (0..count)
            .map(|_| Vec3::new(rng.signed(), rng.signed(), rng.signed()) * extent)
            .collect()
    }

    fn radius_hits(grid: &HashGrid, center: Vec3, radius: f32) -> Vec<u32> {
        let mut hits = Vec::new();
        grid.for_each_in_radius(center, radius, |index, _, _| hits.push(index));
        hits.sort_unstable();
        hits
    }

    #[test]
    fn radius_query_matches_brute_force() {
        let points = cloud(2000, 10.0);
        let grid = HashGrid::build(&points, 1.5);
        assert_eq!(grid.len(), 2000);
        let mut rng = Rng::new(8, 0, Stream::ENVIRONMENT);
        for radius in [0.0_f32, 0.4, 1.5, 3.7, 40.0] {
            for _ in 0..25 {
                let center = Vec3::new(rng.signed(), rng.signed(), rng.signed()) * 11.0;
                let expected: Vec<u32> = (0..2000_u32)
                    .filter(|&i| points[i as usize].distance_squared(center) <= radius * radius)
                    .collect();
                assert_eq!(radius_hits(&grid, center, radius), expected);
            }
        }
    }

    #[test]
    fn cone_query_matches_brute_force() {
        let points = cloud(1500, 6.0);
        let grid = HashGrid::build(&points, 2.0);
        let mut rng = Rng::new(9, 0, Stream::ENVIRONMENT);
        for _ in 0..40 {
            let apex = points[rng.below(1500) as usize];
            let axis = rng.unit_sphere();
            let (half_angle, range) = (rng.range(0.2, 1.5), rng.range(0.5, 5.0));
            let mut hits = Vec::new();
            grid.for_each_in_cone(apex, axis, half_angle, range, |index, _, _| {
                hits.push(index);
            });
            hits.sort_unstable();
            let expected: Vec<u32> = (0..1500_u32)
                .filter(|&i| {
                    let offset = points[i as usize] - apex;
                    let distance = offset.length();
                    distance > 0.0
                        && distance * distance <= range * range
                        && offset.dot(axis) >= half_angle.cos() * distance
                })
                .collect();
            assert_eq!(hits, expected);
        }
    }

    #[test]
    fn nearest_point_is_found() {
        let points = cloud(500, 4.0);
        let grid = HashGrid::build(&points, 1.0);
        let center = Vec3::new(0.3, -0.2, 0.9);
        let brute = (0..500_u32)
            .map(|i| (i, points[i as usize].distance_squared(center)))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .unwrap();
        assert_eq!(
            grid.nearest_in_radius(center, 8.0).map(|hit| hit.0),
            Some(brute.0)
        );
        assert_eq!(grid.nearest_in_radius(Vec3::splat(100.0), 1.0), None);
    }

    #[test]
    fn empty_and_rebuilt_grids_work() {
        let mut grid = HashGrid::build(&[], 1.0);
        assert!(grid.is_empty());
        assert_eq!(radius_hits(&grid, Vec3::ZERO, 5.0), [0_u32; 0]);
        grid.rebuild(&[Vec3::ZERO, Vec3::X, Vec3::X * 10.0], 0.5);
        assert_eq!(radius_hits(&grid, Vec3::ZERO, 1.0), [0, 1]);
        grid.rebuild(&[Vec3::Y], 0.5);
        assert_eq!(radius_hits(&grid, Vec3::ZERO, 1.0), [0]);
    }

    #[test]
    fn duplicate_points_are_all_reported() {
        let points = vec![Vec3::splat(-3.2); 5];
        let grid = HashGrid::build(&points, 0.7);
        assert_eq!(radius_hits(&grid, Vec3::splat(-3.2), 0.0), [0, 1, 2, 3, 4]);
    }
}
