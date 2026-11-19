//! `--annotations`: ML ground truth written next to each plant.
//!
//! The list `skeleton,labels,points=N,branches[=K],metrics,json` selects what is
//! written (see `docs/annotations.md`):
//!
//! * `skeleton`, `labels` and `branches` go to one sidecar, `<stem>.ann.npz`; `json`
//!   writes the same content as `<stem>.ann.json`;
//! * `labels` also adds the `_SEMANTIC` and `_INSTANCE` vertex attributes to every
//!   `.glb` of the plant whose parts the mesher produced;
//! * `points=N` samples `N` labelled points on the mesh into `<stem>.points.ply`;
//! * `metrics` adds the `ICTree` features of the skeleton to the manifest line.
//!
//! Whenever annotations are asked for, the manifest line also gets the file names and
//! `species_params`, the full parameter set of the species the plant was grown with
//! (for a random plant, its jittered individual). The sidecars describe level of detail
//! 0, or the last frame of a growth sequence.

use std::fmt;
use std::fs;
use std::path::Path;
use std::str::FromStr;

use arbrume_core::species::Species;
use arbrume_export::annotations::{
    Annotations, BranchTable, DEFAULT_BRANCH_SAMPLES, GlbLabels, MeshLabels, Skeleton,
    sample_points,
};
use arbrume_gen::{Generator, Plant};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// What `--annotations` asks for.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub struct AnnotationSet {
    /// Which of [`Self::SKELETON`], [`Self::LABELS`], [`Self::METRICS`] and
    /// [`Self::JSON`] are asked for.
    flags: u8,
    /// Number of points of the PLY point cloud.
    pub points: Option<usize>,
    /// Points per branch polyline of the branch table.
    pub branches: Option<usize>,
}

impl AnnotationSet {
    /// Nodes, chains and organ sites in the sidecar.
    pub const SKELETON: u8 = 1;
    /// Per-vertex labels in the sidecar and in the `.glb` files.
    pub const LABELS: u8 = 1 << 1;
    /// `ICTree` features in the manifest.
    pub const METRICS: u8 = 1 << 2;
    /// JSON copy of the sidecar.
    pub const JSON: u8 = 1 << 3;

    /// `true` when `flag` is asked for.
    const fn has(&self, flag: u8) -> bool {
        self.flags & flag != 0
    }

    /// `true` when the skeleton is asked for.
    pub const fn skeleton(&self) -> bool {
        self.has(Self::SKELETON)
    }

    /// `true` when the labels are asked for.
    pub const fn labels(&self) -> bool {
        self.has(Self::LABELS)
    }

    /// `true` when the metrics are asked for.
    pub const fn metrics(&self) -> bool {
        self.has(Self::METRICS)
    }

    /// `true` when the JSON copy of the sidecar is asked for.
    pub const fn json(&self) -> bool {
        self.has(Self::JSON)
    }

    /// `true` when anything is asked for.
    pub const fn any(&self) -> bool {
        self.sidecar() || self.points.is_some() || self.metrics()
    }

    /// `true` when the `.npz` sidecar is written.
    pub const fn sidecar(&self) -> bool {
        self.skeleton() || self.labels() || self.branches.is_some()
    }

    /// Names of the files written for the plant `stem`.
    pub fn files(&self, stem: &str) -> Files {
        Files {
            sidecar: self.sidecar().then(|| format!("{stem}.ann.npz")),
            json: (self.sidecar() && self.json()).then(|| format!("{stem}.ann.json")),
            points: self.points.map(|_| format!("{stem}.points.ply")),
        }
    }
}

impl FromStr for AnnotationSet {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, String> {
        let mut set = Self::default();
        for item in text
            .split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
        {
            let (name, value) = match item.split_once('=') {
                Some((name, value)) => (name.trim(), Some(value.trim())),
                None => (item, None),
            };
            let count = |minimum: usize| -> Result<usize, String> {
                let value = value.ok_or_else(|| format!("`{name}` needs a count: `{name}=N`"))?;
                let count = value
                    .parse::<usize>()
                    .map_err(|e| format!("`{item}`: {e}"))?;
                if count < minimum {
                    return Err(format!("`{item}`: at least {minimum}"));
                }
                Ok(count)
            };
            match (name, value) {
                ("skeleton", None) => set.flags |= Self::SKELETON,
                ("labels", None) => set.flags |= Self::LABELS,
                ("metrics", None) => set.flags |= Self::METRICS,
                ("json", None) => set.flags |= Self::JSON,
                ("points", _) => set.points = Some(count(1)?),
                ("branches", None) => set.branches = Some(DEFAULT_BRANCH_SAMPLES),
                ("branches", Some(_)) => set.branches = Some(count(2)?),
                _ => {
                    return Err(format!(
                        "unknown annotation `{item}`; expected skeleton, labels, \
                         points=N, branches[=K], metrics or json"
                    ));
                }
            }
        }
        if set.json() && !set.sidecar() {
            return Err("`json` needs one of skeleton, labels or branches".to_owned());
        }
        Ok(set)
    }
}

