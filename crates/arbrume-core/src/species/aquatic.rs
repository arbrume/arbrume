//! Aquatic layer: plants that grow under water, up to and along its surface.
//!
//! The table `[aquatic]` is optional. A species without it grows exactly as before; a
//! species with it grows in the same cycle with an orientation stage in which buoyancy
//! takes the place of gravitropism below the water surface, an exposure attenuated
//! with depth, and passes on the grown plant that bend its stems with the current and
//! float its canopy and leaves on the surface (`arbrume-grow`, module `aquatic`). The
//! water itself (the height of its surface, its current, its clarity) is part of the
//! surroundings of a plant; `water_depth` is the depth used when they give none.
//!
//! Optional sub-tables add organs of water plants: `[aquatic.holdfast]` the claw-like
//! haptera that anchor a kelp to rock, `[aquatic.tip_float]` one large float at the tip
//! of each stem that reaches the surface (a bull kelp), and `[aquatic.rhizome]` a
//! rhizome creeping on the bottom that bears clonal shoots (ramets), as in a seagrass
//! meadow.
//!
//! Lengths are in meters. Forces use SI units (newtons, pascals). The defaults are
//! starting points for tuning, not measurements; `docs/underwater.md` cites what was
//! measured and what was chosen.

use super::{SpeciesError, check_color, check_range};
use serde::{Deserialize, Serialize};

/// Most rhizome axes leaving the base of a meadow.
pub const MAX_RHIZOME_AXES: u32 = 16;
/// Most ramets of one meadow.
pub const MAX_RAMETS: u32 = 1024;
/// Most haptera of one holdfast.
pub const MAX_HAPTERA: u32 = 64;

/// What an axis does when it reaches the water surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SurfaceHabit {
    /// It keeps growing along the surface: the canopy of a giant kelp, the mats of a
    /// waterweed.
    #[default]
    Spread,
    /// It stops at the surface: the petiole of a floating leaf, the stipe of a bull kelp.
    Stop,
}

/// The haptera of a kelp: short branched claws from the base of each stem down to the
/// rock.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Holdfast {
    /// Haptera leaving the base of each stem, spread evenly around it.
    pub haptera: u32,
    /// Horizontal reach of a hapteron from the stem, in meters.
    pub reach: f32,
    /// Radius of a hapteron where it leaves the stem, in meters.
    pub radius: f32,
    /// Number of successive forks of a hapteron near its end, 0 to 3.
    pub forks: u32,
    /// Random variation of the reach and azimuth of each hapteron, in `[0, 1)`.
    pub variation: f32,
}

impl Default for Holdfast {
    fn default() -> Self {
        Self {
            haptera: 10,
            reach: 0.2,
            radius: 0.008,
            forks: 2,
            variation: 0.3,
        }
    }
}

/// One large float at the tip of each stem that reaches the surface, bearing a tuft of
/// blades.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TipFloat {
    /// Largest radius of the float, in meters.
    pub radius: f32,
    /// Length of the float along the stem, in meters.
    pub length: f32,
    /// Blades at the far end of the float, replacing the other leaves of the stem.
    pub blades: u32,
}

impl Default for TipFloat {
    fn default() -> Self {
        Self {
            radius: 0.06,
            length: 0.15,
            blades: 24,
        }
    }
}

/// A rhizome creeping on the bottom and bearing ramets, clonal shoots, at its nodes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Rhizome {
    /// Rhizome axes leaving the base of the plant, spread evenly around it.
    pub axes: u32,
    /// Distance between two ramets along a rhizome, in meters.
    pub spacing: f32,
    /// Rhizome nodes added at the tip of each rhizome axis per growth cycle.
    pub extension: u32,
    /// Probability that a node of a rhizome starts a lateral rhizome.
    pub branch_probability: f32,
    /// Angle between a lateral rhizome and its parent, in degrees.
    pub branch_angle_deg: f32,
    /// Half-width of the random turn of a rhizome at each node, in degrees.
    pub wander_deg: f32,
    /// Highest order of lateral rhizomes, 0 for unbranched rhizomes (at most 4).
    pub max_order: u32,
    /// Radius of the rhizome, in meters.
    pub radius: f32,
    /// Fraction of the radius of the rhizome sunk into the bottom, in `[0, 1]`.
    pub burial: f32,
    /// Most ramets of the meadow, the oldest kept.
    pub max_ramets: u32,
}

impl Default for Rhizome {
    fn default() -> Self {
        Self {
            axes: 2,
            spacing: 0.12,
            extension: 2,
            branch_probability: 0.12,
            branch_angle_deg: 55.0,
            wander_deg: 10.0,
            max_order: 2,
            radius: 0.003,
            burial: 0.5,
            max_ramets: 80,
        }
    }
}

