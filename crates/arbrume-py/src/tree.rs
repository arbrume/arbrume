//! The Python classes of a generated tree: `Tree`, `MeshPart`, `Skeleton`, `Organs`.
//!
//! Every class holds an `Arc` to the same [`TreeData`]; array properties return
//! `numpy.asarray` of a [`Buffer`], a read-only view of the Rust memory (see
//! [`crate::buffer`]). Each access builds a new array object over the same memory.

use std::path::PathBuf;
use std::sync::Arc;

use arbrume_export::{GlbOptions, write_glb};
use arbrume_gen::{GENERATOR, metadata};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict};

use crate::buffer::Buffer;
use crate::convert::{glb_error, json_to_py, py_to_json};
use crate::data::{Field, GraphField, PartField, TreeData};

/// `numpy.asarray` of a buffer over `field` of `owner`.
fn array<'py>(py: Python<'py>, owner: &Arc<TreeData>, field: Field) -> PyResult<Bound<'py, PyAny>> {
    let buffer = Bound::new(py, Buffer::new(Arc::clone(owner), field))?;
    py.import("numpy")?.call_method1("asarray", (buffer,))
}

/// A generated tree: mesh parts, skeleton, organ sites, statistics and metadata.
#[pyclass(frozen, module = "arbrume._arbrume")]
#[derive(Debug, Clone)]
pub struct Tree {
    data: Arc<TreeData>,
}

impl Tree {
    /// Wraps generated data.
    pub fn new(data: TreeData) -> Self {
        Self {
            data: Arc::new(data),
        }
    }

    /// The generated data.
    pub fn data(&self) -> &TreeData {
        &self.data
    }

    /// The glb bytes of the tree, root node named `name`, `extras` added to the
    /// metadata.
    fn glb(&self, name: String, extras: Option<&Bound<'_, PyDict>>) -> PyResult<Vec<u8>> {
        let mut value = metadata(&self.data.species, &self.data.plant);
        if let (Some(extras), serde_json::Value::Object(map)) = (extras, &mut value)
            && let serde_json::Value::Object(added) = py_to_json(extras.as_any())?
        {
            map.extend(added);
        }
        let options = GlbOptions {
            name,
            generator: GENERATOR.to_owned(),
            extras: Some(value),
        };
        let mut bytes = Vec::new();
        write_glb(&self.data.plant.mesh, &options, &mut bytes).map_err(glb_error)?;
        Ok(bytes)
    }
}

#[pymethods]
impl Tree {
    /// Mesh parts in draw order (bark first, then foliage).
    #[getter]
    fn parts(&self) -> Vec<MeshPart> {
        (0..self.data.plant.mesh.parts.len())
            .map(|index| MeshPart {
                data: Arc::clone(&self.data),
                index,
            })
            .collect()
    }

    /// The skeleton: one row per node of the plant graph.
    #[getter]
    fn skeleton(&self) -> Skeleton {
        Skeleton {
            data: Arc::clone(&self.data),
        }
    }

    /// The organ sites (leaves, twigs, flowers, fruits) of the plant graph.
    #[getter]
    fn organs(&self) -> Organs {
        Organs {
            data: Arc::clone(&self.data),
        }
    }

    /// Statistics: nodes, chains, leaf sites, vertices, triangles, bounds, height.
    #[getter]
    fn stats<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let value = serde_json::to_value(self.data.plant.stats)
            .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?;
        json_to_py(py, &value)
    }

    /// The metadata written to exported files, enough to generate the same tree again.
    #[getter]
    fn metadata<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        json_to_py(py, &metadata(&self.data.species, &self.data.plant))
    }

    /// Seed of the plant and of its mesh.
    #[getter]
    fn seed(&self) -> u64 {
        self.data.plant.params.seed
    }

    /// Age in growth cycles.
    #[getter]
    fn age(&self) -> u16 {
        self.data.plant.params.age
    }

    /// Last growth cycle shown, for a frame of `growth_frames`; `None` for a whole plant.
    #[getter]
    fn max_cycle(&self) -> Option<u16> {
        self.data.plant.params.mesh.max_cycle
    }

    /// Scale factor.
    #[getter]
    fn scale(&self) -> f32 {
        self.data.plant.params.scale
    }

    /// Display name of the species.
    #[getter]
    fn species_name(&self) -> String {
        self.data.species.name.clone()
    }

    /// The tree as binary glTF bytes.
    #[pyo3(signature = (name="tree", extras=None))]
    fn to_glb<'py>(
        &self,
        py: Python<'py>,
        name: &str,
        extras: Option<&Bound<'py, PyDict>>,
    ) -> PyResult<Bound<'py, PyBytes>> {
        let bytes = self.glb(name.to_owned(), extras)?;
        Ok(PyBytes::new(py, &bytes))
    }

    /// Writes the tree to a `.glb` file; the root node is named after the file stem.
    #[pyo3(signature = (path, name=None, extras=None))]
    fn save_glb(
        &self,
        path: PathBuf,
        name: Option<String>,
        extras: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<()> {
        let name = name.unwrap_or_else(|| {
            path.file_stem()
                .map_or_else(|| "tree".to_owned(), |s| s.to_string_lossy().into_owned())
        });
        let bytes = self.glb(name, extras)?;
        std::fs::write(path, bytes)?;
        Ok(())
    }

    fn __repr__(&self) -> String {
        let plant = &self.data.plant;
        format!(
            "<arbrume.Tree {:?} seed={} age={} vertices={} triangles={}>",
            self.data.species.name,
            plant.params.seed,
            plant.params.age,
            plant.stats.vertices,
            plant.stats.triangles
        )
    }
}

