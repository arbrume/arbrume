//! Billboard clouds: a few planes, each standing for the faces that lie within a
//! distance `epsilon` of it (Décoret et al. 2003), with the changes Fuhrmann et al.
//! (2005) made for trees.
//!
//! **Plane space.** A plane is a unit normal `d`, taken from a fixed set of directions
//! spread over a hemisphere (a Fibonacci lattice, so planes are unoriented), and an
//! offset `rho = (p - c) . d` from the centre `c` of the plant, binned in steps of
//! `epsilon / 2` over the bounding sphere. A face is valid for a plane when its three
//! vertices are within `epsilon` of it, which for a direction is an interval of offsets.
//!
//! **Density.** Each face adds, to the bins of every direction whose offset is within
//! its interval, its area times `|n . d|` (the angular contribution of Fuhrmann et al.,
//! sec. 3.3.1, rather than the projected area of Décoret et al.), times one minus the
//! horizontal penalty: zero up to an elevation of the plane normal of
//! `penalty_cutoff_deg` and growing linearly to `horizontal_penalty` for a horizontal
//! plane (sec. 3.3.5), so that a crown is not cut into horizontal slices. The penalty
//! of Décoret et al. for faces just missing a plane is left out.
//!
//! **Greedy choice.** The bin of highest density gives a plane; the faces valid for it
//! are collapsed and their contributions removed; this repeats until every face is
//! collapsed or the plane budget is spent. Wood (the bark part) and foliage are
//! simplified separately (sec. 3.2), the wood with at most `max_wood_planes` planes. The
//! refinement of the bin and the plane tweaking of the papers are left out: the plane
//! is the centre of its bin. A plane then draws every face of the mesh valid for it, as
//! in Décoret et al.; a face valid for no plane is drawn on the plane nearest to it, so
//! nothing is lost.
//!
//! The cost is that of the density, `directions` times the faces, twice (adding and
//! removing). A mesh with more than `max_faces` faces is sampled with a regular stride
//! for the choice, each sampled face standing for `stride` faces.

use crate::{BillboardOptions, Bounds3, Plane};
use arbrume_core::mesh::{PartKind, TreeMesh};
use glam::Vec3;

/// A face of the mesh as the plane choice sees it.
#[derive(Debug, Clone, Copy)]
struct Face {
    corners: [Vec3; 3],
    /// Area times the sampling stride.
    weight: f32,
    normal: Vec3,
}

/// The density of every plane bin and the faces still to collapse.
struct Density<'a> {
    directions: &'a [Vec3],
    /// Penalty factor of each direction.
    factors: Vec<f32>,
    center: Vec3,
    radius: f32,
    epsilon: f32,
    bins: usize,
    /// `directions.len() * bins` densities.
    values: Vec<f32>,
}

impl Density<'_> {
    /// The bins of direction `dir` that `face` is valid for, as a range.
    fn interval(&self, face: &Face, dir: usize) -> Option<(usize, usize)> {
        let d = self.directions[dir];
        let offsets = face.corners.map(|p| (p - self.center).dot(d));
        let lo = offsets.iter().copied().fold(f32::MIN, f32::max) - self.epsilon;
        let hi = offsets.iter().copied().fold(f32::MAX, f32::min) + self.epsilon;
        if lo > hi {
            return None;
        }
        let step = 0.5 * self.epsilon;
        let first = ((lo + self.radius) / step).ceil().max(0.0) as usize;
        let last = ((hi + self.radius) / step).floor();
        if last < 0.0 {
            return None;
        }
        let last = (last as usize).min(self.bins - 1);
        (first <= last).then_some((first, last))
    }

    /// Adds `sign` times the contribution of `face` to every bin.
    fn add(&mut self, face: &Face, sign: f32) {
        for dir in 0..self.directions.len() {
            let Some((first, last)) = self.interval(face, dir) else {
                continue;
            };
            let weight = sign
                * face.weight
                * face.normal.dot(self.directions[dir]).abs()
                * self.factors[dir];
            let row = dir * self.bins;
            for value in &mut self.values[row + first..=row + last] {
                *value += weight;
            }
        }
    }

    /// The direction index and offset of the densest bin, `None` when every bin is
    /// empty.
    fn best(&self) -> Option<(usize, f32)> {
        let mut best = (0.0_f32, 0);
        for (index, &value) in self.values.iter().enumerate() {
            if value > best.0 * (1.0 + 1e-6) {
                best = (value, index);
            }
        }
        (best.0 > 0.0).then(|| {
            let (dir, bin) = (best.1 / self.bins, best.1 % self.bins);
            (dir, bin as f32 * 0.5 * self.epsilon - self.radius)
        })
    }
}

