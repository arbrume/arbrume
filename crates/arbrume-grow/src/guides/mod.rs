//! Guides: strokes painted by an artist that steer self-organizing growth.
//!
//! A [`Guides`] value is a list of [`Stroke`]s in meters in the frame of the plant (the
//! plant stands at the origin, Y up), like the [`crate::Surroundings`], plus the cycles
//! at which the plant was frozen. Three kinds of stroke act on the cycle of
//! [`crate::grow_in`]:
//!
//! * **crown**: markers of free space scattered uniformly in the union of the balls of
//!   radius `radius` centred along the stroke (the procedural brush of Longay et al.
//!   2012, after the space colonization of Runions et al. 2007). Every live bud perceives
//!   them with the perception cone of the species, as in [`crate::Markers`], from the
//!   `reach` of the stroke (the perception distance of the species by default): a longer
//!   reach draws the crown towards a stroke painted away from it. A bud that owns markers
//!   gets the exposure `Q = strength * C` if it had less, prefers the direction `V`
//!   towards them, and its axis may grow one perception distance past its `axis_length`
//!   while it sees them;
//! * **limb**: a polyline that one axis follows. The stroke is taken by a stem when it
//!   starts within `radius` of the ground, else by the first lateral bud found within
//!   `radius` of its start (the selective growth of Longay et al. 2012). That axis gets
//!   `strength` times the light gathered by the whole plant (at least `C`) as its
//!   exposure, so that its share of the resource is about that of the rest of the plant
//!   at every branching point below it; its full-grown length becomes what it needs to reach the end of
//!   the stroke, and each of its metamers turns towards a point one lookahead further
//!   along the stroke than the closest point (a guiding direction field in the spirit
//!   of the guiding vectors of Xu and Mould 2015), blended with the heading of the
//!   species by `follow`. With `density > 0` the tube of `radius` around the stroke
//!   also holds crown markers, so the limb bears leafy shoots;
//! * **erase**: the union of balls of `radius` along the stroke blocks growth from the
//!   stroke's cycle on, like a blocking occluder; the markers of the guides inside it are
//!   dropped, and at that cycle (when it is after the first) every branch with a live
//!   node inside it is shed with all it bears. Stems are never shed.
//!
//! # Cycles and freezing
//!
//! A stroke acts from its `cycle` on (0 for the whole growth). [`Guides::freezes`] lists
//! cycles at which the plant was frozen: from a freeze on, only the buds that own markers
//! of strokes of that cycle or later, and the axes that took a limb stroke of that cycle
//! or later, receive light, and nothing is shed by exposure. The plant grown before a
//! freeze is therefore kept exactly: the stages never look at the age of the run, so the
//! first cycles of a longer run are those of the shorter one, and a frozen plant only
//! gains growth towards the new strokes. The positions, births and orders of the nodes
//! born before the freeze are equal to the bit to those of the plant grown to the freeze
//! (radii follow the pipe model of the whole plant, and bending under weight, when the
//! species asks for it, carries the new weight).
//!
//! The growth state is serialized as the guides themselves: the strokes with their cycles
//! and the freezes, a few kilobytes of JSON, from which the plant at any cycle is grown
//! again. A resumable snapshot of the engine would hold the shadow grid (megabytes for a
//! large tree) and depend on the internal layout of every stage, while growing again
//! costs milliseconds (see `docs/brush.md`).
//!
//! # Determinism
//!
//! Markers are drawn from the seed of the plant, the index of the stroke and the index
//! of the ball, with the counter-based generator of `arbrume-core`. Everything else is a
//! function of the plant and the strokes. Without strokes and freezes the plant is that
//! of [`crate::grow_in`], which [`grow_guided`] then calls.

mod run;
mod sense;
mod steer;
mod stroke;

pub use run::{grow_guided, grow_guided_timeline, grow_guided_with};
pub use stroke::{Polyline, stroke_balls};

use glam::Vec3;
use serde::{Deserialize, Serialize};

/// What a stroke does. See the [module documentation](self).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StrokeKind {
    /// Markers of free space the crown grows into.
    Crown,
    /// A polyline one axis follows.
    Limb,
    /// A volume that blocks growth and prunes what grew there.
    Erase,
}