/// One drawable part of a tree mesh: buffers and a material.
#[pyclass(frozen, module = "arbrume._arbrume")]
#[derive(Debug, Clone)]
pub struct MeshPart {
    data: Arc<TreeData>,
    index: usize,
}

impl MeshPart {
    fn part(&self) -> &arbrume_core::mesh::MeshPart {
        &self.data.plant.mesh.parts[self.index]
    }

    fn field(&self, field: PartField) -> Field {
        Field::Part(self.index, field)
    }
}

#[pymethods]
impl MeshPart {
    /// Semantic class, such as `"bark"` or `"leaf"`.
    #[getter]
    fn kind(&self) -> String {
        format!("{:?}", self.part().kind).to_lowercase()
    }

    /// Material: name, base colour, roughness, double sided, alpha cutoff, and the PNG
    /// bytes (or `None`) of `base_color_texture`, `normal_texture` and
    /// `roughness_texture` (roughness in the green channel).
    #[getter]
    fn material<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let material = &self.part().material;
        let dict = PyDict::new(py);
        dict.set_item("name", &material.name)?;
        dict.set_item("base_color", material.base_color)?;
        dict.set_item("roughness", material.roughness)?;
        dict.set_item("double_sided", material.double_sided)?;
        dict.set_item("alpha_cutoff", material.alpha_cutoff)?;
        let texture = material
            .base_color_texture
            .as_ref()
            .map(|bytes| PyBytes::new(py, bytes));
        dict.set_item("base_color_texture", texture)?;
        let bytes = |image: &Option<Vec<u8>>| image.as_ref().map(|b| PyBytes::new(py, b));
        dict.set_item("normal_texture", bytes(&material.normal_texture))?;
        dict.set_item("roughness_texture", bytes(&material.roughness_texture))?;
        Ok(dict)
    }

    /// Vertex positions, `(n, 3)` float32, meters, Y up.
    #[getter]
    fn vertices<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        array(py, &self.data, self.field(PartField::Positions))
    }

    /// Unit vertex normals, `(n, 3)` float32.
    #[getter]
    fn normals<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        array(py, &self.data, self.field(PartField::Normals))
    }

    /// Texture coordinates, `(n, 2)` float32.
    #[getter]
    fn uvs<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        array(py, &self.data, self.field(PartField::Uvs))
    }

    /// Linear RGBA vertex colours, `(n, 4)` float32.
    #[getter]
    fn colors<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        array(py, &self.data, self.field(PartField::Colors))
    }

    /// Triangles, `(m, 3)` uint32, counter-clockwise front faces.
    #[getter]
    fn indices<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        array(py, &self.data, self.field(PartField::Indices))
    }

    /// Number of vertices.
    #[getter]
    fn vertex_count(&self) -> usize {
        self.part().buffers.positions.len()
    }

    /// Number of triangles.
    #[getter]
    fn triangle_count(&self) -> usize {
        self.part().buffers.triangle_count()
    }

    /// The raw buffer of an array: `"vertices"`, `"normals"`, `"uvs"`, `"colors"` or
    /// `"indices"`.
    fn buffer(&self, name: &str) -> PyResult<Buffer> {
        let field = match name {
            "vertices" => PartField::Positions,
            "normals" => PartField::Normals,
            "uvs" => PartField::Uvs,
            "colors" => PartField::Colors,
            "indices" => PartField::Indices,
            _ => return Err(unknown_array(name)),
        };
        Ok(Buffer::new(Arc::clone(&self.data), self.field(field)))
    }

    fn __repr__(&self) -> String {
        format!(
            "<arbrume.MeshPart {} vertices={} triangles={}>",
            self.kind(),
            self.vertex_count(),
            self.triangle_count()
        )
    }
}

/// `ValueError` for an unknown array name.
fn unknown_array(name: &str) -> PyErr {
    pyo3::exceptions::PyValueError::new_err(format!("no array named `{name}`"))
}

/// The skeleton of a tree: one row per node (metamer) of the plant graph.
///
/// A parent node always comes before its children, so iterating rows upward goes from
/// base to tips.
#[pyclass(frozen, module = "arbrume._arbrume")]
#[derive(Debug, Clone)]
pub struct Skeleton {
    data: Arc<TreeData>,
}

