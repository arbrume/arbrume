//! Stands of a config file: plants grown together so that they shade one another
//! (`Generator::generate_group`), written as one `.glb` per plant and, with
//! `stand_file` or `--stand-file`, as one more file holding the whole stand.
//!
//! Plant `i` is written as `stand_<i>_<label>_<seed>.glb`, in its own frame with its
//! base at the origin; its `extras` add `stand_index` and `position`. The whole stand is
//! `stand.glb`: a root node with one child node per plant, translated to its position.
//! A thumbnail of the whole stand, `stand.<format>`, is written when thumbnails are on.

use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::Arc;

use arbrume_core::mesh::TreeMesh;
use arbrume_core::species::Species;
use arbrume_export::{ExportOptions, GlbOptions, GlbPlant, write_glb_stand};
use arbrume_gen::{GENERATOR, GenParams, Generator, Junctions, Plant, Surroundings, metadata};
use arbrume_gen::{TextureCache, TextureOptions, species_with_overrides};
use arbrume_raster::{Renderer, ThumbnailOptions};
use glam::Vec3;
use serde_json::Value;

use crate::batch::ThumbFormat;
use crate::config::{Batch, StandFile};
use crate::error::CliError;
use crate::lods::{self, LodConfig};
use crate::scene::merge;
use crate::source::{resolve, sanitize};
use crate::textures;

/// Name of the file of the whole stand, without extension.
const STAND: &str = "stand";

/// One plant of a stand.
#[derive(Debug, Clone)]
pub struct StandPlant {
    /// Label in file names.
    pub label: String,
    /// The species.
    pub species: Species,
    /// Its parameters; the surroundings are those of the stand.
    pub params: GenParams,
    /// Where it stands, in meters.
    pub position: [f32; 3],
}

/// Resolves the `[[stand]]` entries of a config file in `base`, meshed at `quality`, in
/// `surroundings`.
pub fn load_stand(
    files: &[StandFile],
    base: &Path,
    quality: f32,
    surroundings: Option<&Arc<Surroundings>>,
) -> Result<Vec<StandPlant>, CliError> {
    files
        .iter()
        .enumerate()
        .map(|(index, file)| {
            let fail = |e: &dyn std::fmt::Display| CliError::usage(format!("stand {index}: {e}"));
            let source = resolve(&file.species, base)?;
            let mut species =
                species_with_overrides(&source.text, &toml::Table::new()).map_err(|e| fail(&e))?;
            species.asset_dir.clone_from(&source.asset_dir);
            let mut params = GenParams::new(file.seed, file.age)
                .with_scale(file.scale.unwrap_or(1.0))
                .with_quality(quality);
            params.surroundings = surroundings.cloned();
            params.validate().map_err(|e| fail(&e))?;
            if !file.position.iter().all(|value| value.is_finite()) {
                return Err(fail(&"the position is not finite"));
            }
            Ok(StandPlant {
                label: file.name.as_deref().map_or(source.label, sanitize),
                species,
                params,
                position: file.position,
            })
        })
        .collect()
}

