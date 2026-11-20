//! Placement: where every tree of a forest stands, which variant it is, how it is turned.
//!
//! The ground is cut into square cells at least as wide as the largest conflict distance,
//! so that two trees in conflict are always in the same or in neighbouring cells. Each
//! cell draws its candidates from its own random stream (keyed by the seed, the cell and
//! the species), at the intensity that [`crate::density`] computes from the target
//! densities and the largest suitability of the cell (slope and altitude at its centre and
//! corners). A candidate is then kept with the probability of its own suitability over
//! that of the cell times the exclusion factor at its place, so limits are exact per tree,
//! and survives the thinning when no candidate of higher priority stands within the
//! conflict distance of the pair:
//!
//! * [`Mode::Poisson`]: priorities are random and the conflict distance of two species is
//!   the mean of their spacings. The result is a marked Matérn hard-core process whose
//!   density per species is the target (see [`crate::density`]).
//! * [`Mode::Competition`]: the larger tree wins (priority by crown size, `spacing` times
//!   the relative scale), and the conflict distance is the mean of the two scaled
//!   spacings: self-thinning by distance and size, as large trees suppress the small ones
//!   near them. The intensities are those of the Poisson mode at the mean scales, so the
//!   density is close to the target but not exact; `docs/forest.md` has measurements.
//!
//! The ground is processed in bands of cell rows; every band also draws the rows next to
//! it (and a ring of cells outside the terrain) as competitors only, so the result does
//! not depend on the band size nor on the number of threads, and memory is bounded by
//! the band. Trees are returned in band, row, cell and candidate order.

use std::time::{Duration, Instant};

use arbrume_core::rng::{Rng, Stream, mix64};
use glam::{Quat, Vec3};
use rayon::prelude::*;

use crate::density;
use crate::error::ForestError;
use crate::exclude::Exclusions;
use crate::rules::SpeciesRule;
use crate::terrain::Terrain;

/// Stream of the candidates of a cell.
const CANDIDATES: Stream = Stream::custom(0x4F00);
/// Stream of the transform of a tree.
const TRANSFORM: Stream = Stream::custom(0x4F01);
/// Square meters per hectare.
const HECTARE: f64 = 10_000.0;
/// Cells per band, about: a band holds this many cells times the candidates per cell.
const BAND_CELLS: usize = 1 << 18;

/// How conflicts between candidates are settled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    /// Random priorities, fixed distances: densities as asked.
    #[default]
    Poisson,
    /// The larger tree wins, distances grow with size.
    Competition,
}

/// Settings of [`place`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PlaceSettings {
    /// Seed of every draw.
    pub seed: u64,
    /// How conflicts are settled.
    pub mode: Mode,
    /// Cell rows per band; 0 picks about [`BAND_CELLS`] cells per band. The result does
    /// not depend on it.
    pub band_rows: usize,
}

/// One tree of a forest.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Instance {
    /// Base of the trunk, in meters (Y up).
    pub position: [f32; 3],
    /// Unit quaternion `[x, y, z, w]`: the yaw, then the lean.
    pub rotation: [f32; 4],
    /// Uniform scale.
    pub scale: f32,
    /// Index of the species in the rules.
    pub species: u16,
    /// Index of the variant within the species.
    pub variant: u16,
    /// Colour factor per channel and 255, `value / 128` (128 is no change).
    pub tint: [u8; 4],
}

impl Instance {
    /// The colour factor as linear floats, alpha 1.
    #[must_use]
    pub fn tint_factor(&self) -> [f32; 4] {
        let [r, g, b, _] = self.tint.map(|v| f32::from(v) / 128.0);
        [r, g, b, 1.0]
    }
}

/// Counts of one species.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SpeciesCounts {
    /// Candidates drawn inside the terrain, after the exclusion factors.
    pub candidates: usize,
    /// Trees kept.
    pub kept: usize,
    /// Trees the target density gives over the terrain, from the suitability of each
    /// cell, before exclusions.
    pub expected_open: f64,
    /// Cells where the target density cannot be reached with the spacing.
    pub saturated_cells: usize,
}

