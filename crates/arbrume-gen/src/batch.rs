//! The generator and batch generation over the `rayon` thread pool.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use arbrume_core::species::Species;
use arbrume_grow::grow;
use arbrume_mesh::{MeshOptions, Mesher};

use crate::params::{GenError, GenParams};
use crate::plant::{Plant, Stats, Timings};

/// A reusable generator: keeps the working memory of the mesher from one plant to the
/// next.
///
/// A batch loop should keep one `Generator` per thread and, when it consumes each plant
/// before generating the next, one [`Plant`] passed to [`Generator::generate_into`],
/// whose mesh buffers are then reused. The result never depends on what the generator
/// was used for before.
#[derive(Debug, Clone, Default)]
pub struct Generator {
    mesher: Mesher,
}

impl Generator {
    /// A generator with no memory reserved yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
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
        species.validate()?;
        params.validate()?;
        let start = Instant::now();
        let graph = grow(species, &params.grow_params());
        let grown = Instant::now();
        self.mesher
            .mesh_into(&graph, species, &params.mesh, &mut plant.mesh)?;
        let timings = Timings {
            grow: grown - start,
            mesh: grown.elapsed(),
        };
        plant.graph = graph;
        plant.params = *params;
        plant.stats = Stats::of(&plant.graph, &plant.mesh);
        Ok(timings)
    }

    /// Meshes the graph of `plant` again with `options`, for another level of detail.
    /// Updates the mesh, `params.mesh` and the mesh statistics; returns the meshing time.
    ///
    /// # Errors
    ///
    /// Returns [`GenError::Mesh`] for invalid options or a mesh too large; `plant` is
    /// then left untouched.
    pub fn remesh(
        &mut self,
        plant: &mut Plant,
        species: &Species,
        options: &MeshOptions,
    ) -> Result<Duration, GenError> {
        let start = Instant::now();
        self.mesher
            .mesh_into(&plant.graph, species, options, &mut plant.mesh)?;
        let elapsed = start.elapsed();
        plant.params.mesh = *options;
        plant.stats.set_mesh(&plant.mesh);
        Ok(elapsed)
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
