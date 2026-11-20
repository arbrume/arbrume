//! Error type of the forest pipeline.

use arbrume_export::GlbError;
use arbrume_gen::GenError;

/// What can go wrong when a forest is placed, grown or written.
#[derive(Debug, thiserror::Error)]
pub enum ForestError {
    /// The configuration is invalid.
    #[error("forest config: {0}")]
    Config(String),
    /// The terrain cannot be built.
    #[error("terrain: {0}")]
    Terrain(String),
    /// An exclusion mask cannot be read.
    #[error("exclusion: {0}")]
    Exclusion(String),
    /// A variant could not be generated.
    #[error(transparent)]
    Gen(#[from] GenError),
    /// A file could not be written.
    #[error(transparent)]
    Glb(#[from] GlbError),
    /// Input or output failed.
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
}
