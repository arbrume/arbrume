//! Levels of detail, wind and billboards of `arbrume gen`.
//!
//! With `--lods N` above 1, `--wind` or `--billboard`, a plant is written as the chain of
//! `arbrume_gen::Generator::lod_chain`: `N` mesh levels within geometric triangle
//! budgets, then the billboard when asked, each level in its own file (or all in one
//! with `--lod-file`), with its budget, triangles and screen-size hint in the metadata.

use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::Path;

use arbrume_core::mesh::{Material, MeshPart, PartKind, TreeMesh};
use arbrume_core::species::Species;
use arbrume_export::{
    ExportOptions, ExtraAttribute, GlbError, GlbLevel, GlbOptions, InstanceTransform,
    LeafInstancing, SkinOptions, WindLayout, write_glb_levels, write_glb_with,
};
use arbrume_gen::{
    BillboardMethod, BillboardOptions, GENERATOR, Generator, LeafInstances, LodKind, LodMesh,
    LodOptions, LodSettings, Plant,
};
use clap::{Args, ValueEnum};
use glam::{Mat3, Quat, Vec3};
use serde::Deserialize;
use serde_json::{Value, json};

/// Layout of the wind data in the files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WindChoice {
    /// `TEXCOORD_1` to `TEXCOORD_3`.
    Texcoord,
    /// `_WIND_X` and `_WIND_DATA`.
    Custom,
}

/// How the billboard planes are chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BillboardChoice {
    /// Billboard clouds.
    Cloud,
    /// Crossed vertical quads.
    Cross,
}

/// The `lod` table of a batch config file.
#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LodFile {
    wind: Option<WindChoice>,
    billboard: Option<BillboardChoice>,
    #[serde(default)]
    file: bool,
    #[serde(default)]
    skin: bool,
    #[serde(default)]
    instance_leaves: bool,
}

impl LodFile {
    /// The settings of the table.
    pub fn config(self) -> LodConfig {
        LodConfig {
            wind: self.wind,
            billboard: self.billboard,
            single_file: self.file,
            skin: self.skin && self.wind.is_some(),
            instance_leaves: self.instance_leaves && self.file,
        }
    }
}

/// The level-of-detail flags of `arbrume gen`.
#[derive(Debug, Clone, Copy, Args)]
pub struct LodArgs {
    /// Bake wind data into the files, in this layout (docs/wind.md).
    #[arg(long, value_enum, value_name = "LAYOUT", num_args = 0..=1, default_missing_value = "texcoord")]
    wind: Option<WindChoice>,
    /// Add a billboard as the last level of detail, made this way (docs/lod.md).
    #[arg(long, value_enum, value_name = "METHOD", num_args = 0..=1, default_missing_value = "cloud")]
    billboard: Option<BillboardChoice>,
    /// Write all levels of detail of a plant in one file, one node per level.
    #[arg(long)]
    lod_file: bool,
    /// Add a skeleton with skin weights following the wind data (needs --wind).
    #[arg(long, requires = "wind")]
    skin: bool,
    /// Write the leaves as GPU instances of their templates (`EXT_mesh_gpu_instancing`),
    /// in the one file of --lod-file.
    #[arg(long, requires = "lod_file")]
    instance_leaves: bool,
}

impl LodArgs {
    /// `config` with the flags given on the command line set.
    pub fn merge(self, config: LodConfig) -> LodConfig {
        LodConfig {
            wind: self.wind.or(config.wind),
            billboard: self.billboard.or(config.billboard),
            single_file: self.lod_file || config.single_file,
            skin: self.skin || config.skin,
            instance_leaves: self.instance_leaves || config.instance_leaves,
        }
    }
}

/// The level-of-detail settings of a batch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LodConfig {
    /// Wind data, when asked.
    pub wind: Option<WindChoice>,
    /// A billboard as the last level, when asked.
    pub billboard: Option<BillboardChoice>,
    /// All levels in one file.
    pub single_file: bool,
    /// A skeleton with skin weights (needs wind data).
    pub skin: bool,
    /// Leaves as GPU instances, in the one file of `single_file`.
    pub instance_leaves: bool,
}

impl LodConfig {
    /// The chain settings, `None` when the plant is written as before: one level, no
    /// wind, no billboard.
    fn settings(self, lods: u32) -> Option<LodSettings> {
        if lods <= 1 && self.wind.is_none() && self.billboard.is_none() {
            return None;
        }
        let billboard = self.billboard.map(|choice| BillboardOptions {
            method: match choice {
                BillboardChoice::Cloud => BillboardMethod::Cloud,
                BillboardChoice::Cross => BillboardMethod::Cross,
            },
            ..BillboardOptions::default()
        });
        Some(LodSettings {
            lod: LodOptions {
                levels: lods.max(1) as usize,
                ..LodOptions::default()
            },
            billboard,
            billboard_source: 1,
            wind: self.wind.is_some(),
            leaf_instances: self.instance_leaves && self.single_file,
        })
    }

    /// What the writer adds to the files.
    pub fn export(self) -> ExportOptions {
        ExportOptions {
            wind: match self.wind {
                None | Some(WindChoice::Texcoord) => WindLayout::TexCoord,
                Some(WindChoice::Custom) => WindLayout::Custom,
            },
            skin: (self.skin && self.wind.is_some()).then(SkinOptions::default),
        }
    }
}

