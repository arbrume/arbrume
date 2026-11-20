//! Several plants in one file, each at its place.
//!
//! [`write_glb_stand`] writes one root node, named after the file and carrying its
//! `extras`, whose children are one node per plant, translated to where the plant
//! stands, each with its own mesh and the `extras` of its plant. Images shared by
//! several plants are stored once.

use super::{Builder, ExportOptions, GlbError, GlbOptions, emit};
use arbrume_core::mesh::TreeMesh;
use serde_json::Value;
use std::borrow::Cow;
use std::io::Write;

/// One plant of [`write_glb_stand`].
#[derive(Debug, Clone, PartialEq)]
pub struct GlbPlant<'a> {
    /// The mesh of the plant, in its own frame.
    pub mesh: &'a TreeMesh,
    /// Name of its node and mesh.
    pub name: String,
    /// Where the plant stands, in meters.
    pub translation: [f32; 3],
    /// `extras` of its node.
    pub extras: Option<Value>,
}

/// Writes `plants` as one binary glTF 2.0 file: a root node with one translated child
/// node per plant (see the module documentation). The wind data and the image storage
/// of `export` apply; skins are not written.
///
/// # Errors
/// Same as [`write_glb_with`](super::write_glb_with) for each plant.
pub fn write_glb_stand(
    plants: &[GlbPlant],
    options: &GlbOptions,
    export: &ExportOptions,
    out: impl Write,
) -> Result<(), GlbError> {
    let export = ExportOptions {
        skin: None,
        ..*export
    };
    let mut builder = Builder::new(options, &export);
    builder.nodes.push(super::json::Node {
        name: Cow::Borrowed(&options.name),
        children: Vec::new(),
        translation: None,
        mesh: None,
        skin: None,
        extensions: None,
        extras: options.extras.as_ref().map(Cow::Borrowed),
    });
    builder.scene_nodes.push(0);
    for plant in plants {
        builder.mesh = Some(plant.mesh);
        for (index, part) in plant.mesh.parts.iter().enumerate() {
            builder.add_part(index, part, std::iter::empty())?;
        }
        let mesh = builder.close_mesh(Some(plant.name.clone()), None);
        let node = builder.nodes.len() as u32;
        let moved = plant.translation.iter().any(|value| *value != 0.0);
        builder.nodes.push(super::json::Node {
            name: Cow::Owned(plant.name.clone()),
            children: Vec::new(),
            translation: moved.then_some(plant.translation),
            mesh,
            skin: None,
            extensions: None,
            extras: plant.extras.clone().map(Cow::Owned),
        });
        builder.nodes[0].children.push(node);
    }
    let (root, layout) = builder.finish();
    emit(&root, &layout, out)
}
