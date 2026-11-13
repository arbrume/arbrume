//! Checks shared by the integration tests.

use arbrume_core::mesh::{MeshBuffers, MeshPart, PartKind, TreeMesh};
use glam::Vec3;

/// The part of `mesh` of the given kind.
pub fn part(mesh: &TreeMesh, kind: PartKind) -> Option<&MeshPart> {
    mesh.parts.iter().find(|part| part.kind == kind)
}

/// Corner positions of every triangle.
pub fn triangles(buffers: &MeshBuffers) -> impl Iterator<Item = [Vec3; 3]> + '_ {
    let (triangles, _) = buffers.indices.as_chunks::<3>();
    triangles
        .iter()
        .map(|triangle| triangle.map(|index| Vec3::from(buffers.positions[index as usize])))
}

/// Total surface area.
pub fn area(buffers: &MeshBuffers) -> f32 {
    triangles(buffers)
        .map(|[a, b, c]| 0.5 * (b - a).cross(c - a).length())
        .sum()
}

/// Smallest and largest corner of the positions.
pub fn bounds(buffers: &MeshBuffers) -> (Vec3, Vec3) {
    let low = Vec3::splat(f32::INFINITY);
    let points = buffers.positions.iter().map(|&p| Vec3::from(p));
    points.fold((low, -low), |(min, max), p| (min.min(p), max.max(p)))
}

/// Every float of the buffers as raw bits, for exact comparison.
pub fn bits(mesh: &TreeMesh) -> Vec<u32> {
    let mut out = Vec::new();
    for part in &mesh.parts {
        let buffers = &part.buffers;
        out.extend(buffers.positions.iter().flatten().map(|v| v.to_bits()));
        out.extend(buffers.normals.iter().flatten().map(|v| v.to_bits()));
        out.extend(buffers.uvs.iter().flatten().map(|v| v.to_bits()));
        out.extend(buffers.colors.iter().flatten().map(|v| v.to_bits()));
        out.extend(&buffers.indices);
    }
    out
}

/// Asserts the properties every part must have: attribute lengths, finite values, unit
/// normals, indices in range, every vertex used, no degenerate triangle, and triangles
/// wound consistently with their vertex normals.
pub fn assert_well_formed(part: &MeshPart) {
    let buffers = &part.buffers;
    let count = buffers.positions.len();
    assert_eq!(buffers.normals.len(), count);
    assert_eq!(buffers.uvs.len(), count);
    assert!(buffers.colors.is_empty() || buffers.colors.len() == count);
    assert_eq!(buffers.indices.len() % 3, 0);
    assert!(buffers.positions.iter().flatten().all(|v| v.is_finite()));
    assert!(buffers.uvs.iter().flatten().all(|v| v.is_finite()));
    let in_unit = |v: &f32| (0.0..=1.0).contains(v);
    assert!(buffers.colors.iter().flatten().all(in_unit));
    for normal in &buffers.normals {
        let length = Vec3::from(*normal).length();
        assert!((length - 1.0).abs() < 1e-4, "normal of length {length}");
    }
    let mut used = vec![false; count];
    for triangle in buffers.indices.as_chunks::<3>().0 {
        let [first, second, third] = *triangle;
        let distinct = first != second && second != third && first != third;
        assert!(distinct, "repeated index in {triangle:?}");
        assert!(triangle.iter().all(|&index| (index as usize) < count));
        let [a, b, c] = triangle.map(|index| Vec3::from(buffers.positions[index as usize]));
        let face = (b - a).cross(c - a);
        assert!(face.length() > 0.0, "degenerate triangle {triangle:?}");
        assert!(
            a != b && b != c && a != c,
            "coincident corners in {triangle:?}"
        );
        for &index in triangle {
            used[index as usize] = true;
            let normal = Vec3::from(buffers.normals[index as usize]);
            assert!(normal.dot(face) > 0.0, "triangle {triangle:?} faces away");
        }
    }
    assert!(used.iter().all(|&used| used), "unused vertex");
}