/// Skeleton arrays by name.
const SKELETON_ARRAYS: [(&str, GraphField); 7] = [
    ("positions", GraphField::Positions),
    ("radii", GraphField::Radii),
    ("parents", GraphField::Parents),
    ("orders", GraphField::Orders),
    ("births", GraphField::Births),
    ("chains", GraphField::Chains),
    ("shed", GraphField::Shed),
];

/// Organ arrays by name.
const ORGAN_ARRAYS: [(&str, GraphField); 6] = [
    ("nodes", GraphField::OrganNodes),
    ("kinds", GraphField::OrganKinds),
    ("directions", GraphField::OrganDirections),
    ("normals", GraphField::OrganNormals),
    ("scales", GraphField::OrganScales),
    ("births", GraphField::OrganBirths),
];

/// The field named `name` in `table`.
fn named(table: &[(&str, GraphField)], name: &str) -> PyResult<Field> {
    table
        .iter()
        .find(|(known, _)| *known == name)
        .map(|(_, field)| Field::Graph(*field))
        .ok_or_else(|| unknown_array(name))
}

#[pymethods]
impl Skeleton {
    /// Position of the distal end of each node, `(n, 3)` float32.
    #[getter]
    fn positions<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        array(py, &self.data, Field::Graph(GraphField::Positions))
    }

    /// Radius of each internode, `(n,)` float32.
    #[getter]
    fn radii<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        array(py, &self.data, Field::Graph(GraphField::Radii))
    }

    /// Parent node of each node, `(n,)` int32, -1 for the first node of a root chain.
    #[getter]
    fn parents<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        array(py, &self.data, Field::Graph(GraphField::Parents))
    }

    /// Branching order of each node, `(n,)` uint8, 0 for the trunk.
    #[getter]
    fn orders<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        array(py, &self.data, Field::Graph(GraphField::Orders))
    }

    /// Growth cycle in which each node appeared, `(n,)` uint16.
    #[getter]
    fn births<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        array(py, &self.data, Field::Graph(GraphField::Births))
    }

    /// Chain (axis) of each node, `(n,)` int32.
    #[getter]
    fn chains<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        array(py, &self.data, Field::Graph(GraphField::Chains))
    }

    /// Whether each node was shed, `(n,)` bool.
    #[getter]
    fn shed<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        array(py, &self.data, Field::Graph(GraphField::Shed))
    }

    /// The raw buffer of an array, by property name.
    fn buffer(&self, name: &str) -> PyResult<Buffer> {
        let field = named(&SKELETON_ARRAYS, name)?;
        Ok(Buffer::new(Arc::clone(&self.data), field))
    }

    fn __len__(&self) -> usize {
        self.data.plant.graph.node_count()
    }

    fn __repr__(&self) -> String {
        format!("<arbrume.Skeleton nodes={}>", self.__len__())
    }
}

/// The organ sites of a tree: one row per leaf, twig, flower or fruit site.
#[pyclass(frozen, module = "arbrume._arbrume")]
#[derive(Debug, Clone)]
pub struct Organs {
    data: Arc<TreeData>,
}

#[pymethods]
impl Organs {
    /// Node bearing each organ, `(k,)` int32.
    #[getter]
    fn nodes<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        array(py, &self.data, Field::Graph(GraphField::OrganNodes))
    }

    /// Kind of each organ, `(k,)` uint8, an index into `arbrume.ORGAN_KINDS`.
    #[getter]
    fn kinds<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        array(py, &self.data, Field::Graph(GraphField::OrganKinds))
    }

    /// Unit main direction of each organ (base to tip for a leaf), `(k, 3)` float32.
    #[getter]
    fn directions<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        array(py, &self.data, Field::Graph(GraphField::OrganDirections))
    }

    /// Unit normal of each organ (upper side of a leaf blade), `(k, 3)` float32.
    #[getter]
    fn normals<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        array(py, &self.data, Field::Graph(GraphField::OrganNormals))
    }

    /// Size factor of each organ, `(k,)` float32.
    #[getter]
    fn scales<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        array(py, &self.data, Field::Graph(GraphField::OrganScales))
    }

    /// Growth cycle in which each organ appeared, `(k,)` uint16.
    #[getter]
    fn births<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        array(py, &self.data, Field::Graph(GraphField::OrganBirths))
    }

    /// The raw buffer of an array, by property name.
    fn buffer(&self, name: &str) -> PyResult<Buffer> {
        let field = named(&ORGAN_ARRAYS, name)?;
        Ok(Buffer::new(Arc::clone(&self.data), field))
    }

    fn __len__(&self) -> usize {
        self.data.plant.graph.organs().len()
    }

    fn __repr__(&self) -> String {
        format!("<arbrume.Organs sites={}>", self.__len__())
    }
}
