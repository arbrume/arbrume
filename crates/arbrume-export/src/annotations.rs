//! ML ground truth: skeleton, per-vertex labels, branch table and point clouds.
//!
//! Everything here is computed from the plant graph and the mesh that generation already
//! produced; nothing is estimated. The pieces:
//!
//! * [`Skeleton`]: nodes (the node signature of Zhou et al. 2023: position, radius,
//!   parent, order, birth cycle, plus chain id, shed flag and arc lengths) and chains
//!   in depth-first order, and the organ sites.
//! * [`MeshLabels`]: the [`SemanticClass`] (trunk, branch, twig, leaf) and the instance
//!   id (chain id for bark, organ index for foliage) of every vertex, from
//!   `arbrume_core::mesh::MeshProvenance`. [`GlbLabels`] carries them into a `.glb` as
//!   the custom vertex attributes `_SEMANTIC` and `_INSTANCE`.
//! * [`BranchTable`]: one row per axis in depth-first order (Wang et al. 2025).
//! * [`PointCloud`]: area-uniform labelled samples on the mesh, written as PLY.
//!
//! [`Annotations`] bundles the first three into one sidecar, written as an uncompressed
//! `.npz` archive ([`npz`]) that `numpy.load` opens directly, or as JSON with the same
//! keys. Both carry a `meta` object: the format name, [`SCHEMA_VERSION`], the class and
//! kind names, the description of every array present, and caller extras. The layout is
//! documented in `docs/annotations.md`.

mod branches;
pub mod npz;
mod points;
mod skeleton;

pub use branches::{BranchTable, DEFAULT_BRANCH_SAMPLES};
pub use points::{PointCloud, sample_points};
pub use skeleton::{Skeleton, organ_kind_code};

use std::io::{self, Write};

use arbrume_core::mesh::{MeshProvenance, PartKind, PartLabels, SemanticClass};
use serde_json::{Map, Value, json};

use crate::glb::{AttributeData, ExtraAttribute};
use npz::{Array, Dtype};

/// Version of the annotation formats; raised on any incompatible change.
pub const SCHEMA_VERSION: u32 = 1;

/// Name of the format in `meta`.
pub const FORMAT: &str = "arbrume-annotations";

/// Largest instance id a `.glb` label can carry exactly as `f32` (2^24).
pub const MAX_GLB_INSTANCE: u32 = 1 << 24;

/// Per-vertex labels of a whole mesh, parts concatenated in mesh order (the order of
/// the primitives of the `.glb`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MeshLabels {
    /// Kind of each part: 0 bark, 1 leaf, 2 billboard.
    pub part_kind: Vec<u8>,
    /// First vertex of each part, plus the total: `parts + 1` entries.
    pub part_offsets: Vec<u32>,
    /// [`SemanticClass`] value of each vertex.
    pub semantic: Vec<u8>,
    /// Instance id of each vertex: chain id for trunk and branch, organ index for twig
    /// and leaf.
    pub instance: Vec<u32>,
}

impl MeshLabels {
    /// Labels of the mesh described by `provenance`.
    #[must_use]
    pub fn from_provenance(provenance: &MeshProvenance) -> Self {
        let mut labels = Self {
            part_offsets: vec![0],
            ..Self::default()
        };
        for (kind, runs) in &provenance.parts {
            let base = labels.semantic.len();
            let count = runs.last().map_or(0, |run| (run.start + run.len) as usize);
            labels.semantic.resize(base + count, u8::MAX);
            labels.instance.resize(base + count, u32::MAX);
            for run in runs {
                let range = base + run.start as usize..base + (run.start + run.len) as usize;
                labels.semantic[range.clone()].fill(run.class as u8);
                labels.instance[range].fill(run.instance);
            }
            labels.part_kind.push(part_kind_code(*kind));
            labels.part_offsets.push(labels.semantic.len() as u32);
        }
        labels
    }

