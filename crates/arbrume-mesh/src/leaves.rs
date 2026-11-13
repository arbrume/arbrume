//! Leaf instancing: which organ sites get a leaf, in what order, and the leaf part of
//! the mesh.
//!
//! **Selection.** A site gets a leaf when its kind is `Leaf` and its node is not shed.
//! With an age cut at cycle `c`, the site and its node must be born by `c`, and the leaf
//! must not be older than the retention of the species (`c - birth < retention_cycles`).
//!
//! **Order and level of detail.** Leaves are stored in a random order so that any prefix
//! of the leaf buffer is an unbiased sample of the crown (the stochastic simplification
//! of Cook et al. 2007, used for foliage by Livny et al. 2011 and Pirk et al. 2012). Each
//! site draws a key from the random stream of its organ index; sites are sorted by the
//! top bits of the key with a counting sort, sites sharing a bucket staying in organ
//! order. The order of a site therefore depends on the seed and on its own index only: it
//! does not change when other sites appear, when the plant is cut at an earlier age, or
//! with the thread count. A fraction `f` keeps the first `ceil(f n)` leaves and
//! multiplies their size by `sqrt(n / kept)`, which preserves the total leaf area.
//!
//! **Instance.** A leaf is a template of the species (a single leaf, or one of the
//! variants of its twig, see [`crate::twig`]) placed in the frame of its site:
//! `direction`, `normal x direction` and `normal`. The site is on the centerline of the
//! axis, so the leaf is moved out by the radius of its node along the part of
//! `direction` that is orthogonal to the internode. Size, attitude and colour vary from
//! leaf to leaf by the amounts of the species; the draws come from the stream of the
//! organ index, as does the choice of the variant. Colour variation is written as a
//! vertex colour, at most 1 per channel, which multiplies the base colour of the
//! material; the template adds its own tint per vertex (the shade of each leaf of a twig
//! and the colour of its stem).
//!
//! Every instance has the same number of vertices and indices, so instance `k` owns a
//! fixed range of the buffers, and blocks of instances are filled independently.

use crate::MeshOptions;
use crate::twig::TemplateSet;
use arbrume_core::graph::{OrganKind, OrganSite, PlantGraph};
use arbrume_core::mesh::MeshBuffers;
use arbrume_core::rng::{Rng, Stream};
use arbrume_core::species::Foliage;
use glam::Vec3;
use rayon::prelude::*;

/// Number of bits of the random key used to order leaves.
const ORDER_BITS: u32 = 12;

/// Leaves per independently filled block.
const BLOCK: usize = 256;

/// The leaves of one mesh: organ indices in buffer order, and their enlargement. Kept
/// between meshes so that its buffers are reused.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct LeafSet {
    /// Organ index of each kept leaf, in buffer order.
    pub organs: Vec<u32>,
    /// Factor applied to the size of every kept leaf.
    pub enlargement: f32,
    /// Scratch of the counting sort: where the next leaf of each bucket goes.
    starts: Vec<u32>,
}

/// Bucket of the random order the organ `organ` falls in.
fn order_key(seed: u64, organ: usize) -> usize {
    (Rng::new(seed, organ as u32, Stream::FOLIAGE).next_u32() >> (32 - ORDER_BITS)) as usize
}

/// `true` when `site` bears a leaf under `options`.
fn bears_leaf(graph: &PlantGraph, site: &OrganSite, options: &MeshOptions, retention: u16) -> bool {
    let live = site.kind == OrganKind::Leaf && !graph.is_shed(site.node);
    let Some(cycle) = options.max_cycle else {
        return live;
    };
    let born = site.birth <= cycle && graph.births()[site.node.index()] <= cycle;
    live && born && cycle - site.birth < retention
}

