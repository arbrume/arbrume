//! Counter-based random numbers.
//!
//! Every random decision in Arbrume is a pure function of
//! `(seed, node id, stream, counter)`. There is no shared sequential stream, so the order
//! in which nodes are processed, and the number of threads processing them, cannot change
//! the result.
//!
//! # Usage
//!
//! [`Rng::new`] builds a small cursor for one `(seed, node, stream)` key; each draw
//! advances its counter. Building a cursor costs two hash rounds and a draw costs one, so
//! cursors are meant to be created where they are used and dropped.
//!
//! ```
//! use arbrume_core::rng::{Rng, Stream};
//!
//! let mut rng = Rng::new(42, 7, Stream::BRANCH_ANGLE);
//! let angle = rng.range(30.0, 60.0);
//! assert!((30.0..60.0).contains(&angle));
//! assert_eq!(Rng::new(42, 7, Stream::BRANCH_ANGLE).range(30.0, 60.0).to_bits(), angle.to_bits());
//! ```
//!
//! Each kind of decision uses its own [`Stream`] so that adding a draw to one decision
//! does not shift the numbers of another. The node id is the id of the graph node (or
//! bud, or any other stable integer) the decision belongs to.
//!
//! # Mixing function
//!
//! The generator is the `SplitMix64` construction of Steele, Lea and Flood (2014, "Fast
//! splittable pseudorandom number generators"): draw `i` of a key is
//! `mix64(key + (i + 1) * gamma)`, where `mix64` is the 64-bit finalizer known as
//! Stafford's variant 13 of the `MurmurHash3` finalizer (two xor-shift-multiply rounds and
//! a final xor-shift). The 64-bit `key` is two `mix64` rounds over the seed, the node id
//! and the stream. The odd increment `gamma` is derived from the key with the
//! `MurmurHash3` finalizer and repaired when it has too few bit transitions, as in the
//! original paper, so two keys walk the 64-bit state space with different strides.
//!
//! # Reproducibility
//!
//! Integer draws are identical on every platform. Floating-point helpers that call
//! `sin`, `cos`, `ln` or `sqrt` use the platform math library, whose last bit can differ
//! between targets.

use glam::Vec3;

/// Weyl increment of `SplitMix64` (the golden ratio scaled to 64 bits).
const GOLDEN_GAMMA: u64 = 0x9E37_79B9_7F4A_7C15;

/// Identifies one kind of random decision.
///
/// Draws made with different streams for the same seed and node are independent. The
/// associated constants cover the decisions of the growth engine and the mesher; other
/// code takes its own id from [`Stream::custom`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Stream(pub u32);

impl Stream {
    /// Whether a bud flushes, rests or dies.
    pub const BUD_FATE: Self = Self(1);
    /// Number and length of the internodes of a shoot.
    pub const SHOOT_LENGTH: Self = Self(2);
    /// Perturbation of the direction of a growing shoot.
    pub const SHOOT_DIRECTION: Self = Self(3);
    /// Insertion angle of a lateral axis.
    pub const BRANCH_ANGLE: Self = Self(4);
    /// Azimuth of lateral organs around their axis.
    pub const PHYLLOTAXIS: Self = Self(5);
    /// Shedding decisions.
    pub const SHEDDING: Self = Self(6);
    /// Environment sampling (markers, light).
    pub const ENVIRONMENT: Self = Self(7);
    /// Radius variation.
    pub const RADIUS: Self = Self(8);
    /// Leaf placement, orientation and size.
    pub const FOLIAGE: Self = Self(9);
    /// Bark and branch surface variation.
    pub const BARK: Self = Self(10);
    /// Mesh-level variation that is neither foliage nor bark.
    pub const MESH: Self = Self(11);

    /// First id of the range reserved for callers outside this list.
    pub const CUSTOM_BASE: u32 = 0x1_0000;

    /// Stream `index` of the custom range, which never collides with the named streams.
    #[must_use]
    pub const fn custom(index: u16) -> Self {
        Self(Self::CUSTOM_BASE + index as u32)
    }
}

/// Stafford variant 13 of the `MurmurHash3` 64-bit finalizer, the output function of
/// `SplitMix64`. A bijection on `u64` with full avalanche.
#[inline]
#[must_use]
pub const fn mix64(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Derives the odd per-key increment from a hashed key.
#[inline]
const fn mix_gamma(mut z: u64) -> u64 {
    z = (z ^ (z >> 33)).wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    z = (z ^ (z >> 33)).wrapping_mul(0xC4CE_B9FE_1A85_EC53);
    z = (z ^ (z >> 33)) | 1;
    if (z ^ (z >> 1)).count_ones() < 24 {
        z ^= 0xAAAA_AAAA_AAAA_AAAA;
    }
    z
}

/// Cursor over the random numbers of one `(seed, node, stream)` key.
///
/// The cursor is 24 bytes and `Copy`; see the [module documentation](self) for the
/// construction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rng {
    key: u64,
    gamma: u64,
    counter: u64,
}

