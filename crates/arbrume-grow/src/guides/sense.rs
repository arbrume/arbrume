//! The environment stage of guided growth: the environment of the species, then the
//! markers of crown and limb strokes, the axes of limb strokes, freezes and pruning.

use std::cell::RefCell;
use std::rc::Rc;

use crate::markers::{Markers, Perception};
use crate::plant::{MeristemId, MeristemState, Plant};
use crate::stage::{Context, Environment};
use arbrume_core::graph::{BudState, NodeId};
use arbrume_core::rng::{Rng, Stream, mix64};
use glam::Vec3;

use super::steer::{Steering, guiding_direction, position};
use super::stroke::{in_balls, stroke_balls};
use super::{Guides, Stroke, StrokeKind};

/// Salt of the seeds of guide markers, so that they differ from every other draw.
const GUIDE_SALT: u64 = 0x6775_6964_6573;

/// Markers of the strokes that start acting in one cycle with one strength and reach.
#[derive(Debug, Clone)]
struct Group {
    cycle: u16,
    strength: f32,
    perception: Perception,
    markers: Markers,
}

/// Environment of guided growth. See the [module documentation](self).
#[derive(Debug)]
pub(super) struct Guided<'a, E> {
    inner: E,
    cx: Context<'a>,
    guides: &'a Guides,
    steering: Rc<RefCell<Steering>>,
    perception: Perception,
    /// Markers per cubic meter at a stroke density of 1.
    density: f32,
    /// Markers left to scatter before the limit of the species.
    budget: u32,
    full: f32,
    parallel: bool,
    /// Centres of the balls of each erase stroke, empty for the other strokes.
    erase_balls: Vec<Vec<Vec3>>,
    groups: Vec<Group>,
    /// Markers scattered for each stroke.
    pub scattered: Vec<Vec<Vec3>>,
    /// Nodes shed by each stroke.
    pub pruned: Vec<u32>,
    buds: Vec<u32>,
    positions: Vec<Vec3>,
    directions: Vec<Vec3>,
}

impl<'a, E: Environment> Guided<'a, E> {
    /// `inner` guided by `guides` for the run `cx`; markers are given to buds on the
    /// `rayon` pool when `parallel` holds.
    pub fn new(
        inner: E,
        cx: Context<'a>,
        guides: &'a Guides,
        steering: Rc<RefCell<Steering>>,
        parallel: bool,
    ) -> Self {
        let growth = &cx.species.growth;
        let model = &growth.self_organization;
        let unit = growth.internode_unit() * cx.scale;
        let erase_balls = guides
            .strokes
            .iter()
            .map(|stroke| match stroke.kind {
                StrokeKind::Erase => stroke_balls(&stroke.points, stroke.radius),
                _ => Vec::new(),
            })
            .collect();
        Self {
            inner,
            cx,
            guides,
            steering,
            perception: Perception {
                occupancy: model.occupancy_radius * unit,
                reach: model.perception_distance * unit,
                angle: model.perception_angle_deg.to_radians(),
            },
            density: model.marker_density / (unit * unit * unit),
            budget: model.marker_limit,
            full: model.full_exposure,
            parallel,
            erase_balls,
            groups: Vec::new(),
            scattered: vec![Vec::new(); guides.strokes.len()],
            pruned: vec![0; guides.strokes.len()],
            buds: Vec::new(),
            positions: Vec::new(),
            directions: Vec::new(),
        }
    }

    /// Occupancy radius of a bud, in meters.
    pub const fn occupancy(&self) -> f32 {
        self.perception.occupancy
    }

    /// Whether `point` is inside an erase stroke acting in `cycle`.
    fn erased(&self, point: Vec3, cycle: u16) -> bool {
        let strokes = self.guides.strokes.iter().zip(&self.erase_balls);
        strokes
            .filter(|(stroke, _)| stroke.kind == StrokeKind::Erase && stroke.cycle <= cycle)
            .any(|(stroke, balls)| in_balls(balls, stroke.radius, point))
    }

