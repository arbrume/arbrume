//! Shedding of the branches that gather too little light, and death of old buds.
//!
//! Following Palubicki et al. (2009, section 4.4), after Takenaka (1994) and Mech and
//! Prusinkiewicz (1996): the light gathered by a branch (the exposures `Q` of the buds
//! it bears that can still grow, as sensed at the start of the cycle) is divided by its
//! size, the number of its internodes at the start of the cycle. Below the
//! `shedding_threshold` of the species the branch is shed with all it bears. A branch
//! is an axis attached to another one; stems are never shed.
//!
//! Shed metamers stay in the graph, flagged, with the cycle of their shedding, so the
//! pipe model keeps the girth they gave to the axes below, and the plant can be shown
//! as it was before.
//!
//! The stage also ends the lateral buds that stayed dormant for the `bud_lifespan` of
//! the species.

use crate::plant::{Meristem, MeristemState, Plant, SiteKind};
use crate::stage::{Context, Shedding};
use arbrume_core::graph::{BudState, NodeId};

/// Shedding by exposure. See the [module documentation](self).
#[derive(Debug, Clone)]
pub struct ExposureShedding {
    threshold: f32,
    lifespan: u16,
    /// Internodes of each meristem at the start of the cycle.
    sizes: Vec<u32>,
    light: Vec<f32>,
    size: Vec<u32>,
    roots: Vec<NodeId>,
}

impl ExposureShedding {
    /// Shedding with the threshold and the bud lifespan of the species of `cx`.
    #[must_use]
    pub fn new(cx: &Context) -> Self {
        let model = &cx.species.growth.self_organization;
        Self::with(model.shedding_threshold, model.bud_lifespan)
    }

    /// Shedding below `threshold` of exposure per internode (0 for none), dormant buds
    /// dying after `lifespan` cycles (0 for never).
    #[must_use]
    pub const fn with(threshold: f32, lifespan: u16) -> Self {
        Self {
            threshold,
            lifespan,
            sizes: Vec::new(),
            light: Vec::new(),
            size: Vec::new(),
            roots: Vec::new(),
        }
    }

    /// Sums light and size over each branch and lists the first node of the branches to
    /// shed in `roots`. A branch under a branch that is shed is not listed.
    fn select(&mut self, plant: &Plant) {
        let count = plant.meristems.len();
        let exposures = plant.graph.bud_exposures();
        self.sizes.resize(count, 0);
        self.light.clear();
        self.size.clear();
        for (meristem, &size) in plant.meristems.iter().zip(&self.sizes) {
            let light = if alive(meristem) && meristem.bud.is_some() {
                exposures[meristem.bud.index()]
            } else {
                0.0
            };
            self.light.push(light);
            self.size.push(size);
        }
        for id in (0..count).rev() {
            if let Some(parent) = plant.meristems[id].parent {
                self.light[parent.index()] += self.light[id];
                self.size[parent.index()] += self.size[id];
            }
        }
        self.roots.clear();
        for (id, meristem) in plant.meristems.iter().enumerate() {
            let Some(parent) = meristem.parent else {
                continue;
            };
            if self.size[parent.index()] == u32::MAX {
                self.size[id] = u32::MAX;
                continue;
            }
            let size = self.size[id];
            let starved = size > 0 && self.light[id] < self.threshold * size as f32;
            if starved && meristem.chain.is_some() {
                self.roots.push(plant.graph.chain(meristem.chain).first);
                self.size[id] = u32::MAX;
            }
        }
    }
}

/// Whether the apex of `meristem` can still grow.
fn alive(meristem: &Meristem) -> bool {
    matches!(
        meristem.state,
        MeristemState::Resting | MeristemState::Growing
    )
}

impl Shedding for ExposureShedding {
    fn shed(&mut self, plant: &mut Plant, cycle: u16) {
        if self.threshold > 0.0 {
            self.select(plant);
            plant.graph.shed_subtrees(&self.roots, cycle);
        }
        let Plant {
            graph, meristems, ..
        } = plant;
        let states = graph.buds_mut().states;
        for meristem in meristems.iter_mut().filter(|meristem| alive(meristem)) {
            let dormant = meristem.state == MeristemState::Resting
                && meristem.kind == SiteKind::Lateral
                && self.lifespan > 0
                && cycle.saturating_sub(meristem.born) >= self.lifespan;
            let dead = meristem.bud.is_some() && states[meristem.bud.index()] == BudState::Dead;
            if dormant || dead {
                meristem.state = MeristemState::Done;
                if meristem.bud.is_some() {
                    states[meristem.bud.index()] = BudState::Dead;
                }
            }
        }
        self.sizes.clear();
        self.sizes.extend(meristems.iter().map(|m| m.nodes));
    }
}

#[cfg(test)]
mod tests {
    use super::ExposureShedding;
    use crate::plant::{Meristem, MeristemId, MeristemState, Plant, SiteKind};
    use crate::stage::Shedding;
    use arbrume_core::graph::{BudId, BudKind, BudState, ChainId, ChainKind, Metamer, NodeId};
    use glam::Vec3;

