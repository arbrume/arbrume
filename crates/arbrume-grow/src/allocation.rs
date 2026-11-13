//! Bud fate of the self-organizing mode: a resource shared between buds according to
//! the light they receive (Palubicki et al. 2009, section 4.2).
//!
//! # The two passes
//!
//! Each cycle, before any shoot is built, [`ResourceFate`] works on the tree of
//! meristems (one per axis, resting buds included):
//!
//! 1. **basipetal**: the exposure `Q` of every bud that can still grow is summed from
//!    the tips to the base, so each axis knows the light gathered by all it bears;
//! 2. **acropetal**: each stem receives `v_base = alpha * Q_base`, and the resource
//!    flows up, split at every axis between its own terminal bud and the axes it bears.
//!
//! The split is one of two models.
//!
//! * **Extended Borchert-Honda**: the branching points of an axis are taken from its
//!   base to its tip. At each one the resource `v` still in the axis is divided between
//!   the lateral and the rest of the axis ([`borchert_honda`]):
//!   `v_m = v * lambda * Q_m / (lambda * Q_m + (1 - lambda) * Q_l)` and
//!   `v_l = v - v_m`, where `Q_l` is the light of the lateral branch, `Q_m` that of the
//!   axis above the point and `lambda` the `axis_bias` of the species at that point.
//! * **Priority**: the axes borne by an axis and its terminal bud are ranked by the
//!   light they gather per bud, the terminal bud first where the species has apical
//!   control. Item `i` then gets `v * Q_i * w_i / sum(Q_j * w_j)`, the weight `w`
//!   falling linearly with the rank from `priority_max` to `priority_min` over the
//!   fraction `priority_knee` of the list ([`priority_weight`]). The paper shows the
//!   weight function only as a figure; this linear ramp is how this crate reads it.
//!
//! # Shoots
//!
//! A bud that receives `v >= 1` flushes `n = floor(v)` metamers, each `v / n` times the
//! `internode_length` of the species at that place. The gradient curves keep their
//! role as limits: a shoot is at most `shoot_length` long (the metamers above that are
//! not built), an axis stops at its `axis_length`, and buds form with
//! `branch_probability`. A shoot is cut where it would leave the envelope or enter an
//! obstacle of the surroundings, and a bud that cannot take one step is left out of
//! both passes, so that its share goes to the buds that have room: the response to
//! clipping of Prusinkiewicz et al. (1994). The ground the plant stands on blocks in the
//! same way a shoot heading down into it. Buds flush the cycle after they form.
//!
//! The stages do not look at the age of the run, so the plant of a shorter run is the
//! prefix of a longer one: same metamers, born in the same cycles.

use crate::parametric::{lateral_length, seed_length};
use crate::plant::{Meristem, MeristemId, MeristemState, Plant, SiteKind};
use crate::stage::{BudFate, BudSite, Context, Flush};
use crate::surroundings::Surroundings;
use arbrume_core::species::{Allocation, SelfOrganization};
use glam::Vec3;

/// Shares `v` between the continuation of an axis, which gathers the light `q_main`,
/// and a lateral, which gathers `q_lateral`, with the bias `lambda`: the extended
/// Borchert-Honda rule. Returns `(v_main, v_lateral)`, which always sum to `v`. Without
/// any light everything stays in the axis.
#[must_use]
pub fn borchert_honda(v: f32, lambda: f32, q_main: f32, q_lateral: f32) -> (f32, f32) {
    let lateral = (1.0 - lambda) * q_lateral;
    let total = lambda * q_main + lateral;
    if total <= 0.0 {
        return (v, 0.0);
    }
    let to_lateral = v * (lateral / total);
    (v - to_lateral, to_lateral)
}

