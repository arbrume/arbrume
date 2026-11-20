//! Orientation of the shoots of an aquatic plant.
//!
//! [`Submerged`] wraps the orientation stage of the growth mode. Below the surface the
//! heading `H` the stage gives is blended with the upward direction,
//! `H' = (1 - b) H + b Y` with `b` the `buoyancy` of the species: buoyant tissue rises
//! whatever the gravitropic set-point, and a shoot of set-point 0 rises straight. At
//! the surface (the tip within a quarter of an internode below it, or above) an axis of
//! the `spread` habit turns level and aims each metamer at the surface, so that a
//! canopy runs along it; an axis of the `stop` habit grows until its tip is at the
//! surface (within half of `SURFACE_BAND`), then ends its shoot, its apex alive, and
//! grows no further. The step that crosses the surface is kept as it is; the
//! dressing of the grown plant brings every node back under the surface.

use crate::math::{UP, reject};
use crate::stage::{Context, HeadingInput, ShootOrientation};
use crate::water::Water;
use arbrume_core::species::{Aquatic, SurfaceHabit};
use glam::Vec3;

/// Fraction of an internode below the surface from which a tip is at the surface.
const AT_SURFACE: f32 = 0.25;

/// Largest slope, as the sine of the angle, at which a spreading axis returns to the
/// surface from below or above.
const SURFACE_SLOPE: f32 = 0.5;

/// An orientation stage under water. See the [module documentation](self).
#[derive(Debug, Clone, Copy)]
pub struct Submerged<O> {
    inner: O,
    water: Water,
    habit: SurfaceHabit,
    buoyancy: f32,
    scale: f32,
}

impl<O> Submerged<O> {
    /// `inner` under `water` for the run `cx` of a species with the layer `aquatic`.
    #[must_use]
    pub const fn new(inner: O, cx: &Context, aquatic: &Aquatic, water: Water) -> Self {
        Self {
            inner,
            water,
            habit: aquatic.surface,
            buoyancy: aquatic.buoyancy,
            scale: cx.scale,
        }
    }

    /// The level heading of a spreading axis at the surface: `heading` without its
    /// vertical part, turned up or down towards the surface by at most
    /// [`SURFACE_SLOPE`].
    fn along_surface(&self, input: &HeadingInput, heading: Vec3) -> Vec3 {
        let level = reject(heading, UP)
            .try_normalize()
            .or_else(|| reject(input.side, UP).try_normalize())
            .unwrap_or(Vec3::X);
        let step = (input.internode * self.scale).max(f32::MIN_POSITIVE);
        let rise = ((self.water.level() - input.tip.y) / step).clamp(-SURFACE_SLOPE, SURFACE_SLOPE);
        level * (1.0 - rise * rise).sqrt() + UP * rise
    }
}

impl<O: ShootOrientation> ShootOrientation for Submerged<O> {
    fn heading(&mut self, input: &HeadingInput) -> Vec3 {
        let heading = self.inner.heading(input);
        if heading == Vec3::ZERO {
            return heading;
        }
        let step = input.internode * self.scale;
        let surface = self.water.level();
        match self.habit {
            SurfaceHabit::Stop if input.tip.y >= surface - 0.5 * super::SURFACE_BAND => {
                return Vec3::ZERO;
            }
            SurfaceHabit::Spread if input.tip.y >= surface - AT_SURFACE * step => {
                return self.along_surface(input, heading);
            }
            _ => {}
        }
        let b = self.buoyancy;
        (heading * (1.0 - b) + UP * b)
            .try_normalize()
            .unwrap_or(heading)
    }
}

#[cfg(test)]
mod tests {
    use super::Submerged;
    use crate::stage::{Context, HeadingInput, ShootOrientation, Straight};
    use crate::water::Water;
    use arbrume_core::species::{Aquatic, Species, SurfaceHabit};
    use glam::Vec3;

    fn input(tip: Vec3, heading: Vec3) -> HeadingInput {
        HeadingInput {
            meristem: crate::plant::MeristemId(0),
            key: 1,
            order: 0,
            heading,
            side: Vec3::Z,
            tip,
            preferred: Vec3::ZERO,
            position: 0.5,
            internode: 0.2,
            axis_length: 10.0,
            node_index: 3,
            unit_metamers: 2,
            cycle: 2,
        }
    }

    fn stage(habit: SurfaceHabit) -> Submerged<Straight> {
        let species = Species::default();
        let cx = Context {
            species: &species,
            seed: 1,
            age: 5,
            scale: 1.0,
        };
        let aquatic = Aquatic {
            surface: habit,
            buoyancy: 0.5,
            ..Aquatic::default()
        };
        Submerged::new(Straight, &cx, &aquatic, Water::still(3.0))
    }

    #[test]
    fn submerged_shoots_rise_and_canopies_spread_at_the_surface() {
        let mut spread = stage(SurfaceHabit::Spread);
        let level = Vec3::X;
        let rising = spread.heading(&input(Vec3::new(0.0, 1.0, 0.0), level));
        assert!(rising.y > 0.6 && (rising.length() - 1.0).abs() < 1e-5);
        let canopy = spread.heading(&input(Vec3::new(0.0, 2.98, 0.0), Vec3::Y));
        assert!(canopy.y.abs() < 0.2 && (canopy.length() - 1.0).abs() < 1e-5);
        let above = spread.heading(&input(Vec3::new(0.0, 3.2, 0.0), level));
        assert!(above.y < 0.0);
        let mut stop = stage(SurfaceHabit::Stop);
        assert_eq!(
            stop.heading(&input(Vec3::new(0.0, 2.99, 0.0), level)),
            Vec3::ZERO
        );
        assert!(stop.heading(&input(Vec3::new(0.0, 1.0, 0.0), level)).y > 0.6);
    }
}