    fn meristem(parent: Option<u32>, state: MeristemState) -> Meristem {
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
            bud: BudId::NONE,
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

    /// A trunk of four nodes bearing two branches of `size` nodes on its nodes 1 and 2,
    /// and a dormant bud on node 2. Exposures: trunk, first branch, second branch, bud.
    fn plant(size: usize, exposures: [f32; 4]) -> Plant {
        let mut plant = Plant::new();
        let graph = &mut plant.graph;
        let base = graph.add_root_chain(Vec3::ZERO, Metamer::new(Vec3::Y, 0.0, 0));
        let trunk = graph.chain_of(base);
        let mut trunk_nodes = vec![base];
        for height in 2..=4_u8 {
            let position = Vec3::Y * f32::from(height);
            trunk_nodes.push(graph.extend_chain(trunk, Metamer::new(position, 0.0, 0)));
        }
        let mut seed = meristem(None, MeristemState::Growing);
        seed.bud = graph.add_bud(trunk_nodes[3], BudKind::Terminal, Vec3::Y);
        (seed.chain, seed.nodes) = (trunk, 4);
        plant.meristems.push(seed);
        for (side, attach) in [(1.0, trunk_nodes[1]), (-1.0, trunk_nodes[2])] {
            let start = graph.position(attach);
            let first = Metamer::new(start + Vec3::X * side, 0.0, 0);
            let node = graph.add_chain(attach, ChainKind::Lateral, first);
            let chain = graph.chain_of(node);
            let mut tip = node;
            for step in 2..=size {
                let position = start + Vec3::X * (side * step as f32);
                tip = graph.extend_chain(chain, Metamer::new(position, 0.0, 0));
            }
            let mut branch = meristem(Some(0), MeristemState::Growing);
            branch.bud = graph.add_bud(tip, BudKind::Terminal, Vec3::X * side);
            (branch.chain, branch.nodes) = (chain, size as u32);
            plant.meristems.push(branch);
        }
        let mut dormant = meristem(Some(0), MeristemState::Resting);
        dormant.bud = graph.add_bud(trunk_nodes[2], BudKind::Lateral, Vec3::Z);
        plant.meristems.push(dormant);
        graph.buds_mut().exposure.copy_from_slice(&exposures);
        plant
    }

    /// Runs a first cycle in full light, which records the sizes, then cycle `cycle`.
    fn shed(plant: &mut Plant, threshold: f32, lifespan: u16, cycle: u16) {
        let mut shedding = ExposureShedding::with(threshold, lifespan);
        let exposures = plant.graph.bud_exposures().to_vec();
        plant.graph.buds_mut().exposure.fill(1e9);
        shedding.shed(plant, 0);
        plant.graph.buds_mut().exposure.copy_from_slice(&exposures);
        shedding.shed(plant, cycle);
    }

    #[test]
    fn a_branch_below_the_threshold_is_shed_with_its_cycle() {
        let mut plant = plant(5, [1.0, 1.0, 0.4, 1.0]);
        shed(&mut plant, 0.1, 0, 7);
        plant.graph.check_invariants().unwrap();
        let graph = &plant.graph;
        let lit = graph.chain(plant.meristems[1].chain);
        let dark = graph.chain(plant.meristems[2].chain);
        assert!(!graph.is_shed(lit.first) && !graph.is_shed(lit.last));
        assert!(graph.is_shed(dark.first) && graph.is_shed(dark.last));
        assert_eq!(graph.shed_cycles()[dark.first.index()], 7);
        assert!((0..4).all(|node| !graph.is_shed(NodeId::new(node))));
        assert_eq!(plant.meristems[2].state, MeristemState::Done);
        assert_eq!(plant.meristems[1].state, MeristemState::Growing);
        let dead = graph.bud_states()[plant.meristems[2].bud.index()];
        assert_eq!(dead, BudState::Dead);
    }

    #[test]
    fn the_ratio_is_light_per_internode() {
        // 0.4 of light is 0.133 per internode over 3 internodes and 0.08 over 5.
        let mut small = plant(3, [1.0, 1.0, 0.4, 1.0]);
        shed(&mut small, 0.1, 0, 3);
        let chain = small.meristems[2].chain;
        assert!(!small.graph.is_shed(small.graph.chain(chain).first));
        let mut large = plant(5, [1.0, 1.0, 0.4, 1.0]);
        shed(&mut large, 0.1, 0, 3);
        let chain = large.meristems[2].chain;
        assert!(large.graph.is_shed(large.graph.chain(chain).first));
    }

    #[test]
    fn stems_are_never_shed_and_zero_disables_shedding() {
        let mut dark = plant(3, [0.0, 0.0, 0.0, 0.0]);
        shed(&mut dark, 0.5, 0, 2);
        let graph = &dark.graph;
        assert!((0..4).all(|node| !graph.is_shed(NodeId::new(node))));
        assert!(graph.is_shed(graph.chain(dark.meristems[1].chain).first));
        let mut kept = plant(3, [0.0, 0.0, 0.0, 0.0]);
        shed(&mut kept, 0.0, 0, 2);
        assert!(kept.graph.flags().iter().all(|flags| !flags.is_shed()));
    }

    #[test]
    fn dormant_buds_die_at_their_lifespan() {
        let mut plant = plant(3, [1.0; 4]);
        shed(&mut plant, 0.0, 4, 3);
        assert_eq!(plant.meristems[3].state, MeristemState::Resting);
        shed(&mut plant, 0.0, 4, 4);
        assert_eq!(plant.meristems[3].state, MeristemState::Done);
        let bud = plant.meristems[3].bud;
        assert_eq!(plant.graph.bud_states()[bud.index()], BudState::Dead);
        assert_eq!(plant.meristems[1].state, MeristemState::Growing);
    }
}
