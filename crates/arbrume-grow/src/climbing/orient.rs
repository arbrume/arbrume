//! The orientation stage of a climbing plant.
//!
//! [`Climb`] wraps the orientation stage of the growth mode, which gives the heading a
//! free shoot would take (tropism, light, wander), and decides what holds the metamer:
//!
//! 1. **Held.** A support within `reach` of the tip holds it. The wanted heading is
//!    projected on the tangent plane of the surface (the surface adaptation of Hädrich
//!    et al. 2017, eq. 6, taken to its limit), the tropism and the light it contains
//!    acting within that plane as their eq. 7 does. A twining stem instead takes the
//!    helix direction at `twine_angle_deg` from the upward direction on the surface.
//!    The end of the metamer is then moved along the normal to the stem offset, the
//!    gap plus the tip radius, so the stem follows the surface at a fixed distance and
//!    wraps over edges and around posts.
//! 2. **Searching.** A support within `search_distance` turns the heading towards it by
//!    `seek_weight`. A shoot held by the ground keeps creeping while it turns towards a
//!    support it can climb.
//! 3. **Free.** Nothing holds the shoot: it takes only a quarter of the turn the
//!    wrapped stage asks for (a stem without support cannot carry its own tropism) and
//!    bends towards the ground by `droop` degrees per meter it grows free, so long free
//!    shoots hang. The quarter is a choice of this implementation, not a measurement.
//!
//! A tendril or twining habit only holds supports thinner than `max_support_thickness`
//! at the tip; the ground holds every habit when `ground` is set. No metamer ends below
//! the ground, inside a blocking occluder or out of the envelope: a blocked metamer is
//! turned along the surface of the support it runs into (a shoot slides along a wall it
//! cannot hold), then tried straight ahead. A heading whose continuation by one more
//! metamer would be blocked is turned along the obstacle too, since the fate stage
//! holds back a bud whose next step is blocked, and when all are blocked the shoot ends (as
//! [`crate::Confined`] does).

use super::support::{Contact, Supports};
use crate::math::{UP, perpendicular, reject, tilt};
use crate::stage::{Context, HeadingInput, ShootOrientation};
use crate::surroundings::Surroundings;
use arbrume_core::species::{Attachment, Climbing};
use glam::Vec3;

/// Fraction of the turn asked by the wrapped stage that a free shoot takes.
const FREE_TURN: f32 = 0.25;

/// What holds the tip of a shoot.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Grip {
    /// Held by a support, and turning towards another one when there is one.
    Held(Contact, Option<Contact>),
    /// Turning towards a support.
    Searching(Contact),
    /// Free.
    Free,
}

/// Orientation of a climbing plant. See the [module documentation](self).
#[derive(Debug, Clone)]
pub struct Climb<'a, O> {
    inner: O,
    climbing: &'a Climbing,
    supports: Supports<'a>,
    surroundings: &'a Surroundings,
    origin: Vec3,
    scale: f32,
    /// Distance from the axis of a held stem to the surface during growth.
    offset: f32,
}

impl<'a, O> Climb<'a, O> {
    /// `inner` climbing the supports of `surroundings` for the run `cx`. The species of
    /// `cx` must have a `[climbing]` table.
    ///
    /// # Panics
    ///
    /// When the species has no `[climbing]` table.
    #[must_use]
    pub fn new(inner: O, cx: &Context<'a>, surroundings: &'a Surroundings) -> Self {
        let climbing = cx
            .species
            .climbing
            .as_ref()
            .expect("a climbing species has a [climbing] table");
        let ground = climbing.ground.then_some(0.0);
        Self {
            inner,
            climbing,
            supports: Supports::new(surroundings, ground),
            surroundings,
            origin: Vec3::ZERO,
            scale: cx.scale,
            offset: climbing.gap + cx.species.geometry.tip_radius * cx.scale,
        }
    }

