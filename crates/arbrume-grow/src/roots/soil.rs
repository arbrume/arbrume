//! The soil a root system grows in: the ground surface and the resource field.
//!
//! Obstacles are not described here: every blocking occluder of the [`Surroundings`]
//! of the plant (a rock, the foundation of a wall) blocks the roots as it blocks the
//! shoots. A [`Soil`] adds what only the roots see:
//!
//! * the **ground**: the surface the roots stay under, flat, sloping, or given as a grid
//!   of heights (an uneven terrain);
//! * the **resources**: patches richer than the homogeneous soil around them, which
//!   attract the roots and feed those that reach them.
//!
//! Every position is in meters in the frame of the plant, which stands at the origin
//! with its collar on the ground; the scale of the plant does not apply.
//!
//! [`Surroundings`]: crate::Surroundings

use crate::surroundings::Shape;
use glam::Vec3;
use serde::{Deserialize, Serialize};

/// A grid of ground heights, read by bilinear interpolation and continued flat beyond
/// its border.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeightField {
    /// X and Z of the first sample.
    pub origin: [f32; 2],
    /// Spacing of the samples along X and Z, in meters.
    pub cell: f32,
    /// Number of samples along X and Z, at least 2 each.
    pub size: [u32; 2],
    /// Heights, X varying fastest: sample `(i, j)` is `heights[j * size[0] + i]`.
    pub heights: Vec<f32>,
}

impl HeightField {
    /// Height at `(x, z)`.
    #[must_use]
    pub fn height(&self, x: f32, z: f32) -> f32 {
        let [nx, nz] = self.size.map(|n| n.max(2) as usize);
        let local = |value: f32, origin: f32, count: usize| {
            let at = ((value - origin) / self.cell).clamp(0.0, (count - 1) as f32);
            let low = (at.floor() as usize).min(count - 2);
            (low, at - low as f32)
        };
        let (column, across) = local(x, self.origin[0], nx);
        let (row, along) = local(z, self.origin[1], nz);
        let at = |column: usize, row: usize| self.heights[row * nx + column];
        let near = at(column, row) + (at(column + 1, row) - at(column, row)) * across;
        let far = at(column, row + 1) + (at(column + 1, row + 1) - at(column, row + 1)) * across;
        near + (far - near) * along
    }

    fn validate(&self) -> Result<(), String> {
        let [nx, nz] = self.size;
        if nx < 2 || nz < 2 {
            return Err(format!(
                "a height field needs 2 samples a side, not {nx} x {nz}"
            ));
        }
        if self.heights.len() != nx as usize * nz as usize {
            return Err(format!(
                "a height field of {nx} x {nz} samples has {} heights",
                self.heights.len()
            ));
        }
        let finite = self
            .heights
            .iter()
            .chain(&self.origin)
            .all(|v| v.is_finite());
        if !(self.cell > 0.0 && self.cell.is_finite() && finite) {
            return Err("a height field needs a positive cell and finite values".to_owned());
        }
        Ok(())
    }
}

/// The ground surface.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Ground {
    /// The plane `y = 0`.
    #[default]
    Flat,
    /// The plane through the origin of height `rise[0] * x + rise[1] * z`.
    Slope {
        /// Rise per meter along X and along Z.
        rise: [f32; 2],
    },
    /// An uneven terrain.
    Heights(HeightField),
}

impl Ground {
    /// Height of the surface at `(x, z)`.
    #[must_use]
    pub fn height(&self, x: f32, z: f32) -> f32 {
        match self {
            Self::Flat => 0.0,
            Self::Slope { rise } => rise[0] * x + rise[1] * z,
            Self::Heights(field) => field.height(x, z),
        }
    }

    /// Highest point of the surface over the square of half-width `reach` about the
    /// origin.
    #[must_use]
    pub fn highest(&self, reach: f32) -> f32 {
        match self {
            Self::Flat => 0.0,
            Self::Slope { rise } => (rise[0].abs() + rise[1].abs()) * reach,
            Self::Heights(field) => field.heights.iter().copied().fold(0.0, f32::max),
        }
    }

    /// Whether the surface is the plane `y = 0`.
    #[must_use]
    pub fn is_flat(&self) -> bool {
        match self {
            Self::Flat => true,
            Self::Slope { rise } => *rise == [0.0, 0.0],
            Self::Heights(field) => field.heights.iter().all(|&h| h == 0.0),
        }
    }
}

/// A patch of soil richer than its surroundings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Resource {
    /// The volume of the patch.
    #[serde(flatten)]
    pub shape: Shape,
    /// Resource added inside the patch, as a multiple of that of the homogeneous soil.
    #[serde(default = "one")]
    pub richness: f32,
    /// Distance outside the patch over which its richness fades to nothing, in meters:
    /// the reach of the gradient that attracts the roots. 0 for a sharp border.
    #[serde(default)]
    pub reach: f32,
}

const fn one() -> f32 {
    1.0
}

impl Resource {
    /// Richness the patch adds at `point`: all of it inside, fading as the square of
    /// the distance over `reach` outside.
    #[must_use]
    pub fn at(&self, point: Vec3) -> f32 {
        let distance = self.shape.distance(point);
        if distance <= 0.0 {
            return self.richness;
        }
        if distance >= self.reach {
            return 0.0;
        }
        let fade = 1.0 - distance / self.reach;
        self.richness * fade * fade
    }
}

