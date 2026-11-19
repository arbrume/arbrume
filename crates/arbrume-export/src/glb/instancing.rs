//! Leaves and twigs as GPU instances (`EXT_mesh_gpu_instancing`).
//!
//! Instead of the baked leaf part, a level can carry the few templates of its foliage and
//! one transform per leaf. Each template becomes a mesh, and one node per template
//! instances it with the `TRANSLATION`, `ROTATION` and `SCALE` attributes of the
//! extension. The colour of each leaf, its wind data and the flutter of its vertices are
//! not carried: readers keep only the transform (see `docs/wind.md`). Readers without
//! the extension draw one leaf per template at the origin of the node.

use super::json::{FLOAT, Node};
use super::{Builder, Payload};
use arbrume_core::mesh::MeshPart;
use serde_json::json;
use std::borrow::Cow;

/// Name of the extension.
pub(super) const EXTENSION: &str = "EXT_mesh_gpu_instancing";

/// The transform of one leaf: a template vertex `p` lands at
/// `translation + rotation * (scale * p)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InstanceTransform {
    /// Where the template origin goes.
    pub translation: [f32; 3],
    /// Unit quaternion `[x, y, z, w]`.
    pub rotation: [f32; 4],
    /// Uniform scale.
    pub scale: f32,
    /// Index of the template in [`LeafInstancing::templates`].
    pub variant: u32,
}

/// The foliage of a level as instances of templates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LeafInstancing<'a> {
    /// One part per template, in its own frame, with its material.
    pub templates: &'a [MeshPart],
    /// One transform per leaf.
    pub instances: &'a [InstanceTransform],
}

impl<'a> Builder<'a> {
    /// Adds the templates of `leaves` as meshes and one instancing node per template
    /// with instances; returns the node indices.
    pub(super) fn add_instanced_leaves(
        &mut self,
        leaves: &LeafInstancing<'a>,
        name: &str,
    ) -> Result<Vec<u32>, super::GlbError> {
        let mut nodes = Vec::new();
        for (variant, template) in leaves.templates.iter().enumerate() {
            let own: Vec<&InstanceTransform> = leaves
                .instances
                .iter()
                .filter(|i| i.variant as usize == variant)
                .collect();
            if own.is_empty() {
                continue;
            }
            self.add_part(variant, template, std::iter::empty())?;
            let Some(mesh) = self.close_mesh(Some(format!("{name}_leaf{variant}")), None) else {
                continue;
            };
            let mut column = |values: Vec<f32>, kind: &'static str| {
                let bytes = bytemuck::cast_slice(&values).to_vec();
                let view = self.layout.push(Payload::Owned(bytes), None);
                self.accessor(view, FLOAT, own.len(), kind)
            };
            let translation = column(own.iter().flat_map(|i| i.translation).collect(), "VEC3");
            let rotation = column(own.iter().flat_map(|i| i.rotation).collect(), "VEC4");
            let scale = column(own.iter().flat_map(|i| [i.scale; 3]).collect(), "VEC3");
            let attributes =
                json!({ "TRANSLATION": translation, "ROTATION": rotation, "SCALE": scale });
            self.nodes.push(Node {
                name: Cow::Owned(format!("{name}_leaves{variant}")),
                children: Vec::new(),
                translation: None,
                mesh: Some(mesh),
                skin: None,
                extensions: Some(json!({ EXTENSION: { "attributes": attributes } })),
                extras: None,
            });
            nodes.push((self.nodes.len() - 1) as u32);
        }
        if !nodes.is_empty() && !self.extensions_used.contains(&EXTENSION) {
            self.extensions_used.push(EXTENSION);
        }
        Ok(nodes)
    }
}
