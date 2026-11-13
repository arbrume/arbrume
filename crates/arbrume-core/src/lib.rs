//! Core data model of Arbrume: plant graph, random numbers, curves, species schema,
//! spatial grid and mesh buffers.
//!
//! * [`graph`]: the plant itself, an arena of metamers, buds, axes and organ sites.
//! * [`species`]: the serializable description of a kind of plant.
//! * [`curve`]: functions of relative position and branching order, used by species.
//! * [`rng`]: counter-based random numbers keyed by seed, node and stream.
//! * [`grid`]: uniform hash grid for neighbourhood queries.
//! * [`mesh`]: triangle buffers shared by the mesher and the exporters.
//!
//! Output is a function of species, parameters, seed and version only: nothing in this
//! crate depends on thread count, on the iteration order of a hash map, or on time.

pub mod curve;
pub mod graph;
pub mod grid;
pub mod mesh;
pub mod rng;
pub mod species;