    /// Labels of part `part`.
    #[must_use]
    pub fn part(&self, part: usize) -> PartLabels {
        let range = self.part_offsets[part] as usize..self.part_offsets[part + 1] as usize;
        PartLabels {
            semantic: self.semantic[range.clone()].to_vec(),
            instance: self.instance[range].to_vec(),
        }
    }

    /// Labels of every part.
    #[must_use]
    pub fn parts(&self) -> Vec<PartLabels> {
        (0..self.part_kind.len())
            .map(|part| self.part(part))
            .collect()
    }
}

/// Value written for a [`PartKind`].
#[must_use]
pub const fn part_kind_code(kind: PartKind) -> u8 {
    match kind {
        PartKind::Bark => 0,
        PartKind::Leaf => 1,
        PartKind::Billboard => 2,
    }
}

/// The labels of a mesh as `f32` vertex attributes for [`crate::write_glb_with`].
///
/// glTF allows `u32` components for indices only and vertex elements of 4 bytes or more,
/// so `_SEMANTIC` and `_INSTANCE` are `f32` scalars holding integers, exact below
/// [`MAX_GLB_INSTANCE`].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GlbLabels {
    semantic: Vec<Vec<f32>>,
    instance: Vec<Vec<f32>>,
}

impl GlbLabels {
    /// Converts `labels`, one entry per part of the mesh.
    ///
    /// # Errors
    ///
    /// Returns a message when an instance id is [`MAX_GLB_INSTANCE`] or above.
    pub fn new(labels: &MeshLabels) -> Result<Self, String> {
        if labels.instance.iter().any(|&id| id >= MAX_GLB_INSTANCE) {
            return Err("instance ids reach 2^24; `_INSTANCE` would be inexact".to_owned());
        }
        let ranges = labels.part_offsets.windows(2);
        let ranges: Vec<_> = ranges.map(|w| w[0] as usize..w[1] as usize).collect();
        Ok(Self {
            semantic: ranges
                .iter()
                .map(|r| {
                    labels.semantic[r.clone()]
                        .iter()
                        .map(|&c| f32::from(c))
                        .collect()
                })
                .collect(),
            instance: ranges
                .iter()
                .map(|r| {
                    labels.instance[r.clone()]
                        .iter()
                        .map(|&i| i as f32)
                        .collect()
                })
                .collect(),
        })
    }

    /// The `_SEMANTIC` and `_INSTANCE` attributes of every part.
    #[must_use]
    pub fn attributes(&self) -> Vec<ExtraAttribute<'_>> {
        let mut attributes = Vec::with_capacity(self.semantic.len() * 2);
        for (part, (semantic, instance)) in self.semantic.iter().zip(&self.instance).enumerate() {
            for (name, values) in [("_SEMANTIC", semantic), ("_INSTANCE", instance)] {
                attributes.push(ExtraAttribute {
                    part,
                    name: name.to_owned(),
                    components: 1,
                    data: AttributeData::F32(values),
                });
            }
        }
        attributes
    }
}

/// One annotation sidecar: whichever of skeleton, labels and branches were asked for.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Annotations {
    /// Nodes, chains and organ sites.
    pub skeleton: Option<Skeleton>,
    /// Per-vertex labels of the mesh.
    pub labels: Option<MeshLabels>,
    /// Branch table.
    pub branches: Option<BranchTable>,
    /// Caller data copied into `meta.extras` (species, seed, age).
    pub extras: Option<Value>,
}