/// One stroke, in meters in the frame of the plant.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stroke {
    /// What the stroke does.
    pub kind: StrokeKind,
    /// Points of the stroke, in the order they were drawn.
    pub points: Vec<Vec3>,
    /// Radius of the brush: of the balls of a crown or erase stroke, of the tube of
    /// markers of a limb, and the distance within which a limb finds its axis.
    pub radius: f32,
    /// Markers per cubic internode unit, as a multiple of the `marker_density` of the
    /// species. Crown and limb strokes; a limb has no markers at 0.
    #[serde(default = "one")]
    pub density: f32,
    /// Exposure given to the buds a crown or limb stroke serves, as a multiple of the
    /// `full_exposure` of the species.
    #[serde(default = "one")]
    pub strength: f32,
    /// Weight in `[0, 1]` of the stroke direction against the heading of the species,
    /// for each metamer of a limb.
    #[serde(default = "one")]
    pub follow: f32,
    /// Distance in meters from which buds perceive the markers of a crown or limb
    /// stroke; 0 for the `perception_distance` of the species.
    #[serde(default)]
    pub reach: f32,
    /// First growth cycle in which the stroke acts.
    #[serde(default)]
    pub cycle: u16,
}

const fn one() -> f32 {
    1.0
}

impl Stroke {
    /// A stroke of `kind` through `points` with brush `radius`, acting from the first
    /// cycle with the default density, strength and follow (all 1).
    #[must_use]
    pub const fn new(kind: StrokeKind, points: Vec<Vec3>, radius: f32) -> Self {
        Self {
            kind,
            points,
            radius,
            density: 1.0,
            strength: 1.0,
            follow: 1.0,
            reach: 0.0,
            cycle: 0,
        }
    }

    /// The same stroke acting from `cycle` on.
    #[must_use]
    pub const fn from_cycle(mut self, cycle: u16) -> Self {
        self.cycle = cycle;
        self
    }

    /// Checks the points and the numbers of the stroke.
    ///
    /// # Errors
    ///
    /// Returns a message for a stroke without points (a limb needs two distinct ones), a
    /// point that is not finite, a radius that is not positive and finite, a negative or
    /// infinite density or strength, or a follow outside `[0, 1]`.
    pub fn validate(&self) -> Result<(), String> {
        if self.points.is_empty() {
            return Err("a stroke has no points".to_owned());
        }
        if let Some(point) = self.points.iter().find(|p| !p.is_finite()) {
            return Err(format!("stroke point {point} is not finite"));
        }
        if self.kind == StrokeKind::Limb && Polyline::new(&self.points).length() <= 0.0 {
            return Err("a limb stroke needs two distinct points".to_owned());
        }
        let positive = |value: f32| value > 0.0 && value.is_finite();
        let not_negative = |value: f32| value >= 0.0 && value.is_finite();
        if !positive(self.radius) {
            return Err(format!("stroke radius {} is not positive", self.radius));
        }
        let numbers = [self.density, self.strength, self.reach];
        if !numbers.into_iter().all(not_negative) {
            let (density, strength, reach) = (self.density, self.strength, self.reach);
            return Err(format!(
                "stroke density {density}, strength {strength} or reach {reach} is negative or                  not finite"
            ));
        }
        if !(0.0..=1.0).contains(&self.follow) {
            return Err(format!("stroke follow {} is outside [0, 1]", self.follow));
        }
        Ok(())
    }
}

/// Strokes and freezes that steer the growth of one plant. See the
/// [module documentation](self).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Guides {
    /// Strokes, in the order they were drawn.
    pub strokes: Vec<Stroke>,
    /// Cycles at which the plant was frozen, increasing.
    pub freezes: Vec<u16>,
}

impl Guides {
    /// No stroke and no freeze.
    #[must_use]
    pub const fn none() -> Self {
        Self {
            strokes: Vec::new(),
            freezes: Vec::new(),
        }
    }