/// Aquatic parameters of a species. See the [module documentation](self).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Aquatic {
    /// Height of the water surface above the base of the plant when the surroundings
    /// give no water, in meters.
    pub water_depth: f32,
    /// Weight in `[0, 1]` of the upward direction in the heading of a submerged shoot,
    /// blended after the tropism of the species: buoyant tissue rises whatever the
    /// gravitropic set-point.
    pub buoyancy: f32,
    /// What an axis does at the surface.
    pub surface: SurfaceHabit,
    /// Young's modulus of the stems, in pascals: how much they resist the current and
    /// their buoyancy.
    pub stiffness: f32,
    /// Net upward force on a stem per unit of its volume, as a fraction of the weight of
    /// the water it displaces, `1 - rho_tissue / rho_water`; negative for a stem that
    /// sinks.
    pub stem_buoyancy: f32,
    /// Drag coefficient of a stem across the flow.
    pub drag_coefficient: f32,
    /// Drag coefficient of a leaf per unit of its area, for a blade lying in the flow.
    pub leaf_drag_coefficient: f32,
    /// Net upward force on a leaf per unit of its area, in N/m², floats excluded.
    pub leaf_lift: f32,
    /// Weight in `[0, 1]` with which a submerged leaf turns towards the force on it
    /// (lift and drag): 0 rigid, 1 limp.
    pub leaf_flex: f32,
    /// Weight in `[0, 1]` with which the leaves of nodes at the surface lie flat on it.
    pub floating_leaves: f32,
    /// Each axis that reaches the surface ends in one leaf floating flat on it (a water
    /// lily pad); the other leaves are dropped.
    pub pads: bool,
    /// Length of the gas float (pneumatocyst) at the base of every blade, as a fraction
    /// of the leaf length; 0 for none.
    pub blade_float: f32,
    /// Colour of the floats, linear RGB.
    pub float_color: [f32; 3],
    /// Claw-like haptera at the base of each stem; `None` for none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub holdfast: Option<Holdfast>,
    /// A large float at the tip of each stem that reaches the surface; `None` for none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tip_float: Option<TipFloat>,
    /// A rhizome bearing ramets; `None` for a single plant.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rhizome: Option<Rhizome>,
}

impl Default for Aquatic {
    fn default() -> Self {
        Self {
            water_depth: 2.0,
            buoyancy: 0.3,
            surface: SurfaceHabit::Spread,
            stiffness: 5.0e6,
            stem_buoyancy: 0.05,
            drag_coefficient: 1.0,
            leaf_drag_coefficient: 0.05,
            leaf_lift: 1.0,
            leaf_flex: 0.6,
            floating_leaves: 1.0,
            pads: false,
            blade_float: 0.0,
            float_color: [0.35, 0.28, 0.08],
            holdfast: None,
            tip_float: None,
            rhizome: None,
        }
    }
}

impl Aquatic {
    pub(super) fn validate(&self) -> Result<(), SpeciesError> {
        let (tiny, max) = (f32::MIN_POSITIVE, f32::MAX);
        check_range("aquatic.water_depth", self.water_depth, tiny, max)?;
        check_range("aquatic.buoyancy", self.buoyancy, 0.0, 1.0)?;
        check_range("aquatic.stiffness", self.stiffness, tiny, max)?;
        check_range("aquatic.stem_buoyancy", self.stem_buoyancy, -1.0, 1.0)?;
        check_range("aquatic.drag_coefficient", self.drag_coefficient, 0.0, 10.0)?;
        let leaf_drag = self.leaf_drag_coefficient;
        check_range("aquatic.leaf_drag_coefficient", leaf_drag, 0.0, 10.0)?;
        check_range("aquatic.leaf_lift", self.leaf_lift, -1.0e4, 1.0e4)?;
        check_range("aquatic.leaf_flex", self.leaf_flex, 0.0, 1.0)?;
        check_range("aquatic.floating_leaves", self.floating_leaves, 0.0, 1.0)?;
        check_range("aquatic.blade_float", self.blade_float, 0.0, 0.5)?;
        check_color("aquatic.float_color", self.float_color)?;
        if let Some(holdfast) = &self.holdfast {
            holdfast.validate()?;
        }
        if let Some(float) = &self.tip_float {
            float.validate()?;
        }
        if let Some(rhizome) = &self.rhizome {
            rhizome.validate()?;
        }
        Ok(())
    }
}