/// Name, shape (with symbolic dimensions) and meaning of every array of a sidecar.
const ARRAYS: [(&str, &str, &str); 34] = [
    ("node_position", "N,3", "distal end of each node, meters"),
    ("node_radius", "N", "internode radius, meters"),
    ("node_parent", "N", "row of the parent node, -1 for none"),
    ("node_order", "N", "branching order, 0 for the trunk"),
    ("node_birth", "N", "growth cycle in which the node appeared"),
    ("node_chain", "N", "chain id"),
    ("node_shed", "N", "1 when the node was shed"),
    (
        "node_dist_root",
        "N",
        "arc length from the base of the plant, meters",
    ),
    (
        "node_dist_chain",
        "N",
        "arc length from the base of the chain, meters",
    ),
    (
        "chain_id",
        "C",
        "chain id of each chain row, rows in depth-first order",
    ),
    ("chain_first", "C", "row of the first node"),
    ("chain_last", "C", "row of the last node"),
    (
        "chain_parent",
        "C",
        "chain id of the parent chain, -1 for a root chain",
    ),
    (
        "chain_attach",
        "C",
        "row of the attachment node, -1 for a root chain",
    ),
    ("chain_order", "C", "branching order"),
    ("chain_origin", "C,3", "base point of the chain, meters"),
    ("organ_node", "O", "row of the node bearing the organ"),
    ("organ_kind", "O", "0 leaf, 1 twig, 2 flower, 3 fruit"),
    ("organ_direction", "O,3", "unit direction of the organ axis"),
    ("organ_normal", "O,3", "unit normal of the organ"),
    ("organ_scale", "O", "size factor on the species organ size"),
    (
        "organ_birth",
        "O",
        "growth cycle in which the organ appeared",
    ),
    (
        "part_kind",
        "P",
        "kind of each glb primitive: 0 bark, 1 leaf, 2 billboard",
    ),
    (
        "part_offsets",
        "P+1",
        "first vertex of each primitive, then the total",
    ),
    ("vertex_semantic", "V", "0 trunk, 1 branch, 2 twig, 3 leaf"),
    (
        "vertex_instance",
        "V",
        "chain id (trunk, branch) or organ index (twig, leaf)",
    ),
    ("branch_chain", "B", "chain id, rows in depth-first order"),
    (
        "branch_parent",
        "B",
        "row of the parent branch, -1 for a root axis",
    ),
    (
        "branch_attach",
        "B",
        "attachment point along the parent, 0 base to 1 tip",
    ),
    ("branch_order", "B", "branching order"),
    ("branch_birth", "B", "growth cycle of the first node"),
    (
        "branch_scalars",
        "B,4",
        "live nodes, length, base radius, tip radius",
    ),
    (
        "branch_direction",
        "B,3",
        "unit direction of the first internode",
    ),
    (
        "branch_polyline",
        "B,K,4",
        "K points (x, y, z, r) evenly spaced by arc length",
    ),
];

fn flat3(values: &[[f32; 3]]) -> &[f32] {
    bytemuck::cast_slice(values)
}

