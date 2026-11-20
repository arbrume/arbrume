//! Writing a forest: variant files, the instanced file, instance tables, the coupled
//! stand and a JSON manifest.
//!
//! In `out_dir`:
//!
//! * `<species>_v<k>.glb`: variant `k`, one child node per level `<species>_v<k>_LOD<i>`
//!   (the billboard last), with the `extras` of `arbrume gen --lod-file`;
//! * `<name>_instanced.glb`: one group node per level, `<name>_LOD<i>`, holding one node
//!   per variant that instances its level-`i` mesh at every tree of that variant
//!   (`EXT_mesh_gpu_instancing`, per-tree colour in `_COLOR_0`);
//! * `<name>_instances.<csv|bin|json>`: the tables of [`crate::table`]; a coupled tree
//!   has the variant `65535` and is the node `coupled_<index>` of the coupled file;
//! * `<name>_coupled.glb`: the trees of the coupled region, each a node translated to
//!   where it stands;
//! * `<name>.json`: the manifest (species, rules, variants and their files and levels,
//!   tables, counts and timings).

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use arbrume_export::{
    ExportOptions, ForestGroup, ForestMesh, GlbLevel, GlbOptions, GlbPlant, InstanceTransform,
    write_glb_forest, write_glb_levels, write_glb_stand,
};
use arbrume_gen::{GENERATOR, LodKind, LodMesh};
use serde_json::{Value, json};

use crate::config::OutputConfig;
use crate::error::ForestError;
use crate::forest::Forest;
use crate::table::write_table;

/// Variant value of a coupled tree in the tables.
pub const COUPLED_VARIANT: u16 = u16::MAX;

/// One file written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Written {
    /// Its path.
    pub path: PathBuf,
    /// Its size in bytes.
    pub bytes: u64,
}

/// The `extras` of one level.
fn level_extras(index: usize, level: &LodMesh) -> Value {
    json!({
        "lod": index,
        "lod_kind": match level.kind {
            LodKind::Mesh => "mesh",
            LodKind::Billboard => "billboard",
        },
        "lod_triangles": level.triangles,
        "max_screen_height": level.max_screen_height,
    })
}

/// File stem of variant `k` of species `name`.
fn variant_stem(name: &str, k: usize) -> String {
    let clean: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!("{clean}_v{k}")
}

/// Writes `forest` into `out_dir` as `output` asks; returns the files written.
///
/// # Errors
/// [`ForestError::Io`] and [`ForestError::Glb`] when a file cannot be written.
pub fn write_forest(
    forest: &Forest,
    output: &OutputConfig,
    export: &ExportOptions,
    out_dir: &Path,
) -> Result<Vec<Written>, ForestError> {
    std::fs::create_dir_all(out_dir)?;
    let mut written = Vec::new();
    if output.variants {
        for (s, species) in forest.library.iter().enumerate() {
            for k in 0..species.variants.len() {
                record(&mut written, write_variant(forest, s, k, export, out_dir)?)?;
            }
        }
    }
    if output.instanced && !forest.library.is_empty() {
        record(
            &mut written,
            write_instanced(forest, output.levels.as_deref(), export, out_dir)?,
        )?;
    }
    let tables = table_rows(forest);
    for format in &output.tables {
        let path = out_dir.join(format!("{}_instances.{format}", forest.name));
        write_table(&tables, &path)?;
        record(&mut written, path)?;
    }
    if let Some(coupled) = &forest.coupled
        && !coupled.trees.is_empty()
    {
        record(&mut written, write_coupled(forest, export, out_dir)?)?;
    }
    let manifest_path = out_dir.join(format!("{}.json", forest.name));
    let files: Vec<Value> = written
        .iter()
        .map(|w| json!({ "file": file_name(&w.path), "bytes": w.bytes }))
        .collect();
    let manifest = manifest(forest, output, &files);
    let mut out = BufWriter::new(File::create(&manifest_path)?);
    serde_json::to_writer_pretty(&mut out, &manifest).map_err(std::io::Error::other)?;
    out.flush()?;
    record(&mut written, manifest_path)?;
    Ok(written)
}

/// Adds `path` and its size to `written`.
fn record(written: &mut Vec<Written>, path: PathBuf) -> Result<(), ForestError> {
    let bytes = std::fs::metadata(&path)?.len();
    written.push(Written { path, bytes });
    Ok(())
}

/// The file name of `path`.
fn file_name(path: &Path) -> String {
    path.file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
}

