//! Species presets embedded in the library.
//!
//! Each preset is a species file in `crates/arbrume-gen/species/`, compiled into the
//! binary. To add one, drop `<name>.toml` in that directory and add its line to
//! [`PRESETS`]; the tests generate and export every preset. `docs/species.md` describes
//! the library.

use arbrume_core::species::Species;

/// Name and TOML text of every preset, sorted by name.
const PRESETS: &[(&str, &str)] = &[
    ("acacia_like", include_str!("../species/acacia_like.toml")),
    ("bamboo_like", include_str!("../species/bamboo_like.toml")),
    ("birch_like", include_str!("../species/birch_like.toml")),
    ("broadleaf", include_str!("../species/broadleaf.toml")),
    ("conifer", include_str!("../species/conifer.toml")),
    ("cypress_like", include_str!("../species/cypress_like.toml")),
    ("fern_like", include_str!("../species/fern_like.toml")),
    ("larch_like", include_str!("../species/larch_like.toml")),
    ("maple_like", include_str!("../species/maple_like.toml")),
    ("oak_like", include_str!("../species/oak_like.toml")),
    ("palm_like", include_str!("../species/palm_like.toml")),
    ("pine_like", include_str!("../species/pine_like.toml")),
    ("poplar_like", include_str!("../species/poplar_like.toml")),
    ("rosette_herb", include_str!("../species/rosette_herb.toml")),
    ("sapling", include_str!("../species/sapling.toml")),
    ("shrub", include_str!("../species/shrub.toml")),
    (
        "spreading_shrub",
        include_str!("../species/spreading_shrub.toml"),
    ),
    ("spruce_like", include_str!("../species/spruce_like.toml")),
    (
        "upright_shrub",
        include_str!("../species/upright_shrub.toml"),
    ),
    ("willow_like", include_str!("../species/willow_like.toml")),
];

/// Name and TOML text of every built-in preset, sorted by name.
#[must_use]
pub const fn presets() -> &'static [(&'static str, &'static str)] {
    PRESETS
}

/// TOML text of the preset `name`.
#[must_use]
pub fn preset_toml(name: &str) -> Option<&'static str> {
    PRESETS
        .iter()
        .find(|(preset, _)| *preset == name)
        .map(|(_, text)| *text)
}

/// The parsed preset `name`, or `None` if there is no such preset.
///
/// # Panics
///
/// Panics if the embedded file is not a valid species, which the tests of this crate
/// rule out.
#[must_use]
pub fn preset(name: &str) -> Option<Species> {
    preset_toml(name).map(|text| {
        Species::from_toml_str(text)
            .unwrap_or_else(|error| panic!("built-in preset `{name}` is invalid: {error}"))
    })
}

#[cfg(test)]
mod tests {
    use super::{preset, preset_toml, presets};

    #[test]
    fn presets_are_sorted_valid_and_found_by_name() {
        let names: Vec<&str> = presets().iter().map(|(name, _)| *name).collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(names, sorted);
        for name in names {
            assert!(preset(name).is_some());
            assert!(preset_toml(name).is_some());
        }
        assert!(preset("no such preset").is_none());
    }
}