    /// The same stage for a plant standing at `origin` in the frame of the surroundings.
    #[must_use]
    pub fn at(mut self, origin: Vec3) -> Self {
        let ground = self.climbing.ground.then_some(origin.y);
        self.supports = Supports::new(self.surroundings, ground);
        self.origin = origin;
        self
    }

    /// What holds a tip at `point`, in the frame of the surroundings.
    fn grip(&self, point: Vec3) -> Grip {
        let climbing = self.climbing;
        let thin = climbing.needs_thin_support();
        let thickness = climbing.max_support_thickness;
        let search = climbing.search_distance + self.offset;
        let accept = |contact: &Contact| !thin || self.supports.is_thin(contact, point, thickness);
        let support = self.supports.nearest(point, search, accept);
        let reach = climbing.reach + self.offset;
        if let Some(contact) = support.filter(|contact| contact.distance <= reach) {
            return Grip::Held(contact, None);
        }
        let ground = self.supports.ground_contact(point);
        if let Some(ground) = ground.filter(|ground| ground.distance <= reach) {
            return Grip::Held(ground, support);
        }
        support.map_or(Grip::Free, Grip::Searching)
    }

    /// Heading along the surface of a held metamer that wants `wanted`.
    fn follow(&self, wanted: Vec3, held: (&Contact, Option<&Contact>)) -> Vec3 {
        let (contact, target) = held;
        let normal = contact.normal;
        let mut wanted = wanted;
        if let Some(target) = target {
            let weight = self.climbing.seek_weight;
            wanted = (wanted * (1.0 - weight) - target.normal * weight).normalize_or(wanted);
        }
        let along = reject(wanted, normal)
            .try_normalize()
            .unwrap_or_else(|| perpendicular(normal, UP));
        if self.climbing.attachment != Attachment::Twining || contact.support.is_none() {
            return along;
        }
        let upward = reject(UP, normal).try_normalize().unwrap_or(along);
        let angle = self.climbing.twine_angle_deg.to_radians();
        tilt(upward, normal.cross(upward), angle)
    }

    /// `heading` with the end of the metamer moved to the stem offset from the support
    /// of `contact`. A metamer comes closer to the surface by at most a third of its distance
    /// from it, so the straight continuation of the shoot, which the fate stage checks,
    /// stays out of the support.
    fn snap(&self, tip: Vec3, heading: Vec3, step: f32, contact: &Contact) -> Vec3 {
        let end = tip + heading * step;
        let there = self.supports.contact_with(contact, end);
        let snapped = there.surface_point(end, self.offset);
        let normal = there.normal;
        let closest = -contact.distance.max(0.0) / 3.0;
        let rise = (snapped - tip).dot(normal).max(closest).clamp(-step, step);
        let Some(along) = reject(snapped - tip, normal).try_normalize() else {
            return (snapped - tip).try_normalize().unwrap_or(heading);
        };
        let across = (step * step - rise * rise).max(0.0).sqrt();
        (along * across + normal * rise) / step
    }

    /// `heading` bent towards the ground for a free metamer of length `step`.
    fn droop(&self, heading: Vec3, step: f32) -> Vec3 {
        let Some(down) = reject(-UP, heading).try_normalize() else {
            return heading;
        };
        let room = heading.dot(-UP).clamp(-1.0, 1.0).acos();
        let angle = (self.climbing.droop.to_radians() * step).min(room);
        tilt(heading, down, angle).normalize()
    }

    /// `heading` with its end kept at or above the stem offset over the ground.
    fn above_ground(&self, tip: Vec3, heading: Vec3, step: f32) -> Vec3 {
        let Some(ground) = self.supports.ground() else {
            return heading;
        };
        let mut end = tip + heading * step;
        let lowest = ground + self.offset;
        if end.y >= lowest || heading.y >= 0.0 {
            return heading;
        }
        end.y = lowest.min(tip.y.max(lowest));
        (end - tip).try_normalize().unwrap_or(heading)
    }

