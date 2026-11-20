//! `pack.toml`: what a pack is and which species it holds.
//!
//! ```toml
//! format = 1
//! id = "starter"
//! name = "Starter pack"
//! version = "0.1.0"
//! authors = ["Arbrume contributors"]
//! description = "Four species of the built-in library"
//! content_licence = "PLACEHOLDER: content licence to be written"
//! licence_file = "LICENCE-CONTENT.txt"
//! min_schema_version = 0
//!
//! [[species]]
//! name = "oak_like"
//! display_name = "Oak-like tree"
//! category = "broadleaf tree"
//! # file = "species/oak_like.toml"         (the default)
//! # thumbnail = "thumbnails/oak_like.png"  (the default)
//! # preview = "previews/oak_like.glb"      (optional)
//! ```
//!
//! **Versions.** `format` is the version of this file format, [`FORMAT_VERSION`] for the
//! files this crate writes. A file of an older format is brought up to date by the
//! migrations of [`MIGRATIONS`], one format at a time, before it is read; a newer
//! format is rejected with a message asking for a newer Arbrume. `min_schema_version`
//! is the species schema version the species files need: a pack asking for a newer
//! schema than `arbrume_core::species::SCHEMA_VERSION` is rejected the same way, and
//! [`SPECIES_MIGRATIONS`] update species files written for an older schema.

use arbrume_core::species::SCHEMA_VERSION;
use serde::{Deserialize, Serialize};

use crate::paths::{check_name, check_path};

/// Version of the `pack.toml` format written by this crate.
pub const FORMAT_VERSION: u32 = 1;

/// A step that updates a table of format `from` to format `from + 1`.
#[derive(Debug, Clone, Copy)]
pub struct Migration {
    /// Format the step reads.
    pub from: u32,
    /// Changes the table in place, or says why it cannot.
    pub apply: fn(&mut toml::Table) -> Result<(), String>,
}

/// Migrations of `pack.toml`, by format. Format 1 is the first; there is none yet.
pub const MIGRATIONS: &[Migration] = &[];

/// Migrations of species files, by species schema version. Version 0 is the first;
/// there is none yet.
pub const SPECIES_MIGRATIONS: &[Migration] = &[];

/// The contents of `pack.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackManifest {
    /// Version of the format of this file.
    pub format: u32,
    /// Identifier of the pack: lowercase ASCII letters, digits and `_`. Species of the
    /// pack are named `id/name`.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Version of the pack, `MAJOR.MINOR.PATCH`.
    pub version: String,
    /// Authors, free text.
    #[serde(default)]
    pub authors: Vec<String>,
    /// One-paragraph description.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// Name or identifier of the licence of the content (species and assets), which
    /// is not the licence of Arbrume.
    pub content_licence: String,
    /// Path of the file holding the terms of that licence, at the root of the pack.
    pub licence_file: String,
    /// Lowest species schema version the species files need.
    #[serde(default)]
    pub min_schema_version: u32,
    /// The species of the pack.
    #[serde(default)]
    pub species: Vec<SpeciesEntry>,
}

/// One species of a pack, as `pack.toml` lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpeciesEntry {
    /// Name within the pack: lowercase ASCII letters, digits and `_`.
    pub name: String,
    /// Name shown to users.
    pub display_name: String,
    /// Category shown to users, free text such as `broadleaf tree`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub category: String,
    /// Species file, by default `species/<name>.toml`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// Thumbnail, by default `thumbnails/<name>.png` when that file exists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumbnail: Option<String>,
    /// Optional preview model, a `.glb` under `previews/`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
}

impl SpeciesEntry {
    /// Path of the species file.
    #[must_use]
    pub fn file_path(&self) -> String {
        self.file
            .clone()
            .unwrap_or_else(|| format!("species/{}.toml", self.name))
    }

    /// Path of the thumbnail, declared or by default.
    #[must_use]
    pub fn thumbnail_path(&self) -> String {
        self.thumbnail
            .clone()
            .unwrap_or_else(|| format!("thumbnails/{}.png", self.name))
    }
}

/// Brings `table` from format or schema version `found` up to `target` with
/// `migrations`, then sets its `key` to `target`.
///
/// # Errors
///
/// Returns why: a version newer than `target`, or a missing migration step.
pub fn migrate(
    table: &mut toml::Table,
    key: &str,
    target: u32,
    migrations: &[Migration],
) -> Result<(), String> {
    let found = match table.get(key) {
        None => 0,
        Some(toml::Value::Integer(value)) => {
            u32::try_from(*value).map_err(|_| format!("`{key}` {value} is not a version"))?
        }
        Some(other) => return Err(format!("`{key}` is not an integer: {other}")),
    };
    if found > target {
        return Err(format!(
            "`{key}` is {found}, newer than the {target} this Arbrume reads: update Arbrume"
        ));
    }
    for version in found..target {
        let step = migrations
            .iter()
            .find(|step| step.from == version)
            .ok_or_else(|| format!("no migration of `{key}` {version} to {}", version + 1))?;
        (step.apply)(table)?;
    }
    if found != target {
        table.insert(key.to_owned(), toml::Value::Integer(i64::from(target)));
    }
    Ok(())
}

