//! Wind attributes and the branch table of a mesh in the glb.
//!
//! The per-vertex wind data ([`arbrume_core::mesh::WindVertex`], six floats) is stored
//! once, as one interleaved buffer view of stride 24 bytes, and exposed through several
//! accessors into it, so nothing is copied. With [`WindLayout::TexCoord`] they are
//! `TEXCOORD_1` = `(x0, x1)`, `TEXCOORD_2` = `(x2, x3)` and `TEXCOORD_3` = `(branch,
//! flutter)`, the channels every importer we tried keeps (see `docs/wind.md`). With
//! [`WindLayout::Custom`] they are `_WIND_X` = `(x0, x1, x2, x3)` and `_WIND_DATA` =
//! `(branch, flutter)`, application-specific attributes that keep their meaning in their
//! name but that some importers drop. A part without `TEXCOORD_0` gets the custom names
//! in either layout, since texture coordinate sets must be numbered without gaps.
//!
//! The branch table goes to the `extras` of the mesh, under `arbrume_wind`, as columns:
//! one array per field of [`arbrume_core::mesh::WindBranch`], vectors flattened.

use super::json::{ARRAY_BUFFER, Attributes, FLOAT};
use super::{Builder, Payload, WindLayout};
use arbrume_core::mesh::{MeshPart, WindTable};
use serde_json::{Value, json};

/// Bytes of one interleaved wind vertex.
const STRIDE: usize = 24;

impl<'a> Builder<'a> {
    /// Adds the wind attributes of `part`, when it has wind data and the layout writes
    /// it.
    pub(super) fn add_wind(&mut self, part: &'a MeshPart, attributes: &mut Attributes) {
        let wind = &part.buffers.wind;
        if wind.is_empty() || self.export.wind == WindLayout::None {
            return;
        }
        let count = wind.len();
        let bytes: &'a [u8] = bytemuck::cast_slice(wind.as_flattened());
        let view = self.layout.push(Payload::Bytes(bytes), Some(ARRAY_BUFFER));
        self.layout.views[view as usize].byte_stride = Some(STRIDE);
        let texcoord = self.export.wind == WindLayout::TexCoord && attributes.texcoord.is_some();
        let mut add = |name: &str, offset: usize, kind: &'static str| {
            let accessor = self.accessor(view, FLOAT, count, kind);
            self.accessors[accessor as usize].byte_offset = (offset > 0).then_some(offset);
            attributes.extra.insert(name.to_owned(), accessor);
        };
        if texcoord {
            add("TEXCOORD_1", 0, "VEC2");
            add("TEXCOORD_2", 8, "VEC2");
            add("TEXCOORD_3", 16, "VEC2");
        } else {
            add("_WIND_X", 0, "VEC4");
            add("_WIND_DATA", 16, "VEC2");
        }
    }

    /// The `extras` of the mesh being closed: its branch table, when wind data is
    /// written.
    pub(super) fn wind_extras(&self) -> Option<Value> {
        let layout = match self.export.wind {
            WindLayout::None => return None,
            WindLayout::TexCoord => "texcoord",
            WindLayout::Custom => "custom",
        };
        let mesh = self.mesh?;
        let table = mesh.wind.as_ref()?;
        mesh.parts
            .iter()
            .any(|part| !part.buffers.wind.is_empty())
            .then(|| json!({ "arbrume_wind": { "layout": layout, "branches": columns(table) } }))
    }
}

/// The branch table as one array per field.
fn columns(table: &WindTable) -> Value {
    let branches = &table.branches;
    let flat = |pick: &dyn Fn(&arbrume_core::mesh::WindBranch) -> Vec<f32>| -> Vec<f32> {
        branches.iter().flat_map(pick).collect()
    };
    json!({
        "count": branches.len(),
        "chain": branches.iter().map(|b| b.chain).collect::<Vec<_>>(),
        "parent": branches.iter().map(|b| b.parent.map_or(-1, i64::from)).collect::<Vec<_>>(),
        "level": branches.iter().map(|b| b.level).collect::<Vec<_>>(),
        "lineage": branches.iter().flat_map(|b| b.lineage).collect::<Vec<_>>(),
        "lineage_x": flat(&|b| b.lineage_x.to_vec()),
        "pivot": flat(&|b| b.pivot.to_vec()),
        "tangent": flat(&|b| b.tangent.to_vec()),
        "right": flat(&|b| b.right.to_vec()),
        "length": flat(&|b| vec![b.length]),
        "root_radius": flat(&|b| vec![b.root_radius]),
        "tip_radius": flat(&|b| vec![b.tip_radius]),
        "taper": flat(&|b| vec![b.taper]),
        "mass": flat(&|b| vec![b.mass]),
        "shape": flat(&|b| b.shape.to_vec()),
    })
}
