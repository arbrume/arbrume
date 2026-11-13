//! Foliage layer: the leaves the mesher instances at organ sites.
//!
//! Where leaves sit around an axis is the phyllotaxis of the architecture layer; this
//! layer describes the leaf itself: its size, the outline of its blade
//! ([`BladeShape`]), how the blade is folded and curved, its petiole, whether it is
//! compound, and how much leaves differ from one another. All defaults are starting
//! points for tuning, not measured values.

use super::{SpeciesError, check_color, check_range};
use crate::curve::Curve;
use serde::{Deserialize, Serialize};

/// Largest value of [`Foliage::segments`].
pub const MAX_BLADE_SEGMENTS: u32 = 16;

/// Largest value of [`Foliage::leaflet_pairs`].
pub const MAX_LEAFLET_PAIRS: u32 = 32;

/// Outline of a leaf blade (or of each leaflet of a compound leaf).
///
/// The half-width of the blade at the relative position `t` along the midrib is
/// proportional to `t^a (1 - t)^b`; the shape chooses the exponents and so where the
/// blade is widest (at `t = a / (a + b)`). See [`Foliage::blade_width`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BladeShape {
    /// An ellipse, widest at the middle.
    Elliptic,
    /// Egg-shaped, widest near the base.
    #[default]
    Ovate,
    /// Egg-shaped, widest near the tip.
    Obovate,
    /// Widest near the base with a long tapering tip.
    Lanceolate,
    /// Nearly parallel sides with a short point at both ends.
    Needle,
}

impl BladeShape {
    /// Exponents `(a, b)` of the outline `t^a (1 - t)^b`.
    #[must_use]
    pub const fn exponents(self) -> (f32, f32) {
        match self {
            Self::Elliptic => (0.5, 0.5),
            Self::Ovate => (0.45, 0.9),
            Self::Obovate => (0.9, 0.45),
            Self::Lanceolate => (0.5, 1.5),
            Self::Needle => (0.15, 0.3),
        }
    }
}

/// What the mesher builds for one leaf.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LeafGeometry {
    /// A small mesh following the outline of the blade, folded and curved.
    #[default]
    Outline,
    /// A flat rectangle covering the leaf, meant for an alpha-masked texture.
    Card,
}

/// Largest value of [`Twig::leaves`].
pub const MAX_TWIG_LEAVES: u32 = 64;
/// Largest value of [`Twig::fins`].
pub const MAX_TWIG_FINS: u32 = 8;
/// Largest value of [`Twig::variants`].
pub const MAX_TWIG_VARIANTS: u32 = 16;

/// A leafy twig instanced at every organ site instead of a single leaf.
///
/// The twig is a small template built by the mesher: a thin stem carrying several
/// leaves, placed around it like the leaves of a real shoot. A tree uses a few variants
/// of its template, which differ in the size, attitude and shade of their leaves
/// (Raistrick et al. 2023 instance twigs the same way). With `inclination_deg` of the
/// foliage at 0 the twig lies along the axis that bears it, which is how the needles of
/// a conifer shoot are drawn: one twig stands for the needles of one node.
///
/// Needles are far too many and too thin to draw one by one. A needle-bearing twig
/// therefore has `fins`: flat blades along its stem, as wide as the needles reach, that
/// stand for the bulk of the needles, while a few enlarged needles fringe the outline.
///
/// `leaves = 0` with no fins disables twigs: each site then bears one leaf.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Twig {
    /// Number of leaves on one twig, at most [`MAX_TWIG_LEAVES`]; 0 for no twig.
    pub leaves: u32,
    /// Length of the twig stem, in meters.
    pub length: f32,
    /// Leaves at each node of the twig: 1 alternate, 2 opposite, more for a whorl.
    pub leaves_per_node: u8,
    /// Angle in degrees by which successive nodes of the twig are turned: 137.5 for a
    /// spiral, 180 for two flat ranks, 90 with two leaves per node for decussate pairs.
    pub divergence_deg: f32,
    /// Angle in degrees between a leaf and the twig, 0 pointing along the twig.
    pub inclination_deg: f32,
    /// Total angle in degrees by which the twig bends from base to tip, positive
    /// towards the lower side of its organ site.
    pub bend_deg: f32,
    /// Radius of the twig stem at its base, in meters; 0 for no stem (a twig that lies
    /// on the axis bearing it needs none).
    pub stem_radius: f32,
    /// Whether a leaf continues the twig at its tip.
    pub terminal_leaf: bool,
    /// Attitude of the leaves. `false`: the upper side of every leaf faces the upper
    /// side of the organ site, as on a flat spray. `true`: the plane of every leaf
    /// contains the twig, as for needles standing around a shoot.
    pub radial: bool,
    /// Number of flat blades along the stem, evenly turned around it, at most
    /// [`MAX_TWIG_FINS`]; 0 for none.
    pub fins: u32,
    /// Half-width of a fin, in meters.
    pub fin_width: f32,
    /// Brightness of the fins relative to the leaves, in `[0, 1]`: the inside of a
    /// needle-clad shoot is darker than its fringe.
    pub fin_shade: f32,
    /// Number of variants of the template used by one plant, from 1 to
    /// [`MAX_TWIG_VARIANTS`].
    pub variants: u32,
}

