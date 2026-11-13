//! Small vector helpers shared by the growth stages. All directions are unit vectors.

use glam::Vec3;

/// The direction opposite to gravity.
pub(crate) const UP: Vec3 = Vec3::Y;

/// `vector` without its component along the unit vector `axis`.
#[inline]
pub(crate) fn reject(vector: Vec3, axis: Vec3) -> Vec3 {
    vector - axis * vector.dot(axis)
}

/// Unit vector orthogonal to `heading`, as close to `hint` as possible. Falls back to an
/// arbitrary orthogonal vector when `hint` is parallel to `heading`.
#[inline]
pub(crate) fn perpendicular(heading: Vec3, hint: Vec3) -> Vec3 {
    reject(hint, heading)
        .try_normalize()
        .unwrap_or_else(|| heading.any_orthonormal_vector())
}

/// Rotates the unit vector `heading` by `angle` radians towards the unit vector
/// `towards`, which must be orthogonal to it.
#[inline]
pub(crate) fn tilt(heading: Vec3, towards: Vec3, angle: f32) -> Vec3 {
    let (sin, cos) = angle.sin_cos();
    heading * cos + towards * sin
}

/// Unit vector orthogonal to `heading` at `azimuth` radians from `side`, turning around
/// `heading`. `side` must be orthogonal to `heading`.
#[inline]
pub(crate) fn radial(heading: Vec3, side: Vec3, azimuth: f32) -> Vec3 {
    let (sin, cos) = azimuth.sin_cos();
    side * cos + heading.cross(side) * sin
}

/// Horizontal unit vector orthogonal to `heading`; `fallback` when `heading` is
/// vertical.
#[inline]
pub(crate) fn horizontal_side(heading: Vec3, fallback: Vec3) -> Vec3 {
    UP.cross(heading).try_normalize().unwrap_or(fallback)
}

/// Unit vector orthogonal to `heading` that points upwards as much as possible;
/// `fallback` when `heading` is vertical.
#[inline]
pub(crate) fn upward_normal(heading: Vec3, fallback: Vec3) -> Vec3 {
    reject(UP, heading).try_normalize().unwrap_or(fallback)
}

#[cfg(test)]
mod tests {
    use super::{UP, horizontal_side, perpendicular, radial, tilt, upward_normal};
    use glam::Vec3;

    #[test]
    fn helpers_return_orthogonal_unit_vectors() {
        let heading = Vec3::new(0.6, 0.8, 0.0);
        let side = perpendicular(heading, Vec3::Z);
        assert!(side.dot(heading).abs() < 1e-6 && (side.length() - 1.0).abs() < 1e-6);
        let around = radial(heading, side, 1.0);
        assert!(around.dot(heading).abs() < 1e-6 && (around.length() - 1.0).abs() < 1e-6);
        assert!((around.dot(side) - 1.0_f32.cos()).abs() < 1e-6);
        let tilted = tilt(heading, side, 0.5);
        assert!((tilted.dot(heading) - 0.5_f32.cos()).abs() < 1e-6);
        assert!(horizontal_side(heading, side).y.abs() < 1e-6);
        assert!(upward_normal(heading, side).y > 0.0);
    }

    #[test]
    fn vertical_headings_use_the_fallback() {
        assert_eq!(horizontal_side(UP, Vec3::X), Vec3::X);
        assert_eq!(upward_normal(UP, Vec3::Z), Vec3::Z);
        assert!(perpendicular(UP, UP).dot(UP).abs() < 1e-6);
    }
}
