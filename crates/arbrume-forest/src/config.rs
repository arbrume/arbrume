//! The forest config file (`forest.toml`) and its resolution into terrain, exclusions,
//! placement rules and variant specs. `docs/forest.md` documents every key.

use std::path::{Path, PathBuf};

use arbrume_core::species::Species;
use arbrume_gen::{
    BillboardMethod, BillboardOptions, LodOptions, LodSettings, preset_toml, presets,
};
use arbrume_pack::{PackStore, split_species_id};
use serde::Deserialize;

use crate::error::ForestError;
use crate::exclude::{Exclusion, Exclusions, RasterMask, Shape};
use crate::library::{LibrarySettings, VariantSpec};
use crate::place::{Mode, PlaceSettings};
use crate::rules::SpeciesRule;
use crate::terrain::{Hills, Terrain};

/// A whole forest config.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ForestConfig {
    /// Name of the forest, used in file names.
    #[serde(default = "default_name")]
    pub name: String,
    /// Seed of the placement and of the variants.
    #[serde(default)]
    pub seed: u64,
    /// The ground.
    pub terrain: TerrainConfig,
    /// How trees are spread.
    #[serde(default)]
    pub placement: PlacementConfig,
    /// The species mix.
    pub species: Vec<SpeciesConfig>,
    /// Clearings, roads, rivers and masks.
    #[serde(default)]
    pub exclude: Vec<ExcludeConfig>,
    /// The variant library.
    #[serde(default)]
    pub library: LibraryConfig,
    /// What is written.
    #[serde(default)]
    pub output: OutputConfig,
    /// A region grown as one stand, with shared shade.
    #[serde(default)]
    pub coupled: Option<CoupledConfig>,
}

fn default_name() -> String {
    "forest".to_owned()
}

/// `[terrain]`: a flat ground, a heightmap, a grid of heights or procedural hills.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerrainConfig {
    /// Extent along `x` and `z`, in meters.
    pub size: [f32; 2],
    /// Smallest `x` and `z`; the rectangle is centred on the origin by default.
    pub origin: Option<[f32; 2]>,
    /// Height of a flat ground.
    #[serde(default)]
    pub height: f32,
    /// A grayscale PNG (8 or 16 bits), relative to the config file.
    pub heightmap: Option<PathBuf>,
    /// Heights of black and white in the heightmap, in meters.
    #[serde(default = "default_heights")]
    pub heights: [f32; 2],
    /// A grid of heights.
    pub grid: Option<GridConfig>,
    /// Procedural hills.
    pub hills: Option<HillsConfig>,
}

fn default_heights() -> [f32; 2] {
    [0.0, 100.0]
}

/// `[terrain.grid]`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GridConfig {
    /// Samples along `x`.
    pub columns: usize,
    /// Samples along `z`.
    pub rows: usize,
    /// Heights, row-major, rows along `z`.
    pub heights: Vec<f32>,
}

/// `[terrain.hills]`.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HillsConfig {
    /// Height range of the hills, in meters.
    pub amplitude: f32,
    /// Distance between hills, in meters.
    pub wavelength: f32,
    /// Layers of noise.
    #[serde(default = "default_octaves")]
    pub octaves: u32,
    /// Sample spacing, in meters.
    #[serde(default = "default_resolution")]
    pub resolution: f32,
    /// Seed of the noise.
    #[serde(default)]
    pub seed: u64,
}

const fn default_octaves() -> u32 {
    4
}

const fn default_resolution() -> f32 {
    2.0
}

/// `[placement]`.
#[derive(Debug, Clone, Copy, PartialEq, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlacementConfig {
    /// `"poisson"` (default) or `"competition"`.
    #[serde(default)]
    pub mode: ModeConfig,
    /// Ground below this height is water.
    pub water_level: Option<f32>,
    /// Cell rows per band (0: automatic).
    #[serde(default)]
    pub band_rows: usize,
}

/// Placement mode in the config.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModeConfig {
    /// [`Mode::Poisson`].
    #[default]
    Poisson,
    /// [`Mode::Competition`].
    Competition,
}