impl Default for Twig {
    fn default() -> Self {
        Self {
            leaves: 0,
            length: 0.12,
            leaves_per_node: 1,
            divergence_deg: 137.5,
            inclination_deg: 50.0,
            bend_deg: 15.0,
            stem_radius: 0.002,
            terminal_leaf: true,
            radial: false,
            fins: 0,
            fin_width: 0.02,
            fin_shade: 0.8,
            variants: 4,
        }
    }
}

impl Twig {
    /// `true` when sites bear twigs instead of single leaves.
    #[must_use]
    pub const fn enabled(&self) -> bool {
        self.leaves > 0 || self.fins > 0
    }

    fn validate(&self) -> Result<(), SpeciesError> {
        let max = f32::MAX;
        let leaves = self.leaves as f32;
        check_range("foliage.twig.leaves", leaves, 0.0, MAX_TWIG_LEAVES as f32)?;
        check_range("foliage.twig.length", self.length, f32::MIN_POSITIVE, max)?;
        let per_node = f32::from(self.leaves_per_node);
        check_range("foliage.twig.leaves_per_node", per_node, 1.0, 255.0)?;
        let divergence = self.divergence_deg;
        check_range("foliage.twig.divergence_deg", divergence, 0.0, 360.0)?;
        let inclination = self.inclination_deg;
        check_range("foliage.twig.inclination_deg", inclination, 0.0, 180.0)?;
        check_range("foliage.twig.bend_deg", self.bend_deg, -180.0, 180.0)?;
        check_range("foliage.twig.stem_radius", self.stem_radius, 0.0, max)?;
        let fins = self.fins as f32;
        check_range("foliage.twig.fins", fins, 0.0, MAX_TWIG_FINS as f32)?;
        check_range("foliage.twig.fin_width", self.fin_width, 0.0, max)?;
        check_range("foliage.twig.fin_shade", self.fin_shade, 0.0, 1.0)?;
        let variants = self.variants as f32;
        check_range(
            "foliage.twig.variants",
            variants,
            1.0,
            MAX_TWIG_VARIANTS as f32,
        )
    }
}

