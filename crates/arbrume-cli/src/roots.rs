//! `--roots` and `--soil` of `arbrume gen`: root systems and the soil they grow in.

use std::path::Path;
use std::sync::Arc;

use arbrume_core::species::{RootHabit, Roots};
use arbrume_gen::Soil;

use crate::config::Batch;
use crate::error::CliError;

/// What `--roots` asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootChoice {
    /// The `[roots]` layer of each species, if it has one.
    Species,
    /// No root system, whatever the species says.
    None,
    /// The starting values of a habit, replacing the layer of the species.
    Habit(RootHabit),
}

/// Parses the value of `--roots`.
///
/// # Errors
///
/// Returns a message listing the accepted values.
pub fn parse_roots(text: &str) -> Result<RootChoice, String> {
    match text {
        "species" => Ok(RootChoice::Species),
        "none" => Ok(RootChoice::None),
        _ => RootHabit::from_name(text)
            .map(RootChoice::Habit)
            .ok_or_else(|| {
                format!("`{text}` is not one of species, none, tap, heart, plate, fibrous")
            }),
    }
}

/// Reads and checks the soil file at `path`.
fn load_soil(path: &Path) -> Result<Arc<Soil>, CliError> {
    let fail = |error: String| CliError::usage(format!("{}: {error}", path.display()));
    let text = std::fs::read_to_string(path).map_err(|error| fail(error.to_string()))?;
    let soil: Soil = toml::from_str(&text).map_err(|error| fail(error.to_string()))?;
    soil.validate().map_err(fail)?;
    Ok(Arc::new(soil))
}

/// Applies `choice` to the species of every entry of `batch` and gives each the soil of
/// the file at `soil`. The random plants of `--random` keep the layers of their presets.
///
/// # Errors
///
/// Returns a usage error when the soil file cannot be read or is invalid.
pub fn apply(
    batch: &mut Batch,
    choice: Option<RootChoice>,
    soil: Option<&Path>,
) -> Result<(), CliError> {
    let soil = soil.map(load_soil).transpose()?;
    for entry in &mut batch.entries {
        match choice {
            None | Some(RootChoice::Species) => {}
            Some(RootChoice::None) => entry.species.roots = None,
            Some(RootChoice::Habit(habit)) => entry.species.roots = Some(Roots::of(habit)),
        }
        if soil.is_some() {
            entry.soil.clone_from(&soil);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{RootChoice, parse_roots};
    use arbrume_core::species::RootHabit;

    #[test]
    fn root_choices_parse() {
        assert_eq!(parse_roots("none"), Ok(RootChoice::None));
        assert_eq!(parse_roots("species"), Ok(RootChoice::Species));
        assert_eq!(
            parse_roots("plate"),
            Ok(RootChoice::Habit(RootHabit::Plate))
        );
        assert!(parse_roots("taproot").is_err());
    }
}
