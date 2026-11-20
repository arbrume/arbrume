//! Starting a pack and adding species to it.
//!
//! [`new_pack`] writes a `pack.toml` and a licence file whose terms are left to the
//! author: both say plainly that they are placeholders, and `validate` warns about them
//! until they are replaced. [`add_species`] copies a built-in preset or a species file
//! into `species/` and appends its entry to `pack.toml`, keeping the rest of the file as
//! it is.

use std::fmt::Write;
use std::path::Path;

use arbrume_core::species::Species;
use arbrume_gen::preset_toml;

use crate::manifest::{FORMAT_VERSION, PackManifest};
use crate::pack::write_atomic;
use crate::paths::check_name;
use crate::{PACK_FILE, PackError};

/// Name of the licence file of a new pack.
const LICENCE_FILE: &str = "LICENCE-CONTENT.txt";

/// Text of the placeholder licence file of a new pack.
const LICENCE_PLACEHOLDER: &str = "PLACEHOLDER\n\n\
The licence terms of the content of this pack (its species files and assets) are to\n\
be written. Until they are, this pack must not be published.\n";

/// Value of `content_licence` in a new pack.
const CONTENT_LICENCE_PLACEHOLDER: &str = "PLACEHOLDER: content licence terms to be written";

/// A pack id from a directory name: lowercase, other characters turned into `_`, a
/// leading letter added when needed.
fn id_from(name: &str) -> String {
    let mut id: String = name
        .chars()
        .map(|c| {
            let c = c.to_ascii_lowercase();
            if c.is_ascii_lowercase() || c.is_ascii_digit() {
                c
            } else {
                '_'
            }
        })
        .collect();
    if !id.starts_with(|c: char| c.is_ascii_lowercase()) {
        id.insert(0, 'p');
    }
    id.truncate(64);
    id
}

/// Writes `bytes` to `path`, mapping the error.
fn write(path: &Path, bytes: &[u8]) -> Result<(), PackError> {
    write_atomic(path, bytes).map_err(|reason| PackError::Io {
        path: path.to_path_buf(),
        reason,
    })
}

/// Creates a pack in `dir` (which must not exist or be empty): `pack.toml` with id
/// `id` (by default from the directory name), display name `name` (by default the id)
/// and no species yet, the placeholder licence file, and empty `species/` and
/// `textures/` directories. Returns the id.
///
/// # Errors
///
/// Returns [`PackError::Usage`] for a non-empty directory or an invalid id, and
/// [`PackError::Io`] when a file cannot be written.
pub fn new_pack(dir: &Path, id: Option<&str>, name: Option<&str>) -> Result<String, PackError> {
    let occupied = std::fs::read_dir(dir).is_ok_and(|mut entries| entries.next().is_some());
    if occupied {
        let message = format!("{} exists and is not empty", dir.display());
        return Err(PackError::Usage(message));
    }
    let folder = dir.file_name().map(|n| n.to_string_lossy().into_owned());
    let id = id.map_or_else(|| id_from(&folder.unwrap_or_default()), str::to_owned);
    check_name("pack id", &id).map_err(PackError::Usage)?;
    let text = format!(
        "# Species pack (docs/packs.md).\nformat = {FORMAT_VERSION}\nid = {}\nname = {}\n\
         version = \"0.1.0\"\nauthors = []\n\n\
         # Licence of the content of the pack (not of Arbrume): replace this placeholder\n\
         # and the text of the licence file before publishing.\n\
         content_licence = {}\nlicence_file = {}\n\n\
         # Species schema version the species files need.\nmin_schema_version = {}\n\n\
         # Species are added with `arbrume pack add-species`.\n",
        quoted(&id),
        quoted(name.unwrap_or(&id)),
        quoted(CONTENT_LICENCE_PLACEHOLDER),
        quoted(LICENCE_FILE),
        arbrume_core::species::SCHEMA_VERSION
    );
    PackManifest::from_toml_str(&text).map_err(PackError::Usage)?;
    write(&dir.join(PACK_FILE), text.as_bytes())?;
    write(&dir.join(LICENCE_FILE), LICENCE_PLACEHOLDER.as_bytes())?;
    for sub in ["species", "textures"] {
        let path = dir.join(sub);
        std::fs::create_dir_all(&path).map_err(|e| PackError::io(&path, &e))?;
    }
    Ok(id)
}

