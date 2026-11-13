//! The shoot builder: turns the decisions of the stages into metamers, buds and leaves.
//!
//! This is the part of the cycle that does not change with the growth mode. For each
//! [`Flush`] it appends the metamers of the shoot, asking the orientation stage for each
//! heading. On every new node it places the leaves and the axillary buds given by the
//! phyllotaxis of the species, offering each bud to the fate stage. At the end of the
//! shoot it closes the apex, starts the relay of a sympodial axis, or forks.
//!
//! # Frames and azimuths
//!
//! Each meristem carries a unit `side` vector orthogonal to its heading, transported
//! along the axis by projection at every metamer (the rotation-minimizing frame of
//! Prusinkiewicz et al. 2001, equation 10 with no roll). Leaf `j` of the `k` leaves of
//! node `i` sits at azimuth `phase + i * divergence + j * 360 / k` from `side`. On a
//! plagiotropic axis the azimuth only selects one of the two horizontal sides of the
//! axis (the one whose cosine sign matches), which keeps leaves and laterals in one
//! plane, as in the model of Honda where the branch plane is closest to horizontal.
//!
//! # Random draws
//!
//! Every draw is seeded by the key of a meristem (see [`crate::plant`]), with a stream
//! and a counter derived from the index of the metamer, so it does not depend on node
//! ids, on meristem ids or on the order of processing.

use crate::math::{UP, horizontal_side, perpendicular, radial, tilt};
use crate::plant::SiteKind;
use crate::plant::{LeafyNode, Meristem, MeristemId, MeristemState, NodeFrame, Plant};
use crate::plant::{child_key, seed_key};
use crate::stage::{BudFate, BudSite, Context, Flush, HeadingInput, ShootOrientation};
use arbrume_core::graph::{BudId, BudKind, BudState, ChainId, ChainKind, Metamer, NodeFlags};
use arbrume_core::graph::{NodeId, OrganKind, OrganSite};
use arbrume_core::rng::{Rng, Stream};
use arbrume_core::species::{AxisOrientation, Branching, GrowthRhythm};
use glam::Vec3;
use std::f32::consts::TAU;

/// Stream of the fork decisions.
const FORK: Stream = Stream::custom(0);
/// Stream of the placement of basal stems.
const STEM: Stream = Stream::custom(2);

/// The shoot being built.
#[derive(Debug, Clone, Copy)]
struct Unit {
    id: MeristemId,
    cycle: u16,
    metamers: u16,
    internode: f32,
}

impl NodeFrame {
    /// Unit vector pointing away from the axis for organ `slot` of `count`.
    fn radial(&self, slot: u32, count: u32) -> Vec3 {
        let azimuth = self.azimuth + TAU * slot as f32 / count as f32;
        self.radial_at(azimuth)
    }

    /// Unit vector pointing away from the axis at `azimuth` radians.
    fn radial_at(&self, azimuth: f32) -> Vec3 {
        if self.plagiotropic {
            let horizontal = horizontal_side(self.heading, self.side);
            if azimuth.cos() >= 0.0 {
                horizontal
            } else {
                -horizontal
            }
        } else {
            radial(self.heading, self.side, azimuth)
        }
    }
}

/// Frame of the last node built by `meristem`.
fn node_frame(cx: &Context, meristem: &Meristem) -> NodeFrame {
    let architecture = &cx.species.architecture;
    let phyllotaxis = architecture.phyllotaxis_of(meristem.order);
    let node_index = meristem.nodes.saturating_sub(1);
    let mut azimuth = meristem.phase + node_index as f32 * phyllotaxis.divergence_deg.to_radians();
    let jitter = cx.species.growth.phyllotaxis_variation;
    if jitter > 0.0 {
        let mut rng = meristem.rng(Stream::PHYLLOTAXIS, u64::from(node_index));
        azimuth += jitter.to_radians() * rng.signed();
    }
    NodeFrame {
        heading: meristem.heading,
        side: meristem.side,
        azimuth,
        plagiotropic: architecture.axis(meristem.order).orientation
            == AxisOrientation::Plagiotropic,
    }
}