/// Chooses and orders the leaves of `graph` into `set`.
pub(crate) fn select_leaves(
    set: &mut LeafSet,
    graph: &PlantGraph,
    foliage: &Foliage,
    options: &MeshOptions,
) {
    set.organs.clear();
    set.enlargement = 1.0;
    if !foliage.enabled || options.leaf_fraction <= 0.0 {
        return;
    }
    let retention = foliage.retention_cycles;
    let eligible = |(organ, site): (usize, &OrganSite)| {
        bears_leaf(graph, site, options, retention).then(|| (organ, order_key(options.seed, organ)))
    };
    let starts = &mut set.starts;
    starts.clear();
    starts.resize((1 << ORDER_BITS) + 1, 0);
    for (_, key) in graph.organs().iter().enumerate().filter_map(eligible) {
        starts[key + 1] += 1;
    }
    for key in 0..1 << ORDER_BITS {
        starts[key + 1] += starts[key];
    }
    let total = starts[1 << ORDER_BITS] as usize;
    if total == 0 {
        return;
    }
    set.organs.resize(total, 0);
    for (organ, key) in graph.organs().iter().enumerate().filter_map(eligible) {
        set.organs[starts[key] as usize] = organ as u32;
        starts[key] += 1;
    }
    let kept = ((options.leaf_fraction * total as f32).ceil() as usize).clamp(1, total);
    set.organs.truncate(kept);
    set.enlargement = (total as f32 / kept as f32).sqrt();
}

/// The part of the leaf buffers owned by a run of leaves.
struct LeafOut<'a> {
    positions: &'a mut [[f32; 3]],
    normals: &'a mut [[f32; 3]],
    uvs: &'a mut [[f32; 2]],
    colors: &'a mut [[f32; 4]],
    indices: &'a mut [u32],
    /// Index of the first vertex of the run in the whole buffer.
    base: u32,
}

/// Everything needed to write any leaf of one mesh.
struct Instancer<'a> {
    graph: &'a PlantGraph,
    foliage: &'a Foliage,
    templates: &'a TemplateSet,
    seed: u64,
    /// Nominal leaf length times the enlargement of the level of detail.
    length: f32,
    /// Tangent of the largest random rotation about each axis.
    turn: f32,
}

impl Instancer<'_> {
    /// Point of the bark surface where the leaf of `site` is attached.
    fn anchor(&self, site: &OrganSite) -> Vec3 {
        let graph = self.graph;
        let node = graph.position(site.node);
        let axis = (node - graph.proximal(site.node)).normalize_or_zero();
        let outward = site.direction - axis * site.direction.dot(axis);
        let reach = outward.length();
        if reach > 1e-3 {
            node + outward * (graph.radii()[site.node.index()] / reach)
        } else {
            node
        }
    }

    /// Writes the leaf of organ `organ` into `out`, which holds exactly one leaf.
    fn write(&self, organ: u32, out: &mut LeafOut) {
        let site = &self.graph.organs()[organ as usize];
        let foliage = self.foliage;
        let mut rng = Rng::at(self.seed, organ, Stream::FOLIAGE, 1);
        let size = self.length * site.scale * foliage.size_variation.mul_add(rng.signed(), 1.0);
        let side = site.normal.cross(site.direction);
        let yaw = self.turn * rng.signed();
        let pitch = self.turn * rng.signed();
        let roll = self.turn * rng.signed();
        let forward = (site.direction + side * yaw + site.normal * pitch).normalize();
        let up = (site.normal + side * roll)
            .reject_from_normalized(forward)
            .normalize();
        let across = up.cross(forward);
        let brightness = foliage.brightness_variation.mul_add(-rng.f32(), 1.0);
        let hue = foliage.hue_variation * rng.signed();
        let color = [
            (brightness * (1.0 + hue)).min(1.0),
            brightness,
            (brightness * (1.0 - hue)).min(1.0),
        ];
        let anchor = self.anchor(site);
        let variants = &self.templates.variants;
        let template = if variants.len() > 1 {
            &variants[rng.below(variants.len() as u32) as usize]
        } else {
            &variants[0]
        };
        for (slot, local) in out.positions.iter_mut().zip(&template.positions) {
            let offset = forward * local.x + across * local.y + up * local.z;
            *slot = (anchor + offset * size).to_array();
        }
        for (slot, local) in out.normals.iter_mut().zip(&template.normals) {
            *slot = (forward * local.x + across * local.y + up * local.z).to_array();
        }
        out.uvs.copy_from_slice(&template.uvs);
        for (slot, tint) in out.colors.iter_mut().zip(&template.tints) {
            *slot = [
                color[0] * tint[0],
                color[1] * tint[1],
                color[2] * tint[2],
                1.0,
            ];
        }
        for (slot, local) in out.indices.iter_mut().zip(&template.indices) {
            *slot = out.base + local;
        }
    }

    /// Writes the leaves `organs` into `out`, which holds exactly that many leaves.
    fn write_block(&self, organs: &[u32], out: &mut LeafOut) {
        let (vertices, indices) = self.templates.instance_counts();
        let positions = out.positions.chunks_exact_mut(vertices);
        let normals = out.normals.chunks_exact_mut(vertices);
        let uvs = out.uvs.chunks_exact_mut(vertices);
        let colors = out.colors.chunks_exact_mut(vertices);
        let triangles = out.indices.chunks_exact_mut(indices);
        let slots = positions.zip(normals).zip(uvs).zip(colors).zip(triangles);
        for (leaf, ((((positions, normals), uvs), colors), indices)) in slots.enumerate() {
            let mut one = LeafOut {
                positions,
                normals,
                uvs,
                colors,
                indices,
                base: out.base + (leaf * vertices) as u32,
            };
            self.write(organs[leaf], &mut one);
        }
    }
}

