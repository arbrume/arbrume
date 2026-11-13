//! Sky occlusion from a coarse density grid of the plant.
//!
//! A plant crown is a participating medium more than a solid: what darkens its inner
//! leaves is the amount of foliage between them and the sky. The grid treats it that way.
//!
//! 1. Every triangle adds its surface area (both faces for double-sided parts) to the
//!    cell holding its centroid. For randomly oriented surfaces the mean projected area is
//!    a quarter of the surface area (Cauchy's formula), so the extinction coefficient of a
//!    cell is its surface area divided by four times its volume.
//! 2. For each of nine sky directions whose step is a whole number of cells, (0, 1, 0),
//!    (±1, 1, 0), (0, 1, ±1) and (±1, 1, ±1), the optical depth from every cell centre to
//!    the top or sides of the grid is accumulated in one sweep from the top layer down
//!    (each cell adds its neighbour's depth along the direction), and the transmittance is
//!    `exp(-depth)` (Beer-Lambert).
//! 3. The sky visibility of a cell is the mean transmittance over the directions,
//!    weighted by the sine of their elevation (a cosine-weighted sky).
//!
//! Shading points read the visibility by trilinear interpolation between cell centres;
//! points outside the grid see the whole sky. The grid extends past the plant
//! horizontally so that the ground around the trunk receives the crown's occlusion. Cost
//! is one pass over the triangles plus a few operations per cell and direction, whatever
//! the image size. Triangles are not split across cells, so very large triangles
//! concentrate their area in one cell; plant meshes are made of small ones.

use crate::camera::Bounds;
use crate::scene::MeshView;
use glam::{IVec3, Vec3};

/// Grid steps of the sky directions, each one cell up.
const DIRECTIONS: [IVec3; 9] = [
    IVec3::new(0, 1, 0),
    IVec3::new(1, 1, 0),
    IVec3::new(-1, 1, 0),
    IVec3::new(0, 1, 1),
    IVec3::new(0, 1, -1),
    IVec3::new(1, 1, 1),
    IVec3::new(-1, 1, 1),
    IVec3::new(1, 1, -1),
    IVec3::new(-1, 1, -1),
];

/// Sky visibility of a plant sampled on a regular grid.
#[derive(Debug, Clone, Default)]
pub struct OcclusionGrid {
    origin: Vec3,
    cell: Vec3,
    dims: [usize; 3],
    extinction: Vec<f32>,
    depth: Vec<f32>,
    visibility: Vec<f32>,
}

impl OcclusionGrid {
    /// An empty grid: every point sees the whole sky.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Fills the grid for `mesh`, with `cells` cells along the largest side of `bounds`
    /// (cells are as close to cubes as the box allows). 0 cells empties the grid.
    pub(crate) fn build(&mut self, mesh: &MeshView<'_>, bounds: &Bounds, cells: u32) {
        self.dims = [0; 3];
        if cells == 0 {
            return;
        }
        let size = bounds.size();
        let margin = Vec3::new(0.25 * size.x.max(size.z), 0.0, 0.25 * size.x.max(size.z));
        let lo = bounds.min - margin;
        let extent = (bounds.max + margin - lo).max(Vec3::splat(1e-4));
        let edge = extent.max_element() / cells as f32;
        let dims = (extent / edge).ceil().max(Vec3::ONE);
        self.dims = [dims.x as usize, dims.y as usize, dims.z as usize];
        self.origin = lo;
        self.cell = extent / dims;
        let count = self.dims.iter().product();
        self.extinction.clear();
        self.extinction.resize(count, 0.0);
        self.splat(mesh);
        self.sweep();
    }

    fn index(&self, x: usize, y: usize, z: usize) -> usize {
        (y * self.dims[2] + z) * self.dims[0] + x
    }

