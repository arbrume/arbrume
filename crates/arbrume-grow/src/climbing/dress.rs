//! What a grown climbing plant gets once its radii are known.
//!
//! [`dress`] runs after the radii are set, in three passes over the live nodes:
//!
//! 1. **Hugging.** A node held by a support (within `reach` of it, its own radius and
//!    the gap added) whose bark would come closer to the surface than `gap` is moved
//!    out along the normal. Growth placed the axes at the gap from the surface for the
//!    tip radius; the stems that thickened since are moved out by their extra radius,
//!    so no stem enters its support nor shares its surface.
//! 2. **Leaves.** The leaf sites of a held node turn, with the weight `leaf_facing`,
//!    to a frame whose upper side faces away from the surface and a little up, and
//!    whose blade points along the surface and out of it.
//! 3. **Attachment organs**, as short axes appended to the graph: rootlets from held
//!    nodes to the surface for the adhesive habit, coiled tendrils from nodes near a
//!    thin support for the tendril habit. Each node draws its organ from its own random
//!    key, after compaction, so the organs depend on the plant only. An organ that
//!    would enter its support or the ground is left out. Organs are at most half as thick as the tip
//!    of a stem, which tells them from stems.
//!
//! The appended axes carry the birth of their node and keep the layout contiguous.

use super::support::{Contact, Supports};
use crate::math::{UP, perpendicular, reject};
use crate::stage::Context;
use crate::surroundings::Surroundings;
use arbrume_core::graph::{ChainKind, Metamer, NodeId, OrganKind, PlantGraph};
use arbrume_core::rng::{Rng, Stream};
use arbrume_core::species::{Attachment, Climbing};
use glam::Vec3;
use std::f32::consts::TAU;

/// Random stream of the rootlets.
const ROOTLETS: Stream = Stream::custom(0x400);
/// Random stream of the tendrils.
const TENDRILS: Stream = Stream::custom(0x401);
/// Radius of a rootlet or tendril, as a fraction of its length.
const ORGAN_THICKNESS: f32 = 0.012;
/// Fraction of a tendril that runs straight before it coils.
const TENDRIL_STALK: f32 = 0.35;
/// Nodes per turn of a tendril coil.
const COIL_NODES: f32 = 6.0;
/// Most nodes in one tendril.
const MAX_TENDRIL_NODES: usize = 48;

/// Applies the passes of the [module documentation](self) to the graph of a climbing
/// species grown in `surroundings`, the plant standing at `origin`. Does nothing for a
/// species without `[climbing]`.
pub fn dress(graph: &mut PlantGraph, cx: &Context, surroundings: &Surroundings, origin: Vec3) {
    let Some(climbing) = cx.species.climbing.as_ref() else {
        return;
    };
    let supports = Supports::new(surroundings, climbing.ground.then_some(origin.y));
    if supports.is_empty() {
        return;
    }
    let held = hug(graph, climbing, &supports, origin);
    face_leaves(graph, climbing, &held);
    let organs = Organs {
        climbing,
        supports: &supports,
        origin,
        seed: cx.seed,
        thinnest: 0.5 * cx.species.geometry.tip_radius * cx.scale,
    };
    for (index, contact) in held.iter().enumerate() {
        let node = NodeId(index as u32);
        if graph.is_shed(node) {
            continue;
        }
        match climbing.attachment {
            Attachment::Adhesive => organs.add_rootlet(graph, node, *contact),
            Attachment::Tendril => organs.add_tendril(graph, node),
            Attachment::Twining => {}
        }
    }
}

/// Moves the held nodes out to the gap from their support and returns the contact of
/// every node, `None` for those not held.
fn hug(
    graph: &mut PlantGraph,
    climbing: &Climbing,
    supports: &Supports,
    origin: Vec3,
) -> Vec<Option<Contact>> {
    let count = graph.node_count();
    let mut held = Vec::with_capacity(count);
    for index in 0..count {
        let radius = graph.radii()[index];
        let point = graph.positions()[index] + origin;
        let offset = radius + climbing.gap;
        let reach = climbing.reach + offset;
        let contact = supports
            .nearest(point, reach, |_| true)
            .or_else(|| supports.ground_contact(point))
            .filter(|contact| contact.distance <= reach);
        if let Some(contact) = contact.filter(|contact| contact.distance < offset) {
            graph.positions_mut()[index] = contact.surface_point(point, offset) - origin;
        }
        held.push(contact);
    }
    held
}

/// Turns the leaf sites of held nodes away from their support.
fn face_leaves(graph: &mut PlantGraph, climbing: &Climbing, held: &[Option<Contact>]) {
    let weight = climbing.leaf_facing;
    if weight <= 0.0 {
        return;
    }
    for organ in graph.organs_mut() {
        if organ.kind != OrganKind::Leaf && organ.kind != OrganKind::Twig {
            continue;
        }
        let Some(Some(contact)) = held.get(organ.node.index()) else {
            continue;
        };
        let normal = contact.normal;
        let along = reject(organ.direction, normal)
            .try_normalize()
            .unwrap_or_else(|| perpendicular(normal, -UP));
        let facing = (along + normal * 0.6).normalize();
        let upper = (normal + UP * 0.3).normalize();
        let direction = organ.direction.lerp(facing, weight).normalize_or(facing);
        let hint = organ.normal.lerp(upper, weight);
        organ.direction = direction;
        organ.normal = perpendicular(direction, hint);
    }
}