/// Creates the resting meristems of the stems that rise from the ground.
///
/// A single stem starts at the origin and points up. Several stems start evenly spaced
/// on a circle of radius `stem_spread`, the circle being turned by a random angle, and
/// lean outwards by `stem_angle`.
pub(crate) fn plant_seeds<F: BudFate>(plant: &mut Plant, cx: &Context, fate: &mut F) {
    let growth = &cx.species.growth;
    let stems = u32::from(growth.stems);
    let turn = Rng::new(cx.seed, u32::MAX, STEM).angle();
    for stem in 0..stems {
        let key = seed_key(cx.seed, stem);
        let site = BudSite {
            id: plant.next_meristem(),
            key,
            kind: SiteKind::Seed,
            order: 0,
            parent: None,
            node_index: stem,
            slot: 0,
            position: 0.0,
            arc: 0.0,
            unit_position: 0.0,
            cycle: 0,
        };
        let Some(target) = fate.admit(plant, &site) else {
            continue;
        };
        let mut rng = Rng::new(key, 0, STEM);
        let (tip, heading) = if stems > 1 {
            let azimuth = turn + TAU * stem as f32 / stems as f32;
            let outward = Vec3::new(azimuth.cos(), 0.0, azimuth.sin());
            let variation = growth.branching_angle_variation.eval(0, 0.0);
            let lean = (growth.stem_angle + variation * rng.signed()).max(0.0);
            (
                outward * growth.stem_spread * cx.scale,
                tilt(UP, outward, lean.to_radians()),
            )
        } else {
            (Vec3::ZERO, UP)
        };
        plant.meristems.push(Meristem {
            key,
            kind: SiteKind::Seed,
            state: MeristemState::Resting,
            order: 0,
            born: 0,
            bud: BudId::NONE,
            chain: ChainId::NONE,
            tip,
            heading,
            side: perpendicular(heading, Vec3::X),
            phase: rng.angle(),
            nodes: 0,
            shoots: 0,
            start: 0.0,
            length: 0.0,
            target,
            attach_position: 0.0,
        });
    }
}

/// Builds the shoot of one flush: its metamers with their leaves and buds, then the end
/// of the apex.
pub(crate) fn grow_shoot<F: BudFate, O: ShootOrientation>(
    plant: &mut Plant,
    cx: &Context,
    fate: &mut F,
    orientation: &mut O,
    flush: &Flush,
    cycle: u16,
) {
    let id = flush.meristem;
    let mut meristem = plant.meristems[id.index()];
    let unit = Unit {
        id,
        cycle,
        metamers: flush.metamers,
        internode: flush.length / f32::from(flush.metamers.max(1)),
    };
    for index in 0..flush.metamers {
        let node = add_metamer(plant, cx, orientation, &mut meristem, &unit, index);
        plant.meristems[id.index()] = meristem;
        let frame = node_frame(cx, &meristem);
        if cx.species.foliage.enabled {
            plant.leafy.push(LeafyNode {
                node,
                meristem: id,
                shoot: meristem.shoots,
                birth: cycle,
                position: meristem.position(),
                frame,
            });
        }
        add_lateral_buds(plant, cx, fate, &unit, &frame, (index, node));
    }
    meristem.shoots += u16::from(flush.metamers > 0);
    end_shoot(plant, cx, fate, &mut meristem, flush, cycle);
    if meristem.bud.is_some() {
        plant.graph.buds_mut().directions[meristem.bud.index()] = meristem.heading;
    }
    plant.meristems[id.index()] = meristem;
}

