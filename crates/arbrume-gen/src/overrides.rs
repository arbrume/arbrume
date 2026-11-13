//! Changing fields of a species file before it is parsed.

use arbrume_core::species::{Species, SpeciesError};
use toml::{Table, Value};

use crate::params::GenError;

/// Parses the species file `text` after replacing some of its values.
///
/// Each key of `overrides` is a dotted path to a parameter, such as
/// `"growth.branching_angle"` or `"foliage.color"`, and its value replaces the whole
/// value of that parameter (a number, a curve, an array or a table such as
/// `architecture.phyllotaxis`), or is added if the file does not set it. A key naming a
/// section (`growth`, `foliage`, ...) with a table value sets each parameter of that
/// table in the section, so `growth = { gnarl = 10.0 }` and the TOML dotted key
/// `growth.gnarl = 10.0` change one parameter of `[growth]` and keep the others.
/// Elements of an array (such as one entry of `[[architecture.axes]]`) cannot be
/// addressed; replace the whole array instead.
///
/// The result is parsed and validated like a species file, so a misspelt path is an
/// unknown-key error.
///
/// # Errors
///
/// Returns [`GenError::Override`] when a path crosses a value that is not a table, and
/// [`GenError::Species`] when the text or the result is not a valid species.
pub fn species_with_overrides(text: &str, overrides: &Table) -> Result<Species, GenError> {
    let mut table: Table = toml::from_str(text).map_err(SpeciesError::from)?;
    merge(&mut table, overrides, "")?;
    let species: Species = Value::Table(table).try_into().map_err(SpeciesError::from)?;
    species.validate()?;
    Ok(species)
}

/// Applies `overrides` to `target`, whose dotted path is `prefix`: a table value given
/// for a top-level key is applied inside the existing table, every other value replaces
/// the value at its path.
fn merge(target: &mut Table, overrides: &Table, prefix: &str) -> Result<(), GenError> {
    for (key, value) in overrides {
        let path = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };
        let (parent, leaf) = parent_table(target, key, &path)?;
        let existing = parent.get_mut(leaf);
        match (existing, value) {
            (Some(Value::Table(inner)), Value::Table(changes))
                if prefix.is_empty() && !key.contains('.') =>
            {
                merge(inner, changes, &path)?;
            }
            _ => {
                parent.insert(leaf.to_owned(), value.clone());
            }
        }
    }
    Ok(())
}

/// The table holding the last segment of the dotted `key`, created where missing, and
/// that last segment.
fn parent_table<'a, 'k>(
    mut table: &'a mut Table,
    key: &'k str,
    path: &str,
) -> Result<(&'a mut Table, &'k str), GenError> {
    let mut segments: Vec<&str> = key.split('.').collect();
    let leaf = segments.pop().unwrap_or(key);
    for segment in segments {
        let entry = table
            .entry(segment)
            .or_insert_with(|| Value::Table(Table::new()));
        table = match entry {
            Value::Table(inner) => inner,
            other => {
                return Err(GenError::Override {
                    path: path.to_owned(),
                    reason: format!("`{segment}` is a {}, not a table", other.type_str()),
                });
            }
        };
    }
    Ok((table, leaf))
}

#[cfg(test)]
mod tests {
    use super::species_with_overrides;
    use crate::{GenError, preset_toml};
    use arbrume_core::species::Species;

    fn overrides(text: &str) -> toml::Table {
        toml::from_str(text).unwrap()
    }

    #[test]
    fn dotted_and_nested_overrides_change_one_field() {
        let text = preset_toml("broadleaf").unwrap();
        let base = Species::from_toml_str(text).unwrap();
        let dotted = species_with_overrides(text, &overrides("\"foliage.length\" = 0.2")).unwrap();
        let nested =
            species_with_overrides(text, &overrides("foliage = { length = 0.2 }")).unwrap();
        assert_eq!(dotted, nested);
        assert!((dotted.foliage.length - 0.2).abs() < 1e-6);
        assert_eq!(dotted.foliage.width_ratio, base.foliage.width_ratio);
        assert_eq!(dotted.growth, base.growth);
    }

    #[test]
    fn a_parameter_is_replaced_whole() {
        let text = preset_toml("broadleaf").unwrap();
        let curve = "growth.axis_length = { points = [[0.0, 2.0], [1.0, 1.0]] }";
        let species = species_with_overrides(text, &overrides(curve)).unwrap();
        assert!((species.growth.axis_length.eval(0, 0.0) - 2.0).abs() < 1e-6);
        let phyllotaxis = "\"architecture.phyllotaxis\" = { leaves_per_node = 2 }";
        species_with_overrides(text, &overrides(phyllotaxis)).unwrap();
    }

    #[test]
    fn missing_tables_are_created() {
        let species = species_with_overrides("", &overrides("\"bark.roughness\" = 0.5")).unwrap();
        assert!((species.bark.roughness - 0.5).abs() < 1e-6);
    }

    #[test]
    fn bad_overrides_are_errors() {
        let text = preset_toml("shrub").unwrap();
        let typo = species_with_overrides(text, &overrides("\"growth.gnarll\" = 3.0"));
        assert!(matches!(typo, Err(GenError::Species(_))));
        let through_value = species_with_overrides(text, &overrides("\"name.x\" = 1"));
        assert!(matches!(through_value, Err(GenError::Override { .. })));
        let invalid = species_with_overrides(text, &overrides("\"foliage.width_ratio\" = -1.0"));
        assert!(matches!(invalid, Err(GenError::Species(_))));
    }
}