impl Rng {
    /// Cursor at counter 0 for the given key.
    #[inline]
    #[must_use]
    pub const fn new(seed: u64, node: u32, stream: Stream) -> Self {
        let id = ((node as u64) << 32) | stream.0 as u64;
        let key = mix64(mix64(seed.wrapping_add(GOLDEN_GAMMA)) ^ id);
        Self {
            key,
            gamma: mix_gamma(key.wrapping_add(GOLDEN_GAMMA)),
            counter: 0,
        }
    }

    /// Cursor positioned at `counter`: its next draw is draw number `counter` of the key.
    #[inline]
    #[must_use]
    pub const fn at(seed: u64, node: u32, stream: Stream, counter: u64) -> Self {
        let mut rng = Self::new(seed, node, stream);
        rng.counter = counter;
        rng
    }

    /// Number of draws made so far (the index of the next draw).
    #[inline]
    #[must_use]
    pub const fn counter(&self) -> u64 {
        self.counter
    }

    /// Next 64 uniform bits.
    #[inline]
    pub const fn next_u64(&mut self) -> u64 {
        self.counter = self.counter.wrapping_add(1);
        mix64(self.key.wrapping_add(self.counter.wrapping_mul(self.gamma)))
    }

    /// Next 32 uniform bits (the high half of a 64-bit draw).
    #[inline]
    pub const fn next_u32(&mut self) -> u32 {
        (self.next_u64() >> 32) as u32
    }

    /// Uniform `f32` in `[0, 1)` with 24 random bits.
    #[inline]
    pub fn f32(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 * (1.0 / 16_777_216.0)
    }

    /// Uniform `f32` in `[-1, 1)`.
    #[inline]
    pub fn signed(&mut self) -> f32 {
        self.f32().mul_add(2.0, -1.0)
    }

    /// Uniform `f32` in `[lo, hi)`. With `hi < lo` the interval is `(hi, lo]`.
    #[inline]
    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        self.f32().mul_add(hi - lo, lo)
    }

    /// Uniform angle in `[0, 2 pi)` radians.
    #[inline]
    pub fn angle(&mut self) -> f32 {
        self.f32() * std::f32::consts::TAU
    }

    /// Uniform integer in `[0, bound)`, without modulo bias (Lemire 2019). Returns 0 when
    /// `bound` is 0.
    #[inline]
    pub fn below(&mut self, bound: u32) -> u32 {
        let mut product = u64::from(self.next_u32()) * u64::from(bound);
        if (product as u32) < bound {
            let threshold = bound.wrapping_neg() % bound.max(1);
            while (product as u32) < threshold {
                product = u64::from(self.next_u32()) * u64::from(bound);
            }
        }
        (product >> 32) as u32
    }

    /// `true` with probability `probability` (clamped to `[0, 1]`).
    #[inline]
    pub fn chance(&mut self, probability: f32) -> bool {
        self.f32() < probability
    }

    /// Standard normal deviate (Box-Muller, two draws per call).
    #[inline]
    pub fn normal(&mut self) -> f32 {
        let radius = (-2.0 * (1.0 - self.f32()).ln()).sqrt();
        radius * self.angle().cos()
    }

    /// Normal deviate with the given mean and standard deviation.
    #[inline]
    pub fn normal_with(&mut self, mean: f32, std_dev: f32) -> f32 {
        self.normal().mul_add(std_dev, mean)
    }

    /// Unit vector uniformly distributed on the sphere (two draws).
    #[inline]
    pub fn unit_sphere(&mut self) -> Vec3 {
        let z = self.signed();
        let (sin, cos) = self.angle().sin_cos();
        let ring = (1.0 - z * z).max(0.0).sqrt();
        Vec3::new(ring * cos, ring * sin, z)
    }

    /// Unit vector uniformly distributed on the spherical cap of half-angle `half_angle`
    /// radians around the unit vector `axis` (two draws).
    #[inline]
    pub fn unit_cone(&mut self, axis: Vec3, half_angle: f32) -> Vec3 {
        let cos_theta = self.range(half_angle.cos(), 1.0);
        let sin_theta = (1.0 - cos_theta * cos_theta).max(0.0).sqrt();
        let (sin_phi, cos_phi) = self.angle().sin_cos();
        let (u, v) = axis.any_orthonormal_pair();
        axis * cos_theta + (u * cos_phi + v * sin_phi) * sin_theta
    }
}

#[cfg(test)]
mod tests {
    use super::{Rng, Stream, mix64};
    use glam::Vec3;

