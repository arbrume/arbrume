//! Serde mirror of the subset of the glTF 2.0 JSON schema that the writer emits.
//!
//! Optional fields are skipped when absent and empty arrays are left out, as the schema
//! requires (`minItems: 1` on every top-level array).

use std::borrow::Cow;

use serde::Serialize;
use serde_json::Value;

/// `componentType` of 8-bit unsigned integers.
pub(super) const UNSIGNED_BYTE: u32 = 5121;
/// `componentType` of 32-bit floats.
pub(super) const FLOAT: u32 = 5126;
/// `componentType` of 16-bit unsigned integers.
pub(super) const UNSIGNED_SHORT: u32 = 5123;
/// `componentType` of 32-bit unsigned integers.
pub(super) const UNSIGNED_INT: u32 = 5125;
/// Buffer view target for vertex attributes.
pub(super) const ARRAY_BUFFER: u32 = 34962;
/// Buffer view target for indices.
pub(super) const ELEMENT_ARRAY_BUFFER: u32 = 34963;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Root<'a> {
    pub asset: Asset<'a>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub extensions_used: Vec<&'static str>,
    pub scene: u32,
    pub scenes: [Scene<'a>; 1],
    pub nodes: Vec<Node<'a>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub meshes: Vec<Mesh<'a>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub skins: Vec<Skin>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub materials: Vec<Material<'a>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub textures: Vec<Texture>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<Image>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub accessors: Vec<Accessor>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub buffer_views: Vec<BufferView>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub buffers: Vec<Buffer>,
}

#[derive(Debug, Serialize)]
pub(super) struct Asset<'a> {
    pub version: &'static str,
    pub generator: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extras: Option<&'a Value>,
}

#[derive(Debug, Serialize)]
pub(super) struct Scene<'a> {
    pub name: &'a str,
    pub nodes: Vec<u32>,
}

#[derive(Debug, Serialize)]
pub(super) struct Node<'a> {
    pub name: Cow<'a, str>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub translation: Option<[f32; 3]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mesh: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skin: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extensions: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extras: Option<Cow<'a, Value>>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Skin {
    pub inverse_bind_matrices: u32,
    pub skeleton: u32,
    pub joints: Vec<u32>,
}

#[derive(Debug, Serialize)]
pub(super) struct Mesh<'a> {
    pub name: Cow<'a, str>,
    pub primitives: Vec<Primitive>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extras: Option<Value>,
}

#[derive(Debug, Serialize)]
pub(super) struct Primitive {
    pub attributes: Attributes,
    pub indices: u32,
    pub material: u32,
    pub extras: PrimitiveExtras,
}

#[derive(Debug, Serialize)]
pub(super) struct Attributes {
    #[serde(rename = "POSITION")]
    pub position: u32,
    #[serde(rename = "NORMAL", skip_serializing_if = "Option::is_none")]
    pub normal: Option<u32>,
    #[serde(rename = "TEXCOORD_0", skip_serializing_if = "Option::is_none")]
    pub texcoord: Option<u32>,
    #[serde(rename = "COLOR_0", skip_serializing_if = "Option::is_none")]
    pub color: Option<u32>,
    /// Application-specific attributes by semantic name, such as `_SEMANTIC`.
    #[serde(flatten)]
    pub extra: std::collections::BTreeMap<String, u32>,
}

#[derive(Debug, Serialize)]
pub(super) struct PrimitiveExtras {
    pub semantic: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Material<'a> {
    pub name: &'a str,
    pub pbr_metallic_roughness: Pbr,
    pub double_sided: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub normal_texture: Option<TextureInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alpha_mode: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alpha_cutoff: Option<f32>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Pbr {
    pub base_color_factor: [f32; 4],
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_color_texture: Option<TextureInfo>,
    pub metallic_factor: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metallic_roughness_texture: Option<TextureInfo>,
    pub roughness_factor: f32,
}

#[derive(Debug, Serialize)]
pub(super) struct TextureInfo {
    pub index: u32,
}

#[derive(Debug, Serialize)]
pub(super) struct Texture {
    pub source: u32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Image {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub buffer_view: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,
    pub mime_type: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Accessor {
    pub buffer_view: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub byte_offset: Option<usize>,
    pub component_type: u32,
    pub count: usize,
    #[serde(rename = "type")]
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<[f32; 3]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<[f32; 3]>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct BufferView {
    pub buffer: u32,
    pub byte_offset: usize,
    pub byte_length: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub byte_stride: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<u32>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Buffer {
    pub byte_length: usize,
}