/// What the attachment organs of one plant are built from.
struct Organs<'a> {
    climbing: &'a Climbing,
    supports: &'a Supports<'a>,
    origin: Vec3,
    seed: u64,
    /// Largest radius of an organ: half the tip radius of a stem.
    thinnest: f32,
}

impl Organs<'_> {
    /// Radius of an organ of `length`.
    fn radius(&self, length: f32) -> f32 {
        (length * ORGAN_THICKNESS).min(self.thinnest)
    }

    /// Whether every point of an organ of `radius` stays out of the support of
    /// `contact` and above the ground, the points being in the frame of the plant.
    fn clear(&self, contact: &Contact, points: &[Vec3], radius: f32) -> bool {
        points.iter().all(|point| {
            let point = *point + self.origin;
            let there = self.supports.contact_with(contact, point);
            let low = self
                .supports
                .ground()
                .is_some_and(|ground| point.y < ground + radius);
            there.distance > radius && !low
        })
    }

    /// A rootlet from the bark of a held node to the surface of its support.
    fn add_rootlet(&self, graph: &mut PlantGraph, node: NodeId, contact: Option<Contact>) {
        let Some(contact) = contact else {
            return;
        };
        let length = self.climbing.rootlet_length;
        let mut rng = Rng::new(self.seed, node.0, ROOTLETS);
        if length <= 0.0 || !rng.chance(self.climbing.rootlets) {
            return;
        }
        let point = graph.positions()[node.index()];
        let stem = graph.radii()[node.index()];
        let radius = self.radius(length);
        let normal = contact.normal;
        let spread = perpendicular(normal, rng.unit_sphere());
        let there = self.supports.contact_with(&contact, point + self.origin);
        let surface = there.surface_point(point + self.origin, 2.0 * radius) - self.origin;
        let points = [point - normal * stem, surface + spread * (length * 0.7)];
        if self.clear(&contact, &points, radius) {
            add_axis(graph, node, &points, radius);
        }
    }

    /// A tendril from a node near a thin support: a stalk that leaves along the surface
    /// and a little out of it, then a coil about the direction of the stalk.
    fn add_tendril(&self, graph: &mut PlantGraph, node: NodeId) {
        let climbing = self.climbing;
        let point = graph.positions()[node.index()];
        let Some(contact) = nearest_thin(climbing, self.supports, point + self.origin) else {
            return;
        };
        let mut rng = Rng::new(self.seed, node.0, TENDRILS);
        if climbing.tendril_length <= 0.0 || !rng.chance(climbing.tendrils) {
            return;
        }
        let normal = contact.normal;
        let along = perpendicular(normal, rng.unit_sphere());
        let axis = (along + normal * 0.3).normalize();
        let points = coil(point, axis, climbing, rng.angle());
        let radius = self.radius(climbing.tendril_length);
        if self.clear(&contact, &points, radius) {
            add_axis(graph, node, &points, radius);
        }
    }
}

/// The support a tendril at `point` reaches for: the nearest thin one within search.
fn nearest_thin(climbing: &Climbing, supports: &Supports, point: Vec3) -> Option<Contact> {
    let thickness = climbing.max_support_thickness;
    let accept = |contact: &Contact| supports.is_thin(contact, point, thickness);
    supports.nearest(point, climbing.search_distance, accept)
}

/// Appends a short axis through `points` to `node`, of constant `radius`.
fn add_axis(graph: &mut PlantGraph, node: NodeId, points: &[Vec3], radius: f32) {
    let birth = graph.births()[node.index()];
    let Some((&first, rest)) = points.split_first() else {
        return;
    };
    let start = graph.add_chain(node, ChainKind::Lateral, Metamer::new(first, radius, birth));
    let chain = graph.chain_of(start);
    for &point in rest {
        graph.extend_chain(chain, Metamer::new(point, radius, birth));
    }
}

/// Points of a tendril from `start` along `axis`, its coil starting at `phase`.
fn coil(start: Vec3, axis: Vec3, climbing: &Climbing, phase: f32) -> Vec<Vec3> {
    let length = climbing.tendril_length;
    let turns = climbing.tendril_turns.max(0.0);
    let stalk_end = start + axis * (length * TENDRIL_STALK);
    let mut points = vec![start + axis * (length * TENDRIL_STALK * 0.5), stalk_end];
    let coil_length = length * (1.0 - TENDRIL_STALK);
    let nodes = ((turns * COIL_NODES).ceil() as usize).min(MAX_TENDRIL_NODES - 2);
    if nodes == 0 || turns <= 0.0 {
        points.push(stalk_end + axis * coil_length);
        return points;
    }
    let radius = 0.95 * coil_length / (TAU * turns);
    let rise = coil_length * (1.0 - 0.95_f32 * 0.95).sqrt();
    let base = perpendicular(axis, UP);
    let (sin, cos) = phase.sin_cos();
    let u = base * cos + axis.cross(base) * sin;
    let v = axis.cross(u);
    let centre = stalk_end - u * radius;
    for step in 1..=nodes {
        let t = step as f32 / nodes as f32;
        let angle = TAU * turns * t;
        let (sin, cos) = angle.sin_cos();
        let around = u * cos + v * sin;
        points.push(centre + around * radius + axis * (rise * t));
    }
    points
}
