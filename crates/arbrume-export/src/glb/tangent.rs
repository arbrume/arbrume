//! Tangents of parts with a normal texture.
//!
//! glTF asks for a `TANGENT` attribute where a normal texture is used, or leaves the
//! tangent frame to the reader. Each vertex gets the sum of the directions of
//! increasing `u` of its triangles, made orthogonal to its normal, and the handedness
//! `w` that makes `cross(normal, tangent) * w` point towards decreasing `v` (up in the
//! image), the glTF convention.

use arbrume_core::mesh::MeshBuffers;
use glam::{Vec2, Vec3};

/// Per-vertex tangents of `buffers`, which must have normals and texture coordinates.
pub(super) fn tangents(buffers: &MeshBuffers) -> Vec<[f32; 4]> {
    let count = buffers.positions.len();
    let mut along_u = vec![Vec3::ZERO; count];
    let mut along_v = vec![Vec3::ZERO; count];
    for tri in buffers.indices.as_chunks::<3>().0 {
        let [a, b, c] = tri.map(|i| i as usize);
        let p = |i: usize| Vec3::from(buffers.positions[i]);
        let uv = |i: usize| Vec2::from(buffers.uvs[i]);
        let (e1, e2) = (p(b) - p(a), p(c) - p(a));
        let (d1, d2) = (uv(b) - uv(a), uv(c) - uv(a));
        let det = d1.x * d2.y - d2.x * d1.y;
        if det.abs() < 1e-20 {
            continue;
        }
        let du = (e1 * d2.y - e2 * d1.y) / det;
        let dv = (e2 * d1.x - e1 * d2.x) / det;
        for vertex in [a, b, c] {
            along_u[vertex] += du;
            along_v[vertex] += dv;
        }
    }
    (0..count)
        .map(|vertex| {
            let normal = Vec3::from(buffers.normals[vertex]);
            let tangent = along_u[vertex]
                .reject_from(normal)
                .try_normalize()
                .unwrap_or_else(|| normal.any_orthonormal_vector());
            let up = -along_v[vertex];
            let w = if normal.cross(tangent).dot(up) < 0.0 {
                -1.0
            } else {
                1.0
            };
            [tangent.x, tangent.y, tangent.z, w]
        })
        .collect()
}