    /// Sheds, for each erase stroke starting in `cycle` after the first, the branches with
    /// a live node inside it, then ends the meristems whose bud died.
    fn prune(&mut self, plant: &mut Plant, cycle: u16) {
        if cycle == 0 {
            return;
        }
        let guides = self.guides;
        for (index, stroke) in guides.strokes.iter().enumerate() {
            if stroke.kind != StrokeKind::Erase || stroke.cycle != cycle {
                continue;
            }
            let roots = erased_branches(plant, &self.erase_balls[index], stroke.radius);
            let shed = |plant: &Plant| plant.graph.flags().iter().filter(|f| f.is_shed()).count();
            let before = shed(plant);
            plant.graph.shed_subtrees(&roots, cycle);
            self.pruned[index] = (shed(plant) - before) as u32;
        }
        let states = plant.graph.bud_states();
        let mut ended = Vec::new();
        for (index, meristem) in plant.meristems.iter().enumerate() {
            let alive = matches!(
                meristem.state,
                MeristemState::Resting | MeristemState::Growing
            );
            if alive && meristem.bud.is_some() && states[meristem.bud.index()] == BudState::Dead {
                ended.push(index);
            }
        }
        for index in ended {
            plant.meristems[index].state = MeristemState::Done;
        }
    }

    /// Scatters the markers of the crown strokes, and of the limb strokes with a density,
    /// that start acting in `cycle`; drops the markers inside the erase strokes acting
    /// then when one starts in `cycle`.
    fn scatter(&mut self, cycle: u16) {
        let guides = self.guides;
        for (index, stroke) in guides.strokes.iter().enumerate() {
            let marked = match stroke.kind {
                StrokeKind::Crown => true,
                StrokeKind::Limb => stroke.density > 0.0,
                StrokeKind::Erase => false,
            };
            if !marked || stroke.cycle != cycle {
                continue;
            }
            let density = stroke.density * self.density;
            let mut points = scatter_stroke(stroke, index, self.cx.seed, density, self.budget);
            points.retain(|&point| !self.erased(point, cycle));
            self.budget -= points.len() as u32;
            self.scattered[index].clone_from(&points);
            let perception = Perception {
                reach: if stroke.reach > 0.0 {
                    stroke.reach
                } else {
                    self.perception.reach
                },
                ..self.perception
            };
            self.add_markers(cycle, stroke.strength, perception, points);
        }
        let erasing = guides
            .strokes
            .iter()
            .any(|stroke| stroke.kind == StrokeKind::Erase && stroke.cycle == cycle);
        if erasing {
            for index in 0..self.groups.len() {
                let mut points = self.groups[index].markers.points().to_vec();
                points.retain(|&point| !self.erased(point, cycle));
                self.groups[index].markers = Markers::from_points(points);
            }
        }
    }

    /// Adds `points` to the group of `cycle`, `strength` and `perception`.
    fn add_markers(
        &mut self,
        cycle: u16,
        strength: f32,
        perception: Perception,
        points: Vec<Vec3>,
    ) {
        let same = |group: &Group| {
            group.cycle == cycle
                && group.strength.to_bits() == strength.to_bits()
                && group.perception == perception
        };
        if let Some(group) = self.groups.iter_mut().find(|group| same(group)) {
            let mut all = group.markers.points().to_vec();
            all.extend(points);
            group.markers = Markers::from_points(all);
        } else {
            let markers = Markers::from_points(points);
            self.groups.push(Group {
                cycle,
                strength,
                perception,
                markers,
            });
        }
    }

    /// Fills `buds`, `positions` and `directions` with the live buds of `plant`.
    fn collect_buds(&mut self, plant: &Plant) {
        let graph = &plant.graph;
        self.buds.clear();
        self.positions.clear();
        self.directions.clear();
        let (nodes, positions) = (graph.bud_nodes(), graph.positions());
        let directions = graph.bud_directions();
        for (bud, state) in graph.bud_states().iter().enumerate() {
            if *state != BudState::Dead {
                self.buds.push(bud as u32);
                self.positions.push(positions[nodes[bud].index()]);
                self.directions.push(directions[bud]);
            }
        }
    }

