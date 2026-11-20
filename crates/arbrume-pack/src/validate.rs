//! Full validation of a pack, for authors and before installing.
//!
//! On top of the structure checked by `Pack::open`, every species is parsed (with the
//! migrations of its schema), its image files are checked (inside `textures/`, present,
//! a PNG or JPEG that decodes, square with a power-of-two side from 16 to 4096) and
//! turned into textures, and it is grown at the youngest, middle and oldest ages of
//! `variation.age` and meshed: the plant graph must keep its invariants and be
//! contiguous, with positive radii, finite positions and a non-empty mesh. A few
//! individuals of its `[variation]` must be valid species. Thumbnails must decode like
//! image textures.
//!
//! Warnings do not fail a pack: a species without a thumbnail (`arbrume pack thumbs`
//! renders one, `arbrume pack build` renders the missing ones), a content licence that
//! is still a placeholder, image files no species uses.

use std::fmt;
use std::path::Path;

use arbrume_core::mesh::MeshPart;
use arbrume_core::species::Species;
use arbrume_gen::{GenParams, RANDOM_QUALITY, generate};
use arbrume_texture::{TextureImages, TextureOptions, decode_image, textures_with_images};

use crate::manifest::{SpeciesEntry, migrate_species};
use crate::pack::Pack;

/// One problem, in one file of the pack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    /// Path of the file inside the pack; empty for the pack as a whole.
    pub file: String,
    /// What is wrong.
    pub message: String,
}

impl Issue {
    /// An issue of `file`.
    pub fn new(file: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            file: file.into(),
            message: message.into(),
        }
    }
}

impl fmt::Display for Issue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.file.is_empty() {
            f.write_str(&self.message)
        } else {
            write!(f, "{}: {}", self.file, self.message)
        }
    }
}

/// Outcome of [`validate`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    /// Problems that make the pack invalid.
    pub errors: Vec<Issue>,
    /// Problems that do not.
    pub warnings: Vec<Issue>,
}

impl Report {
    /// Whether the pack is valid.
    #[must_use]
    pub fn is_ok(&self) -> bool {
        self.errors.is_empty()
    }
}

/// Validates the pack at `path`, a directory or an archive; a pack that cannot be read
/// at all gives a report with that one error.
#[must_use]
pub fn validate_path(path: &Path) -> Report {
    match Pack::load(path) {
        Ok(pack) => validate(&pack),
        Err(crate::PackError::Invalid { issues, .. }) => Report {
            errors: issues,
            warnings: Vec::new(),
        },
        Err(error) => Report {
            errors: vec![Issue::new("", error.to_string())],
            warnings: Vec::new(),
        },
    }
}

/// Validates `pack` (see the module documentation).
#[must_use]
pub fn validate(pack: &Pack) -> Report {
    let mut report = Report {
        errors: pack.structure(),
        warnings: Vec::new(),
    };
    let mut used = Vec::new();
    for entry in &pack.manifest().species {
        check_species(pack, entry, &mut report, &mut used);
        check_thumbnail(pack, entry, &mut report);
    }
    let manifest = pack.manifest();
    let licence = pack.files().get(&manifest.licence_file);
    let licence = licence.map(|bytes| String::from_utf8_lossy(bytes).to_ascii_lowercase());
    let placeholder = manifest
        .content_licence
        .to_ascii_lowercase()
        .contains("placeholder")
        || licence.is_some_and(|text| text.contains("placeholder"));
    if placeholder {
        let message = "the content licence is a placeholder: write its terms before publishing";
        report
            .warnings
            .push(Issue::new(&manifest.licence_file, message));
    }
    for path in pack.files().keys() {
        if path.starts_with("textures/") && !used.contains(path) {
            report
                .warnings
                .push(Issue::new(path, "no species uses this image"));
        }
    }
    report
}

/// Checks one species: parse, images, growth, variation.
fn check_species(pack: &Pack, entry: &SpeciesEntry, report: &mut Report, used: &mut Vec<String>) {
    let file = entry.file_path();
    let mut error = |message: String| report.errors.push(Issue::new(&file, message));
    let Some(bytes) = pack.files().get(&file) else {
        return;
    };
    let text = match std::str::from_utf8(bytes).map_err(|e| e.to_string()) {
        Ok(text) => migrate_species(text),
        Err(reason) => Err(reason),
    };
    let species = match text.map(|text| Species::from_toml_str(&text).map_err(|e| e.to_string())) {
        Ok(Ok(species)) => species,
        Ok(Err(reason)) | Err(reason) => return error(reason),
    };
    match images(pack, &species, used) {
        Ok(images) if images != TextureImages::default() => {
            let options = TextureOptions::default();
            drop(textures_with_images(&species, &options, &images));
        }
        Ok(_) => {}
        Err(reason) => error(reason),
    }
    for reason in growth_problems(&species) {
        error(reason);
    }
    for index in 0..4 {
        if let Err(reason) = species.varied(1, index).validate() {
            error(format!(
                "individual {index} of its [variation] is invalid: {reason}"
            ));
        }
    }
}

/// Decodes the image files `species` names, which must be in `textures/` of `pack`.
fn images(pack: &Pack, species: &Species, used: &mut Vec<String>) -> Result<TextureImages, String> {
    for (field, path) in species.texture_paths() {
        if !path.starts_with("textures/") || crate::check_path(path).is_err() {
            return Err(format!(
                "`{field}` = `{path}`: an image of a pack is under `textures/`"
            ));
        }
        used.push(path.to_owned());
    }
    crate::thumbs::pack_images(pack, species)
}

/// Problems of the plants of `species` at the youngest, middle and oldest ages of its
/// range.
fn growth_problems(species: &Species) -> Vec<String> {
    let [young, old] = species.variation.age;
    let mut problems = Vec::new();
    for age in [young, young.midpoint(old), old] {
        let params = GenParams::new(1, age).with_quality(RANDOM_QUALITY);
        let plant = match generate(species, &params) {
            Ok(plant) => plant,
            Err(error) => {
                problems.push(format!("does not generate at age {age}: {error}"));
                continue;
            }
        };
        let graph = &plant.graph;
        let finite = |part: &MeshPart| {
            part.buffers
                .positions
                .iter()
                .flatten()
                .all(|c| c.is_finite())
        };
        let radii = graph.radii().iter().all(|r| *r > 0.0 && r.is_finite());
        let checks = [
            graph
                .check_invariants()
                .err()
                .map(|e| format!("breaks a graph invariant: {e}")),
            (!graph.is_contiguous()).then(|| "has a graph that is not contiguous".to_owned()),
            (!radii).then(|| "has a radius that is not positive and finite".to_owned()),
            (!plant.mesh.parts.iter().all(finite)).then(|| "has non-finite positions".to_owned()),
            (plant.stats.triangles == 0).then(|| "has an empty mesh".to_owned()),
        ];
        let failed = checks.into_iter().flatten();
        problems.extend(failed.map(|problem| format!("at age {age}, the plant {problem}")));
    }
    problems
}

/// Checks the thumbnail of a species, or warns that there is none.
fn check_thumbnail(pack: &Pack, entry: &SpeciesEntry, report: &mut Report) {
    let path = entry.thumbnail_path();
    match pack.files().get(&path) {
        Some(bytes) => {
            if let Err(reason) = decode_image(bytes) {
                report.errors.push(Issue::new(&path, reason));
            }
        }
        None if entry.thumbnail.is_none() => {
            let message = format!(
                "species `{}` has no thumbnail: `arbrume pack thumbs` renders it",
                entry.name
            );
            report.warnings.push(Issue::new(&path, message));
        }
        None => {}
    }
}
