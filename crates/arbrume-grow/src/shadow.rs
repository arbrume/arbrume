//! The shadow grid: shadow propagation of Palubicki et al. (2009, section 4.1).
//!
//! Space is cut into cubic voxels holding a shadow value `s`, 0 at first. A metamer in
//! voxel `(I, J, K)` adds `a * b^-q` to the voxels `(I ± p, J - q, K ± p)` for
//! `q = 0..=q_max` and `p = 0..=q`: a pyramid opening downwards, whose layer `q` is the
//! full square of `2q + 1` voxels a side. The exposure of a bud in a voxel is
//! `Q = max(C - s + a, 0)`, the `+ a` cancelling the shadow of the metamer that bears
//! the bud. The direction of most light is the negative gradient of `s`, taken by
//! central differences.
//!
//! The grid is only the field: it knows nothing of plants. Whoever owns it casts and
//! removes the shadow of each metamer ([`ShadowGrid::cast`]), which lets several plants
//! write to one grid.
//!
//! # Storage
//!
//! [`GridStorage::Dense`] keeps one array over the bounding box of everything cast so
//! far and reallocates when a pyramid leaves it. [`GridStorage::Sparse`] keeps blocks of
//! `8 x 8 x 8` voxels in a hash table and allocates a block the first time it is
//! written. Both give the same values; `docs/growth.md` has the measurements behind the
//! default.

use arbrume_core::rng::mix64;
use glam::{IVec3, Vec3};
use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

/// Voxels added around a dense grid each time it has to grow.
const PAD: i32 = 8;
/// Log2 of the edge of a sparse block, in voxels.
const BLOCK_BITS: i32 = 3;
/// Voxels in a sparse block.
const BLOCK_CELLS: usize = 1 << (3 * BLOCK_BITS);

/// Coefficients of the shadow model, in the notation of the paper.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShadowParams {
    /// Edge of a voxel, in meters.
    pub cell: f32,
    /// Shadow `a` a metamer adds to its own voxel.
    pub strength: f32,
    /// Base `b` of the decay with depth.
    pub decay: f32,
    /// Number `q_max` of layers below a metamer its shadow reaches.
    pub depth: u8,
    /// Exposure `C` in full light.
    pub full: f32,
}

impl ShadowParams {
    /// The same shadow on voxels of edge `cell`: a pyramid that reaches as deep in
    /// meters (`q_max` scaled by the ratio of the edges and rounded) and fades as fast
    /// per meter (`b` raised to the inverse ratio), with the same `a` and `C`. Returns
    /// `self` unchanged when `cell` is its own edge.
    #[must_use]
    pub fn in_cell(&self, cell: f32) -> Self {
        if cell.to_bits() == self.cell.to_bits() {
            return *self;
        }
        let ratio = self.cell / cell;
        Self {
            cell,
            decay: self.decay.powf(ratio.recip()),
            depth: (f32::from(self.depth) * ratio).round().clamp(0.0, 255.0) as u8,
            ..*self
        }
    }
}

/// How a [`ShadowGrid`] stores its voxels. See the [module documentation](self).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum GridStorage {
    /// One array over the bounding box.
    #[default]
    Dense,
    /// Hashed blocks of `8 x 8 x 8` voxels.
    Sparse,
}

/// Dense storage: rows along X, then Z, then Y.
#[derive(Debug, Clone, Default, PartialEq)]
struct Dense {
    min: IVec3,
    size: IVec3,
    values: Vec<f32>,
}

impl Dense {
    fn index(&self, cell: IVec3) -> Option<usize> {
        let local = cell - self.min;
        let inside = local.cmpge(IVec3::ZERO).all() && local.cmplt(self.size).all();
        inside.then(|| ((local.y * self.size.z + local.z) * self.size.x + local.x) as usize)
    }

