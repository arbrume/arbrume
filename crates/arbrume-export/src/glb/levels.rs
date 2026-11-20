//! Several levels of detail in one file, the wind layout and the optional skin.
//!
//! [`write_glb_levels`] writes one root node, named after the file and carrying its
//! `extras`, whose children are one node per level, `<name>_LOD0`, `<name>_LOD1`, ...,
//! each with its own mesh and the `extras` of its level. Engines that do not understand
//! levels of detail show every child; Unity's importer convention groups `_LODn`
//! children. We do not write `MSFT_lod`: the importers we tried drop it (Blender keeps
//! its nodes but unlinked, three.js loads level 0 only), see `docs/wind.md`.
//!
//! With a skin, every level's mesh node is a root of the scene (glTF ignores the parent
//! transforms of a skinned mesh and the validator warns about it), next to the
//! skeleton of that level, and the root node keeps the `extras` and lists nothing.

use super::instancing::LeafInstancing;
use super::{Builder, ExtraAttribute, GlbError, GlbOptions, check_extra, emit};
use arbrume_core::mesh::PartKind;
use arbrume_core::mesh::TreeMesh;
use serde_json::Value;
use std::borrow::Cow;
use std::io::Write;

/// How the baked wind data of the mesh is written (see `docs/wind.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WindLayout {
    /// Not written.
    None,
    /// `TEXCOORD_1`, `TEXCOORD_2`, `TEXCOORD_3`: kept by every importer tried.
    #[default]
    TexCoord,
    /// `_WIND_X` and `_WIND_DATA`, application-specific attributes.
    Custom,
}

/// The skeleton built from the wind data (see `glb::skin`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SkinOptions {
    /// Deepest hierarchy level that gets joints (0 for the trunk only).
    pub max_level: u8,
    /// Joints per branch.
    pub segments: u32,
}

impl Default for SkinOptions {
    fn default() -> Self {
        Self {
            max_level: 1,
            segments: 4,
        }
    }
}

/// Where the texture images of the materials go.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ImageStorage {
    /// Inside the binary chunk of the file.
    #[default]
    Embed,
    /// Referenced by a relative URI, the name [`image_file_name`](super::image_file_name)
    /// gives the image; the caller writes the images next to the file.
    External,
}

/// What the writer adds beyond the mesh buffers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ExportOptions {
    /// Layout of the wind data, written for meshes that carry it.
    pub wind: WindLayout,
    /// A skeleton with skin weights, for meshes with wind data.
    pub skin: Option<SkinOptions>,
    /// Where texture images go.
    pub images: ImageStorage,
}

/// One level of detail of [`write_glb_levels`].
#[derive(Debug, Clone, PartialEq)]
pub struct GlbLevel<'a> {
    /// The mesh of the level.
    pub mesh: &'a TreeMesh,
    /// `extras` of the node of the level (level index, budget, screen size).
    pub extras: Option<Value>,
    /// Application-specific vertex attributes of its parts.
    pub extra: &'a [ExtraAttribute<'a>],
    /// The foliage as instances: when given, the leaf part of `mesh` is not written and
    /// the templates are instanced under the node of the level instead.
    pub leaves: Option<LeafInstancing<'a>>,
}

/// Writes `levels` as one binary glTF 2.0 file: a root node with one child node per
/// level (see the module documentation). One level gives the same file as
/// [`write_glb_with`](super::write_glb_with) apart from that node structure.
///
/// # Errors
/// Same as [`write_glb_with`](super::write_glb_with) for each level.
pub fn write_glb_levels(
    levels: &[GlbLevel],
    options: &GlbOptions,
    export: &ExportOptions,
    out: impl Write,
) -> Result<(), GlbError> {
    for level in levels {
        check_extra(level.mesh, level.extra)?;
    }
    let mut builder = Builder::new(options, export);
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
    for (index, level) in levels.iter().enumerate() {
        let name = format!("{}_LOD{index}", options.name);
        builder.mesh = Some(level.mesh);
        let skin = builder.begin_skin(level.mesh, &name);
        for (part_index, part) in level.mesh.parts.iter().enumerate() {
            if level.leaves.is_some() && part.kind == PartKind::Leaf {
                continue;
            }
            let own = level.extra.iter().filter(|a| a.part == part_index);
            builder.add_part(part_index, part, own)?;
        }
        let mesh = builder.close_mesh(Some(name.clone()), None);
        builder.skinning = None;
        let instanced = match &level.leaves {
            Some(leaves) => builder.add_instanced_leaves(leaves, &name)?,
            None => Vec::new(),
        };
        let node = builder.nodes.len() as u32;
        builder.nodes.push(super::json::Node {
            name: Cow::Owned(name),
            children: instanced,
            translation: None,
            mesh,
            skin: skin.filter(|_| mesh.is_some()),
            extensions: None,
            extras: level.extras.clone().map(Cow::Owned),
        });
        if skin.is_some() {
            builder.scene_nodes.push(node);
        } else {
            builder.nodes[0].children.push(node);
        }
    }
    let (root, layout) = builder.finish();
    emit(&root, &layout, out)
}
