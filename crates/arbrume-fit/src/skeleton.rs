//! Target skeletons: a rooted forest of points with radii, and the files it is read from.
//!
//! A [`TreeSkeleton`] is the common input of the distance: one point per node, the
//! radius of the segment that ends at the node, and the index of the parent node (`-1`
//! for a root, whose radius is that of the base of its stem). Nothing else is assumed:
//! no age, no branching order, no internode structure, so the same type holds an
//! Arbrume plant and a skeleton extracted from a scan.
//!
//! [`TreeSkeleton::read`] chooses the reader from the file name:
//!
//! * `.ann.npz`: the skeleton sidecar of `arbrume gen --annotations skeleton`; shed
//!   nodes are left out and the base of every root axis becomes a root node;
//! * `.glb`: a file written by Arbrume, through its `<stem>.ann.npz` sidecar when there
//!   is one, else by growing the plant again from the metadata in its `extras`;
//! * `.csv`: one node per line, columns `id,x,y,z,radius,parent` (by header name, in any
//!   order), `parent` empty or `-1` for a root, `#` comments;
//! * `.json`: `{"position": [[x, y, z], ...], "radius": [...], "parent": [...]}` with
//!   parents as indices into the arrays.
//!
//! Lengths are in meters. Arbrume is Y up; [`Up::Z`] turns a Z-up file (most scans) into
//! Y up when it is read.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::Path;

use arbrume_core::graph::PlantGraph;
use arbrume_core::species::Species;
use arbrume_export::annotations::{Annotations, Skeleton};
use arbrume_gen::GenParams;
use arbrume_grow::{Surroundings, grow_in};
use glam::Vec3;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::FitError;

/// Vertical axis of a skeleton file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Up {
    /// +Y is up, as in Arbrume and glTF.
    #[default]
    Y,
    /// +Z is up: `(x, y, z)` is read as `(x, z, -y)`.
    Z,
}

/// A tree skeleton: points, radii and parents.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TreeSkeleton {
    /// Position of each node, meters, Y up.
    pub position: Vec<[f32; 3]>,
    /// Radius of the segment from the parent to the node; for a root, the radius of the
    /// base of its stem. Meters.
    pub radius: Vec<f32>,
    /// Index of the parent node, `-1` for a root.
    pub parent: Vec<i32>,
}