/// `count` unit directions spread over the upper hemisphere (Fibonacci lattice).
fn hemisphere(count: usize) -> Vec<Vec3> {
    let golden = std::f32::consts::PI * (3.0 - 5.0_f32.sqrt());
    (0..count)
        .map(|i| {
            let y = (i as f32 + 0.5) / count as f32;
            let ring = (1.0 - y * y).sqrt();
            let angle = golden * i as f32;
            Vec3::new(ring * angle.cos(), y, ring * angle.sin())
        })
        .collect()
}

/// Penalty factor of planes of normal `d`: 1 up to the cutoff elevation, then falling
/// linearly to `1 - horizontal_penalty` for a horizontal plane (vertical normal).
fn penalty_factor(d: Vec3, options: &BillboardOptions) -> f32 {
    let elevation = d.y.abs().clamp(0.0, 1.0).asin().to_degrees();
    let cutoff = options.penalty_cutoff_deg.clamp(0.0, 89.0);
    let ramp = ((elevation - cutoff) / (90.0 - cutoff)).clamp(0.0, 1.0);
    1.0 - options.horizontal_penalty.clamp(0.0, 1.0) * ramp
}

/// The faces of the parts of `mesh` of kind `wood` (bark) or not, sampled to at most
/// `max_faces` with a regular stride.
fn faces(mesh: &TreeMesh, wood: bool, max_faces: usize) -> Vec<Face> {
    let parts = || {
        mesh.parts.iter().filter(move |part| {
            (part.kind == PartKind::Bark) == wood && part.kind != PartKind::Billboard
        })
    };
    let total: usize = parts().map(|part| part.buffers.triangle_count()).sum();
    let stride = total.div_ceil(max_faces.max(1)).max(1);
    let mut out = Vec::with_capacity(total / stride + 1);
    let mut counter = 0_usize;
    for part in parts() {
        let positions = &part.buffers.positions;
        for tri in part.buffers.indices.as_chunks::<3>().0 {
            counter += 1;
            if !counter.is_multiple_of(stride) {
                continue;
            }
            let corners = tri.map(|i| Vec3::from(positions[i as usize]));
            let cross = (corners[1] - corners[0]).cross(corners[2] - corners[0]);
            let area = 0.5 * cross.length();
            if area > 0.0 && area.is_finite() {
                out.push(Face {
                    corners,
                    weight: area * stride as f32,
                    normal: cross / (2.0 * area),
                });
            }
        }
    }
    out
}

/// Chooses up to `budget` planes for `faces` greedily; returns (normal, offset) pairs.
fn greedy(faces: &[Face], density: &mut Density, budget: usize) -> Vec<(Vec3, f32)> {
    let mut planes = Vec::new();
    let mut open = vec![true; faces.len()];
    for face in faces {
        density.add(face, 1.0);
    }
    while planes.len() < budget {
        let Some((dir, offset)) = density.best() else {
            break;
        };
        let d = density.directions[dir];
        let mut collapsed = 0;
        for (face, open) in faces.iter().zip(open.iter_mut()) {
            let inside = face
                .corners
                .iter()
                .all(|&p| ((p - density.center).dot(d) - offset).abs() <= density.epsilon);
            if *open && inside {
                *open = false;
                density.add(face, -1.0);
                collapsed += 1;
            }
        }
        if collapsed == 0 {
            break;
        }
        planes.push((d, offset));
    }
    planes
}