/// One `[[species]]` of the mix.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpeciesConfig {
    /// Name in file names and reports; the species name by default.
    pub name: Option<String>,
    /// A preset name, a pack species `pack_id/name`, a species file (relative to the
    /// config file) or the TOML text of a species.
    pub species: String,
    /// Trees per hectare on ground that suits the species.
    pub density: f32,
    /// Smallest trunk distance at the mean scale, in meters; by default
    /// `2 (1 - overlap)` times the mean crown radius of the variants at the mean scale.
    pub spacing: Option<f32>,
    /// Layer: species of different layers ignore each other (understory and canopy).
    #[serde(default)]
    pub layer: u8,
    /// Fraction of the crown diameter two neighbours may share, for the default spacing.
    #[serde(default = "default_overlap")]
    pub overlap: f32,
    /// Age of the variants; the oldest age of the species variation by default.
    pub age: Option<u16>,
    /// Number of variants.
    #[serde(default = "default_variants")]
    pub variants: u16,
    /// Scale range; that of the species variation by default.
    pub scale: Option<[f32; 2]>,
    /// Slopes taken fully, in degrees.
    #[serde(default = "default_slope")]
    pub slope: [f32; 2],
    /// Fade outside `slope`, in degrees.
    #[serde(default)]
    pub slope_falloff: f32,
    /// Ground heights taken fully, in meters.
    pub altitude: Option<[f32; 2]>,
    /// Fade outside `altitude`, in meters.
    #[serde(default)]
    pub altitude_falloff: f32,
    /// Largest lean from the vertical, in degrees.
    #[serde(default)]
    pub lean: f32,
    /// Half-width of the random colour factor.
    #[serde(default)]
    pub tint: f32,
    /// Extra depth of the base, in meters.
    #[serde(default)]
    pub sink: f32,
    /// Water depths taken fully by an aquatic species, in meters below
    /// `placement.water_level` (docs/underwater.md); any depth by default.
    pub depth: Option<[f32; 2]>,
    /// Fade outside `depth`, in meters.
    #[serde(default)]
    pub depth_falloff: f32,
    /// Scale each tree of an aquatic species so that its surface canopy meets the water
    /// surface over its own ground (true by default).
    pub fit_depth: Option<bool>,
}

const fn default_overlap() -> f32 {
    0.5
}

const fn default_variants() -> u16 {
    4
}

const fn default_slope() -> [f32; 2] {
    [0.0, 90.0]
}

/// One `[[exclude]]`: exactly one of `circle`, `path`, `polygon`, `mask` and `mask_grid`.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExcludeConfig {
    /// A clearing: `{ center = [x, z], radius = r }`.
    pub circle: Option<CircleConfig>,
    /// A road or river: `{ points = [[x, z], ...], width = w }`.
    pub path: Option<PathConfig>,
    /// An area: `{ points = [[x, z], ...] }`.
    pub polygon: Option<PolygonConfig>,
    /// A grayscale PNG over the terrain, white excluded, relative to the config file.
    pub mask: Option<PathBuf>,
    /// A mask given as values: `{ columns, rows, values }`, row-major cells covering the
    /// terrain rectangle (rows along `z`), 1 excluded, 0 free.
    pub mask_grid: Option<MaskGridConfig>,
    /// Soft edge outside the shape, in meters.
    #[serde(default)]
    pub falloff: f32,
    /// Depth a path lowers the terrain by, in meters.
    #[serde(default)]
    pub carve: f32,
}

/// A circle of an exclusion.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CircleConfig {
    /// Centre, `[x, z]`.
    pub center: [f32; 2],
    /// Radius, in meters.
    pub radius: f32,
}

/// A mask of an exclusion given as values.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaskGridConfig {
    /// Cells along `x`.
    pub columns: usize,
    /// Cells along `z`.
    pub rows: usize,
    /// Values in `[0, 1]`, row-major.
    pub values: Vec<f32>,
}

/// A path of an exclusion.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PathConfig {
    /// Polyline, `[x, z]` points.
    pub points: Vec<[f32; 2]>,
    /// Full width, in meters.
    pub width: f32,
}

/// A polygon of an exclusion.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolygonConfig {
    /// Corners, `[x, z]` points.
    pub points: Vec<[f32; 2]>,
}