    /// `true` when there is neither stroke nor freeze: the plant is that of
    /// [`crate::grow_in`].
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.strokes.is_empty() && self.freezes.is_empty()
    }

    /// The freeze in force in `cycle`: the last freeze at or before it.
    #[must_use]
    pub fn freeze_at(&self, cycle: u16) -> Option<u16> {
        self.freezes.iter().copied().filter(|&f| f <= cycle).max()
    }

    /// Checks every stroke and the order of the freezes.
    ///
    /// # Errors
    ///
    /// Returns a message naming the first invalid stroke, or freezes that do not
    /// increase.
    pub fn validate(&self) -> Result<(), String> {
        for (index, stroke) in self.strokes.iter().enumerate() {
            stroke
                .validate()
                .map_err(|reason| format!("stroke {index}: {reason}"))?;
        }
        if self.freezes.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err(format!("freezes {:?} do not increase", self.freezes));
        }
        Ok(())
    }
}

/// What the guides did to one plant, stroke by stroke.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct GuideReport {
    /// One entry per stroke, in the order of [`Guides::strokes`].
    pub strokes: Vec<StrokeReport>,
}

/// What one stroke did. Distances are in meters, measured on the grown plant before
/// bending under weight.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct StrokeReport {
    /// Markers scattered for the stroke (crown, limb with a density).
    pub markers: u32,
    /// Those markers lying within the occupancy radius of a live node of the grown
    /// plant: the share of the painted volume the plant fills.
    pub reached: u32,
    /// Whether an axis took the limb stroke.
    pub claimed: bool,
    /// Arc length of the limb stroke along which its axis grew, from the closest point
    /// to where the axis took the stroke.
    pub followed: f32,
    /// Length of the limb stroke.
    pub length: f32,
    /// Mean distance to the stroke of the live nodes its axis built while following it.
    pub mean_distance: f32,
    /// Largest such distance.
    pub max_distance: f32,
    /// Nodes shed by the erase stroke.
    pub pruned: u32,
}

#[cfg(test)]
mod tests {
    use super::{Guides, Stroke, StrokeKind};
    use glam::Vec3;

    #[test]
    fn guides_read_from_json_with_defaults() {
        let text = r#"{"strokes": [{"kind": "crown", "points": [[1, 2, 3]], "radius": 0.5},
            {"kind": "limb", "points": [[0, 0, 0], [0, 1, 0]], "radius": 0.2, "follow": 0.5,
             "cycle": 4}], "freezes": [4]}"#;
        let guides: Guides = serde_json::from_str(text).unwrap();
        guides.validate().unwrap();
        assert_eq!(guides.strokes[0].points, vec![Vec3::new(1.0, 2.0, 3.0)]);
        assert_eq!(guides.strokes[0].density, 1.0);
        assert_eq!(guides.strokes[1].kind, StrokeKind::Limb);
        assert_eq!(guides.strokes[1].cycle, 4);
        assert_eq!(guides.freeze_at(3), None);
        assert_eq!(guides.freeze_at(9), Some(4));
        let back: Guides = serde_json::from_str(&serde_json::to_string(&guides).unwrap()).unwrap();
        assert_eq!(back, guides);
        assert!(serde_json::from_str::<Guides>(r#"{"stroke": []}"#).is_err());
    }

    #[test]
    fn invalid_strokes_are_rejected() {
        let point = vec![Vec3::ZERO];
        let cases = [
            Stroke::new(StrokeKind::Crown, Vec::new(), 1.0),
            Stroke::new(StrokeKind::Crown, point.clone(), 0.0),
            Stroke::new(StrokeKind::Limb, vec![Vec3::ONE, Vec3::ONE], 1.0),
            Stroke::new(StrokeKind::Erase, vec![Vec3::NAN], 1.0),
            Stroke {
                follow: 2.0,
                ..Stroke::new(StrokeKind::Limb, vec![Vec3::ZERO, Vec3::Y], 1.0)
            },
            Stroke {
                density: -1.0,
                ..Stroke::new(StrokeKind::Crown, point, 1.0)
            },
        ];
        for stroke in cases {
            assert!(stroke.validate().is_err(), "{stroke:?}");
        }
        let unordered = Guides {
            strokes: Vec::new(),
            freezes: vec![5, 5],
        };
        assert!(unordered.validate().is_err());
    }
}
