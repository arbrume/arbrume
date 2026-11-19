//! An optional skeleton built from the wind data, for engines without custom vertex
//! shaders: the wind then drives joints instead of a shader.
//!
//! Every branch of level at most `max_level` gets `segments` joints, the first at its
//! pivot and the others where its bark is at `x = k / segments` (the centroid of the
//! bark vertices of that position, or a point along its base direction when it has
//! none). Joints of a branch are chained base to tip; the first joint of a branch is a
//! child of the joint of its parent branch below its attachment. A vertex is bound to
//! the branch of its lineage at level `min(level, max_level)`, by its position `x`
//! along it: to the two joints around `x`, weighted linearly, or to the last joint past
//! it. Unused influences have joint 0 and weight 0, as glTF asks.

use super::json::{ARRAY_BUFFER, Attributes, FLOAT, Skin, UNSIGNED_BYTE, UNSIGNED_SHORT};
use super::{Builder, Payload, SkinOptions};
use arbrume_core::mesh::{MeshPart, PartKind, TreeMesh, WindTable};
use glam::Vec3;
use std::borrow::Cow;

/// The joints of the skin of one mesh.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Skinning {
    /// First joint of each branch (index into `joints`), `None` when not skinned.
    first: Vec<Option<u32>>,
    /// Joints per branch.
    segments: u32,
    /// Deepest skinned level.
    max_level: u8,
    /// World position of each joint.
    positions: Vec<Vec3>,
    /// Node index of each joint.
    pub nodes: Vec<u32>,
}

/// Centroid of the bark vertices of each branch at each joint position, flattened by
/// branch then joint; `None` where no vertex falls.
fn joint_centroids(mesh: &TreeMesh, branches: usize, segments: u32) -> Vec<Option<Vec3>> {
    let slots = segments as usize;
    let mut sums = vec![(Vec3::ZERO, 0_u32); branches * slots];
    for part in mesh.parts.iter().filter(|part| part.kind == PartKind::Bark) {
        let table = mesh.wind.as_ref();
        for (wind, &p) in part.buffers.wind.iter().zip(&part.buffers.positions) {
            let branch = wind[4] as usize;
            let Some(level) = table.and_then(|t| t.branches.get(branch)).map(|b| b.level) else {
                continue;
            };
            let k = (wind[usize::from(level)] * segments as f32).round() as usize;
            if k < slots {
                let slot = &mut sums[branch * slots + k];
                slot.0 += Vec3::from(p);
                slot.1 += 1;
            }
        }
    }
    sums.into_iter()
        .map(|(sum, count)| (count > 0).then(|| sum / count as f32))
        .collect()
}

impl Skinning {
    /// The joints of `mesh` and their parents (as joint indices).
    fn plan(mesh: &TreeMesh, table: &WindTable, options: SkinOptions) -> (Self, Vec<Option<u32>>) {
        let segments = options.segments.max(1);
        let centroids = joint_centroids(mesh, table.branches.len(), segments);
        let mut skinning = Self {
            first: vec![None; table.branches.len()],
            segments,
            max_level: options.max_level.min(3),
            positions: Vec::new(),
            nodes: Vec::new(),
        };
        let mut parents = Vec::new();
        for (index, branch) in table.branches.iter().enumerate() {
            if branch.level > skinning.max_level {
                continue;
            }
            let parent_joint = branch.parent.and_then(|parent| {
                let parent_branch = &table.branches[parent as usize];
                let x = branch.lineage_x[usize::from(parent_branch.level)];
                skinning.joint(parent as usize, x)
            });
            skinning.first[index] = Some(skinning.positions.len() as u32);
            let (pivot, tangent) = (Vec3::from(branch.pivot), Vec3::from(branch.tangent));
            for k in 0..segments {
                let along = pivot + tangent * (branch.length * k as f32 / segments as f32);
                let centroid = centroids[index * segments as usize + k as usize];
                let position = if k == 0 {
                    pivot
                } else {
                    centroid.unwrap_or(along)
                };
                let parent = if k == 0 {
                    parent_joint
                } else {
                    Some(skinning.positions.len() as u32 - 1)
                };
                skinning.positions.push(position);
                parents.push(parent);
            }
        }
        (skinning, parents)
    }

    /// The joint of `branch` below the position `x`, `None` when it is not skinned.
    fn joint(&self, branch: usize, x: f32) -> Option<u32> {
        let first = self.first[branch]?;
        let k = ((x * self.segments as f32).floor() as u32).min(self.segments - 1);
        Some(first + k)
    }