impl Holdfast {
    fn validate(&self) -> Result<(), SpeciesError> {
        let haptera = self.haptera as f32;
        check_range("aquatic.holdfast.haptera", haptera, 1.0, MAX_HAPTERA as f32)?;
        let tiny = f32::MIN_POSITIVE;
        check_range("aquatic.holdfast.reach", self.reach, tiny, 100.0)?;
        check_range("aquatic.holdfast.radius", self.radius, tiny, 10.0)?;
        check_range("aquatic.holdfast.forks", self.forks as f32, 0.0, 3.0)?;
        let variation = self.variation;
        check_range("aquatic.holdfast.variation", variation, 0.0, 0.99)
    }
}

impl TipFloat {
    fn validate(&self) -> Result<(), SpeciesError> {
        let tiny = f32::MIN_POSITIVE;
        check_range("aquatic.tip_float.radius", self.radius, tiny, 10.0)?;
        check_range("aquatic.tip_float.length", self.length, tiny, 10.0)?;
        check_range("aquatic.tip_float.blades", self.blades as f32, 0.0, 256.0)
    }
}

impl Rhizome {
    fn validate(&self) -> Result<(), SpeciesError> {
        let tiny = f32::MIN_POSITIVE;
        let axes = self.axes as f32;
        check_range("aquatic.rhizome.axes", axes, 1.0, MAX_RHIZOME_AXES as f32)?;
        check_range("aquatic.rhizome.spacing", self.spacing, tiny, 100.0)?;
        check_range(
            "aquatic.rhizome.extension",
            self.extension as f32,
            1.0,
            64.0,
        )?;
        let probability = self.branch_probability;
        check_range("aquatic.rhizome.branch_probability", probability, 0.0, 1.0)?;
        let angle = self.branch_angle_deg;
        check_range("aquatic.rhizome.branch_angle_deg", angle, 0.0, 180.0)?;
        check_range("aquatic.rhizome.wander_deg", self.wander_deg, 0.0, 90.0)?;
        check_range("aquatic.rhizome.max_order", self.max_order as f32, 0.0, 4.0)?;
        check_range("aquatic.rhizome.radius", self.radius, tiny, 10.0)?;
        check_range("aquatic.rhizome.burial", self.burial, 0.0, 1.0)?;
        let ramets = self.max_ramets as f32;
        check_range("aquatic.rhizome.max_ramets", ramets, 1.0, MAX_RAMETS as f32)
    }
}

#[cfg(test)]
mod tests {
    use super::{Aquatic, Holdfast, Rhizome, TipFloat};
    use crate::species::{Species, SpeciesError};

    #[test]
    fn defaults_are_valid_and_domains_are_checked() {
        let full = Aquatic {
            holdfast: Some(Holdfast::default()),
            tip_float: Some(TipFloat::default()),
            rhizome: Some(Rhizome::default()),
            ..Default::default()
        };
        full.validate().unwrap();
        let invalid = [
            Aquatic {
                water_depth: 0.0,
                ..Default::default()
            },
            Aquatic {
                buoyancy: 1.5,
                ..Default::default()
            },
            Aquatic {
                leaf_flex: f32::NAN,
                ..Default::default()
            },
            Aquatic {
                holdfast: Some(Holdfast {
                    forks: 4,
                    ..Default::default()
                }),
                ..Default::default()
            },
            Aquatic {
                rhizome: Some(Rhizome {
                    axes: 0,
                    ..Default::default()
                }),
                ..Default::default()
            },
        ];
        for aquatic in invalid {
            assert!(aquatic.validate().is_err(), "{aquatic:?}");
        }
    }

    #[test]
    fn the_table_parses_round_trips_and_is_optional() {
        assert!(Species::default().aquatic.is_none());
        let text = "[aquatic]\nwater_depth = 6.0\nsurface = \"stop\"\n\
                    [aquatic.rhizome]\nspacing = 0.2\n[aquatic.holdfast]\nhaptera = 6";
        let species = Species::from_toml_str(text).unwrap();
        let aquatic = species.aquatic.as_ref().unwrap();
        assert_eq!(aquatic.surface, super::SurfaceHabit::Stop);
        assert!((aquatic.rhizome.as_ref().unwrap().spacing - 0.2).abs() < 1e-6);
        assert!(aquatic.tip_float.is_none());
        let back = Species::from_toml_str(&species.to_toml_string().unwrap()).unwrap();
        assert_eq!(back, species);
        let bad = Species::from_toml_str("[aquatic]\nbuoyancy = 2.0");
        assert!(matches!(bad, Err(SpeciesError::Invalid { .. })));
        let unknown = Species::from_toml_str("[aquatic]\nbuoyance = 0.2");
        assert!(matches!(unknown, Err(SpeciesError::Parse(_))));
    }

    #[test]
    fn climbing_and_aquatic_are_exclusive() {
        let both = Species::from_toml_str("[aquatic]\n[climbing]");
        assert!(matches!(
            both,
            Err(SpeciesError::Invalid {
                field: "aquatic",
                ..
            })
        ));
    }
}