/// `[library]`.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LibraryConfig {
    /// Mesh levels per variant.
    #[serde(default = "default_lods")]
    pub lods: u32,
    /// `"cloud"`, `"cross"` or `"none"`.
    #[serde(default)]
    pub billboard: BillboardConfig,
    /// Side of the billboard atlas, in texels.
    #[serde(default = "default_atlas")]
    pub atlas: u32,
    /// Textures on the variants.
    #[serde(default = "yes")]
    pub textures: bool,
    /// Wind data in the meshes.
    #[serde(default)]
    pub wind: bool,
    /// Quality of level 0.
    #[serde(default = "default_quality")]
    pub quality: f32,
}

impl Default for LibraryConfig {
    fn default() -> Self {
        Self {
            lods: default_lods(),
            billboard: BillboardConfig::default(),
            atlas: default_atlas(),
            textures: true,
            wind: false,
            quality: default_quality(),
        }
    }
}

const fn default_lods() -> u32 {
    3
}

const fn default_atlas() -> u32 {
    512
}

const fn yes() -> bool {
    true
}

const fn default_quality() -> f32 {
    1.0
}

/// Billboard choice of the library.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BillboardConfig {
    /// Billboard clouds.
    #[default]
    Cloud,
    /// Crossed quads.
    Cross,
    /// No billboard.
    None,
}

/// `[output]`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputConfig {
    /// One `.glb` per variant, its levels as child nodes.
    #[serde(default = "yes")]
    pub variants: bool,
    /// One `.glb` instancing every variant with `EXT_mesh_gpu_instancing`.
    #[serde(default = "yes")]
    pub instanced: bool,
    /// Levels of detail written to the instanced file (all by default).
    pub levels: Option<Vec<usize>>,
    /// Instance tables: `"csv"`, `"bin"`, `"json"`.
    #[serde(default = "default_tables")]
    pub tables: Vec<String>,
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            variants: true,
            instanced: true,
            levels: None,
            tables: default_tables(),
        }
    }
}

fn default_tables() -> Vec<String> {
    vec!["csv".to_owned(), "bin".to_owned()]
}

/// `[coupled]`: the trees of a region grown together in tiles.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoupledConfig {
    /// `[x0, z0, x1, z1]`.
    pub region: [f32; 4],
    /// Side of a tile, in meters.
    #[serde(default = "default_tile")]
    pub tile: f32,
    /// Neighbours this far outside a tile shade it, in meters.
    #[serde(default = "default_halo")]
    pub halo: f32,
    /// Mesh quality of the coupled trees.
    #[serde(default = "default_coupled_quality")]
    pub quality: f32,
}

const fn default_tile() -> f32 {
    40.0
}

const fn default_halo() -> f32 {
    10.0
}

const fn default_coupled_quality() -> f32 {
    0.5
}

impl ForestConfig {
    /// Parses a config from TOML text.
    ///
    /// # Errors
    /// [`ForestError::Config`] for malformed text or unknown keys.
    pub fn from_toml_str(text: &str) -> Result<Self, ForestError> {
        toml::from_str(text).map_err(|e| ForestError::Config(e.to_string()))
    }