/// The instances with coupled trees marked by [`COUPLED_VARIANT`].
fn table_rows(forest: &Forest) -> Vec<crate::place::Instance> {
    let mask = forest.coupled_mask();
    forest
        .instances
        .iter()
        .zip(mask)
        .map(|(tree, coupled)| {
            let mut tree = *tree;
            if coupled {
                tree.variant = COUPLED_VARIANT;
            }
            tree
        })
        .collect()
}

/// Writes variant `k` of species `s` with its levels.
fn write_variant(
    forest: &Forest,
    s: usize,
    k: usize,
    export: &ExportOptions,
    out_dir: &Path,
) -> Result<PathBuf, ForestError> {
    let species = &forest.library[s];
    let variant = &species.variants[k];
    let stem = variant_stem(&species.name, k);
    let options = GlbOptions {
        name: stem.clone(),
        generator: GENERATOR.to_owned(),
        extras: Some(json!({
            "species": species.species.name,
            "forest_species": species.name,
            "variant": k,
            "seed": variant.seed,
            "age": forest.specs[s].age,
        })),
    };
    let levels: Vec<GlbLevel> = variant
        .levels
        .iter()
        .enumerate()
        .map(|(index, level)| GlbLevel {
            mesh: &level.mesh,
            extras: Some(level_extras(index, level)),
            extra: &[],
            leaves: None,
        })
        .collect();
    let path = out_dir.join(format!("{stem}.glb"));
    let file = BufWriter::with_capacity(1 << 20, File::create(&path)?);
    write_glb_levels(&levels, &options, export, file)?;
    Ok(path)
}

/// Transforms and colours of the trees of each variant, by species then variant, the
/// coupled trees left out.
type VariantInstances = Vec<Vec<(Vec<InstanceTransform>, Vec<[f32; 3]>)>>;

/// The instances of every variant.
fn per_variant(forest: &Forest) -> VariantInstances {
    let mut groups: VariantInstances = forest
        .library
        .iter()
        .map(|s| vec![(Vec::new(), Vec::new()); s.variants.len()])
        .collect();
    for (tree, coupled) in forest.instances.iter().zip(forest.coupled_mask()) {
        if coupled {
            continue;
        }
        let (transforms, colors) = &mut groups[tree.species as usize][tree.variant as usize];
        transforms.push(InstanceTransform {
            translation: tree.position,
            rotation: tree.rotation,
            scale: tree.scale,
            variant: u32::from(tree.variant),
        });
        let [r, g, b, _] = tree.tint_factor();
        colors.push([r, g, b]);
    }
    groups
}

/// Writes the instanced file.
fn write_instanced(
    forest: &Forest,
    levels: Option<&[usize]>,
    export: &ExportOptions,
    out_dir: &Path,
) -> Result<PathBuf, ForestError> {
    let groups = per_variant(forest);
    let deepest = forest
        .library
        .iter()
        .flat_map(|s| s.variants.iter().map(|v| v.levels.len()))
        .max()
        .unwrap_or(0);
    let chosen: Vec<usize> = levels.map_or_else(|| (0..deepest).collect(), <[usize]>::to_vec);
    let mut forest_groups = Vec::new();
    for &level in &chosen {
        let mut meshes = Vec::new();
        for (s, species) in forest.library.iter().enumerate() {
            for (k, variant) in species.variants.iter().enumerate() {
                let Some(lod) = variant.levels.get(level) else {
                    continue;
                };
                let (transforms, colors) = &groups[s][k];
                meshes.push(ForestMesh {
                    mesh: &lod.mesh,
                    name: format!("{}_LOD{level}", variant_stem(&species.name, k)),
                    instances: transforms,
                    colors: Some(colors),
                    extras: Some(
                        json!({ "species": s, "variant": k, "lod": level_extras(level, lod) }),
                    ),
                });
            }
        }
        forest_groups.push(ForestGroup {
            name: format!("{}_LOD{level}", forest.name),
            extras: Some(json!({ "lod": level })),
            meshes,
        });
    }
    let options = GlbOptions {
        name: forest.name.clone(),
        generator: GENERATOR.to_owned(),
        extras: Some(
            json!({ "forest": forest.name, "seed": forest.seed, "trees": forest.instances.len() }),
        ),
    };
    let path = out_dir.join(format!("{}_instanced.glb", forest.name));
    let file = BufWriter::with_capacity(1 << 20, File::create(&path)?);
    write_glb_forest(&forest_groups, &options, export, file)?;
    Ok(path)
}