/// Weight of the item of rank `rank` (0 first) in a priority list of `count` items: a
/// linear fall from `priority_max` at the head of the list to `priority_min` at the
/// fraction `priority_knee` of its length, constant after.
#[must_use]
pub fn priority_weight(model: &SelfOrganization, rank: usize, count: usize) -> f32 {
    let fraction = if count > 1 {
        rank as f32 / (count - 1) as f32
    } else {
        0.0
    };
    let fall = (fraction / model.priority_knee).min(1.0);
    model.priority_max - (model.priority_max - model.priority_min) * fall
}

/// One entry of a priority list.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Ranked {
    /// Light per bud, the sorting key.
    mean: f32,
    /// Light gathered.
    light: f32,
    /// Meristem that receives the share.
    meristem: u32,
}

/// Bud fate of the self-organizing mode. See the [module documentation](self).
#[derive(Debug, Clone)]
pub struct ResourceFate<'a> {
    cx: Context<'a>,
    surroundings: &'a Surroundings,
    origin: Vec3,
    /// Light gathered by each meristem and all it bears.
    light: Vec<f32>,
    /// Number of buds that can grow on each meristem and all it bears.
    buds: Vec<u32>,
    /// Resource flowing into each meristem, then the share of its own bud.
    resource: Vec<f32>,
    /// Start of the children of each meristem in `children`.
    child_start: Vec<u32>,
    children: Vec<u32>,
    ranked: Vec<Ranked>,
}

impl<'a> ResourceFate<'a> {
    /// Fate for one run in `surroundings`.
    #[must_use]
    pub const fn new(cx: Context<'a>, surroundings: &'a Surroundings) -> Self {
        Self {
            cx,
            surroundings,
            origin: Vec3::ZERO,
            light: Vec::new(),
            buds: Vec::new(),
            resource: Vec::new(),
            child_start: Vec::new(),
            children: Vec::new(),
            ranked: Vec::new(),
        }
    }

    /// The same fate for a plant standing at `origin` in the frame of the surroundings.
    #[must_use]
    pub const fn at(mut self, origin: Vec3) -> Self {
        self.origin = origin;
        self
    }

    /// Resource `v` the bud of each meristem received in the last cycle, indexed by
    /// meristem id.
    #[must_use]
    pub fn resources(&self) -> &[f32] {
        &self.resource
    }

    /// Light gathered in the last cycle by each meristem and all it bears.
    #[must_use]
    pub fn gathered(&self) -> &[f32] {
        &self.light
    }

    /// Whether the first internode of the next shoot of `meristem` is blocked by the
    /// surroundings.
    fn held_back(&self, meristem: &Meristem) -> bool {
        if self.surroundings.is_empty() && meristem.heading.y >= 0.0 {
            return false;
        }
        let internodes = &self.cx.species.growth.internode_length;
        let step = internodes.eval(meristem.order, meristem.position()) * self.cx.scale;
        let start = meristem.tip + self.origin;
        self.blocks(meristem, start, start + meristem.heading * step)
    }

    /// Whether `meristem` may not step from `from` to `to`, both in the frame of the
    /// surroundings: the surroundings block the step, or it goes down into the ground
    /// the plant stands on.
    fn blocks(&self, meristem: &Meristem, from: Vec3, to: Vec3) -> bool {
        let ground = self.origin.y;
        let digs = to.y < ground && meristem.heading.y < 0.0;
        digs || self.surroundings.blocks_step(from, to)
    }

    /// Sums the exposure and the number of the buds that can grow, tips to base. A bud
    /// held back by the surroundings counts for nothing, so its share goes to the others.
    fn gather(&mut self, plant: &Plant) {
        let count = plant.meristems.len();
        let exposures = plant.graph.bud_exposures();
        let full = self.cx.species.growth.self_organization.full_exposure;
        self.light.clear();
        self.buds.clear();
        for meristem in &plant.meristems {
            let grows = can_grow(meristem) && !self.held_back(meristem);
            let exposure = if meristem.bud.is_some() {
                exposures[meristem.bud.index()]
            } else {
                full
            };
            self.light.push(if grows { exposure } else { 0.0 });
            self.buds.push(u32::from(grows));
        }
        for id in (0..count).rev() {
            if let Some(parent) = plant.meristems[id].parent {
                self.light[parent.index()] += self.light[id];
                self.buds[parent.index()] += self.buds[id];
            }
        }
    }

