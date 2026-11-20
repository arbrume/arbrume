//! Limb strokes: which axis takes each one, and the orientation stage that makes it
//! follow the stroke.
//!
//! The axis of a limb turns each of its metamers towards the point of the stroke one
//! *lookahead* further along than the closest point to the tip (the closest point is
//! searched forward from the progress already made, so a stroke that comes back near
//! itself is followed in order). The lookahead is twice the length of the metamer, a
//! choice of this implementation: the axis converges onto the stroke within a few
//! metamers and cuts the bends of the stroke by about a lookahead at most.

use std::cell::RefCell;
use std::rc::Rc;

use crate::plant::{MeristemId, MeristemState, Plant, SiteKind};
use crate::stage::{HeadingInput, ShootOrientation};
use glam::Vec3;

use super::stroke::Polyline;
use super::{Stroke, StrokeKind};

/// Lookahead of the guiding direction, in metamer lengths.
const LOOKAHEAD: f32 = 2.0;

/// One limb stroke and the axis following it.
#[derive(Debug, Clone)]
pub(super) struct Limb {
    /// Index of the stroke in the guides.
    pub stroke: usize,
    pub line: Polyline,
    pub radius: f32,
    pub follow: f32,
    pub cycle: u16,
    /// Meristem following the stroke now.
    pub claimed: Option<MeristemId>,
    /// Every meristem that followed the stroke, relays included, with the cycle it took
    /// the stroke.
    pub followers: Vec<(MeristemId, u16)>,
    /// Arc length of the stroke reached so far.
    pub progress: f32,
    /// Arc length where the axis took the stroke.
    pub start: f32,
    /// The axis has stopped: no meristem follows the stroke any more.
    pub done: bool,
}

/// The limb strokes of a run, shared by the environment and the orientation stages.
#[derive(Debug, Clone, Default)]
pub(super) struct Steering {
    pub limbs: Vec<Limb>,
}

impl Steering {
    /// The limb strokes of `strokes`.
    pub fn new(strokes: &[Stroke]) -> Self {
        let limbs = strokes
            .iter()
            .enumerate()
            .filter(|(_, stroke)| stroke.kind == StrokeKind::Limb)
            .map(|(index, stroke)| Limb {
                stroke: index,
                line: Polyline::new(&stroke.points),
                radius: stroke.radius,
                follow: stroke.follow,
                cycle: stroke.cycle,
                claimed: None,
                followers: Vec::new(),
                progress: 0.0,
                start: 0.0,
                done: false,
            })
            .collect();
        Self { limbs }
    }

    /// Updates which meristem follows each active limb in `cycle`: a relay takes over
    /// from the axis it continues, a finished axis ends its limb, and a limb without an
    /// axis takes the closest candidate within its radius.
    pub fn claim(&mut self, plant: &Plant, cycle: u16) {
        for limb in &mut self.limbs {
            if limb.done || limb.cycle > cycle {
                continue;
            }
            if let Some(id) = limb.claimed {
                follow_relay(limb, plant, id, cycle);
            } else if let Some(id) = candidate(limb, plant) {
                let (arc, _) = limb
                    .line
                    .closest(position(plant, id), 0.0, limb.line.length());
                (limb.claimed, limb.progress, limb.start) = (Some(id), arc, arc);
                limb.followers.push((id, cycle));
            }
        }
    }

    /// The live limb followed by meristem `id`.
    fn limb_of(&mut self, id: MeristemId) -> Option<&mut Limb> {
        self.limbs
            .iter_mut()
            .find(|limb| !limb.done && limb.claimed == Some(id))
    }
}

/// Moves the claim of `limb` from meristem `id` to its relay when its apex died, and ends
/// the limb when the axis is done.
fn follow_relay(limb: &mut Limb, plant: &Plant, id: MeristemId, cycle: u16) {
    match plant.meristem(id).state {
        MeristemState::Resting | MeristemState::Growing => {}
        MeristemState::Relayed => {
            let relay = plant
                .meristems
                .iter()
                .position(|m| m.parent == Some(id) && m.kind == SiteKind::Relay);
            limb.claimed = relay.map(|index| MeristemId(index as u32));
            limb.done = relay.is_none();
            if let Some(relay) = limb.claimed {
                limb.followers.push((relay, cycle));
            }
        }
        MeristemState::Done => {
            limb.claimed = None;
            limb.done = true;
        }
    }
}

/// Where meristem `id` grows from: its bud, or its tip before it has one.
pub(super) fn position(plant: &Plant, id: MeristemId) -> Vec3 {
    let meristem = plant.meristem(id);
    if meristem.bud.is_some() {
        plant.graph.bud_position(meristem.bud)
    } else {
        meristem.tip
    }
}

