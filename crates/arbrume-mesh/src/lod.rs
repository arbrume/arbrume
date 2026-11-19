//! Level-of-detail chains: several meshes of one plant graph, each within a triangle
//! budget.
//!
//! Detail is removed from the graph, not by decimating a mesh. A level is the mesh of
//! [`level_options`] for one quality `q` in `(0, 1]`:
//!
//! * the tolerance is that of the base options divided by `q^2`, so sides, rings and
//!   leaf blade segments drop roughly in proportion to `q`;
//! * axes thinner than [`LodOptions::radius_factor`] times that tolerance are dropped
//!   with what they bear, and their leaves are reattached to the nearest kept ancestor
//!   ([`OrphanLeaves::Reattach`]), so no leaf floats;
//! * the leaf buffer keeps its first `q` fraction of leaves (the stochastic
//!   simplification of Cook et al. 2007: leaves are stored in a random order, any prefix
//!   is an unbiased sample, and the kept leaves are enlarged to preserve the total area);
//! * twig templates keep a fraction `q^twig_exponent` of their leaves, enlarged likewise.
//!
//! Level 0 is the base options unchanged. The budget of level `k` is
//! `triangle_ratio^k` times the triangles of level 0, and its quality is found from the
//! counts of the planning pass (no mesh is built): starting from the quality the ratio
//! predicts, it is corrected by the square root of the budget over the count, a few
//! times at most, keeping the largest count within the budget and within the count of
//! the level above. Every level shares the graph, the seed and the leaf order, so the
//! leaves of a level are a prefix of the leaves of the level above.
//!
//! **Screen size.** A level is drawn without visible loss while its tolerance projects to
//! at most [`LodOptions::pixel_error`] pixels: for a plant of height `h` and a level of
//! tolerance `e`, while the plant spans at most `pixel_error * h / e` pixels on screen.
//! This hint ([`LodLevel::max_screen_height`]) is geometric only; the thinning of the
//! leaves follows the same quality and is not measured separately.

use crate::{MeshCounts, MeshError, MeshOptions, Mesher, OrphanLeaves};
use arbrume_core::graph::PlantGraph;
use arbrume_core::species::Species;

/// Fraction of the base radius of the thinnest root axis that the radius cut of a level
/// may reach, so that a level never loses its trunk.
const ROOT_SHARE: f32 = 0.5;

/// The largest radius cut that keeps every root axis of `graph`.
fn radius_cap(graph: &PlantGraph) -> f32 {
    graph
        .chain_ids()
        .filter_map(|chain| graph.chain_view(chain))
        .filter(|view| view.attach.is_none() && !view.radii.is_empty())
        .map(|view| ROOT_SHARE * view.radii[0])
        .fold(f32::INFINITY, f32::min)
}

/// Corrections of the quality of one level at most.
const SEARCH_STEPS: usize = 6;

/// A level is accepted at once when its count reaches this fraction of its budget.
const CLOSE_ENOUGH: f32 = 0.9;

/// Smallest quality of a level.
const MIN_QUALITY: f32 = 0.01;

/// Safety factor on each quality correction, so that it lands under the budget.
const UNDERSHOOT: f32 = 0.97;

/// Controls of [`Mesher::lod_chain`]. The defaults were chosen by eye on the presets;
/// they are starting points, not measured optima.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LodOptions {
    /// Number of mesh levels, level 0 included (at least 1).
    pub levels: usize,
    /// Triangle budget of each level divided by that of the level above, in `(0, 1)`.
    pub triangle_ratio: f32,
    /// Axes whose base radius is below this many tolerances of their level are dropped
    /// (from level 1 on).
    pub radius_factor: f32,
    /// A dropped axis and what it bears reach at most this many tolerances of their
    /// level from its base (`MeshOptions::max_drop_reach`), which bounds how far its
    /// leaves move.
    pub reach_factor: f32,
    /// Exponent of the quality giving the fraction of the leaves a twig keeps.
    pub twig_exponent: f32,
    /// Largest projected tolerance, in pixels, for the screen-size hint.
    pub pixel_error: f32,
}

impl Default for LodOptions {
    fn default() -> Self {
        Self {
            levels: 3,
            triangle_ratio: 0.3,
            radius_factor: 1.0,
            reach_factor: 10.0,
            twig_exponent: 0.5,
            pixel_error: 1.0,
        }
    }
}

impl LodOptions {
    /// Checks the domain of every option.
    ///
    /// # Errors
    ///
    /// Returns [`MeshError::InvalidOption`] for no level, a ratio outside `(0, 1)`, a
    /// negative radius factor or twig exponent, or a pixel error that is not positive.
    pub fn validate(&self) -> Result<(), MeshError> {
        let invalid = |field, reason: &str| MeshError::InvalidOption {
            field,
            reason: reason.to_owned(),
        };
        if self.levels == 0 {
            return Err(invalid("levels", "at least one level is needed"));
        }
        if !(self.triangle_ratio > 0.0 && self.triangle_ratio < 1.0) {
            return Err(invalid("triangle_ratio", "must be in (0, 1)"));
        }
        if !(self.radius_factor >= 0.0 && self.twig_exponent >= 0.0 && self.reach_factor >= 0.0) {
            return Err(invalid(
                "radius_factor",
                "radius_factor, reach_factor and twig_exponent must be >= 0",
            ));
        }
        if !(self.pixel_error > 0.0 && self.pixel_error.is_finite()) {
            return Err(invalid("pixel_error", "must be positive"));
        }
        Ok(())
    }
}