/// Grows the stand of `batch` and writes its files to `out`.
pub fn write(batch: &Batch, out: &Path, quiet: bool) -> Result<(), CliError> {
    let mut generator = Generator::new();
    if batch
        .textures
        .is_some_and(textures::TextureChoice::textured)
    {
        let cache = TextureCache::new(TextureOptions::default());
        generator.set_textures(Some(Arc::new(cache)));
    }
    let junctions = batch.junctions.map_or(Junctions::TierA, Junctions::from);
    let entries: Vec<(&Species, GenParams, [f32; 3])> = batch
        .stand
        .iter()
        .map(|plant| {
            let mut params = plant.params.clone();
            params.mesh.junctions = junctions;
            (&plant.species, params, plant.position)
        })
        .collect();
    let plants = generator
        .generate_group(&entries)
        .map_err(|e| CliError::failed(format!("stand: {e}")))?;
    let config = LodConfig {
        images: batch.lod.images,
        ..LodConfig::default()
    };
    let mut members = Vec::new();
    for (index, (plant, stand)) in plants.iter().zip(&batch.stand).enumerate() {
        let name = format!("{STAND}_{index}_{}_{}", stand.label, stand.params.seed);
        let extras = plant_extras(stand, plant, index);
        let options = glb_options(&name, Some(extras.clone()));
        write_file(out, &format!("{name}.glb"), |writer| {
            lods::write_mesh(&plant.mesh, &options, &[], config, writer).map_err(|e| e.to_string())
        })?;
        members.push(GlbPlant {
            mesh: &plant.mesh,
            name,
            translation: stand.position,
            extras: Some(extras),
        });
    }
    if config.images == arbrume_export::ImageStorage::External {
        textures::write_images(out, plants.iter().map(|plant| &plant.mesh))
            .map_err(CliError::failed)?;
    }
    if batch.stand_file {
        let extras = serde_json::json!({ "generator": "arbrume", "version": arbrume_gen::VERSION, "plants": members.len() });
        let options = glb_options(STAND, Some(extras));
        let export = ExportOptions {
            images: config.images,
            ..ExportOptions::default()
        };
        write_file(out, &format!("{STAND}.glb"), |writer| {
            write_glb_stand(&members, &options, &export, writer).map_err(|e| e.to_string())
        })?;
    }
    if let Some(thumb) = batch.thumb {
        let mesh = placed(
            plants
                .iter()
                .zip(&batch.stand)
                .map(|(p, s)| (p, s.position)),
        );
        write_thumb(out, &mesh, thumb.size, thumb.format)?;
    }
    if !quiet {
        eprintln!(
            "stand of {} plants written to {}",
            plants.len(),
            out.display()
        );
    }
    Ok(())
}

/// The meshes of `plants` merged, each moved to its position.
pub fn placed<'a>(plants: impl Iterator<Item = (&'a Plant, [f32; 3])>) -> TreeMesh {
    let mut mesh = TreeMesh::default();
    for (plant, position) in plants {
        merge(&mut mesh, &plant.mesh, Vec3::from_array(position));
    }
    mesh
}

/// Options of a file named `name`.
fn glb_options(name: &str, extras: Option<Value>) -> GlbOptions {
    GlbOptions {
        name: name.to_owned(),
        generator: GENERATOR.to_owned(),
        extras,
    }
}

/// The metadata of plant `index` of the stand.
fn plant_extras(stand: &StandPlant, plant: &Plant, index: usize) -> Value {
    let mut extras = metadata(&stand.species, plant);
    if let Value::Object(map) = &mut extras {
        map.insert("label".to_owned(), stand.label.clone().into());
        map.insert("stand_index".to_owned(), index.into());
        map.insert("position".to_owned(), stand.position.to_vec().into());
    }
    extras
}

/// Writes `out/name` with `body` through a temporary file.
fn write_file(
    out: &Path,
    name: &str,
    body: impl FnOnce(&mut BufWriter<File>) -> Result<(), String>,
) -> Result<(), CliError> {
    let fail = |e: &dyn std::fmt::Display| CliError::failed(format!("{name}: {e}"));
    let temp = out.join(format!("{name}.tmp"));
    let file = File::create(&temp).map_err(|e| fail(&e))?;
    let mut writer = BufWriter::with_capacity(1 << 16, file);
    body(&mut writer).map_err(|e| fail(&e))?;
    writer.flush().map_err(|e| fail(&e))?;
    drop(writer);
    fs::rename(&temp, out.join(name)).map_err(|e| fail(&e))
}

