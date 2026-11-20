//! Coupled growth: the trees of a region grown together, so that neighbours shade one
//! another, instead of drawn from the variant library.
//!
//! The region is cut into square tiles. Each tile grows, with
//! `arbrume_grow::grow_together`, the trees whose base lies in it plus the trees within
//! `halo` meters of it, which cast their shade into the tile and are then dropped: memory
//! is that of one tile's stand at a time per thread, whatever the size of the region. A
//! tree near a tile border is therefore shaded by the neighbours within the halo only,
//! and two trees on either side of a border are not grown in the same stand: the shade
//! they cast on each other is that of their own tile's growth of the other, so the
//! coupling is exact inside a tile and approximate across borders. Each tree is grown at
//! its scale and standing upright where it stands (its yaw and lean are not applied, as
//! the shade depends on them), as its variant individual (`Species::varied`) with the
//! seed of its variant. Tiles run in parallel; the result depends on the trees only.

use std::time::{Duration, Instant};

use arbrume_core::species::Species;
use arbrume_gen::{GenParams, Generator, Plant, Stats, Surroundings};
use arbrume_grow::{Standing, grow_together};
use glam::Vec3;
use rayon::prelude::*;

use crate::error::ForestError;
use crate::library::VariantSpec;
use crate::place::Instance;

/// Settings of [`grow_coupled`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CoupledSettings {
    /// `[x0, z0, x1, z1]` of the region.
    pub region: [f32; 4],
    /// Side of a tile, in meters.
    pub tile: f32,
    /// Neighbours within this distance of a tile shade it, in meters.
    pub halo: f32,
    /// Mesh quality.
    pub quality: f32,
}

/// One tree grown in its stand.
#[derive(Debug, Clone)]
pub struct CoupledTree {
    /// Index of the tree in the instances of the forest.
    pub instance: usize,
    /// The grown and meshed plant, in its own frame.
    pub plant: Plant,
}

/// The coupled trees of a forest.
#[derive(Debug, Clone, Default)]
pub struct Coupled {
    /// The trees, in the order of the instances.
    pub trees: Vec<CoupledTree>,
    /// Tiles grown.
    pub tiles: usize,
    /// Most trees grown in one stand, halo included.
    pub largest_stand: usize,
    /// Trees grown in halos and dropped.
    pub halo_trees: usize,
    /// Wall-clock time.
    pub time: Duration,
}

/// Grows the trees of `instances` that stand in the region of `settings`.
///
/// # Errors
/// [`ForestError::Config`] for a tile that is not positive, and the errors of meshing.
pub fn grow_coupled(
    instances: &[Instance],
    specs: &[VariantSpec],
    settings: &CoupledSettings,
) -> Result<Coupled, ForestError> {
    let start = Instant::now();
    if !(settings.tile > 0.0 && settings.halo >= 0.0) {
        return Err(ForestError::Config(
            "coupled: tile must be > 0 and halo >= 0".to_owned(),
        ));
    }
    let individuals: Vec<Vec<Species>> = specs
        .iter()
        .map(|spec| {
            (0..spec.count)
                .map(|k| spec.species.varied(spec.seed, u32::from(k)))
                .collect()
        })
        .collect();
    let [x0, z0, x1, z1] = settings.region;
    let columns = ((x1 - x0) / settings.tile).ceil().max(0.0) as usize;
    let rows = ((z1 - z0) / settings.tile).ceil().max(0.0) as usize;
    let tiles: Vec<[f32; 4]> = (0..rows * columns)
        .map(|t| {
            let (i, j) = ((t % columns) as f32, (t / columns) as f32);
            let low = [x0 + i * settings.tile, z0 + j * settings.tile];
            [
                low[0],
                low[1],
                (low[0] + settings.tile).min(x1),
                (low[1] + settings.tile).min(z1),
            ]
        })
        .collect();
    let nearby: Vec<usize> = (0..instances.len())
        .filter(|&i| {
            within(
                &settings.region,
                instances[i].position[0],
                instances[i].position[2],
                settings.halo,
            )
        })
        .collect();
    let grown: Vec<Result<(Vec<CoupledTree>, usize), ForestError>> = tiles
        .par_iter()
        .map(|tile| grow_tile(instances, &nearby, (specs, &individuals), tile, settings))
        .collect();
    let mut coupled = Coupled {
        tiles: tiles.len(),
        ..Coupled::default()
    };
    for result in grown {
        let (trees, stand) = result?;
        coupled.largest_stand = coupled.largest_stand.max(stand);
        coupled.halo_trees += stand - trees.len();
        coupled.trees.extend(trees);
    }
    coupled.trees.sort_by_key(|tree| tree.instance);
    coupled.time = start.elapsed();
    Ok(coupled)
}

/// `true` when `(x, z)` is in `rect` (`[x0, z0, x1, z1]`, upper edges excluded) grown by
/// `margin`.
fn within(rect: &[f32; 4], x: f32, z: f32, margin: f32) -> bool {
    x >= rect[0] - margin && z >= rect[1] - margin && x < rect[2] + margin && z < rect[3] + margin
}

/// Grows the stand of one tile: its own trees and those of its halo. Returns its own
/// trees, meshed, and the size of the stand.
fn grow_tile(
    instances: &[Instance],
    nearby: &[usize],
    (specs, individuals): (&[VariantSpec], &[Vec<Species>]),
    tile: &[f32; 4],
    settings: &CoupledSettings,
) -> Result<(Vec<CoupledTree>, usize), ForestError> {
    let members: Vec<(usize, bool)> = nearby
        .iter()
        .map(|&index| (index, &instances[index]))
        .filter(|(_, t)| within(tile, t.position[0], t.position[2], settings.halo))
        .map(|(index, t)| (index, within(tile, t.position[0], t.position[2], 0.0)))
        .collect();
    if !members.iter().any(|(_, own)| *own) {
        return Ok((Vec::new(), 0));
    }
    let params = |tree: &Instance| {
        let spec = &specs[tree.species as usize];
        GenParams::new(
            arbrume_gen::plant_seed(spec.seed, u32::from(tree.variant)),
            spec.age,
        )
        .with_scale(tree.scale)
        .with_quality(settings.quality)
    };
    let standing: Vec<Standing> = members
        .iter()
        .map(|&(index, _)| {
            let tree = &instances[index];
            Standing {
                species: &individuals[tree.species as usize][tree.variant as usize],
                params: params(tree).grow_params(),
                position: Vec3::from_array(tree.position),
            }
        })
        .collect();
    let graphs = grow_together(&standing, &Surroundings::none());
    let mut generator = Generator::new();
    let mut own = Vec::new();
    for (&(index, mine), graph) in members.iter().zip(graphs) {
        if !mine {
            continue;
        }
        let tree = &instances[index];
        let species = &individuals[tree.species as usize][tree.variant as usize];
        let tree_params = params(tree);
        let options = tree_params.mesh;
        let mut plant = Plant {
            params: tree_params,
            graph,
            ..Plant::default()
        };
        generator.remesh(&mut plant, species, &options)?;
        plant.stats = Stats::of(&plant.graph, &plant.mesh);
        own.push(CoupledTree {
            instance: index,
            plant,
        });
    }
    Ok((own, members.len()))
}