    /// Makes the grid cover the box `low..=high`, keeping its values.
    fn cover(&mut self, low: IVec3, high: IVec3) {
        let max = self.min + self.size;
        if self.size.x > 0 && low.cmpge(self.min).all() && high.cmplt(max).all() {
            return;
        }
        let (new_min, new_max) = if self.size.x > 0 {
            let grow_low = low.cmplt(self.min);
            let grow_high = high.cmpge(max);
            let pad = (self.size / 4).max(IVec3::splat(PAD));
            (
                IVec3::select(grow_low, low - pad, self.min),
                IVec3::select(grow_high, high + 1 + pad, max),
            )
        } else {
            (low - PAD, high + 1 + PAD)
        };
        let new_size = new_max - new_min;
        let count = new_size.x as usize * new_size.y as usize * new_size.z as usize;
        let mut values = vec![0.0_f32; count];
        let shift = self.min - new_min;
        let width = self.size.x as usize;
        for y in 0..self.size.y {
            for z in 0..self.size.z {
                let from = ((y * self.size.z + z) * self.size.x) as usize;
                let row = (y + shift.y) * new_size.z + z + shift.z;
                let to = (row * new_size.x + shift.x) as usize;
                values[to..to + width].copy_from_slice(&self.values[from..from + width]);
            }
        }
        *self = Self {
            min: new_min,
            size: new_size,
            values,
        };
    }

    /// Adds `amount` to the row `x_low..=x_high` at `(y, z)`, which the grid covers.
    fn add_row(&mut self, (y, z): (i32, i32), (x_low, x_high): (i32, i32), amount: f32) {
        let row = (y - self.min.y) * self.size.z + z - self.min.z;
        let start = (row * self.size.x + x_low - self.min.x) as usize;
        let end = start + (x_high - x_low) as usize;
        for value in &mut self.values[start..=end] {
            *value += amount;
        }
    }
}

/// Hasher of packed block coordinates: one round of `mix64`.
#[derive(Debug, Clone, Copy, Default)]
struct BlockHasher(u64);

impl Hasher for BlockHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.0 = mix64(self.0 ^ u64::from(byte));
        }
    }

    fn write_u64(&mut self, value: u64) {
        self.0 = mix64(value);
    }
}

/// Sparse storage: blocks found through a hash table, their voxels in one array.
#[derive(Debug, Clone, Default)]
struct Sparse {
    blocks: HashMap<u64, u32, BuildHasherDefault<BlockHasher>>,
    values: Vec<f32>,
}

impl Sparse {
    /// Key of the block of `cell` and index of the voxel in the block.
    fn locate(cell: IVec3) -> (u64, usize) {
        let block = cell >> BLOCK_BITS;
        let pack = |v: i32| u64::from(v.cast_unsigned()) & 0x1F_FFFF;
        let key = pack(block.x) | (pack(block.y) << 21) | (pack(block.z) << 42);
        let local = cell & ((1 << BLOCK_BITS) - 1);
        let index = ((local.y << BLOCK_BITS | local.z) << BLOCK_BITS | local.x) as usize;
        (key, index)
    }

    fn get(&self, cell: IVec3) -> f32 {
        let (key, index) = Self::locate(cell);
        self.blocks.get(&key).map_or(0.0, |&block| {
            self.values[block as usize * BLOCK_CELLS + index]
        })
    }

    /// Adds `amount` to the row `x_low..=x_high` at `(y, z)`.
    fn add_row(&mut self, (y, z): (i32, i32), (x_low, x_high): (i32, i32), amount: f32) {
        let mut cached: Option<(u64, usize)> = None;
        for x in x_low..=x_high {
            let (key, index) = Self::locate(IVec3::new(x, y, z));
            let base = match cached {
                Some((cached_key, base)) if cached_key == key => base,
                _ => {
                    let next = (self.values.len() / BLOCK_CELLS) as u32;
                    let block = *self.blocks.entry(key).or_insert(next);
                    if block == next {
                        self.values.resize(self.values.len() + BLOCK_CELLS, 0.0);
                    }
                    let base = block as usize * BLOCK_CELLS;
                    cached = Some((key, base));
                    base
                }
            };
            self.values[base + index] += amount;
        }
    }
}

/// The voxels of a grid.
#[derive(Debug, Clone)]
enum Store {
    Dense(Dense),
    Sparse(Sparse),
}

/// Shadow `a * b^-q` of one metamer at each depth `q` in `0..=q_max`.
fn pyramid_weights(params: &ShadowParams) -> Vec<f32> {
    (0..=i32::from(params.depth))
        .map(|depth| params.strength * params.decay.powi(-depth))
        .collect()
}

