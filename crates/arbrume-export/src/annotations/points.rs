//! Labelled point clouds sampled on the mesh, for scan-to-skeleton methods such as
//! `AdTree` (Du et al. 2019) and Livny et al. (2010).

use std::io::{self, Write};

use arbrume_core::mesh::{PartLabels, TreeMesh};
use arbrume_core::rng::{Rng, Stream};
use glam::Vec3;

/// Random stream of the point sampler.
const POINT_STREAM: Stream = Stream::custom(0x200);

/// Points sampled on the surface of a mesh, with their labels.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PointCloud {
    /// Positions, meters.
    pub position: Vec<[f32; 3]>,
    /// Unit normals, interpolated from the vertex normals of the triangle (the face
    /// normal for a part without normals).
    pub normal: Vec<[f32; 3]>,
    /// Semantic class of the triangle each point lies on.
    pub semantic: Vec<u8>,
    /// Instance id of that triangle.
    pub instance: Vec<u32>,
}

/// One triangle of the mesh: part and first index.
#[derive(Debug, Clone, Copy)]
struct Triangle {
    part: u32,
    first: u32,
}

/// Samples `count` points uniformly by area over every triangle of `mesh`.
///
/// `labels` holds the labels of each part of `mesh` (see
/// `arbrume_core::mesh::MeshProvenance::labels`). Point `i` depends only on `seed`, `i`
/// and the mesh: it draws a triangle with probability proportional to its area (by
/// binary search in the cumulative areas, summed in `f64`), then a point uniform on the
/// triangle with the square-root mapping of Osada et al. (2002). A point takes the
/// labels of the first vertex of its triangle, which every vertex of a triangle shares.
/// An empty mesh, or one of zero area, gives no point.
///
/// # Panics
///
/// Panics when `labels` does not have one entry per part with one label per vertex.
#[must_use]
pub fn sample_points(
    mesh: &TreeMesh,
    labels: &[PartLabels],
    count: usize,
    seed: u64,
) -> PointCloud {
    assert_eq!(labels.len(), mesh.parts.len(), "one label set per part");
    let mut triangles = Vec::with_capacity(mesh.triangle_count());
    let mut cumulative = Vec::with_capacity(mesh.triangle_count());
    let mut total = 0.0_f64;
    for (part, mesh_part) in mesh.parts.iter().enumerate() {
        let buffers = &mesh_part.buffers;
        assert_eq!(labels[part].semantic.len(), buffers.positions.len());
        for (t, corners) in buffers.indices.as_chunks::<3>().0.iter().enumerate() {
            let [p0, p1, p2] = corners_of(&buffers.positions, corners);
            total += f64::from((p1 - p0).cross(p2 - p0).length() * 0.5);
            triangles.push(Triangle {
                part: part as u32,
                first: (t * 3) as u32,
            });
            cumulative.push(total);
        }
    }
    let mut cloud = PointCloud::default();
    if total <= 0.0 {
        return cloud;
    }
    cloud.position.reserve(count);
    cloud.normal.reserve(count);
    for i in 0..count {
        let mut rng = Rng::new(seed, i as u32, POINT_STREAM);
        let target = (rng.next_u64() >> 11) as f64 * (1.0 / (1_u64 << 53) as f64) * total;
        let pick = cumulative
            .partition_point(|&c| c <= target)
            .min(triangles.len() - 1);
        let triangle = triangles[pick];
        let part = &mesh.parts[triangle.part as usize].buffers;
        let corners = &part.indices[triangle.first as usize..triangle.first as usize + 3];
        let (radial, across) = (rng.f32(), rng.f32());
        let root = radial.sqrt();
        let weights = [1.0 - root, root * (1.0 - across), root * across];
        let blend = |[p0, p1, p2]: [Vec3; 3]| p0 * weights[0] + p1 * weights[1] + p2 * weights[2];
        let triangle_corners = corners_of(&part.positions, corners);
        let point = blend(triangle_corners);
        let normal = if part.normals.len() == part.positions.len() {
            blend(corners_of(&part.normals, corners))
        } else {
            let [p0, p1, p2] = triangle_corners;
            (p1 - p0).cross(p2 - p0)
        };
        let normal = normal.try_normalize().unwrap_or(Vec3::Y);
        let first = corners[0] as usize;
        let part_labels = &labels[triangle.part as usize];
        cloud.position.push(point.to_array());
        cloud.normal.push(normal.to_array());
        cloud.semantic.push(part_labels.semantic[first]);
        cloud.instance.push(part_labels.instance[first]);
    }
    cloud
}

fn corners_of(values: &[[f32; 3]], corners: &[u32]) -> [Vec3; 3] {
    [0, 1, 2].map(|k| Vec3::from_array(values[corners[k] as usize]))
}

impl PointCloud {
    /// Number of points.
    #[must_use]
    pub fn len(&self) -> usize {
        self.position.len()
    }

    /// `true` when there is no point.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.position.is_empty()
    }

    /// Writes the cloud as a binary little-endian PLY file.
    ///
    /// One `vertex` element per point with the properties `float x, y, z`,
    /// `float nx, ny, nz`, `uchar semantic` and `uint instance`, packed (29 bytes per
    /// point). `comment` lines carry the format version and the class names.
    ///
    /// # Errors
    ///
    /// Returns the error of `out`.
    pub fn write_ply(&self, mut out: impl Write) -> io::Result<()> {
        let header = format!(
            "ply\nformat binary_little_endian 1.0\n\
             comment arbrume annotations {}\n\
             comment semantic 0 trunk 1 branch 2 twig 3 leaf\n\
             element vertex {}\n\
             property float x\nproperty float y\nproperty float z\n\
             property float nx\nproperty float ny\nproperty float nz\n\
             property uchar semantic\nproperty uint instance\nend_header\n",
            super::SCHEMA_VERSION,
            self.len()
        );
        out.write_all(header.as_bytes())?;
        let mut record = Vec::with_capacity(29 * self.len().min(1 << 16));
        for i in 0..self.len() {
            for value in self.position[i].iter().chain(&self.normal[i]) {
                record.extend_from_slice(&value.to_le_bytes());
            }
            record.push(self.semantic[i]);
            record.extend_from_slice(&self.instance[i].to_le_bytes());
            if record.len() >= 29 << 16 {
                out.write_all(&record)?;
                record.clear();
            }
        }
        out.write_all(&record)?;
        out.flush()
    }
}