impl TreeSkeleton {
    /// Number of nodes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.position.len()
    }

    /// Whether the skeleton has no node.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.position.is_empty()
    }

    /// Checks array lengths, parent indices, finite values and the absence of cycles.
    ///
    /// # Errors
    ///
    /// Returns [`FitError::Skeleton`] describing the first problem found.
    pub fn validate(&self) -> Result<(), FitError> {
        let n = self.len();
        if self.radius.len() != n || self.parent.len() != n {
            return Err(FitError::skeleton(
                "position, radius and parent lengths differ",
            ));
        }
        for (i, (p, r)) in self.position.iter().zip(&self.radius).enumerate() {
            if !p.iter().all(|c| c.is_finite()) || !r.is_finite() || *r < 0.0 {
                return Err(FitError::skeleton(format!(
                    "node {i}: non-finite position or invalid radius"
                )));
            }
        }
        for (i, &parent) in self.parent.iter().enumerate() {
            if parent < -1 || parent >= signed(n) || parent == signed(i) {
                return Err(FitError::skeleton(format!(
                    "node {i}: parent {parent} out of range"
                )));
            }
        }
        if self.topological_order().len() != n {
            return Err(FitError::skeleton("the parent links form a cycle"));
        }
        Ok(())
    }

    /// Children of every node, in increasing index.
    #[must_use]
    pub fn children(&self) -> Vec<Vec<usize>> {
        let mut children = vec![Vec::new(); self.len()];
        for (i, &parent) in self.parent.iter().enumerate() {
            if let Ok(parent) = usize::try_from(parent) {
                children[parent].push(i);
            }
        }
        children
    }

    /// Nodes reachable from the roots, parents before children (breadth first). Shorter
    /// than [`TreeSkeleton::len`] when the parent links have a cycle.
    #[must_use]
    pub fn topological_order(&self) -> Vec<usize> {
        let children = self.children();
        let mut order: Vec<usize> = (0..self.len()).filter(|&i| self.parent[i] < 0).collect();
        let mut next = 0;
        while next < order.len() {
            let node = order[next];
            order.extend_from_slice(&children[node]);
            next += 1;
        }
        order
    }

    /// The live part of `graph`: one root node at the origin of each root axis, then
    /// every live node with the radius of its internode.
    #[must_use]
    pub fn from_graph(graph: &PlantGraph) -> Self {
        let mut skeleton = Self::default();
        let mut index = vec![-1_i32; graph.node_count()];
        for node in graph.node_ids() {
            if graph.is_shed(node) {
                continue;
            }
            let radius = graph.radii()[node.index()];
            let parent = if let Some(parent) = graph.parent(node) {
                index[parent.index()]
            } else {
                let origin = graph.chain_origin(graph.chain_of(node));
                skeleton.push(origin.to_array(), radius, -1)
            };
            index[node.index()] = skeleton.push(graph.position(node).to_array(), radius, parent);
        }
        skeleton
    }

    /// The live nodes of an annotation skeleton, as [`TreeSkeleton::from_graph`] reads a
    /// graph.
    ///
    /// # Errors
    ///
    /// Returns [`FitError::Skeleton`] when the arrays are inconsistent.
    pub fn from_annotations(skeleton: &Skeleton) -> Result<Self, FitError> {
        let graph = skeleton.to_graph().map_err(FitError::skeleton)?;
        Ok(Self::from_graph(&graph))
    }

    /// Appends a node and returns its index.
    fn push(&mut self, position: [f32; 3], radius: f32, parent: i32) -> i32 {
        self.position.push(position);
        self.radius.push(radius);
        self.parent.push(parent);
        signed(self.len()) - 1
    }

    /// The skeleton with `up` turned into +Y.
    #[must_use]
    pub fn with_up(mut self, up: Up) -> Self {
        if up == Up::Z {
            for p in &mut self.position {
                *p = [p[0], p[2], -p[1]];
            }
        }
        self
    }

    /// Reads a CSV skeleton (see the module documentation).
    ///
    /// # Errors
    ///
    /// Returns [`FitError::Format`] for a missing column, a bad number, a duplicate or
    /// unknown id.
    pub fn from_csv(text: &str) -> Result<Self, FitError> {
        let mut lines = text
            .lines()
            .map(str::trim)
            .enumerate()
            .filter(|(_, line)| !line.is_empty() && !line.starts_with('#'));
        let (_, header) = lines.next().ok_or_else(|| FitError::format("empty CSV"))?;
        let names: Vec<&str> = header.split(',').map(str::trim).collect();
        let column = |name: &str| {
            names
                .iter()
                .position(|n| n.eq_ignore_ascii_case(name))
                .ok_or_else(|| FitError::format(format!("CSV header lacks `{name}`")))
        };
        let columns = ["id", "x", "y", "z", "radius", "parent"].map(column);
        let columns: Vec<usize> = columns.into_iter().collect::<Result<_, _>>()?;
        let mut rows = Vec::new();
        for (line_number, line) in lines {
            let cells: Vec<&str> = line.split(',').map(str::trim).collect();
            let cell = |k: usize| cells.get(columns[k]).copied().unwrap_or("");
            let number = |k: usize| {
                cell(k).parse::<f64>().map_err(|_| {
                    FitError::format(format!("line {}: bad value `{}`", line_number + 1, cell(k)))
                })
            };
            let parent = match cell(5) {
                "" | "-1" => None,
                text => Some(text.to_owned()),
            };
            let position = [number(1)? as f32, number(2)? as f32, number(3)? as f32];
            rows.push((cell(0).to_owned(), position, number(4)? as f32, parent));
        }
        Self::from_rows(rows)
    }

    /// Builds a skeleton from `(id, position, radius, parent id)` rows.
    fn from_rows(rows: Vec<(String, [f32; 3], f32, Option<String>)>) -> Result<Self, FitError> {
        let mut index = HashMap::with_capacity(rows.len());
        for (row, (id, ..)) in rows.iter().enumerate() {
            if index.insert(id.clone(), signed(row)).is_some() {
                return Err(FitError::format(format!("id `{id}` given twice")));
            }
        }
        let mut skeleton = Self::default();
        for (id, position, radius, parent) in rows {
            let parent = match parent {
                None => -1,
                Some(parent) => *index.get(&parent).ok_or_else(|| {
                    FitError::format(format!("node `{id}`: unknown parent `{parent}`"))
                })?,
            };
            skeleton.push(position, radius, parent);
        }
        Ok(skeleton)
    }

    /// The skeleton as CSV with ids equal to the indices.
    #[must_use]
    pub fn to_csv(&self) -> String {
        let mut out = String::from("id,x,y,z,radius,parent\n");
        for i in 0..self.len() {
            let [x, y, z] = self.position[i];
            let parent = self.parent[i];
            let radius = self.radius[i];
            let _ = writeln!(out, "{i},{x},{y},{z},{radius},{parent}");
        }
        out
    }

    /// Reads the skeleton in `path`, choosing the reader from its name (see the module
    /// documentation), and turns `up` into +Y. A `.glb` or `.ann.npz` is always Y up.
    ///
    /// # Errors
    ///
    /// Returns [`FitError::Io`] when the file cannot be read, [`FitError::Format`] for an
    /// unknown extension or a malformed file, and [`FitError::Skeleton`] when the result
    /// is not a valid skeleton.
    pub fn read(path: &Path, up: Up) -> Result<Self, FitError> {
        let extension = path
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        let io = |error| FitError::Io(format!("{}: {error}", path.display()));
        let skeleton = if extension == "npz" {
            read_sidecar(&std::fs::read(path).map_err(io)?)?
        } else if extension == "glb" {
            read_glb(path)?
        } else if extension == "csv" {
            Self::from_csv(&std::fs::read_to_string(path).map_err(io)?)?.with_up(up)
        } else if extension == "json" {
            let text = std::fs::read_to_string(path).map_err(io)?;
            let skeleton: Self = serde_json::from_str(&text)
                .map_err(|error| FitError::format(format!("{}: {error}", path.display())))?;
            skeleton.with_up(up)
        } else {
            return Err(FitError::format(format!(
                "{}: expected .ann.npz, .glb, .csv or .json",
                path.display()
            )));
        };
        skeleton.validate()?;
        Ok(skeleton)
    }
}