/// Appends metamer `index` of the shoot and advances the meristem.
fn add_metamer<O: ShootOrientation>(
    plant: &mut Plant,
    cx: &Context,
    orientation: &mut O,
    meristem: &mut Meristem,
    unit: &Unit,
    index: u16,
) -> NodeId {
    let preferred = if meristem.bud.is_some() {
        plant.graph.bud_preferred()[meristem.bud.index()]
    } else {
        Vec3::ZERO
    };
    let heading = orientation.heading(&HeadingInput {
        meristem: unit.id,
        key: meristem.key,
        order: meristem.order,
        heading: meristem.heading,
        side: meristem.side,
        preferred,
        position: meristem.position(),
        internode: unit.internode,
        axis_length: meristem.target,
        node_index: meristem.nodes,
        unit_metamers: unit.metamers,
        cycle: unit.cycle,
    });
    meristem.side = perpendicular(heading, meristem.side);
    meristem.heading = heading;
    let rhythmic = cx.species.architecture.growth_rhythm == GrowthRhythm::Rhythmic;
    let metamer = Metamer {
        position: meristem.tip + heading * (unit.internode * cx.scale),
        radius: 0.0,
        birth: unit.cycle,
        flags: if index == 0 && rhythmic {
            NodeFlags::GROWTH_UNIT_START
        } else {
            NodeFlags::NONE
        },
    };
    let node = if meristem.chain.is_none() {
        start_chain(plant, meristem, unit.id, metamer)
    } else {
        plant.graph.extend_chain(meristem.chain, metamer)
    };
    meristem.tip = metamer.position;
    meristem.length += unit.internode;
    meristem.nodes += 1;
    node
}

/// Starts the chain of a resting meristem with its first metamer.
fn start_chain(
    plant: &mut Plant,
    meristem: &mut Meristem,
    id: MeristemId,
    metamer: Metamer,
) -> NodeId {
    let graph = &mut plant.graph;
    let node = match meristem.kind {
        SiteKind::Seed => {
            let node = graph.add_root_chain(meristem.tip, metamer);
            meristem.bud = graph.add_bud(node, BudKind::Terminal, meristem.heading);
            graph.set_bud_state(meristem.bud, BudState::Active);
            node
        }
        SiteKind::Lateral => graph.sprout(meristem.bud, ChainKind::Lateral, metamer),
        SiteKind::Relay => graph.sprout(meristem.bud, ChainKind::Relay, metamer),
    };
    meristem.chain = graph.chain_of(node);
    meristem.state = MeristemState::Growing;
    debug_assert_eq!(meristem.chain.index(), plant.chain_meristems.len());
    plant.chain_meristems.push(id);
    node
}

/// Moves the leaf sites that are retained into the graph. `remap` is the node id mapping
/// of a compaction done since the nodes were built.
///
/// With `r` the `retention_cycles` of the species, a node keeps its leaves when it was
/// born in the last `r` cycles of the plant, or when it belongs to the last `r` shoots
/// of a meristem whose axis is finished: the tip of a full-grown axis stays leafy, as
/// if it kept producing short shoots.
pub(crate) fn retain_leaves(plant: &mut Plant, cx: &Context, remap: Option<&[NodeId]>) {
    let retention = u32::from(cx.species.foliage.retention_cycles);
    let leafy = std::mem::take(&mut plant.leafy);
    for leaf in &leafy {
        let meristem = plant.meristem(leaf.meristem);
        let recent = u32::from(leaf.birth) + retention >= u32::from(cx.age);
        let terminal = meristem.state == MeristemState::Done
            && u32::from(leaf.shoot) + retention >= u32::from(meristem.shoots);
        if recent || terminal {
            let node = remap.map_or(leaf.node, |remap| remap[leaf.node.index()]);
            add_leaves(plant, cx, leaf, (node, meristem.order));
        }
    }
}