/// Renders `mesh` and writes it as `out/stand.<format>`.
fn write_thumb(
    out: &Path,
    mesh: &TreeMesh,
    size: u32,
    format: ThumbFormat,
) -> Result<(), CliError> {
    let options = ThumbnailOptions {
        width: size,
        height: size,
        azimuth: 20.0,
        ..ThumbnailOptions::default()
    };
    let image = Renderer::new().thumbnail(mesh, &options);
    let bytes = match format {
        ThumbFormat::Jpg => image.to_jpeg(88, [255; 3]),
        ThumbFormat::Png => image.to_png(),
    }
    .map_err(|e| CliError::failed(e.to_string()))?;
    let name = format!("{STAND}.{}", format.extension());
    write_file(out, &name, |writer| {
        writer.write_all(&bytes).map_err(|e| e.to_string())
    })
}

/// The plants of the stand of the config file at `path`, grown together and alone.
fn grown(path: &Path) -> Result<(Batch, Vec<Plant>, Vec<Plant>), CliError> {
    let batch = crate::config::load(path)?;
    if batch.stand.is_empty() {
        return Err(CliError::usage(format!(
            "{}: no [[stand]] to show",
            path.display()
        )));
    }
    let fail = |e: arbrume_gen::GenError| CliError::failed(format!("stand: {e}"));
    let mut generator = Generator::new();
    let entries: Vec<(&Species, GenParams, [f32; 3])> = batch
        .stand
        .iter()
        .map(|plant| (&plant.species, plant.params.clone(), plant.position))
        .collect();
    let together = generator.generate_group(&entries).map_err(fail)?;
    let alone = batch
        .stand
        .iter()
        .map(|plant| generator.generate(&plant.species, &plant.params))
        .collect::<Result<Vec<_>, _>>()
        .map_err(fail)?;
    Ok((batch, together, alone))
}

/// One line comparing plant `index` grown in the stand with the same plant grown alone:
/// height, crown width, and how far the centre of its nodes moved, horizontally.
fn compare(stand: &StandPlant, together: &Plant, alone: &Plant, index: usize) -> String {
    let centre = |plant: &Plant| {
        let positions = plant.graph.positions();
        positions.iter().copied().sum::<Vec3>() / positions.len().max(1) as f32
    };
    let width = |plant: &Plant| {
        let size = plant.stats.bounds.map_or([0.0; 3], |bounds| bounds.size());
        size[0].max(size[2])
    };
    let shift = centre(together) - centre(alone);
    format!(
        "stand {index} {} at {:?}: height {:.2} m (alone {:.2}), width {:.2} m (alone {:.2}), nodes {} (alone {}), centre moved by ({:.2}, {:.2}) m",
        stand.label,
        stand.position,
        together.stats.height,
        alone.stats.height,
        width(together),
        width(alone),
        together.stats.nodes,
        alone.stats.nodes,
        shift.x,
        shift.z,
    )
}

/// For `arbrume sheet --stand`: per azimuth, a thumbnail of the stand of the config file
/// at `path` and one of the same plants grown alone, placed at the same positions and
/// framed alike, then one comparison line per plant.
pub fn sheet_tiles(
    path: &Path,
    size: u32,
    azimuths: &[f32],
) -> Result<(Vec<arbrume_raster::Image>, Vec<String>), CliError> {
    let (batch, together, alone) = grown(path)?;
    let positions = batch.stand.iter().map(|plant| plant.position);
    let together_mesh = placed(together.iter().zip(positions.clone()));
    let alone_mesh = placed(alone.iter().zip(positions));
    let mut both = together_mesh.clone();
    merge(&mut both, &alone_mesh, Vec3::ZERO);
    let mut renderer = Renderer::new();
    let mut tiles = Vec::new();
    for &azimuth in azimuths {
        let options = ThumbnailOptions {
            width: size,
            height: size,
            azimuth,
            ..ThumbnailOptions::default()
        };
        let scene = options.scene(&both);
        for mesh in [&together_mesh, &alone_mesh] {
            tiles.push(renderer.render(mesh, &scene).to_image());
        }
    }
    let lines = batch
        .stand
        .iter()
        .zip(together.iter().zip(&alone))
        .enumerate()
        .map(|(index, (stand, (together, alone)))| compare(stand, together, alone, index))
        .collect();
    Ok((tiles, lines))
}
