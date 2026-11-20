//! Bark textures: tileable albedo, normal and roughness maps from a [`BarkPattern`].
//!
//! The texture covers one repeat of `bark.texture_size` (`[around, along]` the axis, in
//! meters), with `u` around the axis and `v` along it, as the bark texture coordinates
//! of the mesher. Features are laid on a lattice of `n_u x n_v` cells, with
//! `n_u = round(around / feature_size)` and `n_v = round(along / (feature_size *
//! stretch))`, at least 1: a whole number of cells per repeat, and every noise wraps
//! around that lattice (see `noise`), so the texture tiles in both directions. Distances
//! between features are measured in the physical metric, so cells longer along the axis
//! than around it give features stretched along it.
//!
//! Each texel gets a height in `[0, 1]`, a mix towards the secondary colour, a
//! brightness factor, a warm shift and a roughness factor; the kinds differ only in how
//! they compute these:
//!
//! * **furrowed**: the zero lines of gradient noise stretched along the axis, warped
//!   across it by low-frequency noise, are the furrows; between them lie flat-topped
//!   ridges, crossed here and there by short checks (zero lines of noise stretched
//!   around the axis, kept where a second noise is high). Furrows have a V-shaped
//!   profile, which is how Kratt et al. (2015) open the cracks of their simulated bark;
//!   ridge tops carry fine noise.
//! * **plated**: Voronoi plates with a random height and tint each, flaky noise on top
//!   and narrow, deep cracks between them.
//! * **fibrous**: ridged gradient noise, stretched along the axis and warped across it,
//!   in two octaves: strands that part and merge.
//! * **smooth**: a smooth mottled surface with lenticels, one short mark stretched around
//!   the axis per cell of a jittered lattice, kept with probability `coverage`.
//! * **peeling**: strongly warped Voronoi patches; a fraction `coverage` of them has shed
//!   its outer layer and shows a younger layer of another colour, with a raised rim.
//!
//! Colours are then divided by their mean and scaled by the largest level that keeps
//! all but a few texels at most 1 (`surface::level`): the albedo holds the variation
//! of the colour around its mean, and the base colour factor of the material, the
//! species colour divided by that mean, brings it back to the colour of the species
//! (see `SpeciesTextures::apply`). Individuals of a species whose colours differ share
//! the texture. Roughness is `bark.roughness` times the factor; normals come from the
//! heights scaled to `relief * feature_size` meters.

use crate::image::Image;
use crate::noise::{Cell, fbm, random, smoothstep, tiled, unit, worley};
use crate::surface::{height_normals, level, to_ratios};
use arbrume_core::species::{Bark, BarkKind, BarkPattern};
use glam::{Vec2, Vec3};
use rayon::prelude::*;

/// The three maps of a bark texture.
#[derive(Debug, Clone, PartialEq)]
pub struct BarkMaps {
    /// sRGB albedo, RGB.
    pub albedo: Image,
    /// Tangent-space normals, RGB, `+y` towards decreasing `v` (up the image).
    pub normal: Image,
    /// Linear roughness, grey; read as the green channel of a glTF metallic-roughness
    /// texture.
    pub roughness: Image,
    /// Mean linear colour of the albedo: the base colour factor of the material is its
    /// colour divided by this mean (see the module documentation).
    pub mean: [f32; 3],
}

/// What the pattern computes at one texel.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Sample {
    /// Height in `[0, 1]`.
    height: f32,
    /// Mix towards the secondary colour, in `[0, 1]`.
    mix: f32,
    /// Brightness factor of the main colour.
    tint: f32,
    /// Shift of the main colour towards a warmer hue, in `[0, 1]`.
    warm: f32,
    /// Factor on the roughness of the species.
    rough: f32,
}

/// The lattice and the parameters of one bark texture.
#[derive(Debug, Clone, Copy)]
struct Frame {
    pattern: BarkPattern,
    /// Cells around and along the axis.
    cells: [i32; 2],
    /// Physical size of a cell divided by `feature_size`, per axis.
    metric: Vec2,
    seed: u64,
}