impl PackManifest {
    /// Parses `pack.toml`, migrating an older format, and checks it.
    ///
    /// # Errors
    ///
    /// Returns every problem found, one per line.
    pub fn from_toml_str(text: &str) -> Result<Self, String> {
        let mut table: toml::Table = text.parse().map_err(|e| format!("{e}"))?;
        if !table.contains_key("format") {
            return Err(format!(
                "`format` is missing (this Arbrume writes {FORMAT_VERSION})"
            ));
        }
        migrate(&mut table, "format", FORMAT_VERSION, MIGRATIONS)?;
        let manifest: Self = table.try_into().map_err(|e| format!("{e}"))?;
        let problems = manifest.problems();
        if problems.is_empty() {
            Ok(manifest)
        } else {
            Err(problems.join("\n"))
        }
    }

    /// Writes the manifest as TOML.
    ///
    /// # Panics
    ///
    /// Never: every field is representable in TOML.
    #[must_use]
    pub fn to_toml_string(&self) -> String {
        toml::to_string(self).expect("a pack manifest is representable in TOML")
    }

    /// The species entry `name`.
    #[must_use]
    pub fn entry(&self, name: &str) -> Option<&SpeciesEntry> {
        self.species.iter().find(|entry| entry.name == name)
    }

    /// Every rule of the fields broken by the manifest.
    #[must_use]
    pub fn problems(&self) -> Vec<String> {
        let mut problems = Vec::new();
        let mut check = |result: Result<(), String>| {
            if let Err(problem) = result {
                problems.push(problem);
            }
        };
        check(check_name("pack id", &self.id));
        if self.id == "builtin" {
            check(Err(
                "pack id `builtin` is reserved for the built-in presets".to_owned(),
            ));
        }
        check(not_blank("name", &self.name));
        check(check_version(&self.version));
        check(not_blank("content_licence", &self.content_licence));
        check(check_path(&self.licence_file).and_then(|()| {
            let text = crate::paths::kind(&self.licence_file, &self.licence_file);
            text.filter(|_| !self.licence_file.contains('/'))
                .map(|_| ())
                .ok_or_else(|| "`licence_file` is a .txt or .md file at the root".to_owned())
        }));
        if self.min_schema_version > SCHEMA_VERSION {
            check(Err(format!(
                "`min_schema_version` is {}, newer than the species schema {SCHEMA_VERSION} \
                 of this Arbrume: update Arbrume",
                self.min_schema_version
            )));
        }
        for (index, entry) in self.species.iter().enumerate() {
            let duplicate = self.species[..index].iter().any(|e| e.name == entry.name);
            if duplicate {
                check(Err(format!("species `{}` is listed twice", entry.name)));
            }
            entry.problems().into_iter().for_each(|p| check(Err(p)));
        }
        problems
    }
}

impl SpeciesEntry {
    /// Every rule of the fields broken by the entry.
    fn problems(&self) -> Vec<String> {
        let mut problems = Vec::new();
        let name = &self.name;
        let checks = [
            check_name("species name", name),
            not_blank(&format!("display_name of `{name}`"), &self.display_name),
            prefixed(&self.file_path(), "species/", ".toml"),
            prefixed(&self.thumbnail_path(), "thumbnails/", ".png"),
            self.preview
                .as_deref()
                .map_or(Ok(()), |path| prefixed(path, "previews/", ".glb")),
        ];
        problems.extend(checks.into_iter().filter_map(Result::err));
        problems
    }
}

/// Fails unless `path` is a safe path starting with `dir` and ending with `extension`.
fn prefixed(path: &str, dir: &str, extension: &str) -> Result<(), String> {
    check_path(path)?;
    let inside = path
        .strip_prefix(dir)
        .is_some_and(|rest| !rest.contains('/'));
    if inside && path.ends_with(extension) {
        Ok(())
    } else {
        Err(format!(
            "`{path}` must be a `{extension}` file directly in `{dir}`"
        ))
    }
}

