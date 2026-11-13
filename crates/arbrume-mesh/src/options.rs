//! Options of [`mesh_tree`](crate::mesh_tree) and its error type.

use thiserror::Error;

/// Default [`MeshOptions::tolerance`], in meters. A starting point, not a measured value.
pub const DEFAULT_TOLERANCE: f32 = 0.002;

/// Error from [`mesh_tree`](crate::mesh_tree).
#[derive(Debug, Clone, PartialEq, Error)]
pub enum MeshError {
    /// The chains of the graph are not contiguous slices. Call
    /// `PlantGraph::ensure_contiguous` before meshing.
    #[error("the plant graph is not contiguous; call ensure_contiguous() before meshing")]
    NotContiguous,
    /// An option has a value outside its domain.
    #[error("invalid mesh option `{field}`: {reason}")]
    InvalidOption {
        /// Name of the field of [`MeshOptions`].
        field: &'static str,
        /// Why the value is rejected.
        reason: String,
    },
    /// A part needs more vertices than a `u32` index can address.
    #[error("the mesh needs {vertices} vertices in one part, more than u32 indices allow")]
    TooLarge {
        /// Number of vertices the part would have.
        vertices: usize,
    },
}

/// Resolution, level of detail, age and threading controls of the mesher.
///
/// The three level-of-detail controls are independent: `tolerance` sets how closely the
/// triangles follow the ideal surface (it drives the number of rings along a branch, the
/// number of sides of a cross-section and the detail of a leaf blade), `min_radius` drops
/// thin branches, and `leaf_fraction` keeps a random subset of the leaves, enlarged so
/// that the total leaf area stays the same. [`MeshOptions::with_quality`] sets all three
/// from one number.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MeshOptions {
    /// Largest distance in meters allowed between the mesh and the smooth surface it
    /// approximates. Features smaller than the tolerance (a collar, a flare, a bend of an
    /// axis) are left out.
    pub tolerance: f32,
    /// Axes whose base radius is below this value, in meters, are not meshed, nor are the
    /// axes they bear. Their leaves stay. 0 keeps every axis.
    pub min_radius: f32,
    /// Fraction of the leaves to keep, in `[0, 1]`. Leaves are stored in a random order
    /// and the first `ceil(fraction * count)` are kept, each enlarged by
    /// `sqrt(count / kept)` so that the total area is preserved. 0 gives no leaves.
    pub leaf_fraction: f32,
    /// When set, the plant is meshed as it was at the end of this growth cycle: nodes and
    /// leaves born later are left out, and leaves older than the retention of the species
    /// are dropped. Radii are those stored in the graph, so a younger plant keeps the
    /// girth of the grown one unless the caller recomputes them.
    pub max_cycle: Option<u16>,
    /// Seed of the mesh-level variation: leaf order, leaf size, attitude and colour, and
    /// the texture offset of each branch.
    pub seed: u64,
    /// Fill the buffers with the global `rayon` thread pool instead of the calling
    /// thread. The result is bit-identical either way.
    pub parallel: bool,
}

impl Default for MeshOptions {
    fn default() -> Self {
        Self {
            tolerance: DEFAULT_TOLERANCE,
            min_radius: 0.0,
            leaf_fraction: 1.0,
            max_cycle: None,
            seed: 0,
            parallel: false,
        }
    }
}

impl MeshOptions {
    /// Options for the level of detail `quality`, 1 being full detail and smaller values
    /// coarser. The number of sides, of rings and of leaves are roughly proportional to
    /// `quality`: the tolerance is [`DEFAULT_TOLERANCE`] divided by `quality` squared,
    /// axes thinner than half the increase of the tolerance are dropped, and
    /// `leaf_fraction` is `quality` (at most 1). `quality` is clamped to at least 0.01.
    #[must_use]
    pub fn with_quality(quality: f32) -> Self {
        let quality = quality.max(0.01);
        let tolerance = DEFAULT_TOLERANCE / (quality * quality);
        Self {
            tolerance,
            min_radius: 0.5 * (tolerance - DEFAULT_TOLERANCE).max(0.0),
            leaf_fraction: quality.min(1.0),
            ..Self::default()
        }
    }

    /// Checks the domain of every option.
    ///
    /// # Errors
    ///
    /// Returns [`MeshError::InvalidOption`] for a tolerance that is not positive and
    /// finite, a negative or NaN `min_radius`, or a `leaf_fraction` outside `[0, 1]`.
    pub fn validate(&self) -> Result<(), MeshError> {
        let invalid = |field, value: f32, domain: &str| MeshError::InvalidOption {
            field,
            reason: format!("{value} is not {domain}"),
        };
        if !(self.tolerance > 0.0 && self.tolerance.is_finite()) {
            return Err(invalid("tolerance", self.tolerance, "positive and finite"));
        }
        if self.min_radius.is_nan() || self.min_radius < 0.0 {
            return Err(invalid("min_radius", self.min_radius, "zero or more"));
        }
        if !(0.0..=1.0).contains(&self.leaf_fraction) {
            return Err(invalid("leaf_fraction", self.leaf_fraction, "in [0, 1]"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_TOLERANCE, MeshError, MeshOptions};

    #[test]
    fn full_quality_is_the_default() {
        assert_eq!(MeshOptions::with_quality(1.0), MeshOptions::default());
        MeshOptions::default().validate().unwrap();
    }

    #[test]
    fn lower_quality_is_coarser_in_every_control() {
        let half = MeshOptions::with_quality(0.5);
        let quarter = MeshOptions::with_quality(0.25);
        assert!((half.tolerance - 4.0 * DEFAULT_TOLERANCE).abs() < 1e-7);
        assert!(quarter.tolerance > half.tolerance);
        assert!(quarter.min_radius > half.min_radius && half.min_radius > 0.0);
        assert!(quarter.leaf_fraction < half.leaf_fraction);
        MeshOptions::with_quality(0.0).validate().unwrap();
    }

    #[test]
    fn out_of_domain_options_name_the_field() {
        let cases = [
            (
                MeshOptions {
                    tolerance: 0.0,
                    ..Default::default()
                },
                "tolerance",
            ),
            (
                MeshOptions {
                    min_radius: f32::NAN,
                    ..Default::default()
                },
                "min_radius",
            ),
            (
                MeshOptions {
                    leaf_fraction: 1.5,
                    ..Default::default()
                },
                "leaf_fraction",
            ),
        ];
        for (options, expected) in cases {
            match options.validate() {
                Err(MeshError::InvalidOption { field, .. }) => assert_eq!(field, expected),
                other => panic!("expected an invalid `{expected}`, got {other:?}"),
            }
        }
    }
}
