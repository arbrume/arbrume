//! The pluggable stages of the growth cycle and the data they exchange.
//!
//! One cycle runs [`Environment::sense`], then [`BudFate::decide`], then builds each
//! shoot with [`ShootOrientation::heading`], and ends with [`Shedding::shed`]. The
//! structure is that of Palubicki et al. (2009, section 4) and Longay et al. (2012,
//! section 3.2). What does not vary between growth modes (where leaves and buds sit
//! around a node, the insertion angle of a bud, how an apex ends) belongs to the shoot
//! builder and reads the species directly.

use crate::plant::{MeristemId, Plant, SiteKind};
use arbrume_core::species::Species;
use glam::Vec3;
use std::fmt::Debug;

/// What every stage knows about the run.
#[derive(Debug, Clone, Copy)]
pub struct Context<'a> {
    /// The species grown.
    pub species: &'a Species,
    /// Seed of every random draw.
    pub seed: u64,
    /// Number of growth cycles of the run.
    pub age: u16,
    /// Factor applied to every length written to the graph. The stages work in the
    /// unscaled lengths of the species.
    pub scale: f32,
}

/// A place where a bud may form, offered to [`BudFate::admit`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BudSite {
    /// Id the meristem gets when the bud is admitted.
    pub id: MeristemId,
    /// Key the meristem gets: the seed of its random draws, the same at every age of
    /// the plant.
    pub key: u64,
    /// Seed, lateral or relay.
    pub kind: SiteKind,
    /// Branching order of the axis the bud would build.
    pub order: u8,
    /// Meristem of the bearing axis; `None` for a seed.
    pub parent: Option<MeristemId>,
    /// Index of the bearing node among the metamers of the parent meristem; the stem
    /// number for a seed.
    pub node_index: u32,
    /// Index of the bud among those of its node.
    pub slot: u32,
    /// Relative position of the bearing node along the bearing axis.
    pub position: f32,
    /// Arc length of the bearing node along the bearing axis, in meters.
    pub arc: f32,
    /// Position of the bearing node in the shoot of the current cycle, from just above
    /// 0 for the first node to 1 for the last.
    pub unit_position: f32,
    /// Current growth cycle.
    pub cycle: u16,
}

/// Decision that a meristem extends in the current cycle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Flush {
    /// The meristem that grows.
    pub meristem: MeristemId,
    /// Number of metamers of the shoot; they share its length equally.
    pub metamers: u16,
    /// Length of the shoot in meters.
    pub length: f32,
    /// The apex stops for good after this shoot.
    pub last: bool,
}

/// What [`ShootOrientation::heading`] knows about the metamer being added.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HeadingInput {
    /// The growing meristem.
    pub meristem: MeristemId,
    /// Key of the meristem, the seed of its random draws.
    pub key: u64,
    /// Branching order of the axis.
    pub order: u8,
    /// Current unit heading `H`.
    pub heading: Vec3,
    /// Unit vector orthogonal to `heading`, transported along the axis.
    pub side: Vec3,
    /// Direction `V` preferred by the environment; zero when there is none.
    pub preferred: Vec3,
    /// Relative position of the base of the metamer along the full-grown axis.
    pub position: f32,
    /// Length of the metamer, in meters.
    pub internode: f32,
    /// Length of the full-grown axis, in meters.
    pub axis_length: f32,
    /// Index of the metamer among those built by the meristem.
    pub node_index: u32,
    /// Number of metamers of the shoot of the current cycle.
    pub unit_metamers: u16,
    /// Current growth cycle.
    pub cycle: u16,
}

/// Stage 1: gives each bud an exposure `Q` and a preferred direction `V`.
pub trait Environment: Debug {
    /// Writes the exposure and the preferred direction of the buds of `plant.graph`.
    fn sense(&mut self, plant: &mut Plant, cycle: u16);
}

/// Stage 2: decides which buds form, which grow and by how much.
pub trait BudFate: Debug {
    /// Whether a bud forms at `site`. Returns the length in meters of the full-grown
    /// axis the bud would build, or `None` when the bud aborts.
    ///
    /// When it returns `Some`, the caller creates the meristem `site.id` before any
    /// other call to the fate.
    fn admit(&mut self, plant: &Plant, site: &BudSite) -> Option<f32>;

    /// Appends to `flushes`, in increasing id order, the meristems with an id of `from`
    /// or more that grow in `cycle`.
    ///
    /// Called with the first id at the start of a cycle, then again with the first of
    /// the meristems formed since the previous call, until no shoot is returned or no
    /// meristem is formed. A meristem must not be returned twice in one cycle.
    fn decide(&mut self, plant: &Plant, cycle: u16, from: MeristemId, flushes: &mut Vec<Flush>);
}

/// Stage 3: direction of each new metamer.
pub trait ShootOrientation: Debug {
    /// Unit direction of the metamer described by `input`. The metamers of a shoot are
    /// asked in order, base to tip.
    fn heading(&mut self, input: &HeadingInput) -> Vec3;
}

/// Last stage of a cycle: sheds branches.
pub trait Shedding: Debug {
    /// Sheds the branches that do not pay for themselves.
    fn shed(&mut self, plant: &mut Plant, cycle: u16);
}

/// No environment: every bud is fully exposed (`Q = 1`) and has no preferred direction.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NoEnvironment;

impl Environment for NoEnvironment {
    fn sense(&mut self, plant: &mut Plant, _cycle: u16) {
        plant.graph.buds_mut().exposure.fill(1.0);
    }
}

/// No shedding.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NoShedding;

impl Shedding for NoShedding {
    fn shed(&mut self, _plant: &mut Plant, _cycle: u16) {}
}

/// Orientation that keeps every axis straight: each metamer continues its heading.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Straight;

impl ShootOrientation for Straight {
    fn heading(&mut self, input: &HeadingInput) -> Vec3 {
        input.heading
    }
}

/// The four stages of a growth cycle.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Stages<E, F, O, S> {
    /// Stage 1.
    pub environment: E,
    /// Stage 2.
    pub fate: F,
    /// Stage 3.
    pub orientation: O,
    /// Shedding at the end of the cycle.
    pub shedding: S,
}