/// Writes the coupled trees as one stand file.
fn write_coupled(
    forest: &Forest,
    export: &ExportOptions,
    out_dir: &Path,
) -> Result<PathBuf, ForestError> {
    let Some(coupled) = &forest.coupled else {
        return Err(ForestError::Config("no coupled region".to_owned()));
    };
    let plants: Vec<GlbPlant> = coupled
        .trees
        .iter()
        .map(|tree| {
            let instance = &forest.instances[tree.instance];
            GlbPlant {
                mesh: &tree.plant.mesh,
                name: format!("coupled_{}", tree.instance),
                translation: instance.position,
                extras: Some(json!({ "instance": tree.instance, "species": instance.species })),
            }
        })
        .collect();
    let options = GlbOptions {
        name: format!("{}_coupled", forest.name),
        generator: GENERATOR.to_owned(),
        extras: None,
    };
    let path = out_dir.join(format!("{}_coupled.glb", forest.name));
    let file = BufWriter::with_capacity(1 << 20, File::create(&path)?);
    write_glb_stand(&plants, &options, export, file)?;
    Ok(path)
}

/// The manifest of `forest`.
fn manifest(forest: &Forest, output: &OutputConfig, files: &[Value]) -> Value {
    let mut counts = vec![0usize; forest.rules.len()];
    for tree in &forest.instances {
        counts[tree.species as usize] += 1;
    }
    let species: Vec<Value> = forest
        .rules
        .iter()
        .enumerate()
        .map(|(s, rule)| {
            let variants: Vec<Value> = forest.library.get(s).map_or_else(Vec::new, |lib| {
                lib.variants
                    .iter()
                    .enumerate()
                    .map(|(k, v)| {
                        let levels: Vec<Value> =
                            v.levels.iter().enumerate().map(|(i, l)| level_extras(i, l)).collect();
                        json!({
                            "file": output.variants.then(|| format!("{}.glb", variant_stem(&lib.name, k))),
                            "seed": v.seed,
                            "height": v.height,
                            "crown_radius": v.crown_radius,
                            "trunk_radius": v.trunk_radius,
                            "levels": levels,
                            "seconds": v.time.as_secs_f64(),
                        })
                    })
                    .collect()
            });
            json!({
                "name": forest.specs[s].name,
                "species": forest.specs[s].species.name,
                "age": forest.specs[s].age,
                "trees": counts[s],
                "rule": {
                    "density": rule.density, "spacing": rule.spacing, "scale": rule.scale,
                    "slope": rule.slope, "slope_falloff": rule.slope_falloff,
                    "altitude": [rule.altitude[0].max(-1e30), rule.altitude[1].min(1e30)],
                    "altitude_falloff": rule.altitude_falloff, "lean": rule.lean_deg,
                    "tint": rule.tint, "trunk_radius": rule.trunk_radius, "sink": rule.sink,
                },
                "placement": {
                    "candidates": forest.placement.species[s].candidates,
                    "expected_open": forest.placement.species[s].expected_open,
                    "saturated_cells": forest.placement.species[s].saturated_cells,
                },
                "variants": variants,
            })
        })
        .collect();
    let terrain = &forest.terrain;
    json!({
        "generator": GENERATOR,
        "name": forest.name,
        "seed": forest.seed,
        "trees": forest.instances.len(),
        "terrain": {
            "origin": terrain.origin(), "size": terrain.size(),
            "samples": terrain.samples(), "height_range": terrain.height_range(),
        },
        "species": species,
        "placement": {
            "cell": forest.placement.cell, "cells": forest.placement.cells,
            "bands": forest.placement.bands, "peak_candidates": forest.placement.peak_candidates,
            "seconds": forest.placement.time.as_secs_f64(),
        },
        "library_seconds": forest.library_time.as_secs_f64(),
        "coupled": forest.coupled.as_ref().map(|c| json!({
            "trees": c.trees.iter().map(|t| t.instance).collect::<Vec<_>>(),
            "tiles": c.tiles, "largest_stand": c.largest_stand, "halo_trees": c.halo_trees,
            "seconds": c.time.as_secs_f64(),
        })),
        "table_record": "position f32x3, rotation f32x4 (x y z w), scale f32, species u16, variant u16 (65535: coupled), tint u8x4 (/128)",
        "files": files,
    })
}