    /// `heading` turned along the surface of the support it would run into, the shoot
    /// sliding along an obstacle it cannot hold; zero when there is none.
    fn slide(&self, tip: Vec3, heading: Vec3, step: f32) -> Vec3 {
        let end = tip + heading * step;
        let Some(contact) = self.supports.nearest(end, step + self.offset, |_| true) else {
            return Vec3::ZERO;
        };
        reject(heading, contact.normal)
            .try_normalize()
            .map_or(Vec3::ZERO, |along| self.snap(tip, along, step, &contact))
    }

    /// Whether a metamer of length `step` may not go from `tip` in `heading`.
    fn blocked(&self, tip: Vec3, heading: Vec3, step: f32) -> bool {
        let end = tip + heading * step;
        let digs = end.y < self.origin.y && heading.y < 0.0;
        digs || self.surroundings.blocks_step(tip, end)
    }
}

impl<O: ShootOrientation> ShootOrientation for Climb<'_, O> {
    fn heading(&mut self, input: &HeadingInput) -> Vec3 {
        let wanted = self.inner.heading(input);
        let tip = input.tip + self.origin;
        let step = input.internode * self.scale;
        let heading = match self.grip(tip) {
            Grip::Held(contact, target) => {
                let along = self.follow(wanted, (&contact, target.as_ref()));
                self.snap(tip, along, step, &contact)
            }
            Grip::Searching(contact) => {
                let weight = self.climbing.seek_weight;
                (wanted * (1.0 - weight) - contact.normal * weight).normalize_or(wanted)
            }
            Grip::Free => {
                let kept = input.heading.lerp(wanted, FREE_TURN).normalize_or(wanted);
                self.droop(kept, step)
            }
        };
        let heading = self.above_ground(tip, heading, step);
        let ahead = tip + heading * step;
        let slid = if self.blocked(ahead, heading, step) {
            self.slide(ahead, heading, step)
        } else {
            self.slide(tip, heading, step)
        };
        let candidates = [heading, slid, input.heading];
        let open = |heading: &Vec3| *heading != Vec3::ZERO && !self.blocked(tip, *heading, step);
        let clear =
            |heading: &Vec3| open(heading) && !self.blocked(tip + *heading * step, *heading, step);
        candidates
            .iter()
            .find(|heading| clear(heading))
            .or_else(|| candidates.iter().find(|heading| open(heading)))
            .copied()
            .unwrap_or(Vec3::ZERO)
    }
}

#[cfg(test)]
mod tests {
    use super::Climb;
    use crate::plant::MeristemId;
    use crate::stage::{Context, HeadingInput, ShootOrientation, Straight};
    use crate::surroundings::{Occluder, Shape, Surroundings};
    use arbrume_core::species::{Attachment, Climbing, Species};
    use glam::Vec3;

    fn species(climbing: Climbing) -> Species {
        let species = Species {
            climbing: Some(climbing),
            ..Species::default()
        };
        species.validate().unwrap();
        species
    }

    fn input(tip: Vec3, heading: Vec3) -> HeadingInput {
        HeadingInput {
            meristem: MeristemId(0),
            key: 1,
            order: 0,
            heading,
            side: heading.any_orthonormal_vector(),
            tip,
            preferred: Vec3::ZERO,
            position: 0.0,
            internode: 0.05,
            axis_length: 5.0,
            node_index: 0,
            unit_metamers: 1,
            cycle: 0,
        }
    }

