//! The parametric mode: bud fate from gradient curves, and the tropism blend.
//!
//! [`GradientFate`] needs no environment. Following Prusinkiewicz et al. (2001), every
//! quantity is a curve of the species over branching order and relative position:
//!
//! * A bud forms on a node with `branch_probability` (bearing order, position of the
//!   node along the bearing axis), multiplied inside a growth unit by `unit_branching`
//!   when growth and branching pattern are both rhythmic. With a continuous branching
//!   pattern every bud with a positive probability forms. No bud forms on the
//!   `bare_base` of an axis, and a bud older than the `branch_lifespan` of its position
//!   is left out: its axis has been shed.
//! * The axis of an admitted bud gets its full-grown length at once:
//!   `axis_length(order, x) * (1 - apical_dominance * x) * (parent / reference)^e *
//!   (1 + length_variation * u)` with `x` the attachment position, `e` the
//!   `parent_length_exponent`, `reference` the largest `axis_length` of the parent's
//!   order and `u` uniform in `[-1, 1)`. A bud whose axis would be shorter than half an
//!   internode aborts.
//! * Each cycle, the potential length of a growing axis increases by `shoot_length`
//!   until it reaches the full-grown length. Metamers of `internode_length` are added
//!   while they fit under the potential; the remainder waits for the next cycle. The
//!   last shoot is divided into the nearest whole number of internodes, so an axis ends
//!   on its full-grown length to within half an internode.
//! * A lateral bud flushes one cycle after it formed (proleptic) or in the same cycle
//!   (sylleptic); a relay always flushes the cycle after.
//!
//! The age of the plant is used twice: by the `bare_base`, a fraction of the length the
//! bearing axis has at that age, and by the `branch_lifespan`. With neither, a plant
//! grown to `n` cycles is exactly the nodes born before cycle `n` of the same plant
//! grown longer.
//!
//! [`TropismBlend`] is the orientation stage: `H' = alpha * H + beta * T + gamma * E`
//! with `alpha + beta + gamma = 1` (Longay et al. 2012, equation 2), followed by the
//! systematic `curvature`, the per-cycle `gnarl` and the per-metamer
//! `direction_variation` of the species.

use crate::math::{UP, perpendicular, radial, reject, tilt, upward_normal};
use crate::plant::{Meristem, MeristemId, MeristemState, Plant, SiteKind};
use crate::stage::{BudFate, BudSite, Context, Flush, HeadingInput, ShootOrientation};
use arbrume_core::rng::{Rng, Stream};
use arbrume_core::species::{BranchTiming, BranchingPattern, GrowthRhythm};
use glam::Vec3;

/// Stream of the per-cycle bend of an axis.
const GNARL: Stream = Stream::custom(1);

/// Full-grown length of the stem of the seed `site`: the `axis_length` of order 0 with
/// its variation. `None` when it is shorter than half an internode.
pub(crate) fn seed_length(cx: &Context, site: &BudSite) -> Option<f32> {
    let growth = &cx.species.growth;
    let mut rng = Rng::new(site.key, 0, Stream::SHOOT_LENGTH);
    let variation = growth.length_variation.eval(0, 0.0);
    let length = growth.axis_length.eval(0, 0.0) * variation.mul_add(rng.signed(), 1.0);
    (length >= 0.5 * growth.internode_length.eval(0, 0.0)).then_some(length)
}

/// Whether a lateral bud forms at `site` on the axis of `bearing`, and the full-grown
/// length of its axis: the branching probability, then the length formula of the
/// [module documentation](self). `None` when no bud forms.
pub(crate) fn lateral_length(cx: &Context, bearing: &Meristem, site: &BudSite) -> Option<f32> {
    let (growth, architecture) = (&cx.species.growth, &cx.species.architecture);
    let x = site.position;
    let mut probability = growth.branch_probability.eval(bearing.order, x);
    let tiers = architecture.growth_rhythm == GrowthRhythm::Rhythmic
        && architecture.branching_pattern == BranchingPattern::Rhythmic;
    if tiers {
        probability *= growth.unit_branching.eval(site.unit_position);
    }
    if probability <= 0.0 {
        return None;
    }
    let mut rng = Rng::new(site.key, 0, Stream::BUD_FATE);
    let every = architecture.branching_pattern == BranchingPattern::Continuous;
    if !every && !rng.chance(probability) {
        return None;
    }
    let order = site.order;
    let mut length = growth.axis_length.eval(order, x) * (1.0 - growth.apical_dominance * x);
    let reference = growth.axis_length.order(bearing.order).bounds().1;
    if growth.parent_length_exponent > 0.0 && reference > 0.0 {
        length *= (bearing.target / reference).powf(growth.parent_length_exponent);
    }
    length *= growth
        .length_variation
        .eval(order, x)
        .mul_add(rng.signed(), 1.0);
    (length >= 0.5 * growth.internode_length.eval(order, 0.0)).then_some(length)
}

/// Growth bookkeeping of one meristem.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Vigor {
    /// Length the apparent axis could have reached so far.
    potential: f32,
    /// Length the apparent axis reaches at the age of the plant.
    limit: f32,
}