    /// Joints and weights of a vertex with wind data `wind`.
    fn influence(&self, table: &WindTable, wind: &[f32; 6]) -> ([u32; 4], [f32; 4]) {
        let Some(own) = table.branches.get(wind[4] as usize) else {
            return ([0; 4], [1.0, 0.0, 0.0, 0.0]);
        };
        let level = own.level.min(self.max_level);
        let branch = own.lineage[usize::from(level)] as usize;
        let Some(first) = self.first.get(branch).copied().flatten() else {
            return ([0; 4], [1.0, 0.0, 0.0, 0.0]);
        };
        let t = wind[usize::from(level)].clamp(0.0, 1.0) * self.segments as f32;
        let k = (t.floor() as u32).min(self.segments - 1);
        let f = t - k as f32;
        if k + 1 < self.segments && f > 0.0 {
            ([first + k, first + k + 1, 0, 0], [1.0 - f, f, 0.0, 0.0])
        } else {
            ([first + k, 0, 0, 0], [1.0, 0.0, 0.0, 0.0])
        }
    }
}

/// Column-major 4x4 matrix translating by `-p`.
fn inverse_bind(p: Vec3) -> [f32; 16] {
    [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, -p.x, -p.y, -p.z, 1.0,
    ]
}

impl<'a> Builder<'a> {
    /// Creates the joint nodes and the skin of `mesh` when the export asks for a skin
    /// and the mesh has wind data; returns the skin index. The joint roots are added to
    /// the scene roots under one skeleton node.
    pub(super) fn begin_skin(&mut self, mesh: &TreeMesh, name: &str) -> Option<u32> {
        let options = self.export.skin.as_ref()?;
        let table = mesh.wind.as_ref()?;
        let (mut skinning, parents) = Skinning::plan(mesh, table, *options);
        if skinning.positions.is_empty() {
            return None;
        }
        let root = self.nodes.len() as u32;
        self.nodes.push(super::json::Node {
            name: Cow::Owned(format!("{name}_skeleton")),
            children: Vec::new(),
            translation: None,
            mesh: None,
            skin: None,
            extensions: None,
            extras: None,
        });
        self.scene_nodes.push(root);
        let first_node = root + 1;
        for (index, (&position, parent)) in skinning.positions.iter().zip(&parents).enumerate() {
            let translation =
                parent.map_or(position, |p| position - skinning.positions[p as usize]);
            self.nodes.push(super::json::Node {
                name: Cow::Owned(format!("{name}_joint{index}")),
                children: Vec::new(),
                translation: Some(translation.to_array()),
                mesh: None,
                skin: None,
                extensions: None,
                extras: None,
            });
            match parent {
                Some(p) => self.nodes[(first_node + p) as usize]
                    .children
                    .push(first_node + index as u32),
                None => self.nodes[root as usize]
                    .children
                    .push(first_node + index as u32),
            }
        }
        skinning.nodes = (0..skinning.positions.len() as u32)
            .map(|i| first_node + i)
            .collect();
        let matrices: Vec<f32> = skinning
            .positions
            .iter()
            .flat_map(|&p| inverse_bind(p))
            .collect();
        let view = self.layout.push(
            Payload::Owned(bytemuck::cast_slice(&matrices).to_vec()),
            None,
        );
        let inverse_bind_matrices = self.accessor(view, FLOAT, skinning.positions.len(), "MAT4");
        self.skins.push(Skin {
            inverse_bind_matrices,
            skeleton: root,
            joints: skinning.nodes.clone(),
        });
        self.skinning = Some(skinning);
        Some((self.skins.len() - 1) as u32)
    }

    /// Adds `JOINTS_0` and `WEIGHTS_0` to a part of the mesh being skinned.
    pub(super) fn add_skin_weights(&mut self, part: &'a MeshPart, attributes: &mut Attributes) {
        let (Some(skinning), Some(table)) =
            (&self.skinning, self.mesh.and_then(|m| m.wind.as_ref()))
        else {
            return;
        };
        let wind = &part.buffers.wind;
        if wind.len() != part.buffers.positions.len() || wind.is_empty() {
            return;
        }
        let wide = skinning.positions.len() > 256;
        let mut joints: Vec<u8> = Vec::with_capacity(wind.len() * if wide { 8 } else { 4 });
        let mut weights: Vec<f32> = Vec::with_capacity(wind.len() * 4);
        for vertex in wind {
            let (j, w) = skinning.influence(table, vertex);
            for joint in j {
                if wide {
                    joints.extend((joint as u16).to_le_bytes());
                } else {
                    joints.push(joint as u8);
                }
            }
            weights.extend(w);
        }
        let count = wind.len();
        let view = self.layout.push(Payload::Owned(joints), Some(ARRAY_BUFFER));
        let component = if wide { UNSIGNED_SHORT } else { UNSIGNED_BYTE };
        let accessor = self.accessor(view, component, count, "VEC4");
        attributes.extra.insert("JOINTS_0".to_owned(), accessor);
        let view = self.layout.push(
            Payload::Owned(bytemuck::cast_slice(&weights).to_vec()),
            Some(ARRAY_BUFFER),
        );
        let accessor = self.accessor(view, FLOAT, count, "VEC4");
        attributes.extra.insert("WEIGHTS_0".to_owned(), accessor);
    }
}
