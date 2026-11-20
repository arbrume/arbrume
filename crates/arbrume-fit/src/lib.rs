//! Fitting species parameters to target tree skeletons, offline.
//!
//! Start from a measured, scanned or generated tree skeleton and a species preset, and
//! find the parameters with which the preset grows look-alikes of the target:
//!
//! * [`TreeSkeleton`] reads the target: an Arbrume skeleton sidecar or `.glb`, or a CSV
//!   or JSON list of nodes (position, radius, parent) for scans;
//! * [`distance`] and [`Descriptor`] compare two skeletons with shape, geometry and
//!   structure terms after Stava et al. (2014), each in `[0, 1]` ([`Terms`]);
//! * [`Cmaes`] is the covariance matrix adaptation evolution strategy of Hansen (2016);
//! * [`fit`] searches the factors of chosen species parameters, the age and the scale
//!   that minimise the mean distance over several plant seeds, with checkpoints, and
//!   [`FitResult`] gives the fitted species file and a report.
//!
//! Everything is deterministic: a fit depends on the target, the base species, the
//! options and the fit seed, never on the number of threads. The user guide is
//! `docs/fitting.md`.

#![forbid(unsafe_code)]

mod cmaes;
mod distance;
mod fit;
mod linalg;
mod skeleton;
mod space;

pub use cmaes::{Cmaes, CmaesSettings};
pub use distance::{
    AXIS_PROPERTIES, Descriptor, DistanceOptions, ORDER_GROUPS, Terms, Weights, distance,
    normalized_graph, pruned_height, step_for, wasserstein,
};
pub use fit::{
    CHECKPOINT_FORMAT, Candidate, Checkpoint, FitOptions, FitResult, Generation, fit,
    fit_with_checkpoint,
};
pub use skeleton::{TreeSkeleton, Up};
pub use space::{Decoded, PROBE_FACTORS, Parameter, Space, grow_skeleton, skeleton_parameters};

/// Error of the fitting crate.
#[derive(Debug, thiserror::Error)]
pub enum FitError {
    /// A skeleton is inconsistent or empty.
    #[error("invalid skeleton: {0}")]
    Skeleton(String),
    /// A file is not in a known format.
    #[error("{0}")]
    Format(String),
    /// A file cannot be read or written.
    #[error("{0}")]
    Io(String),
    /// A setting or a parameter name is invalid.
    #[error("{0}")]
    Invalid(String),
    /// The base species cannot be parsed or grown.
    #[error(transparent)]
    Gen(#[from] arbrume_gen::GenError),
}

impl FitError {
    pub(crate) fn skeleton(message: impl Into<String>) -> Self {
        Self::Skeleton(message.into())
    }

    pub(crate) fn format(message: impl Into<String>) -> Self {
        Self::Format(message.into())
    }

    pub(crate) fn invalid(message: impl Into<String>) -> Self {
        Self::Invalid(message.into())
    }
}