/// Number of vertices and of indices of the leaf part for `leaves` leaves.
pub(crate) fn leaf_counts(templates: &TemplateSet, leaves: usize) -> (usize, usize) {
    let (vertices, indices) = templates.instance_counts();
    (leaves * vertices, leaves * indices)
}

/// Where the leaves of one mesh come from.
#[derive(Debug, Clone, Copy)]
pub(crate) struct LeafSource<'a> {
    /// The plant.
    pub graph: &'a PlantGraph,
    /// Foliage layer of the species.
    pub foliage: &'a Foliage,
    /// Leaves to instance.
    pub set: &'a LeafSet,
    /// Geometry of one instance.
    pub templates: &'a TemplateSet,
}

/// Sizes `buffers` for the leaves of `source` and fills them. Their allocations are
/// reused.
pub(crate) fn mesh_leaves(source: &LeafSource, options: &MeshOptions, buffers: &mut MeshBuffers) {
    let LeafSource {
        graph,
        foliage,
        set,
        templates,
    } = *source;
    let (vertices, indices) = leaf_counts(templates, set.organs.len());
    buffers.positions.resize(vertices, [0.0; 3]);
    buffers.normals.resize(vertices, [0.0; 3]);
    buffers.uvs.resize(vertices, [0.0; 2]);
    buffers.colors.resize(vertices, [0.0; 4]);
    buffers.indices.resize(indices, 0);
    if vertices == 0 {
        return;
    }
    let instancer = Instancer {
        graph,
        foliage,
        templates,
        seed: options.seed,
        length: foliage.length * set.enlargement,
        turn: foliage.orientation_variation_deg.to_radians().tan(),
    };
    let (per_vertex, per_index) = leaf_counts(templates, BLOCK);
    let block = |(number, ((((organs, positions), normals), uvs), (colors, indices)))| {
        let mut out = LeafOut {
            positions,
            normals,
            uvs,
            colors,
            indices,
            base: (number * per_vertex) as u32,
        };
        instancer.write_block(organs, &mut out);
    };
    if options.parallel {
        let organs = set.organs.par_chunks(BLOCK);
        let positions = buffers.positions.par_chunks_mut(per_vertex);
        let normals = buffers.normals.par_chunks_mut(per_vertex);
        let uvs = buffers.uvs.par_chunks_mut(per_vertex);
        let colors = buffers.colors.par_chunks_mut(per_vertex);
        let indices = buffers.indices.par_chunks_mut(per_index);
        let slots = organs.zip(positions).zip(normals).zip(uvs);
        slots.zip(colors.zip(indices)).enumerate().for_each(block);
    } else {
        let organs = set.organs.chunks(BLOCK);
        let positions = buffers.positions.chunks_mut(per_vertex);
        let normals = buffers.normals.chunks_mut(per_vertex);
        let uvs = buffers.uvs.chunks_mut(per_vertex);
        let colors = buffers.colors.chunks_mut(per_vertex);
        let indices = buffers.indices.chunks_mut(per_index);
        let slots = organs.zip(positions).zip(normals).zip(uvs);
        slots.zip(colors.zip(indices)).enumerate().for_each(block);
    }
}

