//! Binary glTF 2.0 (`.glb`) writer.
//!
//! The file holds one scene with one node and one mesh; every non-empty [`MeshPart`] becomes
//! one primitive with its own material. Each vertex attribute and index list gets its own
//! tightly packed buffer view inside a single binary buffer, so the big arrays are written
//! straight from the mesh with no intermediate copy. Only `u32` indices that fit in `u16`
//! go through a small stack buffer for narrowing.
//!
//! Parts without positions or without indices are skipped: glTF forbids zero-length
//! accessors and buffer views. A mesh with no drawable part produces a valid file with a
//! single empty node.

mod instancing;
mod json;
mod levels;
mod skin;
mod tangent;
mod wind;

pub use instancing::{InstanceTransform, LeafInstancing};
pub use levels::{ExportOptions, GlbLevel, SkinOptions, WindLayout, write_glb_levels};

use std::borrow::Cow;
use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::Path;

use arbrume_core::mesh::{Material, MeshPart, PartKind, TreeMesh};
use serde_json::Value;

use json::{
    ARRAY_BUFFER, Accessor, Attributes, Buffer, BufferView, ELEMENT_ARRAY_BUFFER, FLOAT, Image,
    Pbr, Primitive, PrimitiveExtras, Root, Texture, TextureInfo, UNSIGNED_BYTE, UNSIGNED_INT,
    UNSIGNED_SHORT,
};

#[cfg(target_endian = "big")]
compile_error!("the glb writer casts native buffers to bytes and needs a little-endian target");

const GLB_MAGIC: u32 = 0x4654_6C67;
const GLB_VERSION: u32 = 2;
const CHUNK_JSON: u32 = 0x4E4F_534A;
const CHUNK_BIN: u32 = 0x004E_4942;
const HEADER_LEN: usize = 12;
const CHUNK_HEADER_LEN: usize = 8;
/// Index lists of parts with at most this many vertices are stored as `u16`; 65535 itself is
/// the primitive restart value and may not appear as an index.
const MAX_U16_VERTICES: usize = 65535;
/// Number of indices narrowed per stack buffer when writing `u16` index lists.
const NARROW_CHUNK: usize = 4096;

/// Options of [`write_glb`].
#[derive(Debug, Clone, PartialEq)]
pub struct GlbOptions {
    /// Name of the root node, the scene and the mesh.
    pub name: String,
    /// `asset.generator` string.
    pub generator: String,
    /// Generator metadata (species, seed, version, parameters) written to both the root node
    /// `extras` and `asset.extras`.
    pub extras: Option<Value>,
}

impl Default for GlbOptions {
    fn default() -> Self {
        Self {
            name: "tree".to_owned(),
            generator: concat!("Arbrume ", env!("CARGO_PKG_VERSION")).to_owned(),
            extras: None,
        }
    }
}