/// Adds the leaf sites of one node of an axis of `order` to the graph.
///
/// A leaf points away from the axis at the inclination of the species. Its normal lies
/// in the plane of the axis and the leaf and faces the tip of the axis; on a
/// plagiotropic axis it faces up instead.
fn add_leaves(plant: &mut Plant, cx: &Context, leaf: &LeafyNode, (node, order): (NodeId, u8)) {
    let foliage = &cx.species.foliage;
    let frame = &leaf.frame;
    let phyllotaxis = cx.species.architecture.phyllotaxis_of(order);
    let count = u32::from(phyllotaxis.leaves_per_node);
    let scale = foliage.size_along_axis.eval(leaf.position);
    let (sin, cos) = foliage.inclination_deg.to_radians().sin_cos();
    for slot in 0..count {
        let outward = frame.radial(slot, count);
        let direction = (frame.heading * cos + outward * sin).normalize();
        let adaxial = frame.heading * sin - outward * cos;
        let normal = if frame.plagiotropic {
            perpendicular(direction, UP)
        } else {
            perpendicular(direction, adaxial)
        };
        plant.graph.add_organ(OrganSite {
            node,
            kind: OrganKind::Leaf,
            direction,
            normal,
            scale,
            birth: leaf.birth,
        });
    }
}

/// Offers the axillary buds of a new node to the fate stage and creates the admitted
/// ones.
///
/// The buds follow the leaves, except on the last node of a growth unit with a `whorl`
/// of at least one bud, where that many buds are spread evenly. On the last node of a
/// sympodial shoot the first position is left to the relay.
fn add_lateral_buds<F: BudFate>(
    plant: &mut Plant,
    cx: &Context,
    fate: &mut F,
    unit: &Unit,
    frame: &NodeFrame,
    (index, node): (u16, NodeId),
) {
    let meristem = plant.meristems[unit.id.index()];
    let architecture = &cx.species.architecture;
    let order = meristem.order;
    if order >= architecture.max_order || architecture.axis(order).short {
        return;
    }
    let position = meristem.position();
    let last = index + 1 == unit.metamers;
    let rhythmic = architecture.growth_rhythm == GrowthRhythm::Rhythmic;
    let whorl = if last && rhythmic {
        cx.species.growth.whorl.eval(order, position).round() as u32
    } else {
        0
    };
    let leaves = u32::from(architecture.phyllotaxis_of(order).leaves_per_node);
    let count = if whorl >= 1 { whorl } else { leaves };
    let relayed = last && architecture.branching == Branching::Sympodial;
    for slot in u32::from(relayed)..count {
        let site = BudSite {
            id: plant.next_meristem(),
            key: child_key(meristem.key, SiteKind::Lateral, meristem.nodes - 1, slot),
            kind: SiteKind::Lateral,
            order: order + 1,
            parent: Some(unit.id),
            node_index: meristem.nodes - 1,
            slot,
            position,
            arc: meristem.length,
            unit_position: f32::from(index + 1) / f32::from(unit.metamers),
            cycle: unit.cycle,
        };
        if let Some(target) = fate.admit(plant, &site) {
            let outward = frame.radial(slot, count);
            add_lateral(plant, cx, &site, (node, outward), target);
        }
    }
}

/// Creates the bud and the resting meristem of an admitted lateral site.
fn add_lateral(
    plant: &mut Plant,
    cx: &Context,
    site: &BudSite,
    (node, outward): (NodeId, Vec3),
    target: f32,
) {
    let growth = &cx.species.growth;
    let parent = site.parent.unwrap_or(site.id);
    let bearing = plant.meristems[parent.index()];
    let mut rng = Rng::new(site.key, 0, Stream::BRANCH_ANGLE);
    let variation = growth
        .branching_angle_variation
        .eval(site.order, site.position);
    let angle = growth.branching_angle.eval(site.order, site.position) + variation * rng.signed();
    let heading = tilt(bearing.heading, outward, angle.to_radians()).normalize();
    let orientation = cx.species.architecture.axis(site.order).orientation;
    let phase = if orientation == AxisOrientation::Orthotropic {
        rng.angle()
    } else {
        0.0
    };
    let bud = plant.graph.add_bud(node, BudKind::Lateral, heading);
    plant.meristems.push(Meristem {
        key: site.key,
        kind: SiteKind::Lateral,
        state: MeristemState::Resting,
        order: site.order,
        born: site.cycle,
        bud,
        chain: ChainId::NONE,
        tip: plant.graph.position(node),
        heading,
        side: perpendicular(heading, bearing.heading),
        phase,
        nodes: 0,
        shoots: 0,
        start: 0.0,
        length: 0.0,
        target,
        attach_position: site.position,
    });
}