    /// Reads a config file.
    ///
    /// # Errors
    /// [`ForestError::Config`] when it cannot be read or parsed.
    pub fn from_path(path: &Path) -> Result<Self, ForestError> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| ForestError::Config(format!("{}: {e}", path.display())))?;
        Self::from_toml_str(&text)
    }

    /// The terrain, carved by the exclusions that carve.
    ///
    /// # Errors
    /// [`ForestError::Terrain`] when it cannot be built.
    pub fn terrain(&self, base: &Path, exclusions: &Exclusions) -> Result<Terrain, ForestError> {
        let config = &self.terrain;
        let size = config.size;
        let origin = config.origin.unwrap_or([-size[0] / 2.0, -size[1] / 2.0]);
        let sources = [
            config.heightmap.is_some(),
            config.grid.is_some(),
            config.hills.is_some(),
        ];
        if sources.iter().filter(|s| **s).count() > 1 {
            return Err(ForestError::Terrain(
                "give one of heightmap, grid and hills".to_owned(),
            ));
        }
        let mut terrain = if let Some(path) = &config.heightmap {
            Terrain::from_png(&base.join(path), origin, size, config.heights)?
        } else if let Some(grid) = &config.grid {
            Terrain::from_grid(origin, size, grid.columns, grid.rows, grid.heights.clone())?
        } else if let Some(h) = config.hills {
            let hills = Hills {
                amplitude: h.amplitude,
                wavelength: h.wavelength,
                octaves: h.octaves,
                resolution: h.resolution,
                seed: h.seed,
            };
            Terrain::hills(origin, size, &hills)?
        } else {
            Terrain::from_grid(origin, size, 2, 2, vec![config.height; 4])?
        };
        exclusions.carve(&mut terrain);
        Ok(terrain)
    }

    /// The exclusions.
    ///
    /// # Errors
    /// [`ForestError::Exclusion`] for an entry with no shape or several, or an invalid one.
    pub fn exclusions(&self, base: &Path) -> Result<Exclusions, ForestError> {
        let mut shapes = Vec::new();
        let mut mask = None;
        for (index, entry) in self.exclude.iter().enumerate() {
            let given = [
                entry.circle.is_some(),
                entry.path.is_some(),
                entry.polygon.is_some(),
                entry.mask.is_some(),
                entry.mask_grid.is_some(),
            ];
            if given.iter().filter(|g| **g).count() != 1 {
                return Err(ForestError::Exclusion(format!(
                    "exclude {index}: give exactly one of circle, path, polygon, mask and mask_grid"
                )));
            }
            let shape = if let Some(c) = entry.circle {
                Shape::Circle {
                    center: c.center,
                    radius: c.radius,
                }
            } else if let Some(p) = &entry.path {
                Shape::Path {
                    points: p.points.clone(),
                    width: p.width,
                }
            } else if let Some(p) = &entry.polygon {
                Shape::Polygon {
                    points: p.points.clone(),
                }
            } else if let Some(grid) = &entry.mask_grid {
                mask = Some(RasterMask::new(
                    grid.columns,
                    grid.rows,
                    grid.values.clone(),
                )?);
                continue;
            } else {
                let path = entry
                    .mask
                    .as_ref()
                    .map(|m| base.join(m))
                    .unwrap_or_default();
                mask = Some(RasterMask::from_png(&path)?);
                continue;
            };
            shapes.push(Exclusion {
                shape,
                falloff: entry.falloff,
                carve: entry.carve,
            });
        }
        Exclusions::new(shapes, mask, self.placement.water_level)
    }

    /// The placement settings.
    #[must_use]
    pub fn place_settings(&self) -> PlaceSettings {
        PlaceSettings {
            seed: self.seed,
            mode: match self.placement.mode {
                ModeConfig::Poisson => Mode::Poisson,
                ModeConfig::Competition => Mode::Competition,
            },
            band_rows: self.placement.band_rows,
        }
    }

    /// The library settings.
    #[must_use]
    pub fn library_settings(&self) -> LibrarySettings {
        let library = &self.library;
        let billboard = match library.billboard {
            BillboardConfig::None => None,
            choice => Some(BillboardOptions {
                method: if choice == BillboardConfig::Cross {
                    BillboardMethod::Cross
                } else {
                    BillboardMethod::Cloud
                },
                atlas_size: library.atlas,
                ..BillboardOptions::default()
            }),
        };
        LibrarySettings {
            lod: LodSettings {
                lod: LodOptions {
                    levels: library.lods.max(1) as usize,
                    ..LodOptions::default()
                },
                billboard,
                billboard_source: 1,
                wind: library.wind,
                leaf_instances: false,
            },
            textures: library.textures,
            quality: library.quality,
        }
    }

    /// The species of the mix, loaded, with their names.
    ///
    /// # Errors
    /// [`ForestError::Config`] for a species that cannot be found or parsed.
    pub fn load_species(&self, base: &Path) -> Result<Vec<(String, Species)>, ForestError> {
        self.species
            .iter()
            .enumerate()
            .map(|(index, entry)| {
                let species = load_species(&entry.species, base)
                    .map_err(|e| ForestError::Config(format!("species {index}: {e}")))?;
                let name = entry
                    .name
                    .clone()
                    .unwrap_or_else(|| default_label(&entry.species, &species));
                Ok((name, species))
            })
            .collect()
    }

    /// The variant specs of the mix.
    #[must_use]
    pub fn variant_specs(&self, species: &[(String, Species)]) -> Vec<VariantSpec> {
        self.species
            .iter()
            .zip(species)
            .enumerate()
            .map(|(index, (entry, (name, species)))| VariantSpec {
                name: name.clone(),
                age: entry.age.unwrap_or(species.variation.age[1]),
                count: entry.variants.max(1),
                seed: arbrume_gen::plant_seed(self.seed, index as u32),
                species: species.clone(),
            })
            .collect()
    }

    /// The placement rule of species `index`, with the crown and trunk radii measured
    /// on its variants (`None` when the library is not built).
    ///
    /// # Errors
    /// [`ForestError::Config`] when the spacing is needed from the variants and there
    /// are none.
    pub fn rule(
        &self,
        index: usize,
        species: &Species,
        radii: Option<(f32, f32)>,
    ) -> Result<SpeciesRule, ForestError> {
        let entry = &self.species[index];
        let water = crate::underwater::placement(entry, species, self.placement.water_level)
            .map_err(|reason| ForestError::Config(format!("species {index}: {reason}")))?;
        let scale = entry.scale.unwrap_or(species.variation.scale);
        let mean_scale = f32::midpoint(scale[0], scale[1]);
        let spacing = match (entry.spacing, radii) {
            (Some(spacing), _) => spacing,
            (None, Some((crown, _))) => 2.0 * (1.0 - entry.overlap) * crown * mean_scale,
            (None, None) => {
                return Err(ForestError::Config(format!(
                    "species {index}: give `spacing` when the variants are not grown"
                )));
            }
        };
        Ok(SpeciesRule {
            density: entry.density,
            spacing,
            layer: entry.layer,
            scale,
            slope: entry.slope,
            slope_falloff: entry.slope_falloff,
            altitude: water
                .altitude
                .or(entry.altitude)
                .unwrap_or([f32::NEG_INFINITY, f32::INFINITY]),
            altitude_falloff: water.falloff.unwrap_or(entry.altitude_falloff),
            variants: entry.variants.max(1),
            lean_deg: entry.lean,
            tint: entry.tint,
            trunk_radius: radii.map_or(0.0, |(_, trunk)| trunk),
            sink: entry.sink,
            underwater: water.underwater,
        })
    }
}