impl Frame {
    fn new(bark: &Bark, seed: u64) -> Self {
        let pattern = bark.pattern;
        let [around, along] = bark.texture_size;
        let size = pattern.feature_size;
        let n_u = (around / size).round().clamp(1.0, 4096.0);
        let n_v = (along / (size * pattern.stretch))
            .round()
            .clamp(1.0, 4096.0);
        Self {
            pattern,
            cells: [n_u as i32, n_v as i32],
            metric: Vec2::new(around / n_u / size, along / n_v / size),
            seed,
        }
    }

    /// Point of the lattice domain at `uv`.
    fn domain(&self, uv: Vec2) -> Vec2 {
        uv * Vec2::new(self.cells[0] as f32, self.cells[1] as f32)
    }

    /// Frequency of noise with about `per_cell` waves per cell across and along.
    fn frequency(&self, per_cell: f32) -> [i32; 2] {
        let f = |cells: i32| ((cells as f32 * per_cell).round() as i32).max(1);
        [f(self.cells[0]), f(self.cells[1])]
    }

    /// Low-frequency warp of the domain, in cells, scaled by `amount`.
    fn warp(&self, uv: Vec2, amount: f32) -> Vec2 {
        let frequency = self.frequency(0.5);
        let x = fbm(uv, frequency, 3, self.seed ^ 0x11);
        let y = fbm(uv, frequency, 3, self.seed ^ 0x12);
        Vec2::new(x, y) * (amount * self.pattern.irregularity)
    }

    /// Voronoi cell at `uv` of the lattice, warped by `warp` cells.
    fn cell(&self, uv: Vec2, warp: f32, salt: u64) -> Cell {
        let p = self.domain(uv) + self.warp(uv, warp);
        worley(p, self.cells, 0.9, self.metric, self.seed ^ salt)
    }

    /// Low-frequency mottling in about `[-1, 1]`.
    fn mottle(&self, uv: Vec2) -> f32 {
        fbm(uv, [2, 2], 4, self.seed ^ 0x21)
    }

    fn sample(&self, uv: Vec2) -> Sample {
        match self.pattern.kind {
            BarkKind::Furrowed => furrowed(self, uv),
            BarkKind::Plated => plated(self, uv),
            BarkKind::Fibrous => fibrous(self, uv),
            BarkKind::Smooth => smooth(self, uv),
            BarkKind::Peeling => peeling(self, uv),
        }
    }
}

fn furrowed(frame: &Frame, uv: Vec2) -> Sample {
    let pattern = &frame.pattern;
    let [n_u, n_v] = frame.cells;
    let wander = fbm(uv, [2, (n_v / 2).max(1)], 3, frame.seed ^ 0x31);
    let across = uv.x + wander * pattern.irregularity * 0.8 / n_u as f32;
    let shifted = Vec2::new(across.rem_euclid(1.0), uv.y);
    let long = (n_v / 2).max(1);
    let line = tiled(shifted, [n_u, long], frame.seed ^ 0x32).abs()
        + 0.25 * tiled(shifted, [2 * n_u, n_v], frame.seed ^ 0x34);
    let width = 0.05 + 0.35 * pattern.coverage;
    let ridge = (line / width).min(1.0);
    let ridge = 1.0 - (1.0 - ridge) * (1.0 - ridge);
    let checks = tiled(uv, [(n_u / 2).max(1), 6 * n_v], frame.seed ^ 0x35).abs();
    let gate = smoothstep(0.25, 0.45, tiled(uv, [n_u, 2 * n_v], frame.seed ^ 0x36));
    let cross = gate * (1.0 - smoothstep(0.0, 0.06, checks)) * pattern.coverage.sqrt();
    let detail = fbm(uv, frame.frequency(3.0), 3, frame.seed ^ 0x33);
    let height = (ridge * (0.85 + 0.12 * detail) - 0.35 * cross * ridge).clamp(0.0, 1.0);
    let mottle = frame.mottle(uv);
    Sample {
        height,
        mix: (1.0 - height).powf(1.5),
        tint: 1.0 + pattern.contrast * (0.4 * detail + 0.5 * mottle),
        warm: pattern.contrast * 0.5 * (detail + 0.5).clamp(0.0, 1.0),
        rough: 1.0 + pattern.roughness_variation * (0.6 * (1.0 - height) - 0.2),
    }
}