    fn context(species: &Species) -> Context<'_> {
        Context {
            species,
            seed: 1,
            age: 1,
            scale: 1.0,
        }
    }

    /// Walks `steps` metamers from `tip` and returns the tips.
    fn walk(
        stage: &mut impl ShootOrientation,
        tip: Vec3,
        heading: Vec3,
        steps: usize,
    ) -> Vec<Vec3> {
        let (mut tip, mut heading) = (tip, heading);
        let mut tips = vec![tip];
        for _ in 0..steps {
            let next = stage.heading(&input(tip, heading));
            assert!(next != Vec3::ZERO && (next.length() - 1.0).abs() < 1e-4);
            (tip, heading) = (tip + next * 0.05, next);
            tips.push(tip);
        }
        tips
    }

    #[test]
    fn a_held_stem_follows_a_wall_at_its_offset() {
        let species = species(Climbing::default());
        let wall = Shape::Box {
            min: Vec3::new(-3.0, 0.0, 0.1),
            max: Vec3::new(3.0, 4.0, 0.4),
        };
        let surroundings = Surroundings {
            envelope: None,
            occluders: vec![Occluder::obstacle(wall.clone())],
        };
        let cx = context(&species);
        let mut stage = Climb::new(Straight, &cx, &surroundings);
        let into = Vec3::new(0.0, 0.6, 0.8).normalize();
        let tips = walk(&mut stage, Vec3::new(0.0, 0.5, 0.05), into, 30);
        let offset = species.climbing.as_ref().unwrap().gap + species.geometry.tip_radius;
        let distances: Vec<f32> = tips.iter().map(|tip| wall.distance(*tip)).collect();
        assert!(distances.windows(2).all(|pair| pair[1] <= pair[0] + 1e-4));
        assert!(distances.iter().all(|&distance| distance > offset - 1e-4));
        for distance in &distances[12..] {
            assert!((distance - offset).abs() < 2e-3, "{distance}");
        }
        assert!(tips.last().unwrap().y > 1.8);
    }

    #[test]
    fn a_free_shoot_droops_until_it_hangs() {
        let climbing = Climbing {
            ground: false,
            ..Climbing::default()
        };
        let species = species(climbing);
        let surroundings = Surroundings::none();
        let cx = context(&species);
        let mut stage = Climb::new(Straight, &cx, &surroundings);
        let tips = walk(&mut stage, Vec3::new(0.0, 5.0, 0.0), Vec3::X, 60);
        let last = tips[tips.len() - 1] - tips[tips.len() - 2];
        assert!(last.normalize().y < -0.95, "{last}");
        assert!(tips.windows(2).all(|pair| pair[1].y <= pair[0].y + 1e-6));
    }

    #[test]
    fn a_twining_stem_winds_around_a_post() {
        let climbing = Climbing {
            attachment: Attachment::Twining,
            ..Climbing::default()
        };
        let species = species(climbing);
        let post = Shape::Cylinder {
            base: Vec3::new(0.0, 0.0, 0.0),
            radius: 0.05,
            height: 3.0,
        };
        let surroundings = Surroundings {
            envelope: None,
            occluders: vec![Occluder::obstacle(post.clone())],
        };
        let cx = context(&species);
        let mut stage = Climb::new(Straight, &cx, &surroundings);
        let tips = walk(&mut stage, Vec3::new(0.07, 0.3, 0.0), Vec3::Y, 40);
        let mut turned = 0.0;
        for pair in tips[1..].windows(2) {
            let (a, b) = (pair[0], pair[1]);
            turned += (a.x * b.z - a.z * b.x).atan2(a.x * b.x + a.z * b.z);
            assert!(post.distance(b) > 0.0);
        }
        assert!(turned.abs() > std::f32::consts::TAU, "{turned}");
        assert!(tips.last().unwrap().y > 0.6);
    }

    #[test]
    fn tendrils_do_not_hold_a_thick_wall() {
        let climbing = Climbing {
            attachment: Attachment::Tendril,
            ground: false,
            ..Climbing::default()
        };
        let species = species(climbing);
        let wall = Shape::Box {
            min: Vec3::new(-3.0, 0.0, 0.1),
            max: Vec3::new(3.0, 4.0, 1.1),
        };
        let surroundings = Surroundings {
            envelope: None,
            occluders: vec![Occluder::obstacle(wall)],
        };
        let cx = context(&species);
        let stage = Climb::new(Straight, &cx, &surroundings);
        assert_eq!(stage.grip(Vec3::new(0.0, 1.0, 0.05)), super::Grip::Free);
    }
}