/// What [`place`] did.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PlaceReport {
    /// Counts per species, in the order of the rules.
    pub species: Vec<SpeciesCounts>,
    /// Width of a cell, in meters.
    pub cell: f32,
    /// Cells along `x` and `z`.
    pub cells: [usize; 2],
    /// Bands processed.
    pub bands: usize,
    /// Most candidates held at once (one band).
    pub peak_candidates: usize,
    /// Wall-clock time.
    pub time: Duration,
}

/// One candidate tree.
#[derive(Debug, Clone, Copy)]
struct Candidate {
    x: f32,
    z: f32,
    scale: f32,
    species: u16,
    inside: bool,
    priority: u64,
    /// Index of the candidate within its cell, for the transform stream.
    slot: u32,
}

/// The cell grid and the per-pair constants.
#[derive(Debug)]
struct Grid<'a> {
    terrain: &'a Terrain,
    exclusions: &'a Exclusions,
    rules: &'a [SpeciesRule],
    settings: PlaceSettings,
    cell: f32,
    columns: i64,
    rows: i64,
    /// `pi d_st^2` at the mean scales, per pair.
    areas: Vec<f64>,
    /// Mean spacing of each pair at the mean scales, squared.
    pair_distance2: Vec<f32>,
}

/// What a cell draws with: the candidate intensity and the largest suitability of each
/// species.
#[derive(Debug)]
struct CellState {
    lambda: Vec<f64>,
    best: Vec<f32>,
}

/// The candidates of a range of rows, cell by cell.
#[derive(Debug, Default)]
struct Band {
    first_row: i64,
    /// Start of each cell in `candidates`, row-major over columns `-1..=columns`, plus
    /// the end.
    starts: Vec<usize>,
    candidates: Vec<Candidate>,
    counts: Vec<SpeciesCounts>,
}

/// Places the trees of `rules` on `terrain`, away from `exclusions`.
///
/// # Errors
/// [`ForestError::Config`] for an invalid rule or more than 64 species.
pub fn place(
    terrain: &Terrain,
    exclusions: &Exclusions,
    rules: &[SpeciesRule],
    settings: &PlaceSettings,
) -> Result<(Vec<Instance>, PlaceReport), ForestError> {
    let start = Instant::now();
    let grid = Grid::new(terrain, exclusions, rules, *settings)?;
    let mut report = PlaceReport {
        species: vec![SpeciesCounts::default(); rules.len()],
        cell: grid.cell,
        cells: [grid.columns as usize, grid.rows as usize],
        ..PlaceReport::default()
    };
    let band_rows = match settings.band_rows {
        0 => (BAND_CELLS / (grid.columns as usize + 2)).max(1),
        rows => rows,
    };
    let band_rows = i64::try_from(band_rows).unwrap_or(i64::MAX);
    let mut trees = Vec::new();
    let mut first = 0;
    while first < grid.rows {
        let last = (first + band_rows).min(grid.rows);
        let band = grid.band(first, last);
        report.peak_candidates = report.peak_candidates.max(band.candidates.len());
        for (total, counts) in report.species.iter_mut().zip(&band.counts) {
            total.candidates += counts.candidates;
            total.expected_open += counts.expected_open;
            total.saturated_cells += counts.saturated_cells;
        }
        let kept = grid.survivors(&band, first, last);
        for tree in &kept {
            report.species[tree.species as usize].kept += 1;
        }
        trees.extend(kept);
        report.bands += 1;
        first = last;
    }
    report.time = start.elapsed();
    Ok((trees, report))
}