/// One level of a chain.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LodLevel {
    /// Mesh options of the level; mesh the graph with them to get the level.
    pub options: MeshOptions,
    /// Quality of the level, 1 for level 0.
    pub quality: f32,
    /// Triangle budget of the level.
    pub budget: usize,
    /// Sizes of the mesh of the level.
    pub counts: MeshCounts,
    /// Height of the plant on screen, in pixels, up to which the tolerance of the level
    /// projects to at most `pixel_error` pixels.
    pub max_screen_height: f32,
}

/// The mesh options of quality `quality` derived from `base`, as described in the
/// module header, with a radius cut of at most `radius_cap` meters. `quality` 1 gives
/// `base` back.
#[must_use]
pub fn level_options(
    base: &MeshOptions,
    quality: f32,
    lod: &LodOptions,
    radius_cap: f32,
) -> MeshOptions {
    if quality >= 1.0 {
        return *base;
    }
    let quality = quality.max(MIN_QUALITY);
    let tolerance = base.tolerance / (quality * quality);
    MeshOptions {
        tolerance,
        min_radius: base
            .min_radius
            .max((lod.radius_factor * tolerance).min(radius_cap)),
        max_drop_reach: base.max_drop_reach.min(lod.reach_factor * tolerance),
        leaf_fraction: base.leaf_fraction * quality,
        twig_leaf_fraction: (base.twig_leaf_fraction * quality.powf(lod.twig_exponent))
            .clamp(f32::MIN_POSITIVE, 1.0),
        orphan_leaves: OrphanLeaves::Reattach,
        ..*base
    }
}

impl Mesher {
    /// Plans a level-of-detail chain of `graph`: the options, counts, budget and screen
    /// size of each level (see the [module documentation](crate::lod)). Nothing is
    /// meshed; mesh each level with its options.
    ///
    /// # Errors
    ///
    /// The same as [`crate::mesh_tree`], and [`MeshError::InvalidOption`] for invalid
    /// `lod` options.
    pub fn lod_chain(
        &mut self,
        graph: &PlantGraph,
        species: &Species,
        base: &MeshOptions,
        lod: &LodOptions,
    ) -> Result<Vec<LodLevel>, MeshError> {
        lod.validate()?;
        self.radius_cap = radius_cap(graph);
        let height = graph
            .bounds()
            .map_or(1.0, |bounds| bounds.size().y.max(1e-3));
        let hint = |options: &MeshOptions| lod.pixel_error * height / options.tolerance;
        let counts = self.counts(graph, species, base)?;
        let mut levels = vec![LodLevel {
            options: *base,
            quality: 1.0,
            budget: counts.triangles(),
            counts,
            max_screen_height: hint(base),
        }];
        for k in 1..lod.levels {
            let above = levels[k - 1];
            let budget = (levels[0].budget as f64 * f64::from(lod.triangle_ratio).powf(k as f64))
                .floor() as usize;
            let limit = budget.min(above.counts.triangles());
            let (quality, counts) = self.search(graph, species, base, lod, &above, limit)?;
            let options = level_options(base, quality, lod, self.radius_cap);
            levels.push(LodLevel {
                options,
                quality,
                budget,
                counts,
                max_screen_height: hint(&options),
            });
        }
        Ok(levels)
    }

    /// The largest quality below that of `above` found whose mesh fits `limit`
    /// triangles, with its counts; the smallest quality tried when none fits.
    fn search(
        &mut self,
        graph: &PlantGraph,
        species: &Species,
        base: &MeshOptions,
        lod: &LodOptions,
        above: &LodLevel,
        limit: usize,
    ) -> Result<(f32, MeshCounts), MeshError> {
        let ratio = |count: usize| (limit.max(1) as f32 / count.max(1) as f32).sqrt();
        let mut quality = (above.quality * ratio(above.counts.triangles()) * UNDERSHOOT)
            .clamp(MIN_QUALITY, above.quality);
        let mut best: Option<(f32, MeshCounts)> = None;
        let mut last = (quality, MeshCounts::default());
        for _ in 0..SEARCH_STEPS {
            let options = level_options(base, quality, lod, self.radius_cap);
            let counts = self.counts(graph, species, &options)?;
            let triangles = counts.triangles();
            last = (quality, counts);
            if triangles <= limit {
                if best.is_none_or(|(_, b)| triangles > b.triangles()) {
                    best = Some((quality, counts));
                }
                if triangles as f32 >= CLOSE_ENOUGH * limit as f32 {
                    break;
                }
            }
            let next = (quality * ratio(triangles) * UNDERSHOOT).clamp(MIN_QUALITY, above.quality);
            if (next - quality).abs() <= 1e-4 * quality {
                break;
            }
            quality = next;
        }
        Ok(best.unwrap_or(last))
    }
}
