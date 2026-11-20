//! Aquatic species in a forest: kelp forests, ponds, seagrass meadows.
//!
//! A species with an `[aquatic]` layer is placed under the water of
//! `placement.water_level`, where land species are not: its rule is `underwater`, so
//! the exclusions free the ground below the water level for it and close the ground
//! above. Its `depth = [shallow, deep]` (meters below the water level) becomes its
//! altitude window, `depth_falloff` the fade of that window.
//!
//! The variants of an aquatic species are grown in still water as deep as its own
//! `aquatic.water_depth`, the depth its parameters were tuned for (the length of a kelp
//! frond is absolute, so a variant grown much deeper would lose its canopy). Each tree
//! then stands over its own ground: with
//! `fit_depth` (the default) its scale becomes the depth of the water over its base
//! divided by the depth of its variant, so that its surface canopy, floats and pads
//! reach the surface wherever it stands. The scale of the rule only sets the spacing of
//! such trees. Trees are kept within [`FIT_RANGE`] of the depth of their variant.

use arbrume_core::species::Species;

use crate::config::SpeciesConfig;
use crate::library::VariantSpec;
use crate::place::Instance;
use crate::rules::SpeciesRule;

/// Smallest and largest ratio of the depth over a tree to the depth of its variant.
pub const FIT_RANGE: [f32; 2] = [0.25, 4.0];

/// What the water changes in the rule of a species.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct WaterPlacement {
    /// The species grows under water.
    pub underwater: bool,
    /// Ground heights taken fully, from the depth window.
    pub altitude: Option<[f32; 2]>,
    /// Fade outside them, from the depth window.
    pub falloff: Option<f32>,
}

/// The water placement of `species` given by `entry`, under `water_level`.
///
/// # Errors
/// A message for an aquatic species without a water level, or a depth window on a land
/// species or out of order.
pub fn placement(
    entry: &SpeciesConfig,
    species: &Species,
    water_level: Option<f32>,
) -> Result<WaterPlacement, String> {
    if species.aquatic.is_none() {
        if entry.depth.is_some() {
            return Err("`depth` is only read for aquatic species".to_owned());
        }
        return Ok(WaterPlacement::default());
    }
    let Some(level) = water_level else {
        return Err("an aquatic species needs `placement.water_level`".to_owned());
    };
    let mut placement = WaterPlacement {
        underwater: true,
        ..WaterPlacement::default()
    };
    if let Some([shallow, deep]) = entry.depth {
        if !(shallow >= 0.0 && deep >= shallow && deep.is_finite()) {
            return Err(format!(
                "depth [{shallow}, {deep}] is not 0 <= shallow <= deep"
            ));
        }
        placement.altitude = Some([level - deep, level - shallow]);
        placement.falloff = Some(entry.depth_falloff.max(0.0));
    }
    Ok(placement)
}

/// The depth the variants of `species` are grown at; `None` for a land species.
#[must_use]
pub fn variant_depth(species: &Species) -> Option<f32> {
    species.aquatic.as_ref().map(|aquatic| aquatic.water_depth)
}

/// Scales every tree of an aquatic species that fits its depth so that its canopy
/// meets the water at `water_level`. See the [module documentation](self).
pub fn fit_to_water(
    instances: &mut [Instance],
    specs: &[VariantSpec],
    rules: &[SpeciesRule],
    fits: &[bool],
    water_level: Option<f32>,
) {
    let Some(level) = water_level else {
        return;
    };
    for tree in instances {
        let s = usize::from(tree.species);
        if !rules[s].underwater || !fits[s] {
            continue;
        }
        let Some(grown) = variant_depth(&specs[s].species) else {
            continue;
        };
        let depth = level - tree.position[1];
        let ratio = (depth / grown).clamp(FIT_RANGE[0], FIT_RANGE[1]);
        tree.scale = ratio;
    }
}

#[cfg(test)]
mod tests {
    use super::{FIT_RANGE, placement, variant_depth};
    use crate::config::ForestConfig;

    fn entry(extra: &str) -> crate::config::SpeciesConfig {
        let text = format!(
            "[terrain]\nsize = [10.0, 10.0]\n[[species]]\nspecies = \"kelp_like\"\ndensity = 10.0\n{extra}"
        );
        ForestConfig::from_toml_str(&text)
            .unwrap()
            .species
            .remove(0)
    }

    #[test]
    fn depth_windows_become_altitude_windows_under_the_water_level() {
        let kelp = arbrume_gen::preset("kelp_like").unwrap();
        let windowed = entry("depth = [6.0, 14.0]\ndepth_falloff = 2.0");
        let water = placement(&windowed, &kelp, Some(-1.0)).unwrap();
        assert!(water.underwater);
        assert_eq!(water.altitude, Some([-15.0, -7.0]));
        assert_eq!(water.falloff, Some(2.0));
        assert_eq!(variant_depth(&kelp), Some(10.0));
        assert!(placement(&windowed, &kelp, None).is_err());
        let oak = arbrume_gen::preset("oak_like").unwrap();
        assert!(placement(&windowed, &oak, Some(0.0)).is_err());
        assert!(!placement(&entry(""), &oak, Some(0.0)).unwrap().underwater);
        const { assert!(FIT_RANGE[0] < 1.0 && FIT_RANGE[1] > 1.0) };
    }
}