fn plated(frame: &Frame, uv: Vec2) -> Sample {
    let pattern = &frame.pattern;
    let cell = frame.cell(uv, 0.35, 0x41);
    let crack = 0.03 + 0.25 * pattern.coverage;
    let edge = smoothstep(0.0, crack, cell.border);
    let lip = 0.12 * (1.0 - smoothstep(crack, 3.0 * crack, cell.border));
    let lift = 0.55 + 0.35 * unit(cell.id);
    let flakes = fbm(uv, frame.frequency(2.5), 3, frame.seed ^ 0x42);
    let scale = fbm(uv, frame.frequency(6.0), 2, frame.seed ^ 0x43);
    let surface = lift + 0.12 * flakes + 0.05 * scale + lip;
    let own = unit(cell.id >> 7) - 0.5;
    Sample {
        height: (edge * surface).clamp(0.0, 1.0),
        mix: (1.0 - edge).powi(2),
        tint: 1.0 + pattern.contrast * (0.9 * own + 0.3 * frame.mottle(uv) + 0.3 * flakes),
        warm: (pattern.contrast * (0.5 + own + 0.4 * flakes)).clamp(0.0, 1.0),
        rough: 1.0 + pattern.roughness_variation * (0.7 * (1.0 - edge) - 0.15),
    }
}

/// `1 - |n|`: ridges of gradient noise `n`.
fn ridged(n: f32) -> f32 {
    1.0 - n.abs().min(1.0)
}

fn fibrous(frame: &Frame, uv: Vec2) -> Sample {
    let pattern = &frame.pattern;
    let [n_u, n_v] = frame.cells;
    let wander = fbm(uv, [2, (n_v / 2).max(1)], 3, frame.seed ^ 0x51);
    let across = uv.x + wander * pattern.irregularity * 1.5 / n_u as f32;
    let shifted = Vec2::new(across.rem_euclid(1.0), uv.y);
    let long = (n_v / 3).max(1);
    let coarse = ridged(tiled(shifted, [n_u, long], frame.seed ^ 0x52));
    let fine = ridged(tiled(shifted, [3 * n_u, 2 * long], frame.seed ^ 0x53));
    let strands = 0.65 * coarse + 0.35 * fine;
    let groove = 0.25 + 0.5 * pattern.coverage;
    let height = smoothstep(groove * 0.4, groove + 0.4, strands);
    let streak = fbm(shifted, [2 * n_u, long.max(1)], 2, frame.seed ^ 0x54);
    Sample {
        height,
        mix: (1.0 - height).powi(2),
        tint: 1.0 + pattern.contrast * (0.6 * streak + 0.3 * frame.mottle(uv)),
        warm: (pattern.contrast * (0.5 + streak)).clamp(0.0, 1.0),
        rough: 1.0 + pattern.roughness_variation * (0.5 * (1.0 - height) - 0.1),
    }
}

/// Darkness of the lenticel of the jittered cell around `p`, in `[0, 1]`.
fn lenticel(frame: &Frame, p: Vec2) -> f32 {
    let pattern = &frame.pattern;
    let cell = worley(p, frame.cells, 0.8, frame.metric, frame.seed ^ 0x61);
    let present = unit(cell.id) < (1.4 * pattern.coverage).min(1.0);
    if !present {
        return 0.0;
    }
    let size = 0.5 + unit(cell.id >> 5) * (0.3 + 0.7 * pattern.irregularity);
    let length = 0.45 * size * frame.metric.x;
    let thickness = 0.06 * size * frame.metric.y.max(frame.metric.x);
    let d = Vec2::new(cell.offset.x / length, cell.offset.y / thickness).length();
    1.0 - smoothstep(0.6, 1.0, d)
}

