//! Forests: thousands to millions of trees over a landscape, cheap to store and to draw.
//!
//! ```
//! use arbrume_forest::{Exclusions, PlaceSettings, SpeciesRule, Terrain, place};
//!
//! let terrain = Terrain::flat([0.0, 0.0], [100.0, 100.0], 0.0);
//! let rule = SpeciesRule { density: 200.0, spacing: 4.0, ..SpeciesRule::default() };
//! let (trees, report) =
//!     place(&terrain, &Exclusions::default(), &[rule], &PlaceSettings::default()).unwrap();
//! assert_eq!(trees.len(), report.species[0].kept);
//! ```
//!
//! # Pieces
//!
//! * [`Terrain`]: heights on a regular grid (16-bit PNG, explicit grid or procedural
//!   hills), with bilinear height and slope.
//! * [`Exclusions`]: clearings, roads, rivers (which can carve the terrain), polygons,
//!   painted masks and a water level, each with a soft edge.
//! * [`SpeciesRule`] and [`place`]: a species mix with densities, slope and altitude
//!   preferences, placed by hard-core thinning of Poisson candidates, with random or
//!   size priorities ([`Mode`]); deterministic, in bands of bounded memory, parallel.
//! * [`build_library`]: a few variants per species, grown and meshed once as a chain of
//!   levels of detail, which every tree references with its own transform.
//! * [`grow_coupled`]: the trees of a region grown together in tiles, sharing shade.
//! * [`modules`]: how much branch-module instancing (Makowski et al. 2019) would save on
//!   a library, measured.
//! * [`ForestConfig`], [`Forest::build`] and [`write_forest`]: the `forest.toml` file of
//!   `arbrume forest`, and the files it writes.
//!
//! # Determinism
//!
//! The placement depends only on the terrain, the exclusions, the rules and the seed:
//! every cell draws from its own random stream and the thinning reads fixed priorities,
//! so neither the thread count nor the band size changes a tree. The library and the
//! coupled region inherit the determinism of the generator. `docs/forest.md` is the
//! guide.

#![forbid(unsafe_code)]

mod config;
mod coupled;
pub mod density;
mod error;
mod exclude;
mod export;
mod forest;
mod library;
pub mod modules;
mod place;
mod rules;
pub mod table;
mod terrain;

pub use config::{
    BillboardConfig, CircleConfig, CoupledConfig, ExcludeConfig, ForestConfig, GridConfig,
    HillsConfig, LibraryConfig, MaskGridConfig, ModeConfig, OutputConfig, PathConfig,
    PlacementConfig, PolygonConfig, SpeciesConfig, TerrainConfig,
};
pub use coupled::{Coupled, CoupledSettings, CoupledTree, grow_coupled};
pub use error::ForestError;
pub use exclude::{Exclusion, Exclusions, RasterMask, Shape};
pub use export::{COUPLED_VARIANT, Written, write_forest};
pub use forest::Forest;
pub use library::{LibrarySettings, SpeciesVariants, Variant, VariantSpec, build_library};
pub use place::{Instance, Mode, PlaceReport, PlaceSettings, SpeciesCounts, place};
pub use rules::SpeciesRule;
pub use terrain::{Hills, Terrain};