/// Bud fate of the parametric mode. See the [module documentation](self).
#[derive(Debug, Clone)]
pub struct GradientFate<'a> {
    cx: Context<'a>,
    vigor: Vec<Vigor>,
}

impl<'a> GradientFate<'a> {
    /// Fate for one run.
    #[must_use]
    pub const fn new(cx: Context<'a>) -> Self {
        Self {
            cx,
            vigor: Vec::new(),
        }
    }

    /// Potential length after one more cycle of an axis of `order` and full-grown
    /// length `target`.
    fn advance(&self, order: u8, potential: f32, target: f32) -> f32 {
        let rate = self
            .cx
            .species
            .growth
            .shoot_length
            .eval(order, potential / target);
        (potential + rate).min(target)
    }

    /// Length an axis of `order` starting to grow in cycle `start` reaches at the age of
    /// the plant. Only computed for orders with a bare base, the one use of that length.
    fn length_at_age(&self, order: u8, target: f32, start: u16) -> f32 {
        let bare = self.cx.species.growth.bare_base.order(order).bounds().1;
        if bare <= 0.0 {
            return target;
        }
        let mut potential = 0.0;
        for _ in start..self.cx.age {
            potential = self.advance(order, potential, target);
            if potential >= target {
                break;
            }
        }
        potential
    }

    /// Admission of a seed: the stem gets the `axis_length` of order 0.
    fn admit_seed(&mut self, site: &BudSite) -> Option<f32> {
        let length = seed_length(&self.cx, site)?;
        let limit = self.length_at_age(0, length, 0);
        self.vigor.push(Vigor {
            potential: 0.0,
            limit,
        });
        Some(length)
    }

    /// Admission of a lateral bud: bare base, lifespan, then probability and length.
    fn admit_lateral(&mut self, plant: &Plant, site: &BudSite, parent: MeristemId) -> Option<f32> {
        let species = self.cx.species;
        let (growth, architecture) = (&species.growth, &species.architecture);
        let bearing = plant.meristem(parent);
        let bare = growth
            .bare_base
            .eval(bearing.order, bearing.attach_position);
        if site.arc < bare * self.vigor[parent.index()].limit {
            return None;
        }
        let lifespan = growth.branch_lifespan.eval(bearing.order, site.position);
        if lifespan >= 0.5 && f32::from(self.cx.age.saturating_sub(site.cycle)) > lifespan {
            return None;
        }
        let length = lateral_length(&self.cx, bearing, site)?;
        let delay = u16::from(architecture.branch_timing == BranchTiming::Proleptic);
        let limit = self.length_at_age(site.order, length, site.cycle.saturating_add(delay));
        self.vigor.push(Vigor {
            potential: 0.0,
            limit,
        });
        Some(length)
    }

    /// Shoot of `meristem` for one cycle, `None` when it adds no metamer and goes on.
    ///
    /// Metamers are counted by stepping along the axis with the internode length at
    /// each step, which integrates the node density of Prusinkiewicz et al. (2001,
    /// equation 11); the metamers of one shoot then share its length equally.
    fn shoot(&mut self, id: MeristemId, meristem: &Meristem) -> Option<Flush> {
        let internodes = &self.cx.species.growth.internode_length;
        let (order, target) = (meristem.order, meristem.target);
        let potential = self.advance(order, self.vigor[id.index()].potential, target);
        self.vigor[id.index()].potential = potential;
        let last = potential >= target;
        let reach = potential * (1.0 + 1e-5);
        let (mut arc, mut metamers) = (meristem.length, 0_u16);
        let mut internode = internodes.eval(order, arc / target);
        while arc + internode <= reach && metamers < u16::MAX {
            arc += internode;
            metamers += 1;
            internode = internodes.eval(order, arc / target);
        }
        if !last {
            let length = arc - meristem.length;
            return (metamers > 0).then_some(Flush {
                meristem: id,
                metamers,
                length,
                last,
            });
        }
        let fresh = meristem.nodes == 0 && meristem.kind != SiteKind::Relay;
        if potential - arc >= 0.5 * internode || (metamers == 0 && fresh) {
            metamers = metamers.saturating_add(1);
        }
        Some(Flush {
            meristem: id,
            metamers,
            length: (potential - meristem.length).max(0.0),
            last,
        })
    }
}

