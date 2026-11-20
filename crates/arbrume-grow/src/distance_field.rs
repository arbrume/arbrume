//! Distance field of a voxel mask: the Euclidean distance from the centre of every voxel
//! to the centre of the nearest filled one, read between centres by trilinear
//! interpolation.
//!
//! The field is the exact squared distance transform of Felzenszwalb and Huttenlocher
//! (2012), one pass of lower parabola envelopes along each axis, and is stored as one
//! `u16` per voxel in steps of `1 / STEPS` voxel edge. Outside the grid the distance is
//! that of the nearest point of the grid of centres plus the distance to it, so it
//! keeps falling as a point comes closer to the mask from any side.

use glam::Vec3;
use std::fmt;

/// Steps of the stored distance per voxel edge.
const STEPS: f32 = 32.0;

/// Distances of a voxel grid to its filled voxels. See the [module documentation](self).
#[derive(Clone)]
pub struct DistanceField {
    size: [usize; 3],
    values: Vec<u16>,
}

impl fmt::Debug for DistanceField {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DistanceField")
            .field("size", &self.size)
            .finish_non_exhaustive()
    }
}

/// Lower envelope of the parabolas `f[v] + (q - v)^2` of one line, written to `out`,
/// with `vertices` and `bounds` as scratch of at least `f.len()` and `f.len() + 1`.
fn transform_line(f: &[f32], out: &mut [f32], vertices: &mut [usize], bounds: &mut [f32]) {
    let parabola = |q: usize| f[q] + (q * q) as f32;
    let mut k = 0;
    vertices[0] = 0;
    bounds[0] = f32::NEG_INFINITY;
    bounds[1] = f32::INFINITY;
    for q in 1..f.len() {
        let crossing = loop {
            let v = vertices[k];
            let s = (parabola(q) - parabola(v)) / (2 * (q - v)) as f32;
            if s > bounds[k] || k == 0 {
                break s;
            }
            k -= 1;
        };
        k += 1;
        vertices[k] = q;
        bounds[k] = crossing;
        bounds[k + 1] = f32::INFINITY;
    }
    k = 0;
    for (q, value) in out.iter_mut().enumerate() {
        while bounds[k + 1] < q as f32 {
            k += 1;
        }
        let offset = q.abs_diff(vertices[k]);
        *value = (offset * offset) as f32 + f[vertices[k]];
    }
}

/// Runs [`transform_line`] on every line of `grid` along `axis`.
fn transform_axis(grid: &mut [f32], size: [usize; 3], axis: usize) {
    let strides = [1, size[0], size[0] * size[1]];
    let (length, stride) = (size[axis], strides[axis]);
    let mut line = vec![0.0; length];
    let mut out = vec![0.0; length];
    let mut vertices = vec![0; length];
    let mut bounds = vec![0.0; length + 1];
    let starts = (0..grid.len()).filter(|start| (start / stride) % length == 0);
    for start in starts {
        for (i, value) in line.iter_mut().enumerate() {
            *value = grid[start + i * stride];
        }
        transform_line(&line, &mut out, &mut vertices, &mut bounds);
        for (i, value) in out.iter().enumerate() {
            grid[start + i * stride] = *value;
        }
    }
}

impl DistanceField {
    /// The field of a grid of `size` voxels, rows along X, then Y, then Z, whose voxel
    /// of index `i` is filled when `filled(i)` holds. `None` when no voxel is filled.
    pub fn new(size: [u32; 3], filled: impl Fn(usize) -> bool) -> Option<Self> {
        let size = size.map(|n| n as usize);
        let count = size.iter().product::<usize>();
        let far = size.iter().map(|n| (n * n) as f32).sum::<f32>() + 1.0;
        let mut grid: Vec<f32> = (0..count)
            .map(|i| if filled(i) { 0.0 } else { far })
            .collect();
        if !grid.contains(&0.0) {
            return None;
        }
        for axis in 0..3 {
            transform_axis(&mut grid, size, axis);
        }
        let store = |squared: &f32| (squared.sqrt() * STEPS).round().min(f32::from(u16::MAX));
        let values = grid.iter().map(|squared| store(squared) as u16).collect();
        Some(Self { size, values })
    }

    /// Stored distance of the voxel `[x, y, z]`, in voxel edges.
    fn at(&self, [x, y, z]: [usize; 3]) -> f32 {
        let [nx, ny, _] = self.size;
        f32::from(self.values[(z * ny + y) * nx + x]) / STEPS
    }

    /// Distance, in voxel edges, of the point at `local` voxel edges from the corner of
    /// the grid to the centre of the nearest filled voxel.
    pub fn sample(&self, local: Vec3) -> f32 {
        let last = Vec3::new(
            (self.size[0] - 1) as f32,
            (self.size[1] - 1) as f32,
            (self.size[2] - 1) as f32,
        );
        let centred = local - 0.5;
        let clamped = centred.clamp(Vec3::ZERO, last);
        let low = clamped.floor().min(last);
        let fraction = clamped - low;
        let base = [low.x as usize, low.y as usize, low.z as usize];
        let mut sum = 0.0;
        for corner in 0..8_usize {
            let step = [corner & 1, (corner >> 1) & 1, (corner >> 2) & 1];
            let mut weight = 1.0;
            let mut voxel = base;
            for axis in 0..3 {
                let up = step[axis] == 1 && base[axis] + 1 < self.size[axis];
                weight *= if step[axis] == 1 {
                    fraction[axis]
                } else {
                    1.0 - fraction[axis]
                };
                voxel[axis] += usize::from(up);
            }
            sum += weight * self.at(voxel);
        }
        sum + centred.distance(clamped)
    }
}

#[cfg(test)]
mod tests {
    use super::DistanceField;
    use glam::Vec3;

    #[test]
    fn the_field_is_the_distance_to_the_nearest_filled_centre() {
        let size = [9, 7, 5];
        let filled = [[1, 1, 1], [7, 5, 3]];
        let index = |[x, y, z]: [usize; 3]| (z * 7 + y) * 9 + x;
        let is_filled = |i: usize| filled.iter().any(|&v| index(v) == i);
        let field = DistanceField::new(size, is_filled).unwrap();
        for z in 0..5 {
            for y in 0..7 {
                for x in 0..9 {
                    let point = Vec3::new(x as f32, y as f32, z as f32);
                    let expected = filled
                        .iter()
                        .map(|v| point.distance(Vec3::new(v[0] as f32, v[1] as f32, v[2] as f32)))
                        .fold(f32::INFINITY, f32::min);
                    let read = field.sample(point + 0.5);
                    assert!(
                        (read - expected).abs() < 0.02,
                        "{x} {y} {z}: {read} {expected}"
                    );
                }
            }
        }
        assert!(DistanceField::new(size, |_| false).is_none());
    }

    #[test]
    fn the_distance_falls_towards_the_mask_from_outside_the_grid() {
        let field = DistanceField::new([4, 4, 4], |i| i == 0).unwrap();
        let mut last = f32::INFINITY;
        for step in 0..39 {
            let point = Vec3::new(0.5, 12.0 - 0.3 * step as f32, 0.5);
            let distance = field.sample(point);
            assert!(distance < last, "{point}: {distance} {last}");
            last = distance;
        }
        assert!(last < 0.15, "{last}");
    }
}