    /// Gives the markers of every group to the live buds: the buds that own some get
    /// their exposure raised, the direction towards them and room to grow. Returns, per
    /// bud id, whether the bud owns markers of a group starting at or after `freeze`.
    fn crown(&mut self, plant: &mut Plant, freeze: Option<u16>) -> Vec<bool> {
        let count = plant.graph.bud_states().len();
        let mut allowed = vec![freeze.is_none(); count];
        let mut toward = vec![Vec3::ZERO; self.buds.len()];
        let mut boost = vec![0.0_f32; self.buds.len()];
        let mut near = Near::default();
        for group in &mut self.groups {
            near.select(group, &self.positions, &self.directions);
            let seen = group.markers.perceive(
                &near.positions,
                &near.directions,
                &group.perception,
                self.parallel,
            );
            let newer = freeze.is_some_and(|f| group.cycle >= f);
            for (&live, &direction) in near.buds.iter().zip(seen) {
                if direction != Vec3::ZERO {
                    toward[live] += direction;
                    boost[live] = boost[live].max(group.strength * self.full);
                    allowed[self.buds[live] as usize] |= newer;
                }
            }
        }
        if toward.iter().all(|direction| *direction == Vec3::ZERO) {
            return allowed;
        }
        let owners = bud_owners(plant, count);
        let reach = self.perception.reach;
        let buds = plant.graph.buds_mut();
        for (live, direction) in toward.iter().enumerate() {
            let Some(direction) = direction.try_normalize() else {
                continue;
            };
            let bud = self.buds[live] as usize;
            buds.exposure[bud] = buds.exposure[bud].max(boost[live]);
            buds.preferred[bud] = direction;
            if let Some(owner) = owners[bud] {
                let meristem = &mut plant.meristems[owner.index()];
                meristem.target = meristem.target.max(meristem.length + reach);
            }
        }
        allowed
    }

    /// Gives the axis of every limb its exposure (`strength` times the light `total` of
    /// the plant, at least `C`), its direction and the length it needs to reach the end
    /// of the stroke; marks its bud allowed when the limb starts at or after `freeze`.
    fn limbs(&self, plant: &mut Plant, freeze: Option<u16>, allowed: &mut [bool], total: f32) {
        let steering = self.steering.borrow();
        let growth = &self.cx.species.growth;
        for limb in steering.limbs.iter().filter(|limb| !limb.done) {
            let Some(id) = limb.claimed else {
                continue;
            };
            let stroke = &self.guides.strokes[limb.stroke];
            let from = position(plant, id);
            let meristem = plant.meristem(id);
            let step = growth
                .internode_length
                .eval(meristem.order, meristem.position())
                * self.cx.scale;
            let guided = guiding_direction(limb, from, step);
            let remaining = limb.line.length() - limb.progress;
            let bud = meristem.bud;
            plant.meristems[id.index()].target = meristem.length + remaining;
            if bud.is_none() {
                continue;
            }
            let buds = plant.graph.buds_mut();
            let exposure = &mut buds.exposure[bud.index()];
            *exposure = exposure.max(stroke.strength * total.max(self.full));
            if let Some((direction, _)) = guided {
                buds.preferred[bud.index()] = direction;
            }
            if freeze.is_none_or(|f| limb.cycle >= f) {
                allowed[bud.index()] = true;
            }
        }
    }
}

impl<E: Environment> Environment for Guided<'_, E> {
    fn sense(&mut self, plant: &mut Plant, cycle: u16) {
        self.prune(plant, cycle);
        self.scatter(cycle);
        self.inner.sense(plant, cycle);
        self.steering.borrow_mut().claim(plant, cycle);
        self.collect_buds(plant);
        let total = plant.graph.bud_exposures().iter().sum();
        let freeze = self.guides.freeze_at(cycle);
        let mut allowed = self.crown(plant, freeze);
        self.limbs(plant, freeze, &mut allowed, total);
        if freeze.is_some() {
            let exposure = plant.graph.buds_mut().exposure;
            for (value, allowed) in exposure.iter_mut().zip(allowed) {
                if !allowed {
                    *value = 0.0;
                }
            }
        }
    }
}

/// The live buds that may perceive or occupy a marker of one group: those within the
/// zone radius of the box around its markers, in their order, so that the markers go to
/// the same buds as if every bud were given.
#[derive(Debug, Default)]
struct Near {
    /// Index of each selected bud among the live buds.
    buds: Vec<usize>,
    positions: Vec<Vec3>,
    directions: Vec<Vec3>,
}

impl Near {
    /// Selects the buds near the markers of `group` among the live buds at `positions`.
    fn select(&mut self, group: &Group, positions: &[Vec3], directions: &[Vec3]) {
        self.buds.clear();
        self.positions.clear();
        self.directions.clear();
        let points = group.markers.points();
        let Some(&first) = points.first() else {
            return;
        };
        let (low, high) = points
            .iter()
            .fold((first, first), |(low, high), p| (low.min(*p), high.max(*p)));
        let zone = group.perception.reach.max(group.perception.occupancy);
        let (low, high) = (low - Vec3::splat(zone), high + Vec3::splat(zone));
        for (live, (&position, &direction)) in positions.iter().zip(directions).enumerate() {
            if position.cmpge(low).all() && position.cmple(high).all() {
                self.buds.push(live);
                self.positions.push(position);
                self.directions.push(direction);
            }
        }
    }
}

