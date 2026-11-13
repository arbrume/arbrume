//! Generator metadata written to the `extras` of exported files.

use arbrume_core::species::Species;
use serde_json::{Value, json};

use crate::plant::Plant;

/// Version of this crate, which is the version of the Arbrume workspace.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Generator string written to `asset.generator` of exported files.
pub const GENERATOR: &str = concat!("Arbrume ", env!("CARGO_PKG_VERSION"));

/// The JSON object that makes an exported plant reproducible from its own file.
///
/// Keys: `generator`, `version`, `species_name`, `seed`, `age`, `scale`, `mesh` (every
/// field of the mesh options but `parallel`, which does not change the output), `stats`
/// (see [`Stats`](crate::Stats)) and `species`, the full parameter set of the species.
/// Deserializing `species` gives back an equal [`Species`]; with the other keys it gives
/// back the same plant with the same version.
///
/// Callers may add keys of their own (an index, a level of detail) to the returned
/// object. Nothing in it depends on time or on threads.
#[must_use]
pub fn metadata(species: &Species, plant: &Plant) -> Value {
    let params = &plant.params;
    let mesh = &params.mesh;
    json!({
        "generator": "arbrume",
        "version": VERSION,
        "species_name": species.name,
        "seed": params.seed,
        "age": params.age,
        "scale": params.scale,
        "mesh": {
            "tolerance": mesh.tolerance,
            "min_radius": mesh.min_radius,
            "leaf_fraction": mesh.leaf_fraction,
            "max_cycle": mesh.max_cycle,
            "seed": mesh.seed,
        },
        "stats": plant.stats,
        "species": species,
    })
}

#[cfg(test)]
mod tests {
    use super::metadata;
    use crate::{GenParams, generate, presets};
    use arbrume_core::species::Species;

    #[test]
    fn species_round_trips_through_the_metadata() {
        for (name, text) in presets() {
            let species = Species::from_toml_str(text).unwrap();
            let plant = generate(&species, &GenParams::new(3, 2)).unwrap();
            let value = metadata(&species, &plant);
            let back: Species = serde_json::from_value(value["species"].clone()).unwrap();
            assert_eq!(back, species, "{name}");
            assert_eq!(value["seed"], 3);
            assert_eq!(value["stats"]["triangles"], plant.stats.triangles);
        }
    }
}