/// Fails when `value` is empty or only spaces, or holds a control character.
fn not_blank(field: &str, value: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err(format!("`{field}` is empty"));
    }
    if value.chars().any(char::is_control) {
        return Err(format!("`{field}` holds a control character"));
    }
    Ok(())
}

/// Fails unless `version` is `MAJOR.MINOR.PATCH`, three decimal numbers.
fn check_version(version: &str) -> Result<(), String> {
    let parts: Vec<&str> = version.split('.').collect();
    let numeric = |part: &&str| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit());
    if parts.len() == 3 && parts.iter().all(numeric) {
        Ok(())
    } else {
        Err(format!("version `{version}` is not MAJOR.MINOR.PATCH"))
    }
}

/// Parses a species file of a pack, migrating an older species schema, and returns its
/// text for the current schema.
///
/// # Errors
///
/// Returns why the text is not a species file this Arbrume can read.
pub fn migrate_species(text: &str) -> Result<String, String> {
    let mut table: toml::Table = text.parse().map_err(|e| format!("{e}"))?;
    let before = table.clone();
    migrate(
        &mut table,
        "schema_version",
        SCHEMA_VERSION,
        SPECIES_MIGRATIONS,
    )?;
    if table == before {
        return Ok(text.to_owned());
    }
    toml::to_string(&table).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::{Migration, PackManifest, migrate, migrate_species};

    const BASE: &str = "format = 1\nid = \"demo\"\nname = \"Demo\"\nversion = \"0.1.0\"\n\
        content_licence = \"PLACEHOLDER\"\nlicence_file = \"LICENCE-CONTENT.txt\"\n\
        [[species]]\nname = \"oak\"\ndisplay_name = \"Oak\"\n";

    #[test]
    fn a_manifest_parses_with_defaults_and_round_trips() {
        let manifest = PackManifest::from_toml_str(BASE).unwrap();
        let oak = manifest.entry("oak").unwrap();
        assert_eq!(oak.file_path(), "species/oak.toml");
        assert_eq!(oak.thumbnail_path(), "thumbnails/oak.png");
        let again = PackManifest::from_toml_str(&manifest.to_toml_string()).unwrap();
        assert_eq!(again, manifest);
    }

    #[test]
    fn broken_rules_are_all_reported() {
        let text = BASE
            .replace("\"demo\"", "\"Demo Pack\"")
            .replace("0.1.0", "1.0")
            .replace("\"PLACEHOLDER\"", "\"\"")
            + "[[species]]\nname = \"oak\"\ndisplay_name = \"\"\nfile = \"../oak.toml\"\n";
        let error = PackManifest::from_toml_str(&text).unwrap_err();
        for expected in [
            "pack id `Demo Pack`",
            "version `1.0`",
            "`content_licence` is empty",
            "listed twice",
            "display_name of `oak`",
            "`../oak.toml`",
        ] {
            assert!(error.contains(expected), "{expected} not in:\n{error}");
        }
        let unknown = format!("{BASE}colour = 3\n");
        assert!(PackManifest::from_toml_str(&unknown).is_err());
    }

    #[test]
    fn versions_newer_than_this_arbrume_are_rejected() {
        let newer = BASE.replace("format = 1", "format = 2");
        let error = PackManifest::from_toml_str(&newer).unwrap_err();
        assert!(error.contains("update Arbrume"), "{error}");
        let missing = BASE.replace("format = 1\n", "");
        assert!(
            PackManifest::from_toml_str(&missing)
                .unwrap_err()
                .contains("`format`")
        );
        let schema = format!("min_schema_version = 9\n{BASE}");
        let error = PackManifest::from_toml_str(&schema).unwrap_err();
        assert!(error.contains("min_schema_version"), "{error}");
        let error = migrate_species("schema_version = 3\nname = \"x\"").unwrap_err();
        assert!(error.contains("update Arbrume"), "{error}");
        assert_eq!(migrate_species("name = \"x\"").unwrap(), "name = \"x\"");
    }

    #[test]
    fn migrations_run_one_step_at_a_time() {
        fn rename(table: &mut toml::Table) -> Result<(), String> {
            let title = table.remove("title").ok_or("no title")?;
            table.insert("name".to_owned(), title);
            Ok(())
        }
        let steps = [Migration {
            from: 0,
            apply: rename,
        }];
        let mut table: toml::Table = "version = 0\ntitle = \"x\"".parse().unwrap();
        migrate(&mut table, "version", 1, &steps).unwrap();
        assert_eq!(table["name"].as_str(), Some("x"));
        assert_eq!(table["version"].as_integer(), Some(1));
        let mut table: toml::Table = "version = 0".parse().unwrap();
        assert!(migrate(&mut table, "version", 2, &steps).is_err());
    }
}