impl TryFrom<String> for AnnotationSet {
    type Error = String;

    fn try_from(text: String) -> Result<Self, String> {
        text.parse()
    }
}

impl fmt::Display for AnnotationSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut items = Vec::new();
        for (on, name) in [(self.skeleton(), "skeleton"), (self.labels(), "labels")] {
            if on {
                items.push(name.to_owned());
            }
        }
        if let Some(points) = self.points {
            items.push(format!("points={points}"));
        }
        if let Some(samples) = self.branches {
            items.push(format!("branches={samples}"));
        }
        for (on, name) in [(self.metrics(), "metrics"), (self.json(), "json")] {
            if on {
                items.push(name.to_owned());
            }
        }
        f.write_str(&items.join(","))
    }
}

/// Files written for one plant; `None` for those not asked for.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Files {
    /// The `.npz` sidecar.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sidecar: Option<String>,
    /// Its JSON copy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub json: Option<String>,
    /// The PLY point cloud.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub points: Option<String>,
}

impl Files {
    /// `true` when every file exists in `out`.
    pub fn exist(&self, out: &Path) -> bool {
        [&self.sidecar, &self.json, &self.points]
            .into_iter()
            .flatten()
            .all(|name| out.join(name).is_file())
    }
}

/// What the manifest line of an annotated plant gets.
#[derive(Debug, Serialize)]
pub struct ManifestExtra {
    /// Annotation files of the plant.
    pub annotations: Files,
    /// Full parameter set of the species.
    pub species_params: Value,
    /// `ICTree` features, when asked for.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metrics: Option<arbrume_metrics::Features>,
    /// Milliseconds spent on the annotations.
    pub annotate_ms: f64,
}

/// Labels of the current mesh of `plant`, as sidecar arrays and `.glb` attributes.
#[derive(Debug)]
pub struct Labels {
    /// Per-vertex labels.
    pub mesh: MeshLabels,
    /// The same as `f32` attributes.
    pub glb: GlbLabels,
}

/// Labels of the mesh of `plant`, or `None` when its parts are not exactly those the
/// mesher produced (a mesh with added parts, such as billboards, is not labelled).
pub fn labels(
    generator: &mut Generator,
    plant: &Plant,
    species: &Species,
) -> Result<Option<Labels>, String> {
    let provenance = generator
        .provenance(plant, species)
        .map_err(|e| e.to_string())?;
    let parts = &plant.mesh.parts;
    let matches = provenance.parts.len() == parts.len()
        && provenance
            .parts
            .iter()
            .zip(parts)
            .all(|((kind, runs), part)| {
                let covered = runs.last().map_or(0, |run| (run.start + run.len) as usize);
                *kind == part.kind && covered == part.buffers.positions.len()
            });
    if !matches {
        return Ok(None);
    }
    let mesh = MeshLabels::from_provenance(&provenance);
    let glb = GlbLabels::new(&mesh)?;
    Ok(Some(Labels { mesh, glb }))
}

/// Writes `bytes` to `out/name` through a temporary file.
fn write_atomic(out: &Path, name: &str, bytes: &[u8]) -> Result<(), String> {
    let path = out.join(name);
    let temp = out.join(format!("{name}.tmp"));
    fs::write(&temp, bytes).map_err(|e| format!("{}: {e}", temp.display()))?;
    fs::rename(&temp, &path).map_err(|e| format!("{}: {e}", path.display()))
}

/// The sidecar content of `plant` as `set` asks; `labels` are those of its mesh.
pub fn sidecar(
    set: &AnnotationSet,
    plant: &Plant,
    labels: Option<&Labels>,
    extras: Value,
) -> Annotations {
    Annotations {
        skeleton: set.skeleton().then(|| Skeleton::from_graph(&plant.graph)),
        labels: if set.labels() {
            labels.map(|labels| labels.mesh.clone())
        } else {
            None
        },
        branches: set
            .branches
            .map(|samples| BranchTable::from_graph(&plant.graph, samples)),
        extras: Some(extras),
    }
}