/// Components of an [`ExtraAttribute`], one or more per vertex.
///
/// glTF allows `u32` components for indices only, and asks every vertex element to be a
/// multiple of 4 bytes; pick the component type and count accordingly (for example `f32`,
/// or four `u8`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AttributeData<'a> {
    /// 32-bit floats.
    F32(&'a [f32]),
    /// 16-bit unsigned integers.
    U16(&'a [u16]),
    /// 8-bit unsigned integers.
    U8(&'a [u8]),
}

impl AttributeData<'_> {
    /// Number of components.
    #[must_use]
    pub const fn len(&self) -> usize {
        match self {
            Self::F32(values) => values.len(),
            Self::U16(values) => values.len(),
            Self::U8(values) => values.len(),
        }
    }

    /// `true` when there is no component.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// An application-specific vertex attribute added to one part by [`write_glb_with`].
///
/// `name` is the attribute semantic and must start with an underscore (glTF 2.0,
/// section 3.7.2.1). `data` holds `components` values per vertex of the part, in vertex
/// order.
#[derive(Debug, Clone, PartialEq)]
pub struct ExtraAttribute<'a> {
    /// Index of the part in `TreeMesh::parts`.
    pub part: usize,
    /// Attribute semantic, such as `_SEMANTIC`.
    pub name: String,
    /// Values per vertex: 1, 2, 3 or 4.
    pub components: u8,
    /// The values.
    pub data: AttributeData<'a>,
}

/// Errors of the glb writer.
#[derive(Debug, thiserror::Error)]
pub enum GlbError {
    /// Writing to the output failed.
    #[error("i/o error: {0}")]
    Io(#[from] io::Error),
    /// Encoding the JSON chunk failed.
    #[error("json encoding failed: {0}")]
    Json(#[from] serde_json::Error),
    /// A per-vertex attribute is neither empty nor one entry per position.
    #[error("part {part}: {attribute} has {len} entries for {vertices} positions")]
    AttributeLength {
        /// Index of the part in the mesh.
        part: usize,
        /// Attribute name.
        attribute: &'static str,
        /// Number of entries found.
        len: usize,
        /// Number of positions.
        vertices: usize,
    },
    /// The index list is not a whole number of triangles.
    #[error("part {part}: {count} indices is not a multiple of 3")]
    IndexCount {
        /// Index of the part in the mesh.
        part: usize,
        /// Number of indices.
        count: usize,
    },
    /// An index refers past the last vertex.
    #[error("part {part}: index {index} out of range for {vertices} vertices")]
    IndexRange {
        /// Index of the part in the mesh.
        part: usize,
        /// The offending index.
        index: u32,
        /// Number of vertices.
        vertices: usize,
    },
    /// A position has a NaN or infinite coordinate.
    #[error("part {part}: position {vertex} is not finite")]
    NonFinitePosition {
        /// Index of the part in the mesh.
        part: usize,
        /// Index of the vertex.
        vertex: usize,
    },
    /// The base colour texture bytes are neither PNG nor JPEG.
    #[error("part {part}: base colour texture is neither PNG nor JPEG")]
    ImageFormat {
        /// Index of the part in the mesh.
        part: usize,
    },
    /// The file would exceed the 4 GiB limit of the glb container.
    #[error("glb would be {0} bytes, over the 4 GiB format limit")]
    TooLarge(usize),
    /// An extra attribute has a bad name, part, component count or length.
    #[error("extra attribute `{name}`: {problem}")]
    ExtraAttribute {
        /// Attribute semantic.
        name: String,
        /// What is wrong.
        problem: &'static str,
    },
}

/// Writes `mesh` as a binary glTF 2.0 file to `out`.
///
/// `out` receives a few large writes per buffer; wrap unbuffered sinks in a [`BufWriter`].
///
/// # Errors
/// Returns [`GlbError`] when the mesh is malformed (see the variants) or the output fails.
pub fn write_glb(mesh: &TreeMesh, options: &GlbOptions, out: impl Write) -> Result<(), GlbError> {
    write_glb_with(mesh, options, &[], out)
}

/// Same as [`write_glb`], with application-specific vertex attributes added to the
/// primitives of their parts. Attributes of an empty part, which has no primitive, are
/// dropped.
///
/// # Errors
/// Same as [`write_glb`], and [`GlbError::ExtraAttribute`] for an attribute whose name
/// does not start with an underscore or is used twice on a part, whose part does not
/// exist, or whose length is not `components` per vertex.
pub fn write_glb_with(
    mesh: &TreeMesh,
    options: &GlbOptions,
    extra: &[ExtraAttribute],
    out: impl Write,
) -> Result<(), GlbError> {
    check_extra(mesh, extra)?;
    let export = ExportOptions::default();
    let mut builder = Builder::new(options, &export);
    builder.mesh = Some(mesh);
    for (index, part) in mesh.parts.iter().enumerate() {
        let own = extra.iter().filter(|attribute| attribute.part == index);
        builder.add_part(index, part, own)?;
    }
    builder.close_mesh(None, None);
    let (root, layout) = builder.finish();
    emit(&root, &layout, out)
}

/// Writes the glb container of `root` and `layout` to `out`.
fn emit(root: &Root, layout: &Layout, mut out: impl Write) -> Result<(), GlbError> {
    let mut json = serde_json::to_vec(root)?;
    json.resize(pad4(json.len()), b' ');
    let bin_chunk = if layout.size == 0 {
        0
    } else {
        CHUNK_HEADER_LEN + layout.size
    };
    let total = HEADER_LEN + CHUNK_HEADER_LEN + json.len() + bin_chunk;
    let total32 = u32::try_from(total).map_err(|_| GlbError::TooLarge(total))?;

    out.write_all(&header(&[GLB_MAGIC, GLB_VERSION, total32]))?;
    out.write_all(&header(&[json.len() as u32, CHUNK_JSON]))?;
    out.write_all(&json)?;
    if layout.size > 0 {
        out.write_all(&header(&[layout.size as u32, CHUNK_BIN]))?;
        layout.write(&mut out)?;
    }
    out.flush()?;
    Ok(())
}

/// Writes `mesh` to the file at `path` through a buffered writer, replacing it if present.
///
/// # Errors
/// Same as [`write_glb`], plus file creation failures.
pub fn write_glb_file(
    mesh: &TreeMesh,
    options: &GlbOptions,
    path: impl AsRef<Path>,
) -> Result<(), GlbError> {
    let file = File::create(path)?;
    write_glb(mesh, options, BufWriter::with_capacity(1 << 16, file))
}

/// Rounds `len` up to the next multiple of 4.
const fn pad4(len: usize) -> usize {
    (len + 3) & !3
}

/// Encodes little-endian `u32` words of a glb or chunk header.
fn header<const N: usize>(words: &[u32; N]) -> Vec<u8> {
    words.iter().flat_map(|w| w.to_le_bytes()).collect()
}

/// Returns the MIME type of PNG or JPEG bytes from their magic numbers.
fn image_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else {
        None
    }
}

/// Bytes of one buffer view, borrowed from the mesh.
#[derive(Debug)]
enum Payload<'a> {
    /// Written as is.
    Bytes(&'a [u8]),
    /// `u32` indices narrowed to `u16` while writing.
    Narrow(&'a [u32]),
    /// Bytes computed by the writer.
    Owned(Vec<u8>),
}

impl Payload<'_> {
    fn len(&self) -> usize {
        match self {
            Payload::Bytes(bytes) => bytes.len(),
            Payload::Narrow(indices) => indices.len() * 2,
            Payload::Owned(bytes) => bytes.len(),
        }
    }
}

/// Placement of every buffer view in the binary chunk.
#[derive(Debug, Default)]
struct Layout<'a> {
    payloads: Vec<Payload<'a>>,
    views: Vec<BufferView>,
    size: usize,
}

impl<'a> Layout<'a> {
    /// Appends a view at the next 4-byte boundary and returns its index.
    fn push(&mut self, payload: Payload<'a>, target: Option<u32>) -> u32 {
        let len = payload.len();
        self.views.push(BufferView {
            buffer: 0,
            byte_offset: self.size,
            byte_length: len,
            byte_stride: None,
            target,
        });
        self.payloads.push(payload);
        self.size += pad4(len);
        (self.views.len() - 1) as u32
    }

    /// Writes all views in order, each followed by its zero padding.
    fn write(&self, out: &mut impl Write) -> io::Result<()> {
        for payload in &self.payloads {
            match payload {
                Payload::Bytes(bytes) => out.write_all(bytes)?,
                Payload::Narrow(indices) => write_narrowed(indices, out)?,
                Payload::Owned(bytes) => out.write_all(bytes)?,
            }
            out.write_all(&[0; 3][..pad4(payload.len()) - payload.len()])?;
        }
        Ok(())
    }
}

/// Writes `indices` as little-endian `u16`, a stack-sized chunk at a time.
fn write_narrowed(indices: &[u32], out: &mut impl Write) -> io::Result<()> {
    let mut narrow = [0u16; NARROW_CHUNK];
    for chunk in indices.chunks(NARROW_CHUNK) {
        for (dst, &src) in narrow.iter_mut().zip(chunk) {
            *dst = src as u16;
        }
        out.write_all(bytemuck::cast_slice(&narrow[..chunk.len()]))?;
    }
    Ok(())
}

/// Accumulates the JSON objects and the buffer layout while walking the parts.
#[derive(Debug)]
struct Builder<'a> {
    options: &'a GlbOptions,
    export: &'a ExportOptions,
    /// The mesh whose parts are being added.
    mesh: Option<&'a TreeMesh>,
    primitives: Vec<Primitive>,
    meshes: Vec<json::Mesh<'a>>,
    /// Nodes beyond the single root node of a one-mesh file.
    nodes: Vec<json::Node<'a>>,
    /// Roots of the scene when `nodes` is used.
    scene_nodes: Vec<u32>,
    skins: Vec<json::Skin>,
    /// Extensions the file uses.
    extensions_used: Vec<&'static str>,
    /// Joints of the skin of the mesh being added, when skinned.
    skinning: Option<skin::Skinning>,
    materials: Vec<json::Material<'a>>,
    textures: Vec<Texture>,
    images: Vec<Image>,
    accessors: Vec<Accessor>,
    layout: Layout<'a>,
}