/// Voxel grid of shadow values. See the [module documentation](self).
#[derive(Debug, Clone)]
pub struct ShadowGrid {
    params: ShadowParams,
    inverse_cell: f32,
    weights: Vec<f32>,
    store: Store,
}

impl ShadowGrid {
    /// An empty grid.
    #[must_use]
    pub fn new(params: ShadowParams, storage: GridStorage) -> Self {
        Self {
            params,
            inverse_cell: params.cell.recip(),
            weights: pyramid_weights(&params),
            store: match storage {
                GridStorage::Dense => Store::Dense(Dense::default()),
                GridStorage::Sparse => Store::Sparse(Sparse::default()),
            },
        }
    }

    /// The coefficients of the grid.
    #[must_use]
    pub const fn params(&self) -> &ShadowParams {
        &self.params
    }

    /// Casts, takes back and reads with the coefficients of `params` from now on,
    /// rescaled to the edge of the voxels of the grid ([`ShadowParams::in_cell`]). The
    /// values already in the grid are kept, which lets plants of several species share
    /// one grid, each casting its own pyramid. A shadow is taken back with the
    /// coefficients it was cast with.
    pub fn set_coefficients(&mut self, params: &ShadowParams) {
        let params = params.in_cell(self.params.cell);
        if params != self.params {
            self.weights = pyramid_weights(&params);
            self.params = params;
        }
    }

    /// Shadow `a * b^-q` one metamer adds at each depth `q` in `0..=q_max`.
    #[must_use]
    pub fn weights(&self) -> &[f32] {
        &self.weights
    }

    /// Voxel holding `point`.
    #[inline]
    #[must_use]
    pub fn cell_of(&self, point: Vec3) -> IVec3 {
        (point * self.inverse_cell).floor().as_ivec3()
    }

    /// Adds `sign` times the shadow pyramid of a metamer at `point`: `1.0` casts it,
    /// `-1.0` removes one cast before.
    pub fn cast(&mut self, point: Vec3, sign: f32) {
        let cell = self.cell_of(point);
        let depth = i32::from(self.params.depth);
        if let Store::Dense(dense) = &mut self.store {
            let reach = IVec3::new(depth, 0, depth);
            dense.cover(cell - reach - IVec3::Y * depth, cell + reach);
        }
        for (q, weight) in (0_i32..).zip(&self.weights) {
            let amount = sign * weight;
            for z in cell.z - q..=cell.z + q {
                let (row, span) = ((cell.y - q, z), (cell.x - q, cell.x + q));
                match &mut self.store {
                    Store::Dense(dense) => dense.add_row(row, span, amount),
                    Store::Sparse(sparse) => sparse.add_row(row, span, amount),
                }
            }
        }
    }

    /// Shadow value `s` of the voxel `cell`; 0 where nothing was cast.
    #[inline]
    #[must_use]
    pub fn shadow(&self, cell: IVec3) -> f32 {
        match &self.store {
            Store::Dense(dense) => dense.index(cell).map_or(0.0, |index| dense.values[index]),
            Store::Sparse(sparse) => sparse.get(cell),
        }
    }

    /// Exposure `Q = max(C - s + a, 0)` of a bud at `point` whose own metamer has cast
    /// its shadow, with `extra` more shadow from outside the grid.
    #[inline]
    #[must_use]
    pub fn exposure(&self, point: Vec3, extra: f32) -> f32 {
        let shadow = self.shadow(self.cell_of(point)) + extra;
        (self.params.full - shadow + self.params.strength).max(0.0)
    }

    /// Gradient of the shadow value at `point` by central differences, per voxel. The
    /// direction of most light is its opposite.
    #[must_use]
    pub fn gradient(&self, point: Vec3) -> Vec3 {
        let cell = self.cell_of(point);
        let slope = |axis: IVec3| 0.5 * (self.shadow(cell + axis) - self.shadow(cell - axis));
        Vec3::new(slope(IVec3::X), slope(IVec3::Y), slope(IVec3::Z))
    }