/// Leaf parameters of a species.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Foliage {
    /// Whether the plant bears leaves at all.
    pub enabled: bool,
    /// Length of a full-size leaf blade, in meters. For a compound leaf, the length of
    /// the whole leaf without its petiole.
    pub length: f32,
    /// Blade width divided by blade length. For a compound leaf, of each leaflet.
    pub width_ratio: f32,
    /// Leaf size factor over the position of the leaf along its axis (0 base, 1 tip).
    pub size_along_axis: Curve,
    /// Angle in degrees between the leaf and the axis bearing it, 0 pointing along the
    /// axis towards its tip and 90 perpendicular to it.
    pub inclination_deg: f32,
    /// Number of growth cycles a metamer keeps its leaves: 1 for a plant that carries
    /// leaves only on the shoots of the current cycle, more for evergreens.
    pub retention_cycles: u16,
    /// Probability in `(0, 1]` that a leaf position of a retained node bears a leaf (or
    /// a twig): below 1 the foliage is thinned at random.
    pub density: f32,
    /// Number of leaf positions on each node, spread evenly around it; 0 for those of
    /// the phyllotaxis. A count above that of the phyllotaxis thickens the foliage without
    /// adding buds.
    pub per_node: u8,
    /// Lowest branching order that bears leaves: 1 keeps the stems bare.
    pub min_order: u8,
    /// Twig instanced at each site instead of a single leaf.
    pub twig: Twig,
    /// Linear RGB base colour.
    pub color: [f32; 3],
    /// Surface roughness in `[0, 1]`.
    pub roughness: f32,
    /// Path of the leaf texture, relative to the species file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub texture: Option<String>,
    /// Kind of geometry built for each leaf.
    pub geometry: LeafGeometry,
    /// Outline of the blade.
    pub shape: BladeShape,
    /// Number of segments along the midrib of an outline blade, from 2 (a diamond of
    /// four triangles) to [`MAX_BLADE_SEGMENTS`]. The mesher may use fewer for a coarse
    /// tolerance. 1 draws the blade as a single triangle, widest at its base, which is
    /// meant for needles.
    pub segments: u32,
    /// Angle in degrees by which each half of the blade is raised about the midrib, 0
    /// for a flat blade.
    pub fold_deg: f32,
    /// Total angle in degrees by which the blade bends from base to tip, positive
    /// towards its lower side (drooping), negative towards its upper side.
    pub curvature_deg: f32,
    /// Length of the petiole (leaf stalk) divided by the leaf length, 0 for a sessile
    /// leaf.
    pub petiole_length: f32,
    /// Number of pairs of lateral leaflets of a pinnately compound leaf, 0 for a simple
    /// leaf. A compound leaf also has a terminal leaflet.
    pub leaflet_pairs: u32,
    /// Angle in degrees between a lateral leaflet and the rachis of a compound leaf.
    pub leaflet_angle_deg: f32,
    /// Relative variation of the size of individual leaves, in `[0, 1)`: sizes are
    /// spread uniformly within this fraction around the nominal size.
    pub size_variation: f32,
    /// Largest random rotation of an individual leaf away from the attitude of its
    /// organ site, in degrees, about each of its three axes; at most 45.
    pub orientation_variation_deg: f32,
    /// Variation of leaf hue in `[0, 1]`: the red and blue channels of a leaf are scaled
    /// in opposite directions by up to this fraction.
    pub hue_variation: f32,
    /// Variation of leaf brightness in `[0, 1]`: a leaf is darkened by up to this
    /// fraction.
    pub brightness_variation: f32,
}

impl Default for Foliage {
    fn default() -> Self {
        Self {
            enabled: true,
            length: 0.08,
            width_ratio: 0.5,
            size_along_axis: Curve::constant(1.0),
            inclination_deg: 45.0,
            retention_cycles: 1,
            density: 1.0,
            per_node: 0,
            min_order: 0,
            twig: Twig::default(),
            color: [0.12, 0.3, 0.06],
            roughness: 0.6,
            texture: None,
            geometry: LeafGeometry::default(),
            shape: BladeShape::default(),
            segments: 4,
            fold_deg: 15.0,
            curvature_deg: 25.0,
            petiole_length: 0.15,
            leaflet_pairs: 0,
            leaflet_angle_deg: 55.0,
            size_variation: 0.15,
            orientation_variation_deg: 12.0,
            hue_variation: 0.08,
            brightness_variation: 0.25,
        }
    }
}

impl Foliage {
    /// Half-width of the blade at the relative position `t` along the midrib (0 base,
    /// 1 tip), as a fraction of the largest half-width: 0 at both ends and 1 where the
    /// blade is widest.
    #[must_use]
    pub fn blade_width(&self, t: f32) -> f32 {
        let (a, b) = self.shape.exponents();
        let t = t.clamp(0.0, 1.0);
        let widest = a / (a + b);
        let peak = widest.powf(a) * (1.0 - widest).powf(b);
        t.powf(a) * (1.0 - t).powf(b) / peak
    }

    /// `true` when leaves are flat textured cards instead of outline meshes.
    #[must_use]
    pub fn uses_cards(&self) -> bool {
        self.geometry == LeafGeometry::Card
    }

    pub(super) fn validate(&self) -> Result<(), SpeciesError> {
        let (tiny, max) = (f32::MIN_POSITIVE, f32::MAX);
        check_range("foliage.length", self.length, tiny, max)?;
        check_range("foliage.width_ratio", self.width_ratio, tiny, max)?;
        let (size_lo, size_hi) = self.size_along_axis.bounds();
        check_range("foliage.size_along_axis", size_lo, 0.0, max)?;
        check_range("foliage.size_along_axis", size_hi, 0.0, max)?;
        check_range("foliage.inclination_deg", self.inclination_deg, 0.0, 180.0)?;
        check_color("foliage.color", self.color)?;
        check_range("foliage.roughness", self.roughness, 0.0, 1.0)?;
        check_range("foliage.density", self.density, f32::MIN_POSITIVE, 1.0)?;
        self.twig.validate()?;
        self.validate_blade()?;
        self.validate_variation()
    }