impl<'a> Builder<'a> {
    fn new(options: &'a GlbOptions, export: &'a ExportOptions) -> Self {
        Self {
            options,
            export,
            mesh: None,
            primitives: Vec::new(),
            meshes: Vec::new(),
            nodes: Vec::new(),
            scene_nodes: Vec::new(),
            skins: Vec::new(),
            extensions_used: Vec::new(),
            skinning: None,
            materials: Vec::new(),
            textures: Vec::new(),
            images: Vec::new(),
            accessors: Vec::new(),
            layout: Layout::default(),
        }
    }

    /// Adds one part as a primitive with its material and its extra attributes; empty
    /// parts are skipped.
    fn add_part<'e>(
        &mut self,
        index: usize,
        part: &'a MeshPart,
        extra: impl Iterator<Item = &'e ExtraAttribute<'a>>,
    ) -> Result<(), GlbError>
    where
        'a: 'e,
    {
        let buffers = &part.buffers;
        check_part(index, part)?;
        if buffers.positions.is_empty() || buffers.indices.is_empty() {
            return Ok(());
        }
        let (min, max) = bounds(index, &buffers.positions)?;
        let count = buffers.positions.len();
        let position =
            self.vertex_accessor(bytemuck::cast_slice(&buffers.positions), count, "VEC3");
        self.accessors[position as usize].min = Some(min);
        self.accessors[position as usize].max = Some(max);
        let mut attributes = Attributes {
            position,
            normal: self.optional_accessor(bytemuck::cast_slice(&buffers.normals), count, "VEC3"),
            texcoord: self.optional_accessor(bytemuck::cast_slice(&buffers.uvs), count, "VEC2"),
            color: self.optional_accessor(bytemuck::cast_slice(&buffers.colors), count, "VEC4"),
            extra: std::collections::BTreeMap::new(),
        };
        for attribute in extra {
            let accessor = self.extra_accessor(attribute, count);
            attributes.extra.insert(attribute.name.clone(), accessor);
        }
        self.add_wind(part, &mut attributes);
        self.add_skin_weights(part, &mut attributes);
        let indices = self.index_accessor(&buffers.indices, count);
        let textured = attributes.texcoord.is_some();
        let texture = match &part.material.base_color_texture {
            Some(bytes) if textured => Some(self.add_texture(index, bytes)?),
            _ => None,
        };
        let normal = match &part.material.normal_texture {
            Some(bytes) if textured && attributes.normal.is_some() => {
                let tangents: Vec<f32> = tangent::tangents(buffers).into_iter().flatten().collect();
                let tangent_bytes = bytemuck::cast_slice(&tangents).to_vec();
                let view = self
                    .layout
                    .push(Payload::Owned(tangent_bytes), Some(ARRAY_BUFFER));
                let accessor = self.accessor(view, FLOAT, count, "VEC4");
                attributes.extra.insert("TANGENT".to_owned(), accessor);
                Some(self.add_texture(index, bytes)?)
            }
            _ => None,
        };
        self.materials
            .push(material(&part.material, texture, normal));
        self.primitives.push(Primitive {
            attributes,
            indices,
            material: (self.materials.len() - 1) as u32,
            extras: PrimitiveExtras {
                semantic: semantic(part.kind),
            },
        });
        Ok(())
    }

    fn vertex_accessor(&mut self, bytes: &'a [u8], count: usize, kind: &'static str) -> u32 {
        let view = self.layout.push(Payload::Bytes(bytes), Some(ARRAY_BUFFER));
        self.accessor(view, FLOAT, count, kind)
    }

    fn extra_accessor(&mut self, attribute: &ExtraAttribute<'a>, count: usize) -> u32 {
        let (bytes, component) = match attribute.data {
            AttributeData::F32(values) => (bytemuck::cast_slice(values), FLOAT),
            AttributeData::U16(values) => (bytemuck::cast_slice(values), UNSIGNED_SHORT),
            AttributeData::U8(values) => (values, UNSIGNED_BYTE),
        };
        let kind = ["SCALAR", "VEC2", "VEC3", "VEC4"][usize::from(attribute.components) - 1];
        let view = self.layout.push(Payload::Bytes(bytes), Some(ARRAY_BUFFER));
        self.accessor(view, component, count, kind)
    }

    fn optional_accessor(
        &mut self,
        bytes: &'a [u8],
        count: usize,
        kind: &'static str,
    ) -> Option<u32> {
        (!bytes.is_empty()).then(|| self.vertex_accessor(bytes, count, kind))
    }

    fn index_accessor(&mut self, indices: &'a [u32], vertices: usize) -> u32 {
        let (payload, component) = if vertices <= MAX_U16_VERTICES {
            (Payload::Narrow(indices), UNSIGNED_SHORT)
        } else {
            (Payload::Bytes(bytemuck::cast_slice(indices)), UNSIGNED_INT)
        };
        let view = self.layout.push(payload, Some(ELEMENT_ARRAY_BUFFER));
        self.accessor(view, component, indices.len(), "SCALAR")
    }

    fn accessor(
        &mut self,
        buffer_view: u32,
        component_type: u32,
        count: usize,
        kind: &'static str,
    ) -> u32 {
        self.accessors.push(Accessor {
            buffer_view,
            byte_offset: None,
            component_type,
            count,
            kind,
            min: None,
            max: None,
        });
        (self.accessors.len() - 1) as u32
    }

    fn add_texture(&mut self, part: usize, bytes: &'a [u8]) -> Result<u32, GlbError> {
        let mime_type = image_mime(bytes).ok_or(GlbError::ImageFormat { part })?;
        let buffer_view = self.layout.push(Payload::Bytes(bytes), None);
        self.images.push(Image {
            buffer_view,
            mime_type,
        });
        self.textures.push(Texture {
            source: (self.images.len() - 1) as u32,
        });
        Ok((self.textures.len() - 1) as u32)
    }

    /// Moves the primitives added so far into a mesh named `name` (the file name when
    /// `None`) with `extras`, and returns its index; `None` when there is no primitive.
    fn close_mesh(&mut self, name: Option<String>, extras: Option<Value>) -> Option<u32> {
        if self.primitives.is_empty() {
            return None;
        }
        let name = name.map_or(Cow::Borrowed(self.options.name.as_str()), Cow::Owned);
        let extras = extras.or_else(|| self.wind_extras());
        self.meshes.push(json::Mesh {
            name,
            primitives: std::mem::take(&mut self.primitives),
            extras,
        });
        Some((self.meshes.len() - 1) as u32)
    }

    /// Assembles the JSON root and hands back the binary layout. Without nodes of its
    /// own, the file has one root node holding the first mesh.
    fn finish(mut self) -> (Root<'a>, Layout<'a>) {
        let options = self.options;
        let has_mesh = !self.meshes.is_empty();
        if self.nodes.is_empty() {
            self.nodes.push(json::Node {
                name: Cow::Borrowed(&options.name),
                children: Vec::new(),
                translation: None,
                mesh: has_mesh.then_some(0),
                skin: None,
                extensions: None,
                extras: options.extras.as_ref().map(Cow::Borrowed),
            });
            self.scene_nodes = vec![0];
        }
        let meshes = std::mem::take(&mut self.meshes);
        let buffers = if self.layout.size > 0 {
            vec![Buffer {
                byte_length: self.layout.size,
            }]
        } else {
            Vec::new()
        };
        let root = Root {
            asset: json::Asset {
                version: "2.0",
                generator: &options.generator,
                extras: options.extras.as_ref(),
            },
            extensions_used: std::mem::take(&mut self.extensions_used),
            scene: 0,
            scenes: [json::Scene {
                name: &options.name,
                nodes: std::mem::take(&mut self.scene_nodes),
            }],
            nodes: std::mem::take(&mut self.nodes),
            meshes,
            skins: std::mem::take(&mut self.skins),
            materials: self.materials,
            textures: self.textures,
            images: self.images,
            accessors: self.accessors,
            buffer_views: std::mem::take(&mut self.layout.views),
            buffers,
        };
        (root, self.layout)
    }
}