fn smooth(frame: &Frame, uv: Vec2) -> Sample {
    let pattern = &frame.pattern;
    let warp = frame.warp(uv, 0.3);
    let mark = lenticel(frame, frame.domain(uv) + warp);
    let scar_cells = [(frame.cells[0] / 3).max(1), (frame.cells[1] / 4).max(1)];
    let scar_p = uv * Vec2::new(scar_cells[0] as f32, scar_cells[1] as f32) + warp;
    let scar = worley(scar_p, scar_cells, 0.9, frame.metric, frame.seed ^ 0x62);
    let ragged = 0.08 * fbm(uv, frame.frequency(2.0), 2, frame.seed ^ 0x63);
    let wide = Vec2::new(scar.offset.x / 0.9, scar.offset.y / 0.12 + ragged).length();
    let marked = random(frame.seed, scar.id as u32, 0, 0x64) < 0.6 * pattern.coverage;
    let band = if marked {
        1.0 - smoothstep(0.7, 1.0, wide)
    } else {
        0.0
    };
    let mottle = frame.mottle(uv);
    let fine = fbm(uv, frame.frequency(2.0), 3, frame.seed ^ 0x65);
    Sample {
        height: 0.5 + 0.08 * fine + 0.25 * mark - 0.1 * band,
        mix: mark.max(0.85 * band),
        tint: 1.0 + pattern.contrast * (0.5 * mottle + 0.15 * fine),
        warm: (pattern.contrast * (0.4 + mottle)).clamp(0.0, 1.0),
        rough: 1.0 + pattern.roughness_variation * (0.8 * mark.max(band) - 0.4),
    }
}

fn peeling(frame: &Frame, uv: Vec2) -> Sample {
    let pattern = &frame.pattern;
    let cell = frame.cell(uv, 1.2, 0x71);
    let shed = unit(cell.id) < pattern.coverage;
    let rim = 1.0 - smoothstep(0.0, 0.1, cell.border);
    let fine = fbm(uv, frame.frequency(2.0), 3, frame.seed ^ 0x72);
    let layer = unit(cell.id >> 9);
    let (height, mix, warm) = if shed {
        (0.35 + 0.05 * fine, 0.35 + 0.5 * layer, 1.0 - layer)
    } else {
        (0.6 + 0.15 * rim + 0.06 * fine, 0.1 * rim, 0.2 * layer)
    };
    Sample {
        height,
        mix,
        tint: 1.0 + pattern.contrast * (0.4 * frame.mottle(uv) + 0.2 * fine),
        warm: (warm * pattern.contrast).clamp(0.0, 1.0),
        rough: 1.0 + pattern.roughness_variation * if shed { -0.3 } else { 0.2 },
    }
}

/// Synthesizes the maps of `bark`, `resolution` texels square, from `seed`.
#[must_use]
pub fn bark_maps(bark: &Bark, resolution: u32, seed: u64) -> BarkMaps {
    let frame = Frame::new(bark, seed);
    let side = resolution as usize;
    let mut samples = vec![
        Sample {
            height: 0.0,
            mix: 0.0,
            tint: 1.0,
            warm: 0.0,
            rough: 1.0,
        };
        side * side
    ];
    samples
        .par_chunks_mut(side)
        .enumerate()
        .for_each(|(y, row)| {
            for (x, sample) in row.iter_mut().enumerate() {
                let uv = Vec2::new(
                    (x as f32 + 0.5) / side as f32,
                    (y as f32 + 0.5) / side as f32,
                );
                *sample = frame.sample(uv);
            }
        });
    let base = Vec3::from(bark.color);
    let secondary = Vec3::from(bark.pattern.secondary_color(bark.color));
    let warm = base * Vec3::new(1.35, 1.05, 0.75);
    let mut colors: Vec<Vec3> = samples
        .iter()
        .map(|s| {
            let main = base.lerp(warm, s.warm) * s.tint.max(0.0);
            main.lerp(secondary, s.mix.clamp(0.0, 1.0))
        })
        .collect();
    to_ratios(&mut colors, None);
    let mean = level(&mut [(&mut colors, None)]);
    let roughness: Vec<f32> = samples
        .iter()
        .map(|s| (bark.roughness * s.rough).clamp(0.02, 1.0))
        .collect();
    let heights: Vec<f32> = samples.iter().map(|s| s.height).collect();
    let [around, along] = bark.texture_size;
    let spacing = [around / side as f32, along / side as f32];
    let depth = bark.pattern.relief * bark.pattern.feature_size;
    let normals = height_normals(&heights, side, spacing, depth, true);
    BarkMaps {
        albedo: Image::albedo(resolution, &colors, None),
        normal: Image::normals(resolution, &normals),
        roughness: Image::grey(resolution, &roughness),
        mean: mean.to_array(),
    }
}
