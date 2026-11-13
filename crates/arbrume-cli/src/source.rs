//! Resolving a species argument to a preset or a file.

use std::path::Path;

use arbrume_gen::{preset_toml, presets};

use crate::error::CliError;

/// The TOML text of a species and the label used in file names.
#[derive(Debug, Clone)]
pub struct SpeciesSource {
    /// Preset name or file stem, restricted to ASCII letters, digits, `-` and `_`.
    pub label: String,
    /// TOML text of the species.
    pub text: String,
}

/// Resolves `spec`: a preset name, else a path to a species file, relative to `base`.
pub fn resolve(spec: &str, base: &Path) -> Result<SpeciesSource, CliError> {
    if let Some(text) = preset_toml(spec) {
        return Ok(SpeciesSource {
            label: spec.to_owned(),
            text: text.to_owned(),
        });
    }
    let path = base.join(spec);
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
    })
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
