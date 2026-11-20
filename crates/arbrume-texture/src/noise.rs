//! Tileable noise: hashing, gradient noise and its fractal sums, and cellular (Worley)
//! noise.
//!
//! Every function takes a point of a domain and a `period` of whole lattice cells per
//! axis, and wraps lattice coordinates modulo that period, so the result is periodic:
//! evaluated at `uv * period` for `uv` in `[0, 1)^2` it tiles the unit square. A fractal
//! sum doubles both the frequency and the period from one octave to the next, which
//! keeps it periodic. Composing periodic functions (domain warping) keeps periodicity.
//!
//! Everything is integer hashing (`mix64`) and float arithmetic without transcendental
//! functions, so values are the same on every platform.

use arbrume_core::rng::mix64;
use glam::Vec2;

/// Hash of a lattice point and a seed.
pub(crate) fn hash(seed: u64, x: i32, y: i32) -> u64 {
    let key = (u64::from(x as u32) << 32) | u64::from(y as u32);
    mix64(seed ^ mix64(key.wrapping_add(0x9E37_79B9_7F4A_7C15)))
}

/// A uniform number in `[0, 1)` from the top bits of `hash`.
pub(crate) fn unit(hash: u64) -> f32 {
    (hash >> 40) as f32 / (1u64 << 24) as f32
}

/// A uniform number in `[0, 1)` for the integer pair `(x, y)`; `salt` picks
/// independent values.
pub(crate) fn random(seed: u64, x: u32, y: u32, salt: u64) -> f32 {
    let seed = seed ^ salt.wrapping_mul(0xD6E8_FEB8_6659_FD93);
    unit(hash(seed, x.cast_signed(), y.cast_signed()))
}

/// Gradient directions, 16 unit vectors evenly spaced.
const GRADIENTS: [[f32; 2]; 16] = [
    [1.0, 0.0],
    [0.923_879_5, 0.382_683_43],
    [0.707_106_77, 0.707_106_77],
    [0.382_683_43, 0.923_879_5],
    [0.0, 1.0],
    [-0.382_683_43, 0.923_879_5],
    [-0.707_106_77, 0.707_106_77],
    [-0.923_879_5, 0.382_683_43],
    [-1.0, 0.0],
    [-0.923_879_5, -0.382_683_43],
    [-0.707_106_77, -0.707_106_77],
    [-0.382_683_43, -0.923_879_5],
    [0.0, -1.0],
    [0.382_683_43, -0.923_879_5],
    [0.707_106_77, -0.707_106_77],
    [0.923_879_5, -0.382_683_43],
];

/// Quintic fade of Perlin's improved noise.
fn fade(t: f32) -> f32 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

/// Lattice coordinate `i` wrapped into `[0, period)`.
fn wrap(i: i32, period: i32) -> i32 {
    i.rem_euclid(period.max(1))
}

/// Gradient noise at `p`, periodic with `period` cells per axis, about in `[-1, 1]`.
pub(crate) fn gradient(p: Vec2, period: [i32; 2], seed: u64) -> f32 {
    let cell = p.floor();
    let f = p - cell;
    let (ix, iy) = (cell.x as i32, cell.y as i32);
    let corner = |dx: i32, dy: i32| {
        let h = hash(seed, wrap(ix + dx, period[0]), wrap(iy + dy, period[1]));
        let g = GRADIENTS[(h >> 60) as usize];
        g[0] * (f.x - dx as f32) + g[1] * (f.y - dy as f32)
    };
    let (u, v) = (fade(f.x), fade(f.y));
    let bottom = corner(0, 0) + (corner(1, 0) - corner(0, 0)) * u;
    let top = corner(0, 1) + (corner(1, 1) - corner(0, 1)) * u;
    1.4 * (bottom + (top - bottom) * v)
}

/// Gradient noise of `uv` in the unit square with `frequency` cells per axis: tiles the
/// unit square.
pub(crate) fn tiled(uv: Vec2, frequency: [i32; 2], seed: u64) -> f32 {
    let scale = Vec2::new(frequency[0] as f32, frequency[1] as f32);
    gradient(uv * scale, frequency, seed)
}