/// Readable name of a preset: `oak_like` gives `Oak-like`.
fn readable(name: &str) -> String {
    let text = name.replace("_like", "-like").replace('_', " ");
    let mut chars = text.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

/// TOML string literal of `value`.
fn quoted(value: &str) -> String {
    toml::Value::String(value.to_owned()).to_string()
}

/// Adds a species to the pack directory `dir`. `source` is a built-in preset name or a
/// species file; `name` is its name in the pack (by default the preset name or the file
/// stem), `display_name` and `category` describe it (by default the readable preset
/// name or the `name` of the species file, and no category). Image files the species
/// names are copied from the directory of the species file to the same relative path
/// in the pack, which must be under `textures/`. Returns the name.
///
/// # Errors
///
/// Returns [`PackError::Usage`] for an unknown source, an invalid species or name, a
/// name already in the pack, or an image outside `textures/`, and [`PackError::Io`]
/// when a file cannot be read or written.
pub fn add_species(
    dir: &Path,
    source: &str,
    name: Option<&str>,
    display_name: Option<&str>,
    category: Option<&str>,
) -> Result<String, PackError> {
    let manifest_path = dir.join(PACK_FILE);
    let manifest_text =
        std::fs::read_to_string(&manifest_path).map_err(|e| PackError::io(&manifest_path, &e))?;
    let manifest = PackManifest::from_toml_str(&manifest_text).map_err(PackError::Usage)?;
    let (text, base, default_name, default_display) = read_source(source)?;
    let species = Species::from_toml_str(&text)
        .map_err(|e| PackError::Usage(format!("`{source}` is not a valid species: {e}")))?;
    let name = name.map_or(default_name, str::to_owned);
    check_name("species name", &name).map_err(PackError::Usage)?;
    if manifest.entry(&name).is_some() {
        let message = format!("pack `{}` already has a species `{name}`", manifest.id);
        return Err(PackError::Usage(message));
    }
    let display = display_name.map_or_else(
        || default_display.unwrap_or_else(|| species.name.clone()),
        str::to_owned,
    );
    let mut entry = format!(
        "\n[[species]]\nname = {}\ndisplay_name = {}\n",
        quoted(&name),
        quoted(&display)
    );
    if let Some(category) = category {
        let _ = writeln!(entry, "category = {}", quoted(category));
    }
    let updated = format!(
        "{}{entry}",
        manifest_text.trim_end_matches('\n').to_owned() + "\n"
    );
    PackManifest::from_toml_str(&updated).map_err(|e| {
        PackError::Usage(format!(
            "{PACK_FILE} cannot take a [[species]] table at its end: {e}"
        ))
    })?;
    copy_images(&species, base.as_deref(), dir)?;
    let file = dir.join("species").join(format!("{name}.toml"));
    write(&file, text.as_bytes())?;
    write(&manifest_path, updated.as_bytes())?;
    Ok(name)
}

/// The text of `source`, the directory of its file, the default name and display name.
type Source = (String, Option<std::path::PathBuf>, String, Option<String>);

/// Reads a preset or a species file.
fn read_source(source: &str) -> Result<Source, PackError> {
    if let Some(text) = preset_toml(source) {
        return Ok((
            text.to_owned(),
            None,
            source.to_owned(),
            Some(readable(source)),
        ));
    }
    let path = Path::new(source);
    let text = std::fs::read_to_string(path).map_err(|error| {
        PackError::Usage(format!(
            "`{source}` is neither a preset nor a readable file: {error}"
        ))
    })?;
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned());
    let base = path.parent().map(Path::to_path_buf);
    Ok((text, base, id_from(&stem.unwrap_or_default()), None))
}

/// Copies the images `species` names from `base` into the pack directory `dir`.
fn copy_images(species: &Species, base: Option<&Path>, dir: &Path) -> Result<(), PackError> {
    for (field, path) in species.texture_paths() {
        if !path.starts_with("textures/") || crate::check_path(path).is_err() {
            return Err(PackError::Usage(format!(
                "`{field}` = `{path}`: images of a pack are relative paths under `textures/`"
            )));
        }
        let from = base.unwrap_or(Path::new("")).join(path);
        let bytes = std::fs::read(&from).map_err(|e| PackError::io(&from, &e))?;
        write(&dir.join(path), &bytes)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{id_from, readable};

    #[test]
    fn names_are_derived() {
        assert_eq!(id_from("My Pack-2"), "my_pack_2");
        assert_eq!(id_from("2026 trees"), "p2026_trees");
        assert_eq!(readable("oak_like"), "Oak-like");
        assert_eq!(readable("open_grown_broadleaf"), "Open grown broadleaf");
    }
}