/// The meristem that takes `limb`: a stem when the stroke starts within its radius of
/// the ground, else a lateral; among those that can still grow, the closest to the start
/// of the stroke within its radius, the smallest id on a tie.
fn candidate(limb: &Limb, plant: &Plant) -> Option<MeristemId> {
    let start = limb.line.at(0.0);
    let stem = start.y <= limb.radius;
    let mut best: Option<(f32, MeristemId)> = None;
    for (index, meristem) in plant.meristems.iter().enumerate() {
        let alive = matches!(
            meristem.state,
            MeristemState::Resting | MeristemState::Growing
        );
        let wanted = if stem {
            meristem.kind == SiteKind::Seed
        } else {
            meristem.kind == SiteKind::Lateral
        };
        if !alive || !wanted {
            continue;
        }
        let id = MeristemId(index as u32);
        let distance = position(plant, id).distance(start);
        if distance <= limb.radius && best.is_none_or(|(d, _)| distance < d) {
            best = Some((distance, id));
        }
    }
    best.map(|(_, id)| id)
}

/// Unit direction from `tip` towards the point of `limb` one lookahead past the closest
/// point at or after its progress, and the arc length of that closest point. `None`
/// once the stroke is behind the tip.
pub(super) fn guiding_direction(limb: &Limb, tip: Vec3, step: f32) -> Option<(Vec3, f32)> {
    let length = limb.line.length();
    let lookahead = LOOKAHEAD * step;
    let distance = limb.line.at(limb.progress).distance(tip);
    let window = limb.progress + 2.0 * (distance + lookahead);
    let (arc, _) = limb.line.closest(tip, limb.progress, window);
    if arc >= length {
        return None;
    }
    let target = limb.line.at(arc + lookahead);
    let direction = (target - tip).try_normalize()?;
    Some((direction, arc))
}

/// An orientation stage whose limb axes follow their strokes. See the
/// [module documentation](self).
#[derive(Debug, Clone)]
pub(super) struct Steered<O> {
    inner: O,
    steering: Rc<RefCell<Steering>>,
    scale: f32,
}

impl<O> Steered<O> {
    /// `inner`, with the limbs of `steering` for a run at `scale`.
    pub const fn new(inner: O, steering: Rc<RefCell<Steering>>, scale: f32) -> Self {
        Self {
            inner,
            steering,
            scale,
        }
    }
}

impl<O: ShootOrientation> ShootOrientation for Steered<O> {
    fn heading(&mut self, input: &HeadingInput) -> Vec3 {
        let heading = self.inner.heading(input);
        let mut steering = self.steering.borrow_mut();
        let Some(limb) = steering.limb_of(input.meristem) else {
            return heading;
        };
        let step = input.internode * self.scale;
        let Some((direction, arc)) = guiding_direction(limb, input.tip, step) else {
            limb.progress = limb.line.length();
            return heading;
        };
        limb.progress = arc;
        if heading == Vec3::ZERO {
            return heading;
        }
        let blended = heading * (1.0 - limb.follow) + direction * limb.follow;
        blended.try_normalize().unwrap_or(heading)
    }
}

#[cfg(test)]
mod tests {
    use super::{Limb, guiding_direction};
    use crate::guides::stroke::Polyline;
    use glam::Vec3;

    fn limb() -> Limb {
        Limb {
            stroke: 0,
            line: Polyline::new(&[Vec3::ZERO, Vec3::Y * 4.0, Vec3::new(4.0, 4.0, 0.0)]),
            radius: 0.5,
            follow: 1.0,
            cycle: 0,
            claimed: None,
            followers: Vec::new(),
            progress: 0.0,
            start: 0.0,
            done: false,
        }
    }

    #[test]
    fn the_guiding_direction_leads_along_the_stroke() {
        let mut limb = limb();
        let (direction, arc) = guiding_direction(&limb, Vec3::Y, 0.25).unwrap();
        assert!((arc - 1.0).abs() < 1e-6);
        assert!((direction - Vec3::Y).length() < 1e-6);
        let (direction, _) = guiding_direction(&limb, Vec3::new(1.0, 1.0, 0.0), 0.25).unwrap();
        assert!(direction.x < 0.0 && direction.y > 0.0);
        limb.progress = 3.0;
        let (_, arc) = guiding_direction(&limb, Vec3::new(0.2, 0.5, 0.0), 0.25).unwrap();
        assert!(arc >= 3.0);
        assert!(guiding_direction(&limb, Vec3::new(4.5, 4.0, 0.0), 0.25).is_none());
    }
}