/// Fractal sum of `octaves` octaves of [`tiled`], each of twice the frequency and half
/// the amplitude of the previous one, normalized to about `[-1, 1]`.
pub(crate) fn fbm(uv: Vec2, frequency: [i32; 2], octaves: u32, seed: u64) -> f32 {
    let (mut sum, mut amplitude, mut total) = (0.0, 1.0, 0.0);
    let mut frequency = frequency;
    for octave in 0..octaves {
        sum += amplitude * tiled(uv, frequency, seed.wrapping_add(u64::from(octave)));
        total += amplitude;
        amplitude *= 0.5;
        frequency = [frequency[0] * 2, frequency[1] * 2];
    }
    sum / total.max(1e-6)
}

/// The nearest feature points of [`worley`] around a point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Cell {
    /// Distance to the nearest feature point.
    pub f1: f32,
    /// Distance to the border between the nearest cell and its closest neighbour.
    pub border: f32,
    /// Hash of the (wrapped) lattice cell of the nearest feature point.
    pub id: u64,
    /// Offset from the point to the nearest feature point, in the scaled metric.
    pub offset: Vec2,
}

/// Cellular noise: one feature point per lattice cell, jittered by up to `jitter` of a
/// cell around its centre, periodic with `period` cells per axis. Distances use the
/// metric scaled by `metric` per axis (an anisotropic metric stretches the cells).
///
/// The search covers the 3 x 3 cells around the point, so with a large jitter or a
/// strongly anisotropic metric a farther point can be missed and the border distance
/// overestimated near corners of cells; the patterns tolerate that, and it keeps the
/// cost at nine hashes.
pub(crate) fn worley(p: Vec2, period: [i32; 2], jitter: f32, metric: Vec2, seed: u64) -> Cell {
    let cell = p.floor();
    let (ix, iy) = (cell.x as i32, cell.y as i32);
    let mut points = [(Vec2::ZERO, 0_u64); 9];
    let mut count = 0;
    for dy in -1..=1 {
        for dx in -1..=1 {
            let id = hash(seed, wrap(ix + dx, period[0]), wrap(iy + dy, period[1]));
            let jx = unit(id) - 0.5;
            let jy = unit(mix64(id)) - 0.5;
            let feature = Vec2::new(
                (ix + dx) as f32 + 0.5 + jitter * jx,
                (iy + dy) as f32 + 0.5 + jitter * jy,
            );
            points[count] = ((feature - p) * metric, id);
            count += 1;
        }
    }
    let nearest = points
        .iter()
        .copied()
        .min_by(|a, b| a.0.length_squared().total_cmp(&b.0.length_squared()))
        .unwrap_or_default();
    let border = points
        .iter()
        .filter(|(offset, _)| (*offset - nearest.0).length_squared() > 1e-10)
        .map(|&(offset, _)| {
            let normal = (offset - nearest.0).normalize();
            (0.5 * (offset + nearest.0)).dot(normal)
        })
        .fold(f32::MAX, f32::min);
    Cell {
        f1: nearest.0.length(),
        border,
        id: nearest.1,
        offset: nearest.0,
    }
}

/// Hermite smoothstep of `x` between `edge0` and `edge1`.
pub(crate) fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::{fbm, gradient, worley};
    use glam::Vec2;

    #[test]
    fn gradient_noise_is_periodic_and_bounded() {
        for i in 0..200 {
            let p = Vec2::new(i as f32 * 0.37, i as f32 * 0.61);
            let a = gradient(p, [5, 3], 9);
            let b = gradient(p + Vec2::new(5.0, 3.0), [5, 3], 9);
            assert!((a - b).abs() < 1e-5);
            assert!(a.abs() <= 1.0 + 1e-4, "{a}");
        }
        let uv = Vec2::new(0.25, 0.75);
        assert!((fbm(uv, [2, 3], 4, 1) - fbm(uv + Vec2::ONE, [2, 3], 4, 1)).abs() < 1e-4);
    }

    #[test]
    fn worley_is_periodic_and_borders_are_small_between_cells() {
        let metric = Vec2::new(1.0, 2.0);
        for i in 0..100 {
            let p = Vec2::new(i as f32 * 0.113, i as f32 * 0.271);
            let a = worley(p, [4, 6], 0.9, metric, 3);
            let b = worley(p + Vec2::new(4.0, -6.0), [4, 6], 0.9, metric, 3);
            assert_eq!(a.id, b.id);
            assert!((a.f1 - b.f1).abs() < 1e-4 && (a.border - b.border).abs() < 1e-4);
            assert!(a.border >= -1e-5);
        }
    }
}