/// The name of a species in file names when the config gives none: the preset name,
/// `pack_id-name` for a pack species, the file stem, or the species name in lower case.
fn default_label(spec: &str, species: &Species) -> String {
    let raw = if preset_toml(spec).is_some() || split_species_id(spec).is_some() {
        spec.replace('/', "-")
    } else if spec.contains('\n') || spec.trim_start().starts_with('[') {
        species.name.to_lowercase()
    } else {
        Path::new(spec).file_stem().map_or_else(
            || species.name.clone(),
            |s| s.to_string_lossy().into_owned(),
        )
    };
    raw.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Loads `spec`: a preset, a pack species, a file relative to `base`, or TOML text.
fn load_species(spec: &str, base: &Path) -> Result<Species, String> {
    if let Some(text) = preset_toml(spec) {
        return Species::from_toml_str(text).map_err(|e| e.to_string());
    }
    if spec.contains('\n') || spec.trim_start().starts_with('[') {
        return Species::from_toml_str(spec).map_err(|e| e.to_string());
    }
    let path = base.join(spec);
    if split_species_id(spec).is_some() && !path.exists() {
        let store = PackStore::open_default().map_err(|e| e.to_string())?;
        let found = store.species(spec).map_err(|e| e.to_string())?;
        let mut species = Species::from_toml_str(&found.text).map_err(|e| e.to_string())?;
        species.asset_dir = found.asset_dir;
        return Ok(species);
    }
    let mut species = Species::from_path(&path).map_err(|e| {
        let names: Vec<&str> = presets().iter().map(|(name, _)| *name).collect();
        format!(
            "`{spec}` is neither a preset ({}) nor a readable file: {e}",
            names.join(", ")
        )
    })?;
    species.asset_dir = path.parent().map(Path::to_path_buf);
    Ok(species)
}