#[cfg(test)]
mod tests {
    use super::LeafSet;
    use crate::MeshOptions;
    use crate::synthetic::{SkeletonParams, skeleton};
    use arbrume_core::graph::PlantGraph;
    use arbrume_core::species::Foliage;

    fn select_leaves(graph: &PlantGraph, foliage: &Foliage, options: &MeshOptions) -> LeafSet {
        let mut set = LeafSet::default();
        super::select_leaves(&mut set, graph, foliage, options);
        set
    }

    fn evergreen() -> Foliage {
        Foliage {
            retention_cycles: u16::MAX,
            ..Foliage::default()
        }
    }

    #[test]
    fn every_leaf_is_kept_once_at_full_detail() {
        let graph = skeleton(&SkeletonParams::small(3));
        let set = select_leaves(&graph, &Foliage::default(), &MeshOptions::default());
        let mut organs = set.organs.clone();
        organs.sort_unstable();
        let all: Vec<u32> = (0..graph.organs().len() as u32).collect();
        assert_eq!(organs, all);
        assert_ne!(set.organs, all, "leaves are not in site order");
        assert!((set.enlargement - 1.0).abs() < 1e-6);
    }

    #[test]
    fn a_fraction_keeps_a_prefix_and_preserves_area() {
        let graph = skeleton(&SkeletonParams::small(3));
        let foliage = Foliage::default();
        let full = select_leaves(&graph, &foliage, &MeshOptions::default());
        for fraction in [0.9, 0.5, 0.11, 0.001] {
            let options = MeshOptions {
                leaf_fraction: fraction,
                ..MeshOptions::default()
            };
            let set = select_leaves(&graph, &foliage, &options);
            let kept = set.organs.len();
            assert_eq!(kept, (fraction * full.organs.len() as f32).ceil() as usize);
            assert_eq!(set.organs[..], full.organs[..kept]);
            let area = kept as f32 * set.enlargement * set.enlargement;
            assert!((area / full.organs.len() as f32 - 1.0).abs() < 1e-4);
        }
    }

    #[test]
    fn order_does_not_depend_on_which_other_leaves_exist() {
        let graph = skeleton(&SkeletonParams::small(3));
        let foliage = evergreen();
        let full = select_leaves(&graph, &foliage, &MeshOptions::default());
        let young = MeshOptions {
            max_cycle: Some(6),
            ..MeshOptions::default()
        };
        let young = select_leaves(&graph, &foliage, &young);
        assert!(!young.organs.is_empty() && young.organs.len() < full.organs.len());
        let born = |organ: &&u32| graph.organs()[**organ as usize].birth <= 6;
        let expected: Vec<u32> = full.organs.iter().filter(born).copied().collect();
        assert_eq!(young.organs, expected);
        let reseeded = MeshOptions {
            seed: 1,
            ..MeshOptions::default()
        };
        assert_ne!(
            select_leaves(&graph, &foliage, &reseeded).organs,
            full.organs
        );
    }

    #[test]
    fn retention_drops_old_leaves_only_with_an_age_cut() {
        let graph = skeleton(&SkeletonParams::small(3));
        let foliage = Foliage::default();
        let last = *graph.births().iter().max().unwrap();
        let cut = MeshOptions {
            max_cycle: Some(last),
            ..MeshOptions::default()
        };
        let recent = select_leaves(&graph, &foliage, &cut);
        let sites = graph.organs();
        assert_ne!(recent.organs, []);
        assert!(
            recent
                .organs
                .iter()
                .all(|&organ| sites[organ as usize].birth == last)
        );
        let disabled = Foliage {
            enabled: false,
            ..foliage
        };
        assert_eq!(select_leaves(&graph, &disabled, &cut).organs, []);
    }
}
