//! The generator and batch generation over the `rayon` thread pool.

use std::borrow::Cow;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use arbrume_core::graph::PlantGraph;
use arbrume_core::mesh::TreeMesh;
use arbrume_core::species::Species;
use arbrume_grow::Surroundings;
use arbrume_mesh::{DEFAULT_TOLERANCE, MeshCounts, MeshOptions, Mesher};
use arbrume_texture::{SpeciesTextures, TextureCache, textured_species};

use crate::params::{GenError, GenParams, quality_options};
use crate::plant::{Plant, Stats, Timings};

/// Largest number of times [`Generator::generate_within`] lowers the quality.
const BUDGET_STEPS: usize = 8;
/// Quality below which [`Generator::generate_within`] stops lowering it.
const BUDGET_MIN_QUALITY: f32 = 0.02;
/// Margin taken under the budget at each step, as a factor on the quality.
const BUDGET_MARGIN: f32 = 0.95;

/// Largest size of a mesh, for [`Generator::generate_within`].
///
/// Triangles are what a renderer draws; vertices are what a file stores (a vertex of an
/// exported plant takes several times the bytes of a triangle), and foliage made of
/// separate small triangles has up to three vertices per triangle where bark has about
/// one for two.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Budget {
    /// Largest number of triangles, all parts; `None` for no limit.
    pub triangles: Option<usize>,
    /// Largest number of vertices, all parts; `None` for no limit.
    pub vertices: Option<usize>,
}

impl Budget {
    /// No limit.
    pub const NONE: Self = Self {
        triangles: None,
        vertices: None,
    };

    /// The fraction of `counts` the budget allows, at most 1.
    fn ratio(&self, counts: &MeshCounts) -> f32 {
        let part = |limit: Option<usize>, count: usize| {
            limit.map_or(1.0, |limit| limit as f32 / count.max(1) as f32)
        };
        let vertices = counts.bark_vertices + counts.leaf_vertices;
        part(self.triangles, counts.triangles())
            .min(part(self.vertices, vertices))
            .min(1.0)
    }
}

/// A reusable generator: keeps the working memory of the mesher from one plant to the
/// next.
///
/// A batch loop should keep one `Generator` per thread and, when it consumes each plant
/// before generating the next, one [`Plant`] passed to [`Generator::generate_into`],
/// whose mesh buffers are then reused. The result never depends on what the generator
/// was used for before.
///
/// With [`Generator::set_textures`], every mesh it builds carries the procedural
/// textures of its species, from a cache shared by the batch, and its leaves are
/// meshed as `arbrume_texture::textured_species` asks.
#[derive(Debug, Clone, Default)]
pub struct Generator {
    pub(crate) mesher: Mesher,
    textures: Option<Arc<TextureCache>>,
    texture_base: Option<Arc<Species>>,
}

impl Generator {
    /// A generator with no memory reserved yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Textures every mesh built from now on with `cache`, or with none.
    pub fn set_textures(&mut self, cache: Option<Arc<TextureCache>>) {
        self.textures = cache;
    }

    /// Takes the textures of `base` for the plants generated from now on, which are
    /// individuals varied from it (`Species::varied`, `RandomSampler`), so that they
    /// share one texture set; `None` takes those of the species generated.
    pub fn set_texture_base(&mut self, base: Option<Arc<Species>>) {
        self.texture_base = base;
    }

    /// The texture cache of the generator, if any.
    #[must_use]
    pub fn textures(&self) -> Option<&Arc<TextureCache>> {
        self.textures.as_ref()
    }