impl Annotations {
    /// The arrays of the sidecar, in the order of the format description.
    #[must_use]
    pub fn arrays(&self) -> Vec<Array<'_>> {
        let mut arrays = Vec::new();
        if let Some(s) = &self.skeleton {
            skeleton_arrays(s, &mut arrays);
        }
        if let Some(labels) = &self.labels {
            let parts = labels.part_kind.len();
            let vertices = labels.semantic.len();
            arrays.push(Array::new(
                "part_kind",
                Dtype::U8,
                &[parts],
                &labels.part_kind,
            ));
            let offsets = &labels.part_offsets;
            arrays.push(Array::new(
                "part_offsets",
                Dtype::U32,
                &[parts + 1],
                offsets,
            ));
            let semantic = &labels.semantic;
            arrays.push(Array::new(
                "vertex_semantic",
                Dtype::U8,
                &[vertices],
                semantic,
            ));
            let instance = &labels.instance;
            arrays.push(Array::new(
                "vertex_instance",
                Dtype::U32,
                &[vertices],
                instance,
            ));
        }
        if let Some(b) = &self.branches {
            branch_arrays(b, &mut arrays);
        }
        arrays
    }

    /// The `meta` object: format, version, names of classes and kinds, the description
    /// of each array present, the number of polyline points and the caller extras.
    #[must_use]
    pub fn meta(&self, arrays: &[Array<'_>]) -> Value {
        let mut described = Map::new();
        for array in arrays {
            if let Some((_, shape, meaning)) = ARRAYS.iter().find(|(n, _, _)| *n == array.name) {
                let entry =
                    json!({ "dtype": array.dtype.descr(), "shape": shape, "meaning": meaning });
                described.insert(array.name.clone(), entry);
            }
        }
        let classes: Vec<&str> = SemanticClass::ALL.iter().map(|c| c.name()).collect();
        json!({
            "format": FORMAT,
            "version": SCHEMA_VERSION,
            "semantic_classes": classes,
            "organ_kinds": ["leaf", "twig", "flower", "fruit"],
            "part_kinds": ["bark", "leaf", "billboard"],
            "branch_samples": self.branches.as_ref().map(|b| b.samples),
            "arrays": described,
            "extras": self.extras,
        })
    }

    /// Writes the sidecar as an uncompressed `.npz` archive: every array, plus `meta`,
    /// the UTF-8 bytes of the JSON of [`Annotations::meta`] as a `u8` array.
    ///
    /// # Errors
    ///
    /// Returns the error of `out`.
    pub fn write_npz(&self, out: impl Write) -> io::Result<()> {
        let mut arrays = self.arrays();
        let meta = self.meta(&arrays).to_string().into_bytes();
        arrays.push(Array::owned("meta", Dtype::U8, &[meta.len()], &meta));
        npz::write_npz(&arrays, out)
    }

    /// The sidecar as JSON: `meta`, then every array under its name as nested lists.
    #[must_use]
    pub fn to_json(&self) -> Value {
        let arrays = self.arrays();
        let mut root = Map::new();
        root.insert("meta".to_owned(), self.meta(&arrays));
        for array in &arrays {
            root.insert(array.name.clone(), array_json(array));
        }
        Value::Object(root)
    }

    /// Reads a sidecar written by [`Annotations::write_npz`].
    ///
    /// # Errors
    ///
    /// Returns a message for a malformed archive, an unknown format or version, or an
    /// array of the wrong type or shape.
    pub fn read_npz(bytes: &[u8]) -> Result<Self, String> {
        let arrays = npz::read_npz(bytes)?;
        let get = Fields(&arrays);
        let meta: Value = serde_json::from_slice(&get.data("meta")?.data)
            .map_err(|error| format!("meta: {error}"))?;
        if meta["format"] != FORMAT || meta["version"] != SCHEMA_VERSION {
            return Err(format!("not a version {SCHEMA_VERSION} {FORMAT} file"));
        }
        let skeleton = get
            .has("node_position")
            .then(|| get.skeleton())
            .transpose()?;
        let labels = get
            .has("vertex_semantic")
            .then(|| get.labels())
            .transpose()?;
        let branches = get
            .has("branch_chain")
            .then(|| get.branches())
            .transpose()?;
        Ok(Self {
            skeleton,
            labels,
            branches,
            extras: meta.get("extras").filter(|v| !v.is_null()).cloned(),
        })
    }
}

fn skeleton_arrays<'a>(s: &'a Skeleton, arrays: &mut Vec<Array<'a>>) {
    let (n, c, o) = (s.node_count(), s.chain_id.len(), s.organ_node.len());
    arrays.extend([
        Array::new(
            "node_position",
            Dtype::F32,
            &[n, 3],
            flat3(&s.node_position),
        ),
        Array::new("node_radius", Dtype::F32, &[n], &s.node_radius),
        Array::new("node_parent", Dtype::I32, &[n], &s.node_parent),
        Array::new("node_order", Dtype::U8, &[n], &s.node_order),
        Array::new("node_birth", Dtype::U16, &[n], &s.node_birth),
        Array::new("node_chain", Dtype::U32, &[n], &s.node_chain),
        Array::new("node_shed", Dtype::U8, &[n], &s.node_shed),
        Array::new("node_dist_root", Dtype::F32, &[n], &s.node_dist_root),
        Array::new("node_dist_chain", Dtype::F32, &[n], &s.node_dist_chain),
        Array::new("chain_id", Dtype::U32, &[c], &s.chain_id),
        Array::new("chain_first", Dtype::U32, &[c], &s.chain_first),
        Array::new("chain_last", Dtype::U32, &[c], &s.chain_last),
        Array::new("chain_parent", Dtype::I32, &[c], &s.chain_parent),
        Array::new("chain_attach", Dtype::I32, &[c], &s.chain_attach),
        Array::new("chain_order", Dtype::U8, &[c], &s.chain_order),
        Array::new("chain_origin", Dtype::F32, &[c, 3], flat3(&s.chain_origin)),
        Array::new("organ_node", Dtype::U32, &[o], &s.organ_node),
        Array::new("organ_kind", Dtype::U8, &[o], &s.organ_kind),
        Array::new(
            "organ_direction",
            Dtype::F32,
            &[o, 3],
            flat3(&s.organ_direction),
        ),
        Array::new("organ_normal", Dtype::F32, &[o, 3], flat3(&s.organ_normal)),
        Array::new("organ_scale", Dtype::F32, &[o], &s.organ_scale),
        Array::new("organ_birth", Dtype::U16, &[o], &s.organ_birth),
    ]);
}