    /// Lists the meristems borne by each meristem, in the order they formed, which is
    /// from the base of the bearing axis to its tip.
    fn index_children(&mut self, plant: &Plant) {
        let count = plant.meristems.len();
        self.child_start.clear();
        self.child_start.resize(count + 2, 0);
        for meristem in &plant.meristems {
            if let Some(parent) = meristem.parent {
                self.child_start[parent.index() + 2] += 1;
            }
        }
        for id in 0..count {
            self.child_start[id + 2] += self.child_start[id + 1];
        }
        self.children.clear();
        self.children
            .resize(self.child_start[count + 1] as usize, 0);
        for (id, meristem) in plant.meristems.iter().enumerate() {
            if let Some(parent) = meristem.parent {
                let slot = &mut self.child_start[parent.index() + 1];
                self.children[*slot as usize] = id as u32;
                *slot += 1;
            }
        }
    }

    /// Borchert-Honda split of the resource of meristem `id` along its axis.
    fn split_along(&mut self, plant: &Plant, id: usize) {
        let model = &self.cx.species.growth.self_organization;
        let order = plant.meristems[id].order;
        let range = self.child_start[id] as usize..self.child_start[id + 1] as usize;
        let mut resource = self.resource[id];
        let mut above = self.light[id];
        for slot in range {
            let child = self.children[slot] as usize;
            let lateral = self.light[child];
            above = (above - lateral).max(0.0);
            let lambda = model.axis_bias.eval(order, branch_position(plant, child));
            let (main, share) = borchert_honda(resource, lambda, above, lateral);
            self.resource[child] = share;
            resource = main;
        }
        self.resource[id] = resource;
    }

    /// Priority split of the resource of meristem `id` between its terminal bud and the
    /// axes it bears.
    fn split_by_rank(&mut self, plant: &Plant, id: usize) {
        let model = &self.cx.species.growth.self_organization;
        let meristem = &plant.meristems[id];
        let range = self.child_start[id] as usize..self.child_start[id + 1] as usize;
        let resource = self.resource[id];
        self.ranked.clear();
        let mut own = self.light[id];
        for &child in &self.children[range] {
            let light = self.light[child as usize];
            own -= light;
            self.resource[child as usize] = 0.0;
            self.ranked.push(Ranked {
                mean: light / self.buds[child as usize].max(1) as f32,
                light,
                meristem: child,
            });
        }
        let control = model
            .apical_control
            .eval(meristem.order, meristem.position())
            >= 0.5;
        let terminal = Ranked {
            mean: if control { f32::INFINITY } else { own.max(0.0) },
            light: own.max(0.0),
            meristem: id as u32,
        };
        self.resource[id] = 0.0;
        if can_grow(meristem) {
            self.ranked.push(terminal);
        }
        self.ranked
            .sort_unstable_by(|a, b| b.mean.total_cmp(&a.mean).then(a.meristem.cmp(&b.meristem)));
        let count = self.ranked.len();
        let weight = |rank: usize| priority_weight(model, rank, count);
        let weighted = |(rank, item): (usize, &Ranked)| item.light * weight(rank);
        let total: f32 = self.ranked.iter().enumerate().map(weighted).sum();
        if total <= 0.0 {
            return;
        }
        for (rank, item) in self.ranked.iter().enumerate() {
            self.resource[item.meristem as usize] = resource * (item.light * weight(rank) / total);
        }
    }

    /// Gives every stem its resource and shares it between all buds, base to tips.
    fn allocate(&mut self, plant: &Plant) {
        let model = &self.cx.species.growth.self_organization;
        let count = plant.meristems.len();
        self.resource.clear();
        self.resource.resize(count, 0.0);
        for id in 0..count {
            if plant.meristems[id].parent.is_none() {
                self.resource[id] = model.resource_coefficient * self.light[id];
            }
            match model.allocation {
                Allocation::BorchertHonda => self.split_along(plant, id),
                Allocation::Priority => self.split_by_rank(plant, id),
            }
        }
    }

