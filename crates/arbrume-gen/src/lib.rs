//! One-call generation pipeline: species and seed in, plant graph and mesh out.
//!
//! ```
//! use arbrume_gen::{GenParams, generate, preset};
//!
//! let species = preset("shrub").unwrap();
//! let plant = generate(&species, &GenParams::new(7, 4)).unwrap();
//! assert!(plant.stats.triangles > 0);
//! assert_eq!(plant.stats.nodes, plant.graph.node_count());
//! ```
//!
//! # Pieces
//!
//! * [`generate`] grows a plant with `arbrume-grow` and meshes it with `arbrume-mesh`.
//! * [`Generator`] does the same while keeping the working memory of the mesher, and
//!   with [`Generator::generate_into`] the mesh buffers, from one plant to the next.
//! * [`generate_many`] and [`run_batch`] spread a batch over the current `rayon` thread
//!   pool, one [`Generator`] per thread, results in input order.
//! * [`Sampler`] turns a base seed, an index and distributions of age and scale into the
//!   [`GenParams`] of one plant of a dataset.
//! * [`RandomSampler`] draws a varied individual of a random preset for each index,
//!   within a triangle budget.
//! * [`presets`] are species files embedded in the library; [`species_with_overrides`]
//!   changes fields of a species file by dotted path before it is parsed.
//! * [`GenParams::with_surroundings`] places the plant in an envelope, next to obstacles
//!   or under neighbours ([`Surroundings`]); species in the self-organizing mode respond.
//! * [`Generator::timeline`] and [`Generator::frame`] mesh one grown plant at several
//!   ages, for growth sequences.
//! * [`metadata`] is the JSON stored in the `extras` of an exported file, enough to
//!   generate the same plant again.
//!
//! # Determinism
//!
//! A plant depends only on the species, its [`GenParams`] and the version of the crates.
//! A batch gives the same plants whatever the number of threads, and the same as calling
//! [`generate`] on each job in turn: each job is computed by one thread from its own
//! inputs, and a [`Generator`] never carries state from one plant into the next.

#![forbid(unsafe_code)]

mod batch;
mod metadata;
mod overrides;
mod params;
mod plant;
mod presets;
mod random;
mod sample;
mod timeline;

pub use arbrume_grow::{Occluder, Shape, Surroundings, VoxelMask};
pub use batch::{Budget, Generator, generate, generate_many, run_batch};
pub use metadata::{GENERATOR, VERSION, metadata};
pub use overrides::species_with_overrides;
pub use params::{GenError, GenParams};
pub use plant::{Bounds, Plant, Stats, Timings};
pub use presets::{preset, preset_toml, presets};
pub use random::{RANDOM_BUDGET, RANDOM_QUALITY, RandomPick, RandomSampler};
pub use sample::{Distribution, Sampler, plant_seed};
pub use timeline::Timeline;