/// The planes of a billboard cloud of `mesh`, with the faces each one draws.
#[must_use]
pub fn choose_planes(mesh: &TreeMesh, bounds: &Bounds3, options: &BillboardOptions) -> Vec<Plane> {
    let directions = hemisphere(options.directions.max(1));
    let radius = bounds.radius();
    let epsilon = (options.epsilon * radius).max(1e-4);
    let bins = (2.0 * radius / (0.5 * epsilon)).ceil() as usize + 1;
    let mut density = Density {
        factors: directions
            .iter()
            .map(|&d| penalty_factor(d, options))
            .collect(),
        directions: &directions,
        center: bounds.center(),
        radius,
        epsilon,
        bins,
        values: vec![0.0; directions.len() * bins],
    };
    let wood = faces(mesh, true, options.max_faces);
    let wood_budget = options.max_wood_planes.min(options.max_planes);
    let mut chosen = greedy(&wood, &mut density, wood_budget);
    density.values.fill(0.0);
    let foliage = faces(mesh, false, options.max_faces);
    let foliage_budget = options.max_planes.saturating_sub(chosen.len()).max(1);
    let foliage_planes = greedy(&foliage, &mut density, foliage_budget);
    let wood_planes = chosen.len();
    chosen.extend(foliage_planes);
    assign(mesh, &chosen, wood_planes, (bounds.center(), epsilon))
}

/// Builds the planes and their masks: each triangle is drawn on every plane of its
/// group (wood planes for the bark part, the others for the rest) it is valid for, or
/// on the nearest plane of its group when it is valid for none.
fn assign(
    mesh: &TreeMesh,
    chosen: &[(Vec3, f32)],
    wood_planes: usize,
    (center, epsilon): (Vec3, f32),
) -> Vec<Plane> {
    let mut masks: Vec<Vec<Vec<bool>>> = chosen
        .iter()
        .map(|_| {
            mesh.parts
                .iter()
                .map(|part| vec![false; part.buffers.triangle_count()])
                .collect()
        })
        .collect();
    let mut reach = vec![0.0_f32; chosen.len()];
    for (part_index, part) in mesh.parts.iter().enumerate() {
        let wood = part.kind == PartKind::Bark;
        let group = if wood && wood_planes > 0 {
            0..wood_planes
        } else if wood_planes < chosen.len() {
            wood_planes..chosen.len()
        } else {
            0..chosen.len()
        };
        let positions = &part.buffers.positions;
        for (t, tri) in part.buffers.indices.as_chunks::<3>().0.iter().enumerate() {
            let corners = tri.map(|i| Vec3::from(positions[i as usize]) - center);
            let mut nearest = (f32::INFINITY, group.start);
            let mut any = false;
            for plane in group.clone() {
                let (d, offset) = chosen[plane];
                let distance = corners
                    .iter()
                    .map(|p| (p.dot(d) - offset).abs())
                    .fold(0.0, f32::max);
                if distance <= epsilon {
                    masks[plane][part_index][t] = true;
                    any = true;
                }
                if distance < nearest.0 {
                    nearest = (distance, plane);
                }
            }
            if !any && nearest.0.is_finite() {
                masks[nearest.1][part_index][t] = true;
                reach[nearest.1] = reach[nearest.1].max(nearest.0);
            }
        }
    }
    chosen
        .iter()
        .zip(masks)
        .zip(reach)
        .map(|((&(d, offset), masks), reach)| {
            Plane::new(d, center + d * offset, masks, reach.max(epsilon))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{hemisphere, penalty_factor};
    use crate::BillboardOptions;
    use glam::Vec3;

    #[test]
    fn directions_cover_the_upper_hemisphere() {
        let directions = hemisphere(200);
        assert!(
            directions
                .iter()
                .all(|d| (d.length() - 1.0).abs() < 1e-5 && d.y >= 0.0)
        );
        for probe in [Vec3::X, Vec3::Y, Vec3::new(1.0, 1.0, 1.0).normalize()] {
            let best = directions
                .iter()
                .map(|d| d.dot(probe).abs())
                .fold(0.0, f32::max);
            assert!(best > 0.97, "{probe} {best}");
        }
    }

    #[test]
    fn penalty_spares_vertical_planes_and_hits_horizontal_ones() {
        let options = BillboardOptions::default();
        assert!((penalty_factor(Vec3::X, &options) - 1.0).abs() < 1e-6);
        assert!((penalty_factor(Vec3::Y, &options) - 0.4).abs() < 1e-5);
    }
}
