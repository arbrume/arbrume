//! Architecture layer: the qualitative traits that define how a species builds itself.
//!
//! The traits and their names follow the architectural analysis of Barthelemy and
//! Caraglio (2007). They select behaviours of the growth engine; the quantities those
//! behaviours use live in the growth layer.
//!
//! Positional preferences such as acrotony or basitony (laterals developing mostly near
//! the tip or the base of their parent) are not flags here: they are the shape of the
//! growth curves over the position along the parent axis.

use super::{SpeciesError, check_range};
use serde::{Deserialize, Serialize};

/// What happens to the apex of an axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Branching {
    /// One persistent apical meristem builds the whole axis.
    #[default]
    Monopodial,
    /// The apex dies or transforms and a lateral relay axis continues the axis.
    Sympodial,
}

/// Whether axis extension is interrupted by rest periods.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GrowthRhythm {
    /// Growth flushes alternate with rest; an axis is a sequence of growth units.
    #[default]
    Rhythmic,
    /// No marked endogenous cessation of extension.
    Continuous,
}

/// How lateral axes are distributed along their parent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BranchingPattern {
    /// Laterals grouped in tiers separated by unbranched nodes.
    #[default]
    Rhythmic,
    /// Every axillary meristem develops into a lateral.
    Continuous,
    /// Neither tiers nor every node.
    Diffuse,
}

/// When a lateral axis elongates relative to its parent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BranchTiming {
    /// Immediate branching: the lateral elongates in the cycle its bud is formed.
    Sylleptic,
    /// Delayed branching: the bud rests at least one cycle first.
    #[default]
    Proleptic,
}

/// Arrangement of leaves, and of the axillary buds they subtend, around an axis.
///
/// Each node bears `leaves_per_node` leaves evenly spaced around the axis; the whole set
/// is rotated by `divergence_deg` from one node to the next.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Phyllotaxis {
    /// Leaves per node: 1 for alternate, 2 for opposite, more for whorled arrangements.
    pub leaves_per_node: u8,
    /// Rotation between successive nodes, in degrees.
    pub divergence_deg: f32,
}

impl Phyllotaxis {
    /// Divergence of spiral phyllotaxis in degrees (the golden angle, rounded as usual).
    pub const SPIRAL_DIVERGENCE_DEG: f32 = 137.5;

    /// Alternate spiral: one leaf per node, golden-angle divergence.
    #[must_use]
    pub const fn spiral() -> Self {
        Self {
            leaves_per_node: 1,
            divergence_deg: Self::SPIRAL_DIVERGENCE_DEG,
        }
    }

    /// Alternate distichous: one leaf per node, in two opposite ranks.
    #[must_use]
    pub const fn distichous() -> Self {
        Self {
            leaves_per_node: 1,
            divergence_deg: 180.0,
        }
    }

    /// Opposite decussate: pairs of leaves, each pair at a right angle to the previous.
    #[must_use]
    pub const fn decussate() -> Self {
        Self {
            leaves_per_node: 2,
            divergence_deg: 90.0,
        }
    }
}

impl Default for Phyllotaxis {
    fn default() -> Self {
        Self::spiral()
    }
}

/// Orientation syndrome of an axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AxisOrientation {
    /// Erect, radially symmetric axis with laterals in all directions.
    #[default]
    Orthotropic,
    /// Horizontal to slanted, bilaterally symmetric axis with laterals in one plane.
    Plagiotropic,
}

/// Differentiation of the axes of one branching order.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AxisCategory {
    /// Orientation syndrome.
    pub orientation: AxisOrientation,
    /// Short shoot: an axis that exploits its position instead of exploring, with very
    /// short internodes and no laterals of its own.
    pub short: bool,
    /// Leaf arrangement of this category when it differs from the species default.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phyllotaxis: Option<Phyllotaxis>,
}

/// Qualitative architectural traits of a species.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Architecture {
    /// Monopodial or sympodial axes.
    pub branching: Branching,
    /// Rhythmic or continuous extension.
    pub growth_rhythm: GrowthRhythm,
    /// Distribution of laterals along their parent.
    pub branching_pattern: BranchingPattern,
    /// Immediate or delayed branching.
    pub branch_timing: BranchTiming,
    /// Highest branching order produced (0 is a plant reduced to its trunk).
    pub max_order: u8,
    /// Leaf arrangement, unless an axis category overrides it.
    pub phyllotaxis: Phyllotaxis,
    /// Axis categories indexed by branching order; orders past the end use the last one.
    /// Never empty.
    pub axes: Vec<AxisCategory>,
}

impl Default for Architecture {
    fn default() -> Self {
        Self {
            branching: Branching::default(),
            growth_rhythm: GrowthRhythm::default(),
            branching_pattern: BranchingPattern::default(),
            branch_timing: BranchTiming::default(),
            max_order: 3,
            phyllotaxis: Phyllotaxis::default(),
            axes: vec![AxisCategory::default()],
        }
    }
}

impl Architecture {
    /// Category of the axes of `order`. Falls back to the default category when `axes`
    /// is empty, which [`Species::validate`](super::Species::validate) rejects.
    #[must_use]
    pub fn axis(&self, order: u8) -> AxisCategory {
        let index = usize::from(order).min(self.axes.len().saturating_sub(1));
        self.axes.get(index).copied().unwrap_or_default()
    }

    /// Leaf arrangement of the axes of `order`.
    #[must_use]
    pub fn phyllotaxis_of(&self, order: u8) -> Phyllotaxis {
        self.axis(order).phyllotaxis.unwrap_or(self.phyllotaxis)
    }

    pub(super) fn validate(&self) -> Result<(), SpeciesError> {
        if self.axes.is_empty() {
            let reason = String::from("at least one axis category is required");
            return Err(SpeciesError::Invalid {
                field: "architecture.axes",
                reason,
            });
        }
        check_phyllotaxis("architecture.phyllotaxis", self.phyllotaxis)?;
        self.axes
            .iter()
            .filter_map(|axis| axis.phyllotaxis)
            .try_for_each(|phyllotaxis| check_phyllotaxis("architecture.axes", phyllotaxis))
    }
}

fn check_phyllotaxis(field: &'static str, phyllotaxis: Phyllotaxis) -> Result<(), SpeciesError> {
    check_range(field, f32::from(phyllotaxis.leaves_per_node), 1.0, 255.0)?;
    check_range(field, phyllotaxis.divergence_deg, 0.0, 360.0)
}

#[cfg(test)]
mod tests {
    use super::{Architecture, AxisCategory, AxisOrientation, Phyllotaxis};

    #[test]
    fn axis_categories_extend_to_deeper_orders() {
        let plagiotropic = AxisCategory {
            orientation: AxisOrientation::Plagiotropic,
            short: false,
            phyllotaxis: Some(Phyllotaxis::distichous()),
        };
        let architecture = Architecture {
            axes: vec![AxisCategory::default(), plagiotropic],
            ..Default::default()
        };
        assert_eq!(
            architecture.axis(0).orientation,
            AxisOrientation::Orthotropic
        );
        assert_eq!(architecture.axis(9), plagiotropic);
        assert_eq!(architecture.phyllotaxis_of(0), Phyllotaxis::spiral());
        assert_eq!(architecture.phyllotaxis_of(4), Phyllotaxis::distichous());
        assert_eq!(Phyllotaxis::decussate().leaves_per_node, 2);
    }
}
