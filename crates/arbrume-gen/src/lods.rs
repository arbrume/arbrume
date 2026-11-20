//! Level-of-detail chains of a generated plant: mesh levels from the graph and an
//! optional billboard, with the wind data baked in when asked.

use std::time::{Duration, Instant};

use arbrume_core::mesh::TreeMesh;
use arbrume_core::species::Species;
use arbrume_lod::{BillboardOptions, bake_billboard};
use arbrume_mesh::{LeafInstances, LodOptions, MeshOptions};

use crate::batch::Generator;
use crate::params::GenError;
use crate::plant::Plant;

/// What [`Generator::lod_chain`] builds. With textures (`Generator::set_textures`)
/// every mesh level carries them and the billboard is baked from the textured leaves.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct LodSettings {
    /// The mesh levels (see `arbrume_mesh::lod`).
    pub lod: LodOptions,
    /// Add a billboard as the last level, baked from the mesh level `billboard_source`.
    pub billboard: Option<BillboardOptions>,
    /// Mesh level the billboard is baked from (clamped to the last mesh level).
    pub billboard_source: usize,
    /// Bake the wind data into every level.
    pub wind: bool,
    /// Also give the foliage of every mesh level as instances of its templates
    /// ([`LodMesh::leaves`]), for exporters that instance it.
    pub leaf_instances: bool,
}

/// What one level is made of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LodKind {
    /// A mesh of the graph.
    Mesh,
    /// Textured planes.
    Billboard,
}

/// One level of a chain.
#[derive(Debug, Clone, PartialEq)]
pub struct LodMesh {
    /// Kind of the level.
    pub kind: LodKind,
    /// The mesh of the level.
    pub mesh: TreeMesh,
    /// Mesh options of the level (those of the source level for a billboard).
    pub options: MeshOptions,
    /// Quality of the level (`arbrume_mesh::LodLevel::quality`); 0 for a billboard.
    pub quality: f32,
    /// Triangle budget of the level; the triangle count for a billboard.
    pub budget: usize,
    /// Triangles of the level.
    pub triangles: usize,
    /// Plant height on screen, in pixels, up to which the level is meant to be drawn.
    pub max_screen_height: f32,
    /// Wall-clock time spent building the level (meshing or baking).
    pub time: Duration,
    /// The foliage as instances, when asked and the level is a mesh.
    pub leaves: Option<LeafInstances>,
}

impl Generator {
    /// Builds the level-of-detail chain of `plant`, grown from `species`: the mesh
    /// levels planned by `arbrume_mesh::Mesher::lod_chain` from the mesh options of the
    /// plant, then the billboard when asked. The mesh of `plant` is not changed.
    ///
    /// # Errors
    ///
    /// Returns [`GenError::Mesh`] for invalid options or a mesh too large.
    pub fn lod_chain(
        &mut self,
        plant: &Plant,
        species: &Species,
        settings: &LodSettings,
    ) -> Result<Vec<LodMesh>, GenError> {
        let meshed = self.meshed(species);
        let (textured, species) = (species, meshed.as_ref());
        let base = MeshOptions {
            wind: settings.wind,
            ..plant.params.mesh
        };
        let levels = self
            .mesher
            .lod_chain(&plant.graph, species, &base, &settings.lod)?;
        let mut out = Vec::with_capacity(levels.len() + 1);
        for level in &levels {
            let start = Instant::now();
            let mut mesh = self.mesher.mesh(&plant.graph, species, &level.options)?;
            self.texture(textured, &mut mesh)?;
            let mut leaves = settings
                .leaf_instances
                .then(|| {
                    self.mesher
                        .leaf_instances(&plant.graph, species, &level.options)
                })
                .transpose()?;
            if let Some(leaves) = &mut leaves {
                self.remap_templates(textured, leaves)?;
            }
            out.push(LodMesh {
                leaves,
                kind: LodKind::Mesh,
                triangles: mesh.triangle_count(),
                mesh,
                options: level.options,
                quality: level.quality,
                budget: level.budget,
                max_screen_height: level.max_screen_height,
                time: start.elapsed(),
            });
        }
        if let Some(options) = &settings.billboard {
            let source = &out[settings.billboard_source.min(out.len() - 1)];
            let start = Instant::now();
            if let Some(billboard) = bake_billboard(&source.mesh, options) {
                let mesh = TreeMesh {
                    parts: vec![billboard.part],
                    wind: source.mesh.wind.clone(),
                };
                out.push(LodMesh {
                    leaves: None,
                    kind: LodKind::Billboard,
                    triangles: mesh.triangle_count(),
                    budget: mesh.triangle_count(),
                    mesh,
                    options: source.options,
                    quality: 0.0,
                    max_screen_height: billboard.max_screen_height,
                    time: start.elapsed(),
                });
            }
        }
        Ok(out)
    }
}