impl<'a> Grid<'a> {
    fn new(
        terrain: &'a Terrain,
        exclusions: &'a Exclusions,
        rules: &'a [SpeciesRule],
        settings: PlaceSettings,
    ) -> Result<Self, ForestError> {
        if rules.len() > 64 {
            return Err(ForestError::Config("at most 64 species".to_owned()));
        }
        for (index, rule) in rules.iter().enumerate() {
            rule.validate()
                .map_err(|e| ForestError::Config(format!("species {index}: {e}")))?;
        }
        let n = rules.len();
        let mut areas = vec![0.0; n * n];
        let mut pair_distance2 = vec![0.0; n * n];
        let mut cell: f32 = 0.0;
        for (s, a) in rules.iter().enumerate() {
            for (t, b) in rules.iter().enumerate() {
                if a.layer != b.layer {
                    continue;
                }
                let mean = f32::midpoint(a.spacing, b.spacing);
                areas[s * n + t] = std::f64::consts::PI * f64::from(mean).powi(2);
                pair_distance2[s * n + t] = mean * mean;
                let largest = match settings.mode {
                    Mode::Poisson => mean,
                    Mode::Competition => {
                        let grow = |r: &SpeciesRule| r.spacing * r.scale[1] / r.mean_scale();
                        f32::midpoint(grow(a), grow(b))
                    }
                };
                cell = cell.max(largest);
            }
        }
        let [width, depth] = terrain.size();
        let cell = cell.max(width.max(depth) / 1e6).max(1e-3);
        Ok(Self {
            terrain,
            exclusions,
            rules,
            settings,
            cell,
            columns: (width / cell).ceil() as i64,
            rows: (depth / cell).ceil() as i64,
            areas,
            pair_distance2,
        })
    }

    /// Index of cell `(row, column)` in `band.starts`.
    fn cell_index(&self, band: &Band, row: i64, column: i64) -> usize {
        ((row - band.first_row) * (self.columns + 2) + column + 1) as usize
    }

    /// The key of cell `(row, column)` in the random streams.
    fn cell_seed(&self, row: i64, column: i64) -> u64 {
        let key = ((row as u64) << 32) ^ (column as u64 & 0xFFFF_FFFF);
        mix64(self.settings.seed ^ mix64(key))
    }

    /// Suitability of the cell with corner `low` for each species: at its centre, and the
    /// largest over its centre and corners.
    fn suitabilities(&self, low: [f32; 2], centre: &mut [f32], best: &mut [f32]) {
        best.fill(0.0);
        for (k, [u, v]) in [[0.5, 0.5], [0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]]
            .into_iter()
            .enumerate()
        {
            let (x, z) = (low[0] + u * self.cell, low[1] + v * self.cell);
            let slope = self.terrain.slope_deg(x, z);
            let altitude = self.terrain.height(x, z);
            for (s, rule) in self.rules.iter().enumerate() {
                let suits = rule.suitability(slope, altitude);
                if k == 0 {
                    centre[s] = suits;
                }
                best[s] = best[s].max(suits);
            }
        }
    }

    /// Area of cell `(row, column)` inside the terrain.
    fn area_inside(&self, row: i64, column: i64) -> f64 {
        let [width, depth] = self.terrain.size();
        let span = |index: i64, extent: f32| {
            let low = index as f32 * self.cell;
            f64::from(((low + self.cell).min(extent) - low.max(0.0)).max(0.0))
        };
        span(column, width) * span(row, depth)
    }

    /// Draws the candidates of one row of cells (columns `-1..=columns`): the number of
    /// candidates of each cell, the candidates, and the counts when `core`.
    fn draw_row(&self, row: i64, core: bool) -> (Vec<usize>, Vec<Candidate>, Vec<SpeciesCounts>) {
        let n = self.rules.len();
        let mut cell = CellState {
            lambda: vec![0.0; n],
            best: vec![0.0; n],
        };
        let (mut rho, mut centre) = (vec![0.0; n], vec![0.0; n]);
        let mut last_rho = vec![f64::NAN; n];
        let mut saturated = 0;
        let mut counts = vec![SpeciesCounts::default(); n];
        let (mut sizes, mut candidates) = (Vec::new(), Vec::new());
        let [x0, z0] = self.terrain.origin();
        for column in -1..=self.columns {
            let low = [x0 + column as f32 * self.cell, z0 + row as f32 * self.cell];
            self.suitabilities(low, &mut centre, &mut cell.best);
            for (s, rule) in self.rules.iter().enumerate() {
                rho[s] = f64::from(rule.density) * f64::from(cell.best[s]) / HECTARE;
            }
            if rho != last_rho {
                saturated = density::intensities(&rho, &self.areas, &mut cell.lambda);
                last_rho.clone_from(&rho);
            }
            let before = candidates.len();
            self.draw_cell(row, column, low, &cell, &mut candidates);
            sizes.push(candidates.len() - before);
            if !core || !(0..self.columns).contains(&column) {
                continue;
            }
            let area = self.area_inside(row, column);
            for (s, count) in counts.iter_mut().enumerate() {
                let open = f64::from(self.rules[s].density) * f64::from(centre[s]) / HECTARE;
                count.expected_open += open * area;
                count.saturated_cells += usize::from(saturated >> s & 1 == 1);
            }
            for candidate in candidates[before..].iter().filter(|c| c.inside) {
                counts[candidate.species as usize].candidates += 1;
            }
        }
        (sizes, candidates, counts)
    }

