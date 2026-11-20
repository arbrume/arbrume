//! Forests: a few plant meshes drawn many times each, with `EXT_mesh_gpu_instancing`.
//!
//! [`write_glb_forest`] writes one root node, named after the file and carrying its
//! `extras`, with one child node per [`ForestGroup`] (a level of detail of the whole
//! forest, say). Under a group, every [`ForestMesh`] with at least one instance becomes
//! one mesh and one node that instances it with the `TRANSLATION`, `ROTATION` and `SCALE`
//! attributes of the extension, plus `_COLOR_0` (one linear RGB float colour per instance)
//! when colours are given. The extension allows application-specific attributes whose
//! names start with an underscore; three.js reads `_COLOR_0` as the instance colour of
//! an `InstancedMesh`, which its materials multiply into the base colour. Each
//! mesh is stored once whatever its instance count. Images shared by several meshes are
//! stored once. Readers without the extension draw each mesh once, at the origin.

use super::instancing::{EXTENSION, InstanceTransform};
use super::json::{FLOAT, Node};
use super::{Builder, ExportOptions, GlbError, GlbOptions, Payload, emit};
use arbrume_core::mesh::TreeMesh;
use serde_json::{Value, json};
use std::borrow::Cow;
use std::io::Write;

/// One mesh of a forest and the places it is drawn at.
#[derive(Debug, Clone, PartialEq)]
pub struct ForestMesh<'a> {
    /// The mesh, in its own frame.
    pub mesh: &'a TreeMesh,
    /// Name of its node and mesh.
    pub name: String,
    /// One transform per instance; their `variant` is not read.
    pub instances: &'a [InstanceTransform],
    /// One linear RGB colour per instance, written as `_COLOR_0`, or `None`.
    pub colors: Option<&'a [[f32; 3]]>,
    /// `extras` of its node.
    pub extras: Option<Value>,
}

/// A named group of instanced meshes: one child node of the root.
#[derive(Debug, Clone, PartialEq)]
pub struct ForestGroup<'a> {
    /// Name of the group node.
    pub name: String,
    /// `extras` of the group node.
    pub extras: Option<Value>,
    /// Its meshes.
    pub meshes: Vec<ForestMesh<'a>>,
}

/// Writes `groups` as one binary glTF 2.0 file of instanced meshes (see the module
/// documentation). The wind data and the image storage of `export` apply; skins are not
/// written.
///
/// # Errors
/// Same as [`write_glb_with`](super::write_glb_with) for each mesh, and
/// [`GlbError::ExtraAttribute`] when the colours of a mesh are not one per instance.
pub fn write_glb_forest(
    groups: &[ForestGroup],
    options: &GlbOptions,
    export: &ExportOptions,
    out: impl Write,
) -> Result<(), GlbError> {
    let export = ExportOptions {
        skin: None,
        ..*export
    };
    let mut builder = Builder::new(options, &export);
    builder.nodes.push(Node {
        name: Cow::Borrowed(&options.name),
        children: Vec::new(),
        translation: None,
        mesh: None,
        skin: None,
        extensions: None,
        extras: options.extras.as_ref().map(Cow::Borrowed),
    });
    builder.scene_nodes.push(0);
    for group in groups {
        let mut children = Vec::new();
        for entry in &group.meshes {
            if let Some(node) = builder.add_forest_mesh(entry)? {
                children.push(node);
            }
        }
        let node = builder.nodes.len() as u32;
        builder.nodes.push(Node {
            name: Cow::Owned(group.name.clone()),
            children,
            translation: None,
            mesh: None,
            skin: None,
            extensions: None,
            extras: group.extras.clone().map(Cow::Owned),
        });
        builder.nodes[0].children.push(node);
    }
    if builder.nodes.len() > 1 && !builder.extensions_used.contains(&EXTENSION) {
        builder.extensions_used.push(EXTENSION);
    }
    let (root, layout) = builder.finish();
    emit(&root, &layout, out)
}

impl<'a> Builder<'a> {
    /// Adds the mesh of `entry` and its instancing node; `None` when it has no instance
    /// or nothing to draw.
    fn add_forest_mesh(&mut self, entry: &ForestMesh<'a>) -> Result<Option<u32>, GlbError> {
        let count = entry.instances.len();
        if count == 0 {
            return Ok(None);
        }
        if let Some(colors) = entry.colors
            && colors.len() != count
        {
            return Err(GlbError::ExtraAttribute {
                name: "_COLOR_0".to_owned(),
                problem: "not one colour per instance",
            });
        }
        self.mesh = Some(entry.mesh);
        for (index, part) in entry.mesh.parts.iter().enumerate() {
            self.add_part(index, part, std::iter::empty())?;
        }
        let Some(mesh) = self.close_mesh(Some(entry.name.clone()), None) else {
            return Ok(None);
        };
        let instances = entry.instances;
        let translation = self.instance_column(
            &instances
                .iter()
                .flat_map(|i| i.translation)
                .collect::<Vec<_>>(),
            count,
            "VEC3",
        );
        let rotation = self.instance_column(
            &instances
                .iter()
                .flat_map(|i| i.rotation)
                .collect::<Vec<_>>(),
            count,
            "VEC4",
        );
        let scale = self.instance_column(
            &instances
                .iter()
                .flat_map(|i| [i.scale; 3])
                .collect::<Vec<_>>(),
            count,
            "VEC3",
        );
        let mut attributes =
            json!({ "TRANSLATION": translation, "ROTATION": rotation, "SCALE": scale });
        if let Some(colors) = entry.colors {
            let color = self.instance_column(colors.as_flattened(), count, "VEC3");
            attributes["_COLOR_0"] = json!(color);
        }
        self.nodes.push(Node {
            name: Cow::Owned(entry.name.clone()),
            children: Vec::new(),
            translation: None,
            mesh: Some(mesh),
            skin: None,
            extensions: Some(json!({ EXTENSION: { "attributes": attributes } })),
            extras: entry.extras.clone().map(Cow::Owned),
        });
        Ok(Some((self.nodes.len() - 1) as u32))
    }

    /// One float accessor of `count` elements of type `kind` over `values`.
    fn instance_column(&mut self, values: &[f32], count: usize, kind: &'static str) -> u32 {
        let bytes = bytemuck::cast_slice(values).to_vec();
        let view = self.layout.push(Payload::Owned(bytes), None);
        self.accessor(view, FLOAT, count, kind)
    }
}