    /// `species` as the mesher sees it: with textures, its leaves become cards.
    pub(crate) fn meshed<'s>(&self, species: &'s Species) -> Cow<'s, Species> {
        if self.textures.is_some() {
            textured_species(species)
        } else {
            Cow::Borrowed(species)
        }
    }

    /// The textures of `species` (or of the texture base), when the generator has
    /// textures.
    pub(crate) fn texture_set(
        &self,
        species: &Species,
    ) -> Result<Option<Arc<SpeciesTextures>>, GenError> {
        let Some(cache) = &self.textures else {
            return Ok(None);
        };
        let source = self.texture_base.as_deref().unwrap_or(species);
        Ok(Some(cache.get(source)?))
    }

    /// Puts the textures of `species` (or of the texture base) on `mesh`, a mesh of
    /// `species`, when the generator has textures.
    pub(crate) fn texture(&self, species: &Species, mesh: &mut TreeMesh) -> Result<(), GenError> {
        if let Some(textures) = self.texture_set(species)? {
            textures.apply(mesh, species);
        }
        Ok(())
    }

    /// Moves the texture coordinates of the leaf templates of `leaves`, of `species`, to
    /// the atlas of the textures, when the generator has textures.
    pub(crate) fn remap_templates(
        &self,
        species: &Species,
        leaves: &mut arbrume_mesh::LeafInstances,
    ) -> Result<(), GenError> {
        if let Some(textures) = self.texture_set(species)? {
            for template in &mut leaves.templates {
                textures.remap(species, &mut template.uvs);
            }
        }
        Ok(())
    }

    /// Same as [`generate`], reusing the memory of the generator.
    ///
    /// # Errors
    ///
    /// The same as [`generate`].
    pub fn generate(&mut self, species: &Species, params: &GenParams) -> Result<Plant, GenError> {
        let mut plant = Plant::default();
        self.generate_into(species, params, &mut plant)?;
        Ok(plant)
    }

    /// Same as [`generate`], writing into `plant` and reusing the allocations of its
    /// mesh. Returns the time spent growing and meshing.
    ///
    /// # Errors
    ///
    /// The same as [`generate`]. On error `plant` is left untouched.
    pub fn generate_into(
        &mut self,
        species: &Species,
        params: &GenParams,
        plant: &mut Plant,
    ) -> Result<Timings, GenError> {
        self.generate_within(species, params, &Budget::NONE, plant)
    }

    /// Same as [`Generator::generate_into`], with a mesh that fits `budget`.
    ///
    /// The plant is grown once. When its mesh at the quality of `params` would exceed
    /// the budget (known from the planning pass of the mesher, without building the
    /// mesh), the quality is lowered in proportion and the count taken again, a few
    /// times at most, and the mesh is built at the quality reached: the level-of-detail
    /// controls become those of `MeshOptions::with_quality` for that quality. `params`
    /// of the plant records the options used. The quality is not lowered below 0.02, so
    /// a budget smaller than the coarsest mesh of the plant is exceeded.
    ///
    /// # Errors
    ///
    /// The same as [`generate`]. On error `plant` is left untouched.
    pub fn generate_within(
        &mut self,
        species: &Species,
        params: &GenParams,
        budget: &Budget,
        plant: &mut Plant,
    ) -> Result<Timings, GenError> {
        species.validate()?;
        params.validate()?;
        let start = Instant::now();
        let open = Surroundings::none();
        let surroundings = params.surroundings.as_deref().unwrap_or(&open);
        let (graph, guides) = crate::guided::grow(species, params, surroundings);
        let graph = crate::rooted::add(species, params, surroundings, graph);
        let grown = Instant::now();
        self.mesh_grown(species, params, budget, graph, plant)?;
        plant.guides = guides;
        Ok(Timings {
            grow: grown - start,
            mesh: grown.elapsed(),
        })
    }

    /// Meshes `graph`, grown from `species` and `params`, into `plant` within `budget`
    /// (see [`Generator::generate_within`]) and fills in the rest of `plant`.
    pub(crate) fn mesh_grown(
        &mut self,
        species: &Species,
        params: &GenParams,
        budget: &Budget,
        graph: PlantGraph,
        plant: &mut Plant,
    ) -> Result<(), GenError> {
        let meshed = self.meshed(species);
        let (textured, species) = (species, meshed.as_ref());
        let textures = self.texture_set(textured)?;
        let mut options = params.mesh;
        if *budget != Budget::NONE {
            let mut quality = (DEFAULT_TOLERANCE / options.tolerance).sqrt();
            for _ in 0..BUDGET_STEPS {
                let ratio = budget.ratio(&self.mesher.counts(&graph, species, &options)?);
                if ratio >= 1.0 || quality <= BUDGET_MIN_QUALITY {
                    break;
                }
                quality = (quality * ratio * BUDGET_MARGIN).max(BUDGET_MIN_QUALITY);
                options = quality_options(&options, quality);
            }
        }
        self.mesher
            .mesh_into(&graph, species, &options, &mut plant.mesh)?;
        if let Some(textures) = textures {
            textures.apply(&mut plant.mesh, textured);
        }
        plant.graph = graph;
        plant.params = GenParams {
            mesh: options,
            ..params.clone()
        };
        plant.stats = Stats::of(&plant.graph, &plant.mesh);
        plant.guides = None;
        Ok(())
    }

    /// Meshes `graph` into the mesh of `plant` with `options`.
    pub(crate) fn mesh_graph(
        &mut self,
        graph: &PlantGraph,
        species: &Species,
        options: &MeshOptions,
        plant: &mut Plant,
    ) -> Result<(), GenError> {
        let meshed = self.meshed(species);
        self.mesher
            .mesh_into(graph, &meshed, options, &mut plant.mesh)?;
        self.texture(species, &mut plant.mesh)
    }

    /// Meshes the graph of `plant` again with `options`, for another level of detail.
    /// Updates the mesh, `params.mesh` and the mesh statistics; returns the meshing time.
    ///
    /// # Errors
    ///
    /// Returns [`GenError::Mesh`] for invalid options or a mesh too large and
    /// [`GenError::Texture`] for an image texture that cannot be read; `plant` is then
    /// left untouched.
    pub fn remesh(
        &mut self,
        plant: &mut Plant,
        species: &Species,
        options: &MeshOptions,
    ) -> Result<Duration, GenError> {
        let start = Instant::now();
        let textures = self.texture_set(species)?;
        let meshed = self.meshed(species);
        self.mesher
            .mesh_into(&plant.graph, &meshed, options, &mut plant.mesh)?;
        if let Some(textures) = textures {
            textures.apply(&mut plant.mesh, species);
        }
        let elapsed = start.elapsed();
        plant.params.mesh = *options;
        plant.stats.set_mesh(&plant.mesh);
        Ok(elapsed)
    }

    /// Where every vertex of the mesh of `plant` comes from (axis or organ site, and
    /// semantic class), for ML labels. Runs the planning pass of the mesher on the graph
    /// and mesh options of `plant`, which must be those its mesh was built with, as
    /// every method of the generator leaves them.
    ///
    /// # Errors
    ///
    /// Returns [`GenError::Mesh`] when the mesher rejects the graph or the options.
    pub fn provenance(
        &mut self,
        plant: &Plant,
        species: &Species,
    ) -> Result<arbrume_core::mesh::MeshProvenance, GenError> {
        let options = &plant.params.mesh;
        let meshed = self.meshed(species);
        Ok(self.mesher.provenance(&plant.graph, &meshed, options)?)
    }
}

