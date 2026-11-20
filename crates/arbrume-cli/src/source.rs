//! Resolving a species argument to a preset, a species of an installed pack or a file.

use std::path::{Path, PathBuf};

use arbrume_gen::{preset_toml, presets};
use arbrume_pack::{PackStore, split_species_id};

use crate::error::CliError;

/// The TOML text of a species and the label used in file names.
#[derive(Debug, Clone)]
pub struct SpeciesSource {
    /// Preset name or file stem, restricted to ASCII letters, digits, `-` and `_`.
    pub label: String,
    /// TOML text of the species.
    pub text: String,
    /// Directory the texture paths of the species are relative to: the pack, or the
    /// directory of the species file.
    pub asset_dir: Option<PathBuf>,
}

/// Resolves `spec`: a preset name, a species `pack_id/name` of an installed pack (when
/// no file of that name exists), else a path to a species file, relative to `base`.
pub fn resolve(spec: &str, base: &Path) -> Result<SpeciesSource, CliError> {
    if let Some(text) = preset_toml(spec) {
        return Ok(SpeciesSource {
            label: spec.to_owned(),
            text: text.to_owned(),
            asset_dir: None,
        });
    }
    let path = base.join(spec);
    if split_species_id(spec).is_some() && !path.exists() {
        let store = PackStore::open_default().map_err(CliError::usage)?;
        let species = store.species(spec).map_err(CliError::usage)?;
        return Ok(SpeciesSource {
            label: pack_label(spec),
            text: species.text,
            asset_dir: species.asset_dir,
        });
    }
    let text = std::fs::read_to_string(&path).map_err(|error| {
        let names: Vec<&str> = presets().iter().map(|(name, _)| *name).collect();
        CliError::usage(format!(
            "species `{spec}` is neither a preset ({}) nor a readable file: {error}",
            names.join(", ")
        ))
    })?;
    let stem = path
        .file_stem()
        .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
    Ok(SpeciesSource {
        label: sanitize(&stem),
        text,
        asset_dir: path.parent().map(Path::to_path_buf),
    })
}

/// Label of the pack species `pack_id/name` in file names: `pack_id-name`, which no
/// preset and no other pack species can have, since names hold no `-`.
pub fn pack_label(id: &str) -> String {
    id.replace('/', "-")
}

/// `name` with every character other than ASCII letters, digits, `-` and `_` replaced by
/// `_`, or `species` if empty.
pub fn sanitize(name: &str) -> String {
    let clean: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if clean.is_empty() {
        "species".to_owned()
    } else {
        clean
    }
}

#[cfg(test)]
mod tests {
    use super::sanitize;

    #[test]
    fn labels_are_file_name_safe() {
        assert_eq!(sanitize("my oak.v2"), "my_oak_v2");
        assert_eq!(sanitize(""), "species");
        assert_eq!(sanitize("conifer-b_2"), "conifer-b_2");
    }
}
