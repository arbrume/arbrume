//! Keeping shoots inside the envelope, out of the obstacles and above the ground.
//!
//! The fate stage only allots the metamers that fit along the straight continuation of
//! an axis. The orientation stage then bends each metamer, so the last word is here:
//! [`Confined`] wraps an orientation stage and checks every metamer where it will
//! really be. A metamer whose end is blocked ([`Surroundings::blocks_step`], or the
//! ground for a shoot heading down) is tried again straight ahead, and when that is
//! blocked too the shoot ends there. No node is ever placed inside a blocking occluder
//! or, once the axis is inside it, outside the envelope.

use crate::stage::{Context, HeadingInput, ShootOrientation};
use crate::surroundings::Surroundings;
use glam::Vec3;

/// An orientation stage confined by surroundings. See the [module documentation](self).
#[derive(Debug, Clone, Copy)]
pub struct Confined<'a, O> {
    inner: O,
    surroundings: &'a Surroundings,
    origin: Vec3,
    scale: f32,
}

impl<'a, O> Confined<'a, O> {
    /// `inner` confined by `surroundings` for the run `cx`.
    #[must_use]
    pub const fn new(inner: O, cx: &Context, surroundings: &'a Surroundings) -> Self {
        Self {
            inner,
            surroundings,
            origin: Vec3::ZERO,
            scale: cx.scale,
        }
    }

    /// The same stage for a plant standing at `origin` in the frame of the surroundings.
    #[must_use]
    pub fn at(mut self, origin: Vec3) -> Self {
        self.origin = origin;
        self
    }

    /// Whether the metamer of `input` may not take the direction `heading`.
    fn blocked(&self, input: &HeadingInput, heading: Vec3) -> bool {
        let end = input.tip + heading * (input.internode * self.scale);
        let digs = end.y < 0.0 && heading.y < 0.0;
        let (from, to) = (input.tip + self.origin, end + self.origin);
        digs || self.surroundings.blocks_step(from, to)
    }
}

impl<O: ShootOrientation> ShootOrientation for Confined<'_, O> {
    fn heading(&mut self, input: &HeadingInput) -> Vec3 {
        let heading = self.inner.heading(input);
        if !self.blocked(input, heading) {
            heading
        } else if self.blocked(input, input.heading) {
            Vec3::ZERO
        } else {
            input.heading
        }
    }
}