    /// Shoot of `meristem` for the resource `v`, `None` when it does not grow.
    fn shoot(&self, id: MeristemId, meristem: &Meristem, v: f32) -> Option<Flush> {
        if !can_grow(meristem) || v < 1.0 {
            return None;
        }
        let growth = &self.cx.species.growth;
        let (order, position) = (meristem.order, meristem.position());
        let internode = (v / v.floor()) * growth.internode_length.eval(order, position);
        let cap = (growth.shoot_length.eval(order, position) / internode).round();
        let mut metamers = v.floor().min(cap.max(1.0)).min(f32::from(u16::MAX)) as u16;
        let remaining = meristem.target - meristem.length;
        let mut last = false;
        if f32::from(metamers) * internode >= remaining {
            metamers = (remaining / internode).round().max(0.0) as u16;
            last = true;
        }
        let step = internode * self.cx.scale;
        let start = meristem.tip + self.origin;
        let point = |k: u16| start + meristem.heading * (f32::from(k) * step);
        let free = (1..=metamers)
            .take_while(|&k| !self.blocks(meristem, point(k - 1), point(k)))
            .count() as u16;
        if free < metamers {
            (metamers, last) = (free, false);
        }
        let length = if last {
            remaining.max(0.0)
        } else {
            f32::from(metamers) * internode
        };
        (metamers > 0 || last).then_some(Flush {
            meristem: id,
            metamers,
            length,
            last,
        })
    }
}

/// Whether the bud of `meristem` can still flush.
fn can_grow(meristem: &Meristem) -> bool {
    matches!(
        meristem.state,
        MeristemState::Resting | MeristemState::Growing
    )
}

/// Relative position along its bearing axis of the point where meristem `child` is
/// attached. A relay starts where the axis it continues stopped.
fn branch_position(plant: &Plant, child: usize) -> f32 {
    let meristem = &plant.meristems[child];
    if meristem.kind == SiteKind::Relay && meristem.target > 0.0 {
        (meristem.start / meristem.target).min(1.0)
    } else {
        meristem.attach_position
    }
}