/// The meristem of each bud id, `None` for a bud no meristem owns.
fn bud_owners(plant: &Plant, count: usize) -> Vec<Option<MeristemId>> {
    let mut owners = vec![None; count];
    for (index, meristem) in plant.meristems.iter().enumerate() {
        if meristem.bud.is_some() {
            owners[meristem.bud.index()] = Some(MeristemId(index as u32));
        }
    }
    owners
}

/// First nodes of the branches (axes that are not stems) with a live node inside the
/// balls of `radius` about `balls`.
fn erased_branches(plant: &Plant, balls: &[Vec3], radius: f32) -> Vec<NodeId> {
    let graph = &plant.graph;
    let mut roots = Vec::new();
    let (positions, flags, chains) = (graph.positions(), graph.flags(), graph.node_chains());
    for (node, &point) in positions.iter().enumerate() {
        if flags[node].is_shed() || !in_balls(balls, radius, point) {
            continue;
        }
        let chain = chains[node];
        if plant.meristem(plant.meristem_of(chain)).parent.is_none() {
            continue;
        }
        let first = graph.chain(chain).first;
        if !roots.contains(&first) {
            roots.push(first);
        }
    }
    roots
}

/// Markers of `stroke`, number `index` of the guides of a plant grown with `seed`:
/// `density` per cubic meter, uniform in the union of the balls of the brush, at most
/// `limit`.
///
/// Candidate `i` of ball `b` is drawn in the cube around the ball from a key of the seed,
/// the stroke and the ball, and kept when it lies in the ball and in none of the balls
/// before it, so that the union is filled once.
pub(super) fn scatter_stroke(
    stroke: &Stroke,
    index: usize,
    seed: u64,
    density: f32,
    limit: u32,
) -> Vec<Vec3> {
    let radius = stroke.radius;
    let centers = stroke_balls(&stroke.points, radius);
    let side = 2.0 * f64::from(radius);
    let candidates = (side * side * side * f64::from(density)).min(f64::from(limit)) as u32;
    let mut points = Vec::new();
    for (ball, &center) in centers.iter().enumerate() {
        let site = ((index as u64) << 32) | ball as u64;
        let key = mix64(seed ^ mix64(GUIDE_SALT ^ site));
        for candidate in 0..candidates {
            let mut rng = Rng::new(key, candidate, Stream::ENVIRONMENT);
            let unit = Vec3::new(rng.signed(), rng.signed(), rng.signed());
            let point = center + unit * radius;
            let inside = point.distance_squared(center) <= radius * radius;
            if inside && !in_balls(&centers[..ball], radius, point) {
                points.push(point);
            }
        }
        if points.len() >= limit as usize {
            points.truncate(limit as usize);
            break;
        }
    }
    points
}

#[cfg(test)]
mod tests {
    use super::scatter_stroke;
    use crate::guides::{Stroke, StrokeKind};
    use glam::Vec3;

    #[test]
    fn markers_fill_the_brush_once_and_depend_on_the_seed() {
        let stroke = Stroke::new(StrokeKind::Crown, vec![Vec3::ZERO, Vec3::X * 4.0], 0.5);
        let points = scatter_stroke(&stroke, 0, 7, 400.0, u32::MAX);
        let volume = std::f32::consts::PI * 0.25 * 4.0 + 4.0 / 3.0 * std::f32::consts::PI * 0.125;
        let expected = 400.0 * volume;
        let count = points.len() as f32;
        assert!(
            (count - expected).abs() < 0.1 * expected,
            "{count} vs {expected}"
        );
        assert!(
            points
                .iter()
                .all(|p| p.x >= -0.5 && p.x <= 4.5 && p.y.abs() <= 0.5)
        );
        assert_eq!(points, scatter_stroke(&stroke, 0, 7, 400.0, u32::MAX));
        assert_ne!(points, scatter_stroke(&stroke, 0, 8, 400.0, u32::MAX));
        assert_ne!(points, scatter_stroke(&stroke, 1, 7, 400.0, u32::MAX));
        assert_eq!(scatter_stroke(&stroke, 0, 7, 400.0, 10).len(), 10);
    }
}