    /// Draws the candidates of cell `(row, column)` whose corner is `low`.
    fn draw_cell(
        &self,
        row: i64,
        column: i64,
        low: [f32; 2],
        cell: &CellState,
        out: &mut Vec<Candidate>,
    ) {
        let seed = self.cell_seed(row, column);
        let mut slot = 0u32;
        for (s, rule) in self.rules.iter().enumerate() {
            if cell.lambda[s] <= 0.0 {
                continue;
            }
            let mut rng = Rng::new(seed, s as u32, CANDIDATES);
            let mean = cell.lambda[s] * f64::from(self.cell * self.cell);
            for _ in 0..poisson(mean, &mut rng) {
                let x = low[0] + self.cell * rng.f32();
                let z = low[1] + self.cell * rng.f32();
                let scale = rng.range(rule.scale[0], rule.scale[1]);
                let keep = rng.f32();
                let priority = match self.settings.mode {
                    Mode::Poisson => rng.next_u64(),
                    Mode::Competition => {
                        let size = rule.spacing * scale / rule.mean_scale();
                        (u64::from(size.to_bits()) << 32) | u64::from(rng.next_u32())
                    }
                };
                slot += 1;
                let suits = if rule.restricts() {
                    let here =
                        rule.suitability(self.terrain.slope_deg(x, z), self.terrain.height(x, z));
                    here / cell.best[s]
                } else {
                    1.0
                };
                let free = self
                    .exclusions
                    .factor_for(self.terrain, x, z, rule.underwater);
                if keep >= suits * free {
                    continue;
                }
                out.push(Candidate {
                    x,
                    z,
                    scale,
                    species: s as u16,
                    inside: self.terrain.contains(x, z),
                    priority,
                    slot: slot - 1,
                });
            }
        }
    }

    /// Squared conflict distance of candidates `a` and `b`.
    fn limit2(&self, a: &Candidate, b: &Candidate) -> f32 {
        let n = self.rules.len();
        if self.rules[a.species as usize].layer != self.rules[b.species as usize].layer {
            return 0.0;
        }
        match self.settings.mode {
            Mode::Poisson => self.pair_distance2[a.species as usize * n + b.species as usize],
            Mode::Competition => {
                let size = |c: &Candidate| {
                    let rule = &self.rules[c.species as usize];
                    rule.spacing * c.scale / rule.mean_scale()
                };
                let mean = f32::midpoint(size(a), size(b));
                mean * mean
            }
        }
    }

    /// `true` when candidate `index` of `band`, in cell `(row, column)`, is beaten by a
    /// neighbour of higher priority (ties go to the earlier candidate).
    fn beaten(&self, band: &Band, index: usize, row: i64, column: i64) -> bool {
        let me = &band.candidates[index];
        let neighbours = (-1..=1).flat_map(|dr| (-1..=1).map(move |dc| (row + dr, column + dc)));
        for (r, c) in neighbours {
            let cell = self.cell_index(band, r, c);
            let start = band.starts[cell];
            let others = &band.candidates[start..band.starts[cell + 1]];
            for (offset, other) in others.iter().enumerate() {
                let other_index = start + offset;
                let wins = other.priority > me.priority
                    || (other.priority == me.priority && other_index < index);
                if !wins {
                    continue;
                }
                let d2 = (other.x - me.x).powi(2) + (other.z - me.z).powi(2);
                if d2 < self.limit2(me, other) {
                    return true;
                }
            }
        }
        false
    }

    /// The trees of rows `first..last` that survive the thinning.
    fn survivors(&self, band: &Band, first: i64, last: i64) -> Vec<Instance> {
        let rows: Vec<Vec<Instance>> = (first..last)
            .into_par_iter()
            .map(|row| self.row_survivors(band, row))
            .collect();
        rows.into_iter().flatten().collect()
    }

