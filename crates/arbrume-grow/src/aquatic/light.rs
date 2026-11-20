//! Light under water.
//!
//! [`Attenuated`] wraps an environment stage and multiplies the exposure `Q` it gives
//! each bud by the fraction of the light below the surface that reaches the depth of
//! the bud, `exp(-Kd (surface - y))`, `Kd` the attenuation of the water (the
//! Beer-Lambert law for downwelling light). The preferred direction `V` is left as the
//! wrapped stage gives it. In the self-organizing mode deep buds then receive less of
//! the resource and deep branches are shed first, so growth concentrates near the
//! surface; in the parametric mode exposures are not read and nothing changes.

use crate::plant::Plant;
use crate::stage::Environment;
use crate::water::Water;

/// An environment stage under water. See the [module documentation](self).
#[derive(Debug, Clone)]
pub struct Attenuated<E> {
    inner: E,
    water: Water,
}

impl<E> Attenuated<E> {
    /// `inner` under `water`.
    #[must_use]
    pub const fn new(inner: E, water: Water) -> Self {
        Self { inner, water }
    }
}

impl<E: Environment> Environment for Attenuated<E> {
    fn sense(&mut self, plant: &mut Plant, cycle: u16) {
        self.inner.sense(plant, cycle);
        let buds = plant.graph.buds_mut();
        for (exposure, node) in buds.exposure.iter_mut().zip(buds.nodes) {
            let height = buds.node_positions[node.index()].y;
            *exposure *= self.water.light_at(height);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Attenuated;
    use crate::plant::Plant;
    use crate::stage::{Environment, NoEnvironment};
    use crate::water::Water;
    use arbrume_core::graph::{BudKind, Metamer};
    use glam::Vec3;

    #[test]
    fn deep_buds_get_less_light() {
        let mut plant = Plant::new();
        let base = plant
            .graph
            .add_root_chain(Vec3::ZERO, Metamer::new(Vec3::Y * 0.5, 0.01, 0));
        let chain = plant.graph.chain_of(base);
        let top = plant
            .graph
            .extend_chain(chain, Metamer::new(Vec3::Y * 3.0, 0.01, 0));
        plant.graph.add_bud(base, BudKind::Lateral, Vec3::X);
        plant.graph.add_bud(top, BudKind::Lateral, Vec3::X);
        let water = Water {
            attenuation: 0.4,
            ..Water::still(3.0)
        };
        Attenuated::new(NoEnvironment, water).sense(&mut plant, 0);
        let exposures = plant.graph.bud_exposures();
        assert!((exposures[1] - 1.0).abs() < 1e-6);
        assert!((exposures[0] - (-0.4_f32 * 2.5).exp()).abs() < 1e-6);
    }
}