impl BudFate for ResourceFate<'_> {
    fn admit(&mut self, plant: &Plant, site: &BudSite) -> Option<f32> {
        match (site.kind, site.parent) {
            (SiteKind::Lateral, Some(parent)) => {
                lateral_length(&self.cx, plant.meristem(parent), site)
            }
            (SiteKind::Relay, Some(parent)) => Some(plant.meristem(parent).target),
            _ => seed_length(&self.cx, site),
        }
    }

    fn decide(&mut self, plant: &Plant, _cycle: u16, from: MeristemId, flushes: &mut Vec<Flush>) {
        if from.index() != 0 {
            return;
        }
        self.gather(plant);
        self.index_children(plant);
        self.allocate(plant);
        for (index, meristem) in plant.meristems.iter().enumerate() {
            let id = MeristemId(index as u32);
            flushes.extend(self.shoot(id, meristem, self.resource[index]));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{borchert_honda, priority_weight};
    use arbrume_core::species::SelfOrganization;

    #[test]
    fn the_split_conserves_the_resource_and_follows_the_published_rule() {
        let cases = [
            (3.0, 0.5, 2.0, 1.0),
            (7.5, 0.46, 0.3, 4.0),
            (1.0, 0.54, 5.0, 5.0),
        ];
        for (v, lambda, q_main, q_lateral) in cases {
            let (main, lateral) = borchert_honda(v, lambda, q_main, q_lateral);
            assert!((main + lateral - v).abs() <= f32::EPSILON * v);
            let total = lambda * q_main + (1.0 - lambda) * q_lateral;
            assert!((main - v * lambda * q_main / total).abs() < 1e-5);
            assert!((lateral - v * (1.0 - lambda) * q_lateral / total).abs() < 1e-5);
        }
        assert_eq!(borchert_honda(2.0, 0.5, 0.0, 0.0), (2.0, 0.0));
        assert_eq!(borchert_honda(2.0, 0.5, 0.0, 3.0), (0.0, 2.0));
    }

    #[test]
    fn the_bias_moves_the_resource_towards_the_main_axis() {
        let neutral = borchert_honda(4.0, 0.5, 1.0, 1.0);
        assert!((neutral.0 - 2.0).abs() < 1e-6 && (neutral.1 - 2.0).abs() < 1e-6);
        let mut previous = 0.0;
        for lambda in [0.1, 0.46, 0.5, 0.54, 0.9] {
            let (main, _) = borchert_honda(4.0, lambda, 1.0, 1.0);
            assert!(main > previous);
            previous = main;
        }
        assert_eq!(borchert_honda(4.0, 1.0, 1.0, 1.0), (4.0, 0.0));
        assert_eq!(borchert_honda(4.0, 0.0, 1.0, 1.0), (0.0, 4.0));
    }

    #[test]
    fn priority_weights_fall_to_the_knee_then_stay() {
        let model = SelfOrganization {
            priority_knee: 0.5,
            ..SelfOrganization::default()
        };
        let weights: Vec<f32> = (0..9)
            .map(|rank| priority_weight(&model, rank, 9))
            .collect();
        assert!((weights[0] - 1.0).abs() < 1e-6);
        assert!(weights.windows(2).take(4).all(|pair| pair[1] < pair[0]));
        assert!((weights[2] - (1.0 - 0.994 * 0.5)).abs() < 1e-6);
        assert!(weights[4..].iter().all(|w| (w - 0.006).abs() < 1e-6));
        assert!((priority_weight(&model, 0, 1) - 1.0).abs() < 1e-6);
    }
}

#[cfg(test)]
mod plant_tests {
    use super::ResourceFate;
    use crate::plant::{Meristem, MeristemId, MeristemState, Plant, SiteKind};
    use crate::stage::Context;
    use crate::surroundings::Surroundings;
    use arbrume_core::curve::OrderCurve;
    use arbrume_core::graph::{BudId, BudKind, ChainId, Metamer};
    use arbrume_core::species::{Allocation, Species};
    use glam::Vec3;

    /// A meristem of the test plant.
    fn meristem(parent: Option<u32>, bud: BudId, state: MeristemState) -> Meristem {
        Meristem {
            key: 0,
            kind: if parent.is_some() {
                SiteKind::Lateral
            } else {
                SiteKind::Seed
            },
            parent: parent.map(MeristemId),
            state,
            order: u8::from(parent.is_some()),
            born: 0,
            bud,
            chain: ChainId::NONE,
            tip: Vec3::ZERO,
            heading: Vec3::Y,
            side: Vec3::X,
            phase: 0.0,
            nodes: 0,
            shoots: 0,
            forks: 0,
            start: 0.0,
            length: 0.0,
            target: 10.0,
            attach_position: 0.5,
        }
    }

    /// A trunk whose terminal bud has the exposure `exposures[0]`, bearing one resting
    /// lateral bud for each other exposure, from its base to its tip.
    fn plant(exposures: &[f32]) -> Plant {
        let mut plant = Plant::new();
        let first = Metamer::new(Vec3::Y, 0.0, 0);
        let node = plant.graph.add_root_chain(Vec3::ZERO, first);
        let terminal = plant.graph.add_bud(node, BudKind::Terminal, Vec3::Y);
        let trunk = meristem(None, terminal, MeristemState::Growing);
        plant.meristems.push(trunk);
        for _ in 1..exposures.len() {
            let bud = plant.graph.add_bud(node, BudKind::Lateral, Vec3::X);
            let lateral = meristem(Some(0), bud, MeristemState::Resting);
            plant.meristems.push(lateral);
        }
        plant.graph.buds_mut().exposure.copy_from_slice(exposures);
        plant
    }

    /// Resource of each bud of `plant` under `species`.
    fn shares(species: &Species, plant: &Plant) -> Vec<f32> {
        let cx = Context {
            species,
            seed: 0,
            age: 1,
            scale: 1.0,
        };
        let open = Surroundings::none();
        let mut fate = ResourceFate::new(cx, &open);
        fate.gather(plant);
        fate.index_children(plant);
        fate.allocate(plant);
        fate.resources().to_vec()
    }

    /// Resource per unit of exposure of each bud.
    fn per_light(shares: &[f32], exposures: &[f32]) -> Vec<f32> {
        let ratio = |(share, exposure): (&f32, &f32)| share / exposure;
        shares.iter().zip(exposures).map(ratio).collect()
    }

    #[test]
    fn borchert_honda_conserves_the_resource_of_the_base() {
        let mut species = Species::default();
        let exposures = [0.9, 0.2, 0.7, 1.0, 0.4];
        let total: f32 = exposures.iter().sum();
        for lambda in [0.3, 0.5, 0.56] {
            species.growth.self_organization.axis_bias = OrderCurve::constant(lambda);
            let shares = shares(&species, &plant(&exposures));
            let sum: f32 = shares.iter().sum();
            assert!((sum - 2.0 * total).abs() < 1e-4, "{lambda}: {sum}");
            if (lambda - 0.5_f32).abs() < 1e-6 {
                for (share, exposure) in shares.iter().zip(&exposures) {
                    assert!((share - 2.0 * exposure).abs() < 1e-4);
                }
            }
        }
    }

    #[test]
    fn the_axis_bias_feeds_the_leader() {
        let mut species = Species::default();
        let exposures = [1.0, 1.0, 1.0, 1.0];
        let mut leader = Vec::new();
        for lambda in [0.4, 0.5, 0.6] {
            species.growth.self_organization.axis_bias = OrderCurve::constant(lambda);
            leader.push(shares(&species, &plant(&exposures))[0]);
        }
        assert!(leader[0] < leader[1] && leader[1] < leader[2]);
        assert!((leader[1] - 2.0).abs() < 1e-4);
    }

    #[test]
    fn priority_ranks_by_light_and_puts_the_leader_first() {
        let mut species = Species::default();
        let model = &mut species.growth.self_organization;
        model.allocation = Allocation::Priority;
        model.priority_min = 0.1;
        model.priority_knee = 1.0;
        let exposures = [0.3, 0.5, 1.0, 0.8];
        let total: f32 = exposures.iter().sum();
        let with_control = shares(&species, &plant(&exposures));
        let sum: f32 = with_control.iter().sum();
        assert!((sum - 2.0 * total).abs() < 1e-4);
        // Ranks: the leader (bud 0), then by exposure the buds 2, 3 and 1.
        let ratio = per_light(&with_control, &exposures);
        assert!(ratio[0] > ratio[2] && ratio[2] > ratio[3] && ratio[3] > ratio[1]);
        species.growth.self_organization.apical_control = OrderCurve::constant(0.0);
        let without = shares(&species, &plant(&exposures));
        // Without apical control the dim leader comes last: 2, 3, 1, 0.
        let ratio = per_light(&without, &exposures);
        assert!(ratio[2] > ratio[3] && ratio[3] > ratio[1] && ratio[1] > ratio[0]);
        assert!(without[0] < with_control[0]);
    }

    #[test]
    fn dead_and_dark_buds_receive_nothing() {
        let species = Species::default();
        let mut plant = plant(&[1.0, 0.0, 1.0]);
        plant.meristems[2].state = MeristemState::Done;
        let shares = shares(&species, &plant);
        assert!((shares[0] - 2.0).abs() < 1e-5);
        assert!(shares[1].abs() < 1e-9 && shares[2].abs() < 1e-9);
    }
}