/// The chain of `plant`, `None` when `config` and `lods` ask for the plain path.
pub fn chain(
    generator: &mut Generator,
    plant: &Plant,
    species: &Species,
    lods: u32,
    config: LodConfig,
) -> Result<Option<Vec<LodMesh>>, String> {
    let Some(settings) = config.settings(lods) else {
        return Ok(None);
    };
    generator
        .lod_chain(plant, species, &settings)
        .map(Some)
        .map_err(|e| e.to_string())
}

/// Makes `level` the current mesh of `plant`.
pub fn install(plant: &mut Plant, level: &LodMesh) {
    plant.mesh.clone_from(&level.mesh);
    plant.params.mesh = level.options;
    plant.stats.set_mesh(&plant.mesh);
}

/// The metadata keys of one level, added to the `extras` of its file or node.
pub fn level_extras(index: usize, level: &LodMesh) -> Value {
    json!({
        "lod": index,
        "lod_kind": match level.kind {
            LodKind::Mesh => "mesh",
            LodKind::Billboard => "billboard",
        },
        "lod_budget": level.budget,
        "lod_triangles": level.triangles,
        "max_screen_height": level.max_screen_height,
        "quality": level.quality,
    })
}

/// Writes `mesh` with the export options of `config`: the plain one-node file when they
/// are the defaults, one level otherwise.
pub fn write_mesh(
    mesh: &TreeMesh,
    options: &GlbOptions,
    extra: &[ExtraAttribute],
    config: LodConfig,
    out: impl Write,
) -> Result<(), GlbError> {
    let export = config.export();
    if export == ExportOptions::default() {
        return write_glb_with(mesh, options, extra, out);
    }
    let level = GlbLevel {
        mesh,
        extras: None,
        extra,
        leaves: None,
    };
    write_glb_levels(&[level], options, &export, out)
}

/// Writes every level of `chain` in one file.
pub fn write_chain(
    chain: &[LodMesh],
    options: &GlbOptions,
    config: LodConfig,
    out: impl Write,
) -> Result<(), GlbError> {
    let instanced: Vec<Option<Instanced>> = chain
        .iter()
        .map(|level| level.leaves.as_ref().map(|l| instanced(l, &level.mesh)))
        .collect();
    let levels: Vec<GlbLevel> = chain
        .iter()
        .zip(&instanced)
        .enumerate()
        .map(|(index, (level, leaves))| GlbLevel {
            mesh: &level.mesh,
            extras: Some(level_extras(index, level)),
            extra: &[],
            leaves: leaves
                .as_ref()
                .map(|(templates, instances)| LeafInstancing {
                    templates,
                    instances,
                }),
        })
        .collect();
    write_glb_levels(&levels, options, &config.export(), out)
}

/// Templates as parts and the transform of every instance.
type Instanced = (Vec<MeshPart>, Vec<InstanceTransform>);

/// The instancing data of `leaves`, the foliage of `mesh`, whose leaf material the
/// templates take.
fn instanced(leaves: &LeafInstances, mesh: &TreeMesh) -> Instanced {
    let material = mesh
        .parts
        .iter()
        .find(|part| part.kind == PartKind::Leaf)
        .map(|part| part.material.clone());
    let templates = leaves
        .templates
        .iter()
        .map(|buffers| MeshPart {
            kind: PartKind::Leaf,
            material: material.clone().unwrap_or_else(|| Material {
                name: "leaf".to_owned(),
                base_color: [
                    leaves.base_color[0],
                    leaves.base_color[1],
                    leaves.base_color[2],
                    1.0,
                ],
                roughness: 0.6,
                double_sided: true,
                alpha_cutoff: None,
                base_color_texture: None,
                normal_texture: None,
            }),
            buffers: buffers.clone(),
        })
        .collect();
    let instances = leaves
        .instances
        .iter()
        .map(|leaf| {
            let [forward, across, up] = leaf.axes.map(Vec3::from);
            let rotation = Quat::from_mat3(&Mat3::from_cols(forward, across, up)).normalize();
            InstanceTransform {
                translation: leaf.anchor,
                rotation: rotation.to_array(),
                scale: leaf.size,
                variant: leaf.variant,
            }
        })
        .collect();
    (templates, instances)
}

/// Number of files of a plant: one per level, the billboard included, or one.
pub fn file_count(lods: u32, config: LodConfig) -> u32 {
    if config.single_file && config.settings(lods).is_some() {
        1
    } else {
        lods.max(1) + u32::from(config.billboard.is_some())
    }
}

/// Writes every level of `chain` to `out/name` through a temporary file, the root node
/// carrying `extras`; returns the file size.
pub fn write_chain_file(
    out: &Path,
    name: &str,
    extras: Value,
    chain: &[LodMesh],
    config: LodConfig,
) -> Result<u64, String> {
    let options = GlbOptions {
        name: name.trim_end_matches(".glb").to_owned(),
        generator: GENERATOR.to_owned(),
        extras: Some(extras),
    };
    let path = out.join(name);
    let temp = out.join(format!("{name}.tmp"));
    let file = File::create(&temp).map_err(|e| format!("{}: {e}", temp.display()))?;
    let mut writer = BufWriter::with_capacity(1 << 16, file);
    write_chain(chain, &options, config, &mut writer).map_err(|e| e.to_string())?;
    let file = writer.into_inner().map_err(|e| e.error().to_string())?;
    let bytes = file.metadata().map_err(|e| e.to_string())?.len();
    drop(file);
    fs::rename(&temp, &path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(bytes)
}