fn branch_arrays<'a>(b: &'a BranchTable, arrays: &mut Vec<Array<'a>>) {
    let count = b.len();
    let scalars: Vec<f32> = (0..count)
        .flat_map(|i| {
            [
                b.nodes[i] as f32,
                b.length[i],
                b.base_radius[i],
                b.tip_radius[i],
            ]
        })
        .collect();
    let polyline: &[f32] = bytemuck::cast_slice(&b.polyline);
    arrays.extend([
        Array::new("branch_chain", Dtype::U32, &[count], &b.chain),
        Array::new("branch_parent", Dtype::I32, &[count], &b.parent),
        Array::new("branch_attach", Dtype::F32, &[count], &b.attach),
        Array::new("branch_order", Dtype::U8, &[count], &b.order),
        Array::new("branch_birth", Dtype::U16, &[count], &b.birth),
        Array::owned("branch_scalars", Dtype::F32, &[count, 4], &scalars),
        Array::new(
            "branch_direction",
            Dtype::F32,
            &[count, 3],
            flat3(&b.direction),
        ),
        Array::new(
            "branch_polyline",
            Dtype::F32,
            &[count, b.samples, 4],
            polyline,
        ),
    ]);
}

/// Nested JSON lists of the values of `array`.
fn array_json(array: &Array<'_>) -> Value {
    let flat: Vec<Value> = match array.dtype {
        Dtype::U8 => array.data.iter().map(|&v| v.into()).collect(),
        Dtype::U16 => values::<u16>(array).into_iter().map(Into::into).collect(),
        Dtype::U32 => values::<u32>(array).into_iter().map(Into::into).collect(),
        Dtype::I32 => values::<i32>(array).into_iter().map(Into::into).collect(),
        Dtype::F32 => values::<f32>(array).into_iter().map(Into::into).collect(),
    };
    nest(&flat, &array.shape)
}

fn values<T: bytemuck::Pod>(array: &Array<'_>) -> Vec<T> {
    array.values().unwrap_or_default()
}

fn nest(flat: &[Value], shape: &[usize]) -> Value {
    match shape {
        [] => flat.first().cloned().unwrap_or(Value::Null),
        [_] => Value::Array(flat.to_vec()),
        [_, inner @ ..] => {
            let stride = inner.iter().product::<usize>().max(1);
            let rows = flat.chunks(stride).map(|row| nest(row, inner));
            Value::Array(rows.collect())
        }
    }
}