/// Writes the annotation files of `plant` as `stem` into `out` and returns what its
/// manifest line gets. `labels` are those of the mesh of `plant`.
pub fn write(
    set: &AnnotationSet,
    out: &Path,
    stem: &str,
    plant: &Plant,
    species: &Species,
    labels: Option<&Labels>,
) -> Result<ManifestExtra, String> {
    let start = std::time::Instant::now();
    let files = set.files(stem);
    let params = &plant.params;
    let extras = serde_json::json!({
        "stem": stem,
        "species_name": species.name,
        "seed": params.seed,
        "age": params.age,
        "scale": params.scale,
        "max_cycle": params.mesh.max_cycle,
    });
    let annotations = sidecar(set, plant, labels, extras);
    if let Some(name) = &files.sidecar {
        let mut bytes = Vec::new();
        annotations
            .write_npz(&mut bytes)
            .map_err(|e| e.to_string())?;
        write_atomic(out, name, &bytes)?;
    }
    if let Some(name) = &files.json {
        let text = serde_json::to_vec(&annotations.to_json()).map_err(|e| e.to_string())?;
        write_atomic(out, name, &text)?;
    }
    if let (Some(name), Some(count)) = (&files.points, set.points) {
        let parts = labels
            .ok_or("the mesh has parts that cannot be labelled; no point cloud")?
            .mesh
            .parts();
        let cloud = sample_points(&plant.mesh, &parts, count, params.mesh.seed);
        let mut bytes = Vec::with_capacity(512 + 29 * count);
        cloud.write_ply(&mut bytes).map_err(|e| e.to_string())?;
        write_atomic(out, name, &bytes)?;
    }
    let metrics = set
        .metrics()
        .then(|| arbrume_metrics::features(&plant.graph));
    let species_params = serde_json::to_value(species).map_err(|e| e.to_string())?;
    Ok(ManifestExtra {
        annotations: files,
        species_params,
        metrics,
        annotate_ms: start.elapsed().as_secs_f64() * 1e3,
    })
}

/// Computes every annotation of `set` for `plant` in memory, as `arbrume bench` times
/// them: labels, sidecar bytes, point cloud bytes and metrics. Returns the bytes made.
pub fn in_memory(
    set: &AnnotationSet,
    generator: &mut Generator,
    plant: &Plant,
    species: &Species,
    buffer: &mut Vec<u8>,
) -> Result<usize, String> {
    buffer.clear();
    let labels = if set.labels() || set.points.is_some() {
        labels(generator, plant, species)?
    } else {
        None
    };
    if set.sidecar() {
        let annotations = sidecar(set, plant, labels.as_ref(), Value::Null);
        annotations
            .write_npz(&mut *buffer)
            .map_err(|e| e.to_string())?;
    }
    if let (Some(count), Some(labels)) = (set.points, &labels) {
        let seed = plant.params.mesh.seed;
        let cloud = sample_points(&plant.mesh, &labels.mesh.parts(), count, seed);
        cloud.write_ply(&mut *buffer).map_err(|e| e.to_string())?;
    }
    if set.metrics() {
        std::hint::black_box(arbrume_metrics::features(&plant.graph));
    }
    Ok(buffer.len())
}

#[cfg(test)]
mod tests {
    use super::AnnotationSet;

    #[test]
    fn lists_parse_and_print() {
        let set: AnnotationSet = "skeleton, labels,points=100,branches,metrics,json"
            .parse()
            .unwrap();
        assert!(set.skeleton() && set.labels() && set.metrics() && set.json());
        assert_eq!((set.points, set.branches), (Some(100), Some(8)));
        assert_eq!(set.to_string().parse::<AnnotationSet>().unwrap(), set);
        assert_eq!(
            "branches=3".parse::<AnnotationSet>().unwrap().branches,
            Some(3)
        );
        for bad in [
            "points",
            "points=0",
            "branches=1",
            "bones",
            "json",
            "labels=2",
        ] {
            assert!(bad.parse::<AnnotationSet>().is_err(), "{bad}");
        }
        assert!(!"".parse::<AnnotationSet>().unwrap().any());
    }
}
