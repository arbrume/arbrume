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
    /// axes they bear, within the limit of `max_drop_reach`. Their leaves follow
    /// `orphan_leaves`. 0 keeps every axis.
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
    /// An axis thinner than `min_radius` is dropped only when every node of the axes it
    /// bears, and of itself, is within this distance of its base, in meters. A finite
    /// value bounds how far a reattached leaf moves ([`OrphanLeaves::Reattach`]), which
    /// keeps the outline of the crown. Infinite by default: `min_radius` alone decides.
    pub max_drop_reach: f32,
    /// What happens to the leaves of axes dropped by `min_radius`.
    pub orphan_leaves: OrphanLeaves,
    /// Fraction of the leaves of a twig template kept, in `(0, 1]`: a twig of `n` leaves
    /// keeps `max(1, round(fraction * n))` of them, spread along its stem and enlarged
    /// so that the leaf area of the twig is preserved. No effect without a twig.
    pub twig_leaf_fraction: f32,
    /// Compute the baked wind data: [`MeshBuffers::wind`](arbrume_core::mesh::MeshBuffers)
    /// on every part and [`TreeMesh::wind`](arbrume_core::mesh::TreeMesh), see
    /// [`crate::wind`].
    pub wind: bool,
}

/// What [`MeshOptions::min_radius`] does to the leaves of the axes it drops.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OrphanLeaves {
    /// The leaves stay where they are, in the air where their axis was.
    #[default]
    Keep,
    /// Each leaf moves to the nearest point of the kept axis its dropped axis grows
    /// from, on its surface, so that no leaf floats.
    Reattach,
    /// The leaves are left out, and the others are enlarged so that the total leaf area
    /// is preserved.
    Drop,
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
            max_drop_reach: f32::INFINITY,
            orphan_leaves: OrphanLeaves::Keep,
            twig_leaf_fraction: 1.0,
            wind: false,
        }
    }
}

impl MeshOptions {
    /// Options for the level of detail `quality`, 1 being full detail and smaller values
    /// coarser. The number of sides, of rings and of leaves are roughly proportional to
    /// `quality`: the tolerance is [`DEFAULT_TOLERANCE`] divided by `quality` squared
    /// and `leaf_fraction` is `quality` (at most 1). No axis is dropped (`min_radius`
    /// stays 0): a thin axis costs few triangles at a coarse tolerance, and dropping it
    /// would leave its leaves in the air. `quality` is clamped to at least 0.01.
    #[must_use]
    pub fn with_quality(quality: f32) -> Self {
        let quality = quality.max(0.01);
        let tolerance = DEFAULT_TOLERANCE / (quality * quality);
        Self {
            tolerance,
            leaf_fraction: quality.min(1.0),
            ..Self::default()
        }
    }

    /// Checks the domain of every option.
    ///
    /// # Errors
    ///
    /// Returns [`MeshError::InvalidOption`] for a tolerance that is not positive and
    /// finite, a negative or NaN `min_radius`, a `leaf_fraction` outside `[0, 1]` or a
    /// `twig_leaf_fraction` outside `(0, 1]`.
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
        if self.max_drop_reach.is_nan() || self.max_drop_reach < 0.0 {
            return Err(invalid(
                "max_drop_reach",
                self.max_drop_reach,
                "zero or more",
            ));
        }
        if !(self.twig_leaf_fraction > 0.0 && self.twig_leaf_fraction <= 1.0) {
            let value = self.twig_leaf_fraction;
            return Err(invalid("twig_leaf_fraction", value, "in (0, 1]"));
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
    fn lower_quality_is_coarser_and_keeps_every_axis() {
        let half = MeshOptions::with_quality(0.5);
        let quarter = MeshOptions::with_quality(0.25);
        assert!((half.tolerance - 4.0 * DEFAULT_TOLERANCE).abs() < 1e-7);
        assert!(quarter.tolerance > half.tolerance);
        assert_eq!((half.min_radius, quarter.min_radius), (0.0, 0.0));
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