/// Grows a plant of `species` and builds its mesh.
///
/// # Errors
///
/// Returns [`GenError::Species`] for an invalid species, [`GenError::InvalidParam`] or
/// [`GenError::Mesh`] for invalid parameters, and [`GenError::Mesh`] when the mesh would
/// be too large.
pub fn generate(species: &Species, params: &GenParams) -> Result<Plant, GenError> {
    Generator::new().generate(species, params)
}

/// Runs `task(generator, plant, index)` for every index in `0..count` on the current
/// `rayon` thread pool and returns the results in index order.
///
/// Every thread of the pool takes indices one at a time from a shared counter and runs
/// them with its own [`Generator`] and its own scratch [`Plant`], so the pool stays busy
/// when plants differ in size. A task that generates into the scratch plant with
/// [`Generator::generate_into`], writes it to disk and returns a small record keeps the
/// memory of the batch bounded by one plant per thread and reuses its buffers. Inside `ThreadPool::install` the batch runs
/// on that pool.
///
/// # Panics
///
/// Propagates a panic of `task`.
pub fn run_batch<T, F>(count: usize, task: F) -> Vec<T>
where
    T: Send,
    F: Fn(&mut Generator, &mut Plant, usize) -> T + Sync,
{
    let next = AtomicUsize::new(0);
    let per_thread: Vec<Vec<(usize, T)>> = rayon::broadcast(|_| {
        let mut generator = Generator::new();
        let mut plant = Plant::default();
        let mut done = Vec::new();
        loop {
            let index = next.fetch_add(1, Ordering::Relaxed);
            if index >= count {
                break done;
            }
            done.push((index, task(&mut generator, &mut plant, index)));
        }
    });
    let mut slots: Vec<Option<T>> = std::iter::repeat_with(|| None).take(count).collect();
    for (index, value) in per_thread.into_iter().flatten() {
        slots[index] = Some(value);
    }
    slots.into_iter().flatten().collect()
}

/// Generates every `(species, params)` job on the current `rayon` thread pool, one
/// [`Generator`] per thread. The results are in input order and equal those of
/// [`generate`] on each job, whatever the number of threads.
///
/// Every plant is held in memory until the batch ends; for large batches use
/// [`run_batch`] and consume each plant in the task.
#[must_use]
pub fn generate_many(jobs: &[(&Species, GenParams)]) -> Vec<Result<Plant, GenError>> {
    run_batch(jobs.len(), |generator, _, index| {
        let (species, params) = &jobs[index];
        generator.generate(species, params)
    })
}