/// Checks the name, part, component count and length of every extra attribute.
fn check_extra(mesh: &TreeMesh, extra: &[ExtraAttribute]) -> Result<(), GlbError> {
    for (position, attribute) in extra.iter().enumerate() {
        let fail = |problem| {
            Err(GlbError::ExtraAttribute {
                name: attribute.name.clone(),
                problem,
            })
        };
        let Some(part) = mesh.parts.get(attribute.part) else {
            return fail("no such part");
        };
        if !attribute.name.starts_with('_') {
            return fail("an application-specific semantic must start with `_`");
        }
        if !(1..=4).contains(&attribute.components) {
            return fail("components must be 1 to 4");
        }
        let vertices = part.buffers.positions.len();
        if attribute.data.len() != vertices * usize::from(attribute.components) {
            return fail("length is not `components` values per vertex");
        }
        let twice = extra[..position]
            .iter()
            .any(|other| other.part == attribute.part && other.name == attribute.name);
        if twice {
            return fail("given twice for the same part");
        }
    }
    Ok(())
}

/// Checks attribute lengths, triangle count and index range of one part.
fn check_part(part: usize, mesh_part: &MeshPart) -> Result<(), GlbError> {
    let buffers = &mesh_part.buffers;
    let vertices = buffers.positions.len();
    let lengths = [
        ("normals", buffers.normals.len()),
        ("uvs", buffers.uvs.len()),
        ("colors", buffers.colors.len()),
    ];
    for (attribute, len) in lengths {
        if len != 0 && len != vertices {
            return Err(GlbError::AttributeLength {
                part,
                attribute,
                len,
                vertices,
            });
        }
    }
    let count = buffers.indices.len();
    if !count.is_multiple_of(3) {
        return Err(GlbError::IndexCount { part, count });
    }
    match buffers.indices.iter().copied().max() {
        Some(index) if index as usize >= vertices => Err(GlbError::IndexRange {
            part,
            index,
            vertices,
        }),
        _ => Ok(()),
    }
}