/// Typed access to the arrays of an archive by name.
struct Fields<'a>(&'a [Array<'static>]);

impl Fields<'_> {
    fn has(&self, name: &str) -> bool {
        self.0.iter().any(|array| array.name == name)
    }

    fn data(&self, name: &str) -> Result<&Array<'static>, String> {
        self.0
            .iter()
            .find(|array| array.name == name)
            .ok_or_else(|| format!("missing array `{name}`"))
    }

    fn get<T: bytemuck::Pod>(&self, name: &str, dtype: Dtype) -> Result<Vec<T>, String> {
        let array = self.data(name)?;
        if array.dtype != dtype {
            return Err(format!("`{name}` is {}", array.dtype.descr()));
        }
        array.values()
    }

    fn vec3(&self, name: &str) -> Result<Vec<[f32; 3]>, String> {
        let flat: Vec<f32> = self.get(name, Dtype::F32)?;
        let (rows, rest) = flat.as_chunks::<3>();
        if !rest.is_empty() {
            return Err(format!("`{name}` is not a list of 3-vectors"));
        }
        Ok(rows.to_vec())
    }

    fn skeleton(&self) -> Result<Skeleton, String> {
        Ok(Skeleton {
            node_position: self.vec3("node_position")?,
            node_radius: self.get("node_radius", Dtype::F32)?,
            node_parent: self.get("node_parent", Dtype::I32)?,
            node_order: self.get("node_order", Dtype::U8)?,
            node_birth: self.get("node_birth", Dtype::U16)?,
            node_chain: self.get("node_chain", Dtype::U32)?,
            node_shed: self.get("node_shed", Dtype::U8)?,
            node_dist_root: self.get("node_dist_root", Dtype::F32)?,
            node_dist_chain: self.get("node_dist_chain", Dtype::F32)?,
            chain_id: self.get("chain_id", Dtype::U32)?,
            chain_first: self.get("chain_first", Dtype::U32)?,
            chain_last: self.get("chain_last", Dtype::U32)?,
            chain_parent: self.get("chain_parent", Dtype::I32)?,
            chain_attach: self.get("chain_attach", Dtype::I32)?,
            chain_order: self.get("chain_order", Dtype::U8)?,
            chain_origin: self.vec3("chain_origin")?,
            organ_node: self.get("organ_node", Dtype::U32)?,
            organ_kind: self.get("organ_kind", Dtype::U8)?,
            organ_direction: self.vec3("organ_direction")?,
            organ_normal: self.vec3("organ_normal")?,
            organ_scale: self.get("organ_scale", Dtype::F32)?,
            organ_birth: self.get("organ_birth", Dtype::U16)?,
        })
    }

    fn labels(&self) -> Result<MeshLabels, String> {
        Ok(MeshLabels {
            part_kind: self.get("part_kind", Dtype::U8)?,
            part_offsets: self.get("part_offsets", Dtype::U32)?,
            semantic: self.get("vertex_semantic", Dtype::U8)?,
            instance: self.get("vertex_instance", Dtype::U32)?,
        })
    }

    fn branches(&self) -> Result<BranchTable, String> {
        let polyline = self.data("branch_polyline")?;
        let samples = polyline.shape.get(1).copied().unwrap_or(0);
        let scalars: Vec<f32> = self.get("branch_scalars", Dtype::F32)?;
        let column = |k: usize| {
            scalars
                .iter()
                .skip(k)
                .step_by(4)
                .copied()
                .collect::<Vec<_>>()
        };
        let points: Vec<f32> = self.get("branch_polyline", Dtype::F32)?;
        let polyline: Vec<[f32; 4]> = points.as_chunks::<4>().0.to_vec();
        let ends = |k: usize| -> Vec<[f32; 4]> {
            polyline
                .chunks(samples.max(1))
                .map(|row| row[k.min(row.len() - 1)])
                .collect()
        };
        Ok(BranchTable {
            samples,
            chain: self.get("branch_chain", Dtype::U32)?,
            parent: self.get("branch_parent", Dtype::I32)?,
            attach: self.get("branch_attach", Dtype::F32)?,
            order: self.get("branch_order", Dtype::U8)?,
            birth: self.get("branch_birth", Dtype::U16)?,
            nodes: column(0).into_iter().map(|n| n as u32).collect(),
            length: column(1),
            base_radius: column(2),
            tip_radius: column(3),
            direction: self.vec3("branch_direction")?,
            start: ends(0),
            end: ends(samples.saturating_sub(1)),
            polyline,
        })
    }
}
