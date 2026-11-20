//! Geometry of strokes: polylines measured by arc length, and the balls of a brush.

use glam::Vec3;

/// A polyline with the arc length at each of its points.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Polyline {
    points: Vec<Vec3>,
    arcs: Vec<f32>,
}

impl Polyline {
    /// The polyline through `points`, consecutive equal points merged.
    #[must_use]
    pub fn new(points: &[Vec3]) -> Self {
        let mut line = Self::default();
        for &point in points {
            match line.points.last() {
                Some(&last) if last == point => {}
                Some(&last) => {
                    let arc = line.length() + last.distance(point);
                    line.points.push(point);
                    line.arcs.push(arc);
                }
                None => {
                    line.points.push(point);
                    line.arcs.push(0.0);
                }
            }
        }
        line
    }

    /// The points of the polyline.
    #[must_use]
    pub fn points(&self) -> &[Vec3] {
        &self.points
    }

    /// Total arc length; 0 for fewer than two points.
    #[must_use]
    pub fn length(&self) -> f32 {
        self.arcs.last().copied().unwrap_or(0.0)
    }

    /// The point at arc length `arc`, clamped to the ends.
    #[must_use]
    pub fn at(&self, arc: f32) -> Vec3 {
        let Some(&first) = self.points.first() else {
            return Vec3::ZERO;
        };
        if arc <= 0.0 || self.points.len() == 1 {
            return first;
        }
        let segment = self.arcs.partition_point(|&a| a < arc);
        if segment >= self.points.len() {
            return self.points[self.points.len() - 1];
        }
        let (start, end) = (self.arcs[segment - 1], self.arcs[segment]);
        let t = (arc - start) / (end - start);
        self.points[segment - 1].lerp(self.points[segment], t)
    }

    /// Closest point to `point` among the parts of the polyline between the arc lengths
    /// `from` and `to`: its arc length and its distance. The whole polyline when `from`
    /// is 0 and `to` its length.
    #[must_use]
    pub fn closest(&self, point: Vec3, from: f32, to: f32) -> (f32, f32) {
        let (from, to) = (from.max(0.0), to.min(self.length()));
        let mut best = (from, self.at(from).distance(point));
        for segment in 1..self.points.len() {
            let (start, end) = (self.arcs[segment - 1], self.arcs[segment]);
            if end < from || start > to {
                continue;
            }
            let (a, b) = (self.points[segment - 1], self.points[segment]);
            let along = (point - a).dot(b - a) / (end - start).powi(2);
            let low = (from - start) / (end - start);
            let high = (to - start) / (end - start);
            let t = along.clamp(low.max(0.0), high.min(1.0));
            let distance = a.lerp(b, t).distance(point);
            if distance < best.1 {
                best = (start + t * (end - start), distance);
            }
        }
        best
    }

    /// Distance from `point` to the polyline.
    #[must_use]
    pub fn distance(&self, point: Vec3) -> f32 {
        self.closest(point, 0.0, self.length()).1
    }

    /// Points along the polyline every `spacing` of arc length or less, ends included.
    #[must_use]
    pub fn resample(&self, spacing: f32) -> Vec<Vec3> {
        let length = self.length();
        if self.points.len() < 2 || spacing.is_nan() || spacing <= 0.0 {
            return self.points.iter().take(1).copied().collect();
        }
        let steps = (length / spacing).ceil().max(1.0) as u32;
        (0..=steps)
            .map(|step| self.at(length * step as f32 / steps as f32))
            .collect()
    }
}

/// Centres of the balls of a brush of `radius` along `points`: the stroke resampled
/// every half radius. The volume of a crown or erase stroke is the union of the balls of
/// `radius` about these centres.
#[must_use]
pub fn stroke_balls(points: &[Vec3], radius: f32) -> Vec<Vec3> {
    Polyline::new(points).resample(0.5 * radius)
}

/// Whether `point` lies in one of the balls of `radius` about `centers`.
pub(crate) fn in_balls(centers: &[Vec3], radius: f32, point: Vec3) -> bool {
    let limit = radius * radius;
    centers.iter().any(|c| c.distance_squared(point) <= limit)
}

#[cfg(test)]
mod tests {
    use super::{Polyline, in_balls, stroke_balls};
    use glam::Vec3;

    fn ell() -> Polyline {
        Polyline::new(&[
            Vec3::ZERO,
            Vec3::X * 2.0,
            Vec3::X * 2.0,
            Vec3::new(2.0, 3.0, 0.0),
        ])
    }

    #[test]
    fn arc_length_and_points_along() {
        let line = ell();
        assert_eq!(line.points().len(), 3);
        assert_eq!(line.length(), 5.0);
        assert_eq!(line.at(1.0), Vec3::X);
        assert_eq!(line.at(3.0), Vec3::new(2.0, 1.0, 0.0));
        assert_eq!(line.at(-1.0), Vec3::ZERO);
        assert_eq!(line.at(9.0), Vec3::new(2.0, 3.0, 0.0));
        let samples = line.resample(1.0);
        assert_eq!(samples.len(), 6);
        assert!(
            samples
                .windows(2)
                .all(|p| (p[0].distance(p[1]) - 1.0).abs() < 1e-6)
        );
    }

    #[test]
    fn closest_point_within_an_arc_range() {
        let line = ell();
        let (arc, distance) = line.closest(Vec3::new(1.0, -1.0, 0.0), 0.0, 5.0);
        assert!((arc - 1.0).abs() < 1e-6 && (distance - 1.0).abs() < 1e-6);
        let (arc, _) = line.closest(Vec3::new(3.0, 2.0, 0.0), 0.0, 5.0);
        assert!((arc - 4.0).abs() < 1e-6);
        let (arc, distance) = line.closest(Vec3::new(1.0, 0.0, 0.0), 3.0, 5.0);
        assert!((arc - 3.0).abs() < 1e-6);
        assert!((distance - 2.0_f32.sqrt()).abs() < 1e-6);
        assert!((line.distance(Vec3::new(2.5, 2.0, 0.0)) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn brush_balls_cover_the_stroke() {
        let centers = stroke_balls(&[Vec3::ZERO, Vec3::Y * 2.0], 0.5);
        assert_eq!(centers.len(), 9);
        assert!(in_balls(&centers, 0.5, Vec3::new(0.45, 1.1, 0.0)));
        assert!(!in_balls(&centers, 0.5, Vec3::new(0.0, 2.6, 0.0)));
        assert_eq!(stroke_balls(&[Vec3::ONE], 0.5), vec![Vec3::ONE]);
    }
}