/// Ground and resources of the soil. See the [module documentation](self).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Soil {
    /// The ground surface; flat by default.
    pub ground: Ground,
    /// Patches richer than the rest of the soil; none for a homogeneous soil.
    pub resources: Vec<Resource>,
}

impl Soil {
    /// Homogeneous soil under flat ground.
    #[must_use]
    pub const fn homogeneous() -> Self {
        Self {
            ground: Ground::Flat,
            resources: Vec::new(),
        }
    }

    /// Resource of the soil at `point`, 1 in homogeneous soil.
    #[must_use]
    pub fn richness(&self, point: Vec3) -> f32 {
        1.0 + self
            .resources
            .iter()
            .map(|patch| patch.at(point))
            .sum::<f32>()
    }

    /// Upper bound of [`Soil::richness`].
    #[must_use]
    pub fn richest(&self) -> f32 {
        1.0 + self
            .resources
            .iter()
            .map(|p| p.richness.max(0.0))
            .sum::<f32>()
    }

    /// Direction in which the resource rises fastest at `point`, from central
    /// differences over `step` meters; zero where it is uniform.
    #[must_use]
    pub fn gradient(&self, point: Vec3, step: f32) -> Vec3 {
        if self.resources.is_empty() {
            return Vec3::ZERO;
        }
        let slope =
            |axis: Vec3| self.richness(point + axis * step) - self.richness(point - axis * step);
        Vec3::new(slope(Vec3::X), slope(Vec3::Y), slope(Vec3::Z)).normalize_or_zero()
    }

    /// Checks the ground and every patch.
    ///
    /// # Errors
    ///
    /// Returns a message naming the first height field with the wrong number of samples
    /// or a value that is not finite, the first patch without a positive, finite volume,
    /// or a richness or reach that is negative or not finite.
    pub fn validate(&self) -> Result<(), String> {
        if let Ground::Heights(field) = &self.ground {
            field.validate()?;
        }
        if let Ground::Slope { rise } = self.ground
            && !rise.iter().all(|r| r.is_finite())
        {
            return Err("the rise of a slope is not finite".to_owned());
        }
        for patch in &self.resources {
            patch.shape.validate()?;
            let ok = |v: f32| v >= 0.0 && v.is_finite();
            if !(ok(patch.richness) && ok(patch.reach)) {
                return Err(format!(
                    "a resource patch needs a positive richness and reach, not {} and {}",
                    patch.richness, patch.reach
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{Ground, HeightField, Resource, Soil};
    use crate::surroundings::Shape;
    use glam::Vec3;

    #[test]
    fn a_height_field_interpolates_its_samples() {
        let field = HeightField {
            origin: [-1.0, -1.0],
            cell: 1.0,
            size: [3, 2],
            heights: vec![0.0, 1.0, 2.0, 1.0, 2.0, 3.0],
        };
        assert!((field.height(-1.0, -1.0) - 0.0).abs() < 1e-6);
        assert!((field.height(0.5, -1.0) - 1.5).abs() < 1e-6);
        assert!((field.height(0.0, -0.5) - 1.5).abs() < 1e-6);
        assert!((field.height(9.0, 9.0) - 3.0).abs() < 1e-6);
        let ground = Ground::Heights(field);
        assert!(!ground.is_flat());
        assert!((ground.highest(5.0) - 3.0).abs() < 1e-6);
    }

    #[test]
    fn a_patch_attracts_and_fades() {
        let soil = Soil {
            ground: Ground::Flat,
            resources: vec![Resource {
                shape: Shape::Sphere {
                    center: Vec3::new(2.0, -1.0, 0.0),
                    radius: 0.5,
                },
                richness: 3.0,
                reach: 1.0,
            }],
        };
        assert!((soil.richness(Vec3::new(2.0, -1.0, 0.0)) - 4.0).abs() < 1e-6);
        assert!((soil.richness(Vec3::new(-3.0, -1.0, 0.0)) - 1.0).abs() < 1e-6);
        let between = soil.richness(Vec3::new(1.0, -1.0, 0.0));
        assert!(between > 1.0 && between < 4.0);
        let towards = soil.gradient(Vec3::new(1.2, -1.0, 0.0), 0.05);
        assert!(towards.x > 0.9, "{towards}");
        assert!((soil.richest() - 4.0).abs() < 1e-6);
        soil.validate().unwrap();
    }

    #[test]
    fn soils_read_from_toml() {
        let text = r#"
            ground = { kind = "slope", rise = [0.2, 0.0] }
            [[resources]]
            shape = "box"
            min = [1.0, -2.0, -1.0]
            max = [3.0, -0.5, 1.0]
            richness = 4.0
            reach = 1.5
        "#;
        let soil: Soil = toml::from_str(text).unwrap();
        assert!((soil.ground.height(1.0, 0.0) - 0.2).abs() < 1e-6);
        assert_eq!(soil.resources.len(), 1);
        soil.validate().unwrap();
        assert!(toml::from_str::<Soil>("grund = 1").is_err());
        let bad: Soil = toml::from_str("ground = { kind = \"heights\", origin = [0.0, 0.0], cell = 1.0, size = [2, 2], heights = [0.0] }").unwrap();
        assert!(bad.validate().is_err());
    }
}