    fn splat(&mut self, mesh: &MeshView<'_>) {
        let volume = self.cell.x * self.cell.y * self.cell.z;
        for part in &mesh.parts {
            let faces = if part.double_sided { 2.0 } else { 1.0 };
            for tri in part.indices.as_chunks::<3>().0 {
                let Some(corners) = triangle(part.positions, *tri) else {
                    continue;
                };
                let [p0, p1, p2] = corners;
                let area = 0.5 * (p1 - p0).cross(p2 - p0).length() * faces;
                let cell = ((p0 + p1 + p2) / 3.0 - self.origin) / self.cell;
                if !(cell.cmpge(Vec3::ZERO).all() && area.is_finite()) {
                    continue;
                }
                let (ix, iy, iz) = (cell.x as usize, cell.y as usize, cell.z as usize);
                if ix < self.dims[0] && iy < self.dims[1] && iz < self.dims[2] {
                    let i = self.index(ix, iy, iz);
                    self.extinction[i] += 0.25 * area / volume;
                }
            }
        }
    }

    /// Accumulates the optical depth along every direction and the weighted visibility.
    fn sweep(&mut self) {
        let [nx, ny, nz] = self.dims;
        let count = nx * ny * nz;
        self.visibility.clear();
        self.visibility.resize(count, 0.0);
        self.depth.resize(count, 0.0);
        let mut total_weight = 0.0;
        for step in DIRECTIONS {
            let offset = step.as_vec3() * self.cell;
            let length = offset.length();
            let weight = offset.y / length;
            total_weight += weight;
            for y in (0..ny).rev() {
                for z in 0..nz {
                    self.sweep_row(step, length, weight, (y, z));
                }
            }
        }
        for v in &mut self.visibility {
            *v /= total_weight;
        }
    }

    /// One row of [`OcclusionGrid::sweep`] for the cells `(.., y, z)`.
    fn sweep_row(&mut self, step: IVec3, length: f32, weight: f32, (y, z): (usize, usize)) {
        let [nx, ny, nz] = self.dims;
        for x in 0..nx {
            let next_x = x.checked_add_signed(step.x as isize).filter(|&v| v < nx);
            let next_z = z.checked_add_signed(step.z as isize).filter(|&v| v < nz);
            let beyond = match (next_x, next_z) {
                (Some(ahead_x), Some(ahead_z)) if y + 1 < ny => {
                    let n = self.index(ahead_x, y + 1, ahead_z);
                    self.depth[n] + self.extinction[n] * length
                }
                _ => 0.0,
            };
            let i = self.index(x, y, z);
            self.depth[i] = beyond;
            let from_centre = beyond + 0.5 * self.extinction[i] * length;
            self.visibility[i] += weight * (-from_centre).exp();
        }
    }

    /// Sky visibility in `[0, 1]` at a world point; 1 outside the grid.
    #[must_use]
    pub fn visibility(&self, point: Vec3) -> f32 {
        if self.dims[0] == 0 {
            return 1.0;
        }
        let local = (point - self.origin) / self.cell;
        let dims = Vec3::new(
            self.dims[0] as f32,
            self.dims[1] as f32,
            self.dims[2] as f32,
        );
        if !(local.cmpge(Vec3::ZERO).all() && local.cmplt(dims).all()) {
            return 1.0;
        }
        let u = (local - 0.5).clamp(Vec3::ZERO, dims - 1.0);
        let base = u.floor();
        let t = u - base;
        let (x0, y0, z0) = (base.x as usize, base.y as usize, base.z as usize);
        let x1 = (x0 + 1).min(self.dims[0] - 1);
        let y1 = (y0 + 1).min(self.dims[1] - 1);
        let z1 = (z0 + 1).min(self.dims[2] - 1);
        let at = |x, y, z| self.visibility[self.index(x, y, z)];
        let lerp = |a: f32, b: f32, s: f32| a + (b - a) * s;
        let face = |y| {
            lerp(
                lerp(at(x0, y, z0), at(x1, y, z0), t.x),
                lerp(at(x0, y, z1), at(x1, y, z1), t.x),
                t.z,
            )
        };
        lerp(face(y0), face(y1), t.y)
    }
}

/// The corners of a triangle, `None` when an index is out of range.
fn triangle(positions: &[[f32; 3]], tri: [u32; 3]) -> Option<[Vec3; 3]> {
    let fetch = |i: u32| positions.get(i as usize).map(|&p| Vec3::from(p));
    Some([fetch(tri[0])?, fetch(tri[1])?, fetch(tri[2])?])
}