    fn validate_blade(&self) -> Result<(), SpeciesError> {
        let segments = self.segments as f32;
        check_range("foliage.segments", segments, 1.0, MAX_BLADE_SEGMENTS as f32)?;
        check_range("foliage.fold_deg", self.fold_deg, 0.0, 90.0)?;
        check_range("foliage.curvature_deg", self.curvature_deg, -180.0, 180.0)?;
        check_range("foliage.petiole_length", self.petiole_length, 0.0, f32::MAX)?;
        let pairs = self.leaflet_pairs as f32;
        check_range(
            "foliage.leaflet_pairs",
            pairs,
            0.0,
            MAX_LEAFLET_PAIRS as f32,
        )?;
        check_range(
            "foliage.leaflet_angle_deg",
            self.leaflet_angle_deg,
            0.0,
            90.0,
        )
    }

    fn validate_variation(&self) -> Result<(), SpeciesError> {
        let size = self.size_variation;
        if !(0.0..1.0).contains(&size) {
            return Err(SpeciesError::Invalid {
                field: "foliage.size_variation",
                reason: format!("{size} is outside [0, 1)"),
            });
        }
        let turn = self.orientation_variation_deg;
        check_range("foliage.orientation_variation_deg", turn, 0.0, 45.0)?;
        check_range("foliage.hue_variation", self.hue_variation, 0.0, 1.0)?;
        let brightness = self.brightness_variation;
        check_range("foliage.brightness_variation", brightness, 0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::{BladeShape, Foliage, LeafGeometry, Twig};

    const SHAPES: [BladeShape; 5] = [
        BladeShape::Elliptic,
        BladeShape::Ovate,
        BladeShape::Obovate,
        BladeShape::Lanceolate,
        BladeShape::Needle,
    ];

    fn width(shape: BladeShape, t: f32) -> f32 {
        let foliage = Foliage {
            shape,
            ..Default::default()
        };
        foliage.blade_width(t)
    }

    #[test]
    fn blade_outline_is_closed_and_normalized() {
        for shape in SHAPES {
            assert!(width(shape, 0.0).abs() < 1e-6, "{shape:?}");
            assert!(width(shape, 1.0).abs() < 1e-6, "{shape:?}");
            let (a, b) = shape.exponents();
            assert!((width(shape, a / (a + b)) - 1.0).abs() < 1e-5);
            let mut widths = (0..=100).map(|i| width(shape, i as f32 / 100.0));
            assert!(widths.all(|w| (0.0..=1.0 + 1e-5).contains(&w)));
        }
    }

    #[test]
    fn shapes_differ_in_where_they_are_widest() {
        assert!(width(BladeShape::Ovate, 0.25) > width(BladeShape::Ovate, 0.75));
        assert!(width(BladeShape::Obovate, 0.25) < width(BladeShape::Obovate, 0.75));
        let elliptic = width(BladeShape::Elliptic, 0.25) - width(BladeShape::Elliptic, 0.75);
        assert!(elliptic.abs() < 1e-5);
        assert!(width(BladeShape::Needle, 0.5) > 0.95);
    }

    #[test]
    fn blade_parameters_are_validated() {
        let invalid = [
            Foliage {
                segments: 0,
                ..Default::default()
            },
            Foliage {
                density: 0.0,
                ..Default::default()
            },
            Foliage {
                twig: Twig {
                    leaves: 1000,
                    ..Twig::default()
                },
                ..Default::default()
            },
            Foliage {
                fold_deg: 120.0,
                ..Default::default()
            },
            Foliage {
                leaflet_pairs: 100,
                ..Default::default()
            },
            Foliage {
                size_variation: 1.0,
                ..Default::default()
            },
            Foliage {
                hue_variation: f32::NAN,
                ..Default::default()
            },
            Foliage {
                roughness: 2.0,
                ..Default::default()
            },
        ];
        let card = Foliage {
            geometry: LeafGeometry::Card,
            ..Default::default()
        };
        card.validate().unwrap();
        assert!(card.uses_cards() && !Foliage::default().uses_cards());
        for foliage in invalid {
            assert!(foliage.validate().is_err(), "{foliage:?}");
        }
    }
}