impl BudFate for GradientFate<'_> {
    fn admit(&mut self, plant: &Plant, site: &BudSite) -> Option<f32> {
        debug_assert_eq!(site.id.index(), self.vigor.len());
        match (site.kind, site.parent) {
            (SiteKind::Lateral, Some(parent)) => self.admit_lateral(plant, site, parent),
            (SiteKind::Relay, Some(parent)) => {
                self.vigor.push(self.vigor[parent.index()]);
                Some(plant.meristem(parent).target)
            }
            _ => self.admit_seed(site),
        }
    }

    fn decide(&mut self, plant: &Plant, cycle: u16, from: MeristemId, flushes: &mut Vec<Flush>) {
        let timing = self.cx.species.architecture.branch_timing;
        for (index, meristem) in plant.meristems.iter().enumerate().skip(from.index()) {
            let ready = match (meristem.state, meristem.kind) {
                (MeristemState::Done | MeristemState::Relayed, _) => false,
                (MeristemState::Growing, _) | (MeristemState::Resting, SiteKind::Seed) => true,
                (MeristemState::Resting, SiteKind::Lateral) => {
                    meristem.born < cycle || timing == BranchTiming::Sylleptic
                }
                (MeristemState::Resting, SiteKind::Relay) => meristem.born < cycle,
            };
            if ready {
                flushes.extend(self.shoot(MeristemId(index as u32), meristem));
            }
        }
    }
}

/// Orientation stage of the species: tropism blend, curvature, gnarl and wander. See the
/// [module documentation](self).
#[derive(Debug, Clone, Copy)]
pub struct TropismBlend<'a> {
    cx: Context<'a>,
    gnarl: Gnarl,
}

/// The gnarl draws of one shoot, kept while its metamers are built.
#[derive(Debug, Clone, Copy)]
struct Gnarl {
    /// Meristem and cycle of the shoot.
    shoot: Option<(MeristemId, u16)>,
    /// Bend of the whole shoot as a fraction of the half-width, in `[-1, 1)`.
    fraction: f32,
    /// Azimuth of the bend around the heading, in radians.
    azimuth: f32,
}

impl<'a> TropismBlend<'a> {
    /// Orientation for one run.
    #[must_use]
    pub const fn new(cx: Context<'a>) -> Self {
        let gnarl = Gnarl {
            shoot: None,
            fraction: 0.0,
            azimuth: 0.0,
        };
        Self { cx, gnarl }
    }

    /// The gnarl draws of the shoot of `input`.
    fn gnarl(&mut self, input: &HeadingInput) -> Gnarl {
        let shoot = Some((input.meristem, input.cycle));
        if self.gnarl.shoot != shoot {
            let key = u64::from(input.cycle) << 8;
            let mut rng = Rng::at(input.key, 0, GNARL, key);
            self.gnarl = Gnarl {
                shoot,
                fraction: rng.signed(),
                azimuth: rng.angle(),
            };
        }
        self.gnarl
    }

    /// `alpha * H + beta * T + gamma * E`, normalized.
    ///
    /// `T` makes the set-point angle with the upward vertical, in the vertical plane of
    /// the heading (of `side` when the heading is vertical).
    fn blend(&self, input: &HeadingInput) -> Vec3 {
        let growth = &self.cx.species.growth;
        let (order, x, heading) = (input.order, input.position, input.heading);
        let beta = growth.tropism_weight.eval(order, x);
        let has_preference = input.preferred != Vec3::ZERO;
        let gamma = if has_preference {
            growth.environment_weight
        } else {
            0.0
        };
        if beta <= 0.0 && gamma <= 0.0 {
            return heading;
        }
        let horizontal = reject(heading, UP)
            .try_normalize()
            .or_else(|| reject(input.side, UP).try_normalize())
            .unwrap_or(Vec3::X);
        let setpoint = growth.gravitropic_setpoint.eval(order, x).to_radians();
        let tropism = tilt(UP, horizontal, setpoint);
        let blended = heading * (1.0 - beta - gamma) + tropism * beta + input.preferred * gamma;
        blended.try_normalize().unwrap_or(heading)
    }
}

impl ShootOrientation for TropismBlend<'_> {
    /// The blend, then one rotation that adds three bends as rotation vectors: the
    /// systematic curvature (downwards when positive), the gnarl of the cycle and the
    /// wander of the metamer (both at a random azimuth around the heading).
    fn heading(&mut self, input: &HeadingInput) -> Vec3 {
        let growth = &self.cx.species.growth;
        let (order, x) = (input.order, input.position);
        let metamers = f32::from(input.unit_metamers.max(1));
        let heading = self.blend(input);
        let side = perpendicular(heading, input.side);
        let mut bend = Vec3::ZERO;
        let curvature = growth.curvature.eval(order, x);
        if curvature != 0.0 && input.axis_length > 0.0 {
            let angle = curvature.to_radians() * input.internode / input.axis_length;
            bend -= upward_normal(heading, side) * angle;
        }
        let gnarl = growth.gnarl.eval(order, x);
        if gnarl > 0.0 {
            let draws = self.gnarl(input);
            let angle = gnarl.to_radians() * draws.fraction / metamers;
            bend += radial(heading, side, draws.azimuth) * angle;
        }
        let wander = growth.direction_variation.eval(order, x);
        if wander > 0.0 {
            let key = u64::from(input.node_index) << 8;
            let mut rng = Rng::at(input.key, 0, Stream::SHOOT_DIRECTION, key);
            let angle = wander.to_radians() * rng.signed();
            bend += radial(heading, side, rng.angle()) * angle;
        }
        let angle = bend.length();
        if angle > 0.0 {
            tilt(heading, bend / angle, angle).normalize()
        } else {
            heading
        }
    }
}