/// Ends the shoot of `flush`: closes the apex after its last shoot, starts the relay of
/// a sympodial axis, or tests for a fork.
fn end_shoot<F: BudFate>(
    plant: &mut Plant,
    cx: &Context,
    fate: &mut F,
    meristem: &mut Meristem,
    flush: &Flush,
    cycle: u16,
) {
    let id = flush.meristem;
    if flush.last {
        close(plant, meristem);
        return;
    }
    if flush.metamers == 0 {
        return;
    }
    let growth = &cx.species.growth;
    let (order, position) = (meristem.order, meristem.position());
    let frame = node_frame(cx, meristem);
    if cx.species.architecture.branching == Branching::Sympodial {
        let angle = growth.relay_angle.eval(order, position).to_radians();
        let heading = tilt(meristem.heading, frame.radial(0, 1), angle).normalize();
        let divergence = cx.species.architecture.phyllotaxis_of(order).divergence_deg;
        let phase = frame.azimuth + divergence.to_radians();
        let relayed = add_relay(plant, fate, (id, meristem), (heading, phase), cycle);
        close(plant, meristem);
        if relayed {
            meristem.state = MeristemState::Relayed;
        }
        return;
    }
    let probability = growth.fork_probability.eval(order, position);
    if probability <= 0.0 {
        return;
    }
    let mut rng = meristem.rng(FORK, u64::from(meristem.nodes) << 8);
    if rng.chance(probability) {
        let angle = growth.fork_angle.eval(order, position).to_radians();
        let outward = frame.radial_at(rng.angle());
        let heading = tilt(meristem.heading, outward, angle).normalize();
        let phase = rng.angle();
        if add_relay(plant, fate, (id, meristem), (heading, phase), cycle) {
            meristem.heading = tilt(meristem.heading, -outward, angle).normalize();
            meristem.side = perpendicular(meristem.heading, meristem.side);
        }
    }
}

/// Stops the apex of `meristem` for good.
fn close(plant: &mut Plant, meristem: &mut Meristem) {
    meristem.state = MeristemState::Done;
    if meristem.bud.is_some() {
        plant.graph.set_bud_state(meristem.bud, BudState::Dead);
    }
}

/// Offers a relay bud at the tip of `meristem` to the fate stage and creates it when
/// admitted. The relay continues the arc length of the axis; `phase` is the azimuth of
/// the leaves of its first node.
fn add_relay<F: BudFate>(
    plant: &mut Plant,
    fate: &mut F,
    (id, meristem): (MeristemId, &Meristem),
    (heading, phase): (Vec3, f32),
    cycle: u16,
) -> bool {
    if meristem.chain.is_none() {
        return false;
    }
    let node_index = meristem.nodes.saturating_sub(1);
    let site = BudSite {
        id: plant.next_meristem(),
        key: child_key(meristem.key, SiteKind::Relay, node_index, 0),
        kind: SiteKind::Relay,
        order: meristem.order,
        parent: Some(id),
        node_index,
        slot: 0,
        position: meristem.position(),
        arc: meristem.length,
        unit_position: 1.0,
        cycle,
    };
    let Some(target) = fate.admit(plant, &site) else {
        return false;
    };
    let tip = plant.graph.chain(meristem.chain).last;
    let bud = plant.graph.add_bud(tip, BudKind::Lateral, heading);
    plant.meristems.push(Meristem {
        key: site.key,
        kind: SiteKind::Relay,
        state: MeristemState::Resting,
        order: meristem.order,
        born: cycle,
        bud,
        chain: ChainId::NONE,
        tip: meristem.tip,
        heading,
        side: perpendicular(heading, meristem.side),
        phase,
        nodes: 0,
        shoots: 0,
        start: meristem.length,
        length: meristem.length,
        target,
        attach_position: meristem.attach_position,
    });
    true
}