    /// Number of voxels allocated.
    #[must_use]
    pub fn allocated_cells(&self) -> usize {
        match &self.store {
            Store::Dense(dense) => dense.values.len(),
            Store::Sparse(sparse) => sparse.values.len(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{GridStorage, ShadowGrid, ShadowParams};
    use glam::{IVec3, Vec3};

    const PARAMS: ShadowParams = ShadowParams {
        cell: 0.5,
        strength: 0.2,
        decay: 2.0,
        depth: 4,
        full: 1.0,
    };

    /// Every voxel offset of the box `x`, `y`, `z`.
    fn offsets(
        x: std::ops::Range<i32>,
        y: std::ops::Range<i32>,
        z: std::ops::Range<i32>,
    ) -> impl Iterator<Item = IVec3> {
        y.flat_map(move |dy| {
            let x = x.clone();
            z.clone()
                .flat_map(move |dz| x.clone().map(move |dx| IVec3::new(dx, dy, dz)))
        })
    }

    #[test]
    fn a_single_metamer_casts_the_published_pyramid() {
        for storage in [GridStorage::Dense, GridStorage::Sparse] {
            let mut grid = ShadowGrid::new(PARAMS, storage);
            let point = Vec3::new(1.2, 3.1, -0.7);
            grid.cast(point, 1.0);
            let origin = grid.cell_of(point);
            assert_eq!(origin, IVec3::new(2, 6, -2));
            for offset in offsets(-6..7, -6..3, -6..7) {
                let q = -offset.y;
                let inside = (0..=4).contains(&q) && offset.x.abs() <= q && offset.z.abs() <= q;
                let expected = if inside { 0.2 * 0.5_f32.powi(q) } else { 0.0 };
                let value = grid.shadow(origin + offset);
                assert!((value - expected).abs() < 1e-7, "{storage:?} {offset}");
            }
        }
    }

    #[test]
    fn exposure_follows_the_published_formula() {
        let mut grid = ShadowGrid::new(PARAMS, GridStorage::Dense);
        let bud = Vec3::new(0.1, 2.0, 0.1);
        assert!((grid.exposure(bud, 0.0) - 1.2).abs() < 1e-6);
        grid.cast(bud, 1.0);
        assert!((grid.exposure(bud, 0.0) - 1.0).abs() < 1e-6);
        grid.cast(bud + Vec3::Y * 0.5, 1.0);
        assert!((grid.exposure(bud, 0.0) - 0.9).abs() < 1e-6);
        assert!((grid.exposure(bud, 0.3) - 0.6).abs() < 1e-6);
        for _ in 0..40 {
            grid.cast(bud + Vec3::Y, 1.0);
        }
        assert_eq!(grid.exposure(bud, 0.0).to_bits(), 0.0_f32.to_bits());
    }

    #[test]
    fn the_gradient_points_into_the_shade() {
        let mut grid = ShadowGrid::new(PARAMS, GridStorage::Dense);
        grid.cast(Vec3::new(2.0, 3.0, 0.0), 1.0);
        let gradient = grid.gradient(Vec3::new(0.75, 2.0, 0.25));
        assert!(gradient.x > 0.0 && gradient.z.abs() < 1e-7);
    }

    #[test]
    fn storages_agree_and_removal_restores_the_grid() {
        let mut dense = ShadowGrid::new(PARAMS, GridStorage::Dense);
        let mut sparse = ShadowGrid::new(PARAMS, GridStorage::Sparse);
        let points: Vec<Vec3> = (0..200)
            .map(|i| {
                let t = i as f32;
                Vec3::new((t * 0.37).sin() * 9.0, t * 0.11, (t * 0.23).cos() * 7.0)
            })
            .collect();
        for &point in &points {
            dense.cast(point, 1.0);
            sparse.cast(point, 1.0);
        }
        let cells = |grid: &ShadowGrid| -> Vec<u32> {
            let voxels = offsets(-28..28, -12..50, -24..24);
            voxels.map(|voxel| grid.shadow(voxel).to_bits()).collect()
        };
        assert_eq!(cells(&dense), cells(&sparse));
        assert!(sparse.allocated_cells() > 0 && dense.allocated_cells() > 0);
        for &point in &points {
            dense.cast(point, -1.0);
        }
        let residue = cells(&dense).into_iter().map(f32::from_bits);
        assert!(residue.fold(0.0_f32, |worst, v| worst.max(v.abs())) < 1e-5);
    }
}