/// The skeleton of an annotation sidecar.
fn read_sidecar(bytes: &[u8]) -> Result<TreeSkeleton, FitError> {
    let annotations = Annotations::read_npz(bytes).map_err(FitError::format)?;
    let skeleton = annotations
        .skeleton
        .ok_or_else(|| FitError::format("the sidecar has no skeleton arrays"))?;
    TreeSkeleton::from_annotations(&skeleton)
}

/// The skeleton of an Arbrume `.glb`: from its sidecar, else grown from its metadata.
fn read_glb(path: &Path) -> Result<TreeSkeleton, FitError> {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
    let stem = name
        .as_deref()
        .and_then(|n| n.strip_suffix(".glb"))
        .unwrap_or("");
    let sidecar = path.with_file_name(format!("{stem}.ann.npz"));
    if let Ok(bytes) = std::fs::read(&sidecar) {
        return read_sidecar(&bytes);
    }
    let bytes = std::fs::read(path)
        .map_err(|error| FitError::Io(format!("{}: {error}", path.display())))?;
    let extras = glb_extras(&bytes)?;
    let graph = grow_from_metadata(&extras)?;
    Ok(TreeSkeleton::from_graph(&graph))
}

/// The `extras` of the asset of a binary glTF file.
fn glb_extras(bytes: &[u8]) -> Result<Value, FitError> {
    let word = |at: usize| {
        bytes
            .get(at..at + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
    };
    if bytes.get(0..4) != Some(b"glTF".as_slice()) || word(16) != Some(0x4E4F_534A) {
        return Err(FitError::format("not a binary glTF file with a JSON chunk"));
    }
    let length = word(12).ok_or_else(|| FitError::format("truncated glTF"))?;
    let json = bytes
        .get(20..20 + length)
        .ok_or_else(|| FitError::format("truncated glTF"))?;
    let root: Value = serde_json::from_slice(json).map_err(|e| FitError::format(e.to_string()))?;
    let extras = root["asset"]["extras"].clone();
    if extras.get("species").is_none() {
        return Err(FitError::format(
            "the glTF has no Arbrume metadata and no `.ann.npz` sidecar next to it",
        ));
    }
    Ok(extras)
}

/// Grows the plant that the metadata `extras` of an exported file describe.
fn grow_from_metadata(extras: &Value) -> Result<PlantGraph, FitError> {
    if !extras["surroundings"].is_null() {
        return Err(FitError::format(
            "the plant grew in surroundings; read its `.ann.npz` sidecar instead",
        ));
    }
    let species: Species = serde_json::from_value(extras["species"].clone())
        .map_err(|e| FitError::format(format!("species in the metadata: {e}")))?;
    let field = |key: &str| {
        extras[key]
            .as_f64()
            .ok_or_else(|| FitError::format(format!("metadata lacks `{key}`")))
    };
    let seed = extras["seed"]
        .as_u64()
        .ok_or_else(|| FitError::format("metadata lacks `seed`"))?;
    let params = GenParams::new(seed, field("age")? as u16).with_scale(field("scale")? as f32);
    species
        .validate()
        .map_err(|e| FitError::format(e.to_string()))?;
    Ok(grow_in(
        &species,
        &params.grow_params(),
        &Surroundings::none(),
    ))
}

/// `index` as an `i32`; skeleton sizes stay far below `i32::MAX`.
pub(crate) fn signed(index: usize) -> i32 {
    i32::try_from(index).unwrap_or(i32::MAX)
}

/// Position of node `i` as a vector.
pub(crate) fn point(skeleton: &TreeSkeleton, i: usize) -> Vec3 {
    Vec3::from_array(skeleton.position[i])
}

#[cfg(test)]
mod tests {
    use super::{TreeSkeleton, Up};

    #[test]
    fn csv_round_trips_and_ids_are_free() {
        let text =
            "# a fork\nid,x,y,z,radius,parent\nb,0,1,0,0.1,a\na,0,0,0,0.1,\nc,1,2,0,0.05,b\n";
        let skeleton = TreeSkeleton::from_csv(text).unwrap();
        skeleton.validate().unwrap();
        assert_eq!(skeleton.parent, vec![1, -1, 0]);
        let again = TreeSkeleton::from_csv(&skeleton.to_csv()).unwrap();
        assert_eq!(again, skeleton);
        let z_up = skeleton.clone().with_up(Up::Z);
        assert_eq!(z_up.position[0], [0.0, 0.0, -1.0]);
    }

    #[test]
    fn bad_skeletons_are_rejected() {
        let cycle = TreeSkeleton {
            position: vec![[0.0; 3]; 2],
            radius: vec![0.1; 2],
            parent: vec![1, 0],
        };
        assert!(cycle.validate().is_err());
        assert!(TreeSkeleton::from_csv("id,x,y,z,radius\n").is_err());
        assert!(TreeSkeleton::from_csv("id,x,y,z,radius,parent\na,0,0,0,0.1,q\n").is_err());
    }
}