    /// The trees of one row of cells that survive the thinning.
    fn row_survivors(&self, band: &Band, row: i64) -> Vec<Instance> {
        let mut trees = Vec::new();
        for column in 0..self.columns {
            let cell = self.cell_index(band, row, column);
            for index in band.starts[cell]..band.starts[cell + 1] {
                let candidate = &band.candidates[index];
                if candidate.inside && !self.beaten(band, index, row, column) {
                    trees.push(self.instance(candidate, row, column));
                }
            }
        }
        trees
    }

    /// The tree of a surviving candidate.
    fn instance(&self, candidate: &Candidate, row: i64, column: i64) -> Instance {
        let rule = &self.rules[candidate.species as usize];
        let mut rng = Rng::new(self.cell_seed(row, column), candidate.slot, TRANSFORM);
        let yaw = rng.angle();
        let lean = rule.lean_deg.to_radians() * rng.f32();
        let heading = rng.angle();
        let variant = rng.below(u32::from(rule.variants)) as u16;
        let shade = 1.0 + rule.tint * rng.signed();
        let mut tint = [255u8; 4];
        for channel in &mut tint[..3] {
            let factor = shade * (1.0 + rule.tint / 3.0 * rng.signed());
            *channel = (factor * 128.0).round().clamp(0.0, 255.0) as u8;
        }
        let axis = Vec3::new(heading.cos(), 0.0, heading.sin());
        let rotation = Quat::from_axis_angle(axis, lean) * Quat::from_rotation_y(yaw);
        let base = self.ground(
            candidate.x,
            candidate.z,
            rule.trunk_radius * candidate.scale,
        );
        Instance {
            position: [candidate.x, base - rule.sink, candidate.z],
            rotation: rotation.normalize().to_array(),
            scale: candidate.scale,
            species: candidate.species,
            variant,
            tint,
        }
    }

    /// Lowest ground under a trunk of `radius` at `(x, z)`: the centre and four points
    /// on its rim.
    fn ground(&self, x: f32, z: f32, radius: f32) -> f32 {
        let h = |dx: f32, dz: f32| self.terrain.height(x + dx, z + dz);
        h(0.0, 0.0)
            .min(h(radius, 0.0))
            .min(h(-radius, 0.0))
            .min(h(0.0, radius))
            .min(h(0.0, -radius))
    }

    /// Draws the candidates of rows `first - 1 ..= last` (clamped to the ring of cells
    /// around the terrain).
    fn band(&self, first: i64, last: i64) -> Band {
        let from = (first - 1).max(-1);
        let to = (last + 1).min(self.rows + 1);
        let rows: Vec<(Vec<usize>, Vec<Candidate>, Vec<SpeciesCounts>)> = (from..to)
            .into_par_iter()
            .map(|row| self.draw_row(row, (first..last).contains(&row)))
            .collect();
        let mut band = Band {
            first_row: from,
            counts: vec![SpeciesCounts::default(); self.rules.len()],
            ..Band::default()
        };
        for (sizes, candidates, counts) in rows {
            let mut running = band.candidates.len();
            for size in sizes {
                band.starts.push(running);
                running += size;
            }
            band.candidates.extend(candidates);
            for (total, c) in band.counts.iter_mut().zip(counts) {
                total.candidates += c.candidates;
                total.expected_open += c.expected_open;
                total.saturated_cells += c.saturated_cells;
            }
        }
        band.starts.push(band.candidates.len());
        band
    }
}

/// A Poisson draw of mean `mean`: by inversion below 30, else a rounded normal.
fn poisson(mean: f64, rng: &mut Rng) -> u32 {
    if mean <= 0.0 {
        return 0;
    }
    if mean >= 30.0 {
        let draw = f64::from(rng.normal()).mul_add(mean.sqrt(), mean);
        return draw.round().max(0.0) as u32;
    }
    let uniform = f64::from(rng.f32());
    let mut probability = (-mean).exp();
    let mut cumulative = probability;
    let mut k = 0u32;
    while uniform > cumulative && probability > 0.0 {
        k += 1;
        probability *= mean / f64::from(k);
        cumulative += probability;
    }
    k
}