    #[test]
    fn same_key_gives_same_numbers() {
        let mut a = Rng::new(1, 2, Stream::BUD_FATE);
        let mut b = Rng::new(1, 2, Stream::BUD_FATE);
        for _ in 0..100 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn known_answer_is_stable() {
        assert_eq!(mix64(super::GOLDEN_GAMMA), 0xE220_A839_7B1D_CDAF);
        let mut rng = Rng::new(0, 0, Stream(0));
        let first = rng.next_u64();
        assert_eq!(Rng::at(0, 0, Stream(0), 0).next_u64(), first);
        assert_ne!(first, rng.next_u64());
    }

    #[test]
    fn draws_do_not_depend_on_call_order() {
        let sequential: Vec<u64> = (0..64)
            .flat_map(|node| {
                let mut rng = Rng::new(9, node, Stream::SHOOT_LENGTH);
                [rng.next_u64(), rng.next_u64(), rng.next_u64()]
            })
            .collect();
        let mut interleaved = vec![0_u64; sequential.len()];
        for counter in (0..3_u64).rev() {
            for node in (0..64_u32).rev() {
                let slot = node as usize * 3 + counter as usize;
                interleaved[slot] = Rng::at(9, node, Stream::SHOOT_LENGTH, counter).next_u64();
            }
        }
        assert_eq!(sequential, interleaved);
    }

    #[test]
    fn keys_are_distinct() {
        let base = Rng::new(5, 5, Stream::BUD_FATE).next_u64();
        assert_ne!(base, Rng::new(6, 5, Stream::BUD_FATE).next_u64());
        assert_ne!(base, Rng::new(5, 6, Stream::BUD_FATE).next_u64());
        assert_ne!(base, Rng::new(5, 5, Stream::SHOOT_LENGTH).next_u64());
        assert_ne!(Stream::custom(0), Stream::MESH);
    }

    /// Chi-square statistic of `counts` against a uniform expectation.
    fn chi_square(counts: &[u32], total: u32) -> f64 {
        let expected = f64::from(total) / counts.len() as f64;
        counts
            .iter()
            .map(|&c| (f64::from(c) - expected).powi(2) / expected)
            .sum()
    }

    #[test]
    fn floats_are_roughly_uniform_within_a_key() {
        let mut counts = [0_u32; 64];
        let mut rng = Rng::new(3, 0, Stream::FOLIAGE);
        for _ in 0..64_000 {
            let x = rng.f32();
            assert!((0.0..1.0).contains(&x));
            counts[(x * 64.0) as usize] += 1;
        }
        // 63 degrees of freedom: 120 is far beyond the 0.9999 quantile (about 110).
        assert!(chi_square(&counts, 64_000) < 120.0);
    }

    #[test]
    fn first_draws_are_roughly_uniform_across_nodes_and_seeds() {
        let mut by_node = [0_u32; 64];
        let mut by_seed = [0_u32; 64];
        for i in 0..64_000_u32 {
            by_node[Rng::new(1, i, Stream::BUD_FATE).below(64) as usize] += 1;
            by_seed[Rng::new(u64::from(i), 0, Stream::BUD_FATE).below(64) as usize] += 1;
        }
        assert!(chi_square(&by_node, 64_000) < 120.0);
        assert!(chi_square(&by_seed, 64_000) < 120.0);
    }

    #[test]
    fn neighbouring_nodes_are_uncorrelated() {
        let n = 50_000_u32;
        let mut sum = 0.0_f64;
        for node in 0..n {
            let a = f64::from(Rng::new(8, node, Stream::RADIUS).signed());
            let b = f64::from(Rng::new(8, node + 1, Stream::RADIUS).signed());
            sum += a * b;
        }
        // The product of two independent uniform [-1, 1) values has variance 1/9.
        let z = sum / (f64::from(n) / 9.0).sqrt();
        assert!(z.abs() < 5.0, "z = {z}");
    }

    #[test]
    fn below_stays_in_range() {
        let mut rng = Rng::new(4, 4, Stream::MESH);
        assert_eq!(rng.below(0), 0);
        assert_eq!(rng.below(1), 0);
        let mut seen = [false; 7];
        for _ in 0..500 {
            seen[rng.below(7) as usize] = true;
        }
        assert!(seen.iter().all(|&s| s));
    }

    #[test]
    fn normal_has_unit_moments() {
        let mut rng = Rng::new(11, 0, Stream::SHOOT_DIRECTION);
        let n = 100_000;
        let (mut sum, mut sum_sq) = (0.0_f64, 0.0_f64);
        for _ in 0..n {
            let x = f64::from(rng.normal());
            sum += x;
            sum_sq += x * x;
        }
        let mean = sum / f64::from(n);
        let variance = sum_sq / f64::from(n) - mean * mean;
        assert!(mean.abs() < 0.02, "mean = {mean}");
        assert!((variance - 1.0).abs() < 0.03, "variance = {variance}");
    }

    #[test]
    fn directions_are_unit_and_inside_the_cone() {
        let mut rng = Rng::new(2, 3, Stream::BRANCH_ANGLE);
        let axis = Vec3::new(1.0, 2.0, -0.5).normalize();
        let half_angle = 0.4_f32;
        let mut mean = Vec3::ZERO;
        for _ in 0..2000 {
            let s = rng.unit_sphere();
            assert!((s.length() - 1.0).abs() < 1e-5);
            mean += s;
            let c = rng.unit_cone(axis, half_angle);
            assert!((c.length() - 1.0).abs() < 1e-4);
            assert!(c.dot(axis) >= half_angle.cos() - 1e-4);
        }
        assert!((mean / 2000.0).length() < 0.06);
    }
}