/// Component-wise bounds of `positions`, which must be non-empty and finite.
fn bounds(part: usize, positions: &[[f32; 3]]) -> Result<([f32; 3], [f32; 3]), GlbError> {
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for (vertex, p) in positions.iter().enumerate() {
        if !p.iter().all(|c| c.is_finite()) {
            return Err(GlbError::NonFinitePosition { part, vertex });
        }
        for axis in 0..3 {
            min[axis] = min[axis].min(p[axis]);
            max[axis] = max[axis].max(p[axis]);
        }
    }
    Ok((min, max))
}

/// glTF material of one part; `texture` and `normal` are the base colour and normal
/// texture indices when embedded.
fn material(material: &Material, texture: Option<u32>, normal: Option<u32>) -> json::Material<'_> {
    json::Material {
        name: &material.name,
        pbr_metallic_roughness: Pbr {
            base_color_factor: material.base_color,
            base_color_texture: texture.map(|index| TextureInfo { index }),
            metallic_factor: 0.0,
            roughness_factor: material.roughness,
        },
        double_sided: material.double_sided,
        normal_texture: normal.map(|index| TextureInfo { index }),
        alpha_mode: material.alpha_cutoff.map(|_| "MASK"),
        alpha_cutoff: material.alpha_cutoff,
    }
}

/// Semantic label of a part kind, written to the primitive `extras`.
const fn semantic(kind: PartKind) -> &'static str {
    match kind {
        PartKind::Bark => "bark",
        PartKind::Leaf => "leaf",
        PartKind::Billboard => "billboard",
    }
}
