//! Curves over a relative position, the form of most species parameters.
//!
//! Following Prusinkiewicz et al. (2001, "The use of positional information in the
//! modeling of plants"), a parameter such as a branching angle or a branch length is not
//! a number but a function of the relative position `x` in `[0, 1]` along an axis
//! (0 at the base, 1 at the tip), and usually also of the branching order.
//!
//! * [`Curve`] is one function of `x`: a constant, or control points joined by straight
//!   lines ([`Interpolation::Linear`]) or by a monotone cubic
//!   ([`Interpolation::Smooth`]).
//! * [`OrderCurve`] holds one [`Curve`] per branching order; orders past the end of the
//!   list reuse the last curve.
//!
//! # File format
//!
//! Both types are written compactly in TOML:
//!
//! ```toml
//! a = 0.5                                                  # constant
//! b = [[0.0, 1.0], [0.6, 0.8], [1.0, 0.1]]                 # linear control points
//! c = { points = [[0.0, 1.0], [1.0, 0.0]], interp = "smooth" }
//! d = { by_order = [60.0, [[0.0, 70.0], [1.0, 40.0]]] }    # order 0, then order 1 and up
//! ```
//!
//! Control points need `x` in `[0, 1]`, strictly increasing. Outside the first and last
//! control point a curve keeps the value of that point.
//!
//! # Evaluation cost
//!
//! A constant costs one branch. A control-point curve scans its points linearly (species
//! curves have a handful) and evaluates one segment; tangents of smooth curves are
//! computed once at construction. Nothing allocates during evaluation.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// How a [`Curve`] joins its control points.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Interpolation {
    /// Straight segments.
    #[default]
    Linear,
    /// Monotone cubic Hermite segments (Fritsch and Carlson 1980): continuous slope, and
    /// no overshoot, so the curve stays within the range of its control points.
    Smooth,
}

/// Why a list of control points is not a valid curve.
#[derive(Debug, Clone, Copy, PartialEq, Error)]
pub enum CurveError {
    /// The list of control points is empty.
    #[error("a curve needs at least one control point")]
    Empty,
    /// A coordinate is NaN or infinite.
    #[error("control point {index} is not finite")]
    NotFinite {
        /// Index of the offending control point.
        index: usize,
    },
    /// An abscissa lies outside `[0, 1]`.
    #[error("control point {index} has x = {x}, outside [0, 1]")]
    OutOfRange {
        /// Index of the offending control point.
        index: usize,
        /// Its abscissa.
        x: f32,
    },
    /// Abscissas are not strictly increasing.
    #[error("control point {index} does not have a larger x than the previous one")]
    NotIncreasing {
        /// Index of the offending control point.
        index: usize,
    },
    /// A per-order list has no curve.
    #[error("a per-order curve needs at least one curve")]
    NoOrders,
}

/// Control point with its tangent (`dy/dx`, used by smooth curves only).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Knot {
    x: f32,
    y: f32,
    slope: f32,
}

#[derive(Debug, Clone, PartialEq)]
enum Shape {
    Constant(f32),
    Knots {
        knots: Box<[Knot]>,
        interpolation: Interpolation,
    },
}

/// A scalar function on `[0, 1]`. See the [module documentation](self).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "CurveRepr", into = "CurveRepr")]
pub struct Curve {
    shape: Shape,
}

impl Default for Curve {
    /// The constant 0.
    fn default() -> Self {
        Self::constant(0.0)
    }
}

impl Curve {
    /// Curve with the same value everywhere.
    #[must_use]
    pub const fn constant(value: f32) -> Self {
        Self {
            shape: Shape::Constant(value),
        }
    }

    /// Curve through `points` (`[x, y]` pairs) with straight segments.
    ///
    /// # Errors
    ///
    /// Returns a [`CurveError`] when the points are empty, not finite, outside `[0, 1]`
    /// in `x`, or not strictly increasing in `x`.
    pub fn linear(points: &[[f32; 2]]) -> Result<Self, CurveError> {
        Self::from_points(points, Interpolation::Linear)
    }

    /// Curve through `points` (`[x, y]` pairs) with monotone cubic segments.
    ///
    /// # Errors
    ///
    /// Same conditions as [`Curve::linear`].
    pub fn smooth(points: &[[f32; 2]]) -> Result<Self, CurveError> {
        Self::from_points(points, Interpolation::Smooth)
    }

    /// Curve through `points` with the given interpolation. A single point gives a
    /// constant.
    ///
    /// # Errors
    ///
    /// Same conditions as [`Curve::linear`].
    pub fn from_points(
        points: &[[f32; 2]],
        interpolation: Interpolation,
    ) -> Result<Self, CurveError> {
        validate_points(points)?;
        if let [[_, y]] = points {
            return Ok(Self::constant(*y));
        }
        let mut knots: Box<[Knot]> = points
            .iter()
            .map(|&[x, y]| Knot { x, y, slope: 0.0 })
            .collect();
        if interpolation == Interpolation::Smooth {
            set_monotone_slopes(&mut knots);
        }
        Ok(Self {
            shape: Shape::Knots {
                knots,
                interpolation,
            },
        })
    }

    /// Value at `x`. `x` outside the control points takes the value of the nearest one.
    #[inline]
    #[must_use]
    pub fn eval(&self, x: f32) -> f32 {
        match &self.shape {
            Shape::Constant(value) => *value,
            Shape::Knots {
                knots,
                interpolation,
            } => eval_knots(knots, *interpolation, x),
        }
    }

    /// The curve with every value multiplied by `factor` and clamped to `[lo, hi]`.
    /// Abscissas and interpolation are kept.
    #[must_use]
    pub fn scaled(&self, factor: f32, lo: f32, hi: f32) -> Self {
        let scale = |y: f32| (y * factor).clamp(lo, hi);
        match &self.shape {
            Shape::Constant(value) => Self::constant(scale(*value)),
            Shape::Knots {
                knots,
                interpolation,
            } => {
                let mut knots: Box<[Knot]> = knots
                    .iter()
                    .map(|knot| Knot {
                        x: knot.x,
                        y: scale(knot.y),
                        slope: 0.0,
                    })
                    .collect();
                if *interpolation == Interpolation::Smooth {
                    set_monotone_slopes(&mut knots);
                }
                Self {
                    shape: Shape::Knots {
                        knots,
                        interpolation: *interpolation,
                    },
                }
            }
        }
    }

    /// The value when the curve is a constant.
    #[must_use]
    pub const fn as_constant(&self) -> Option<f32> {
        match self.shape {
            Shape::Constant(value) => Some(value),
            Shape::Knots { .. } => None,
        }
    }

    /// Interpolation between control points ([`Interpolation::Linear`] for a constant).
    #[must_use]
    pub const fn interpolation(&self) -> Interpolation {
        match self.shape {
            Shape::Constant(_) => Interpolation::Linear,
            Shape::Knots { interpolation, .. } => interpolation,
        }
    }

    /// Control points as `[x, y]` pairs; a constant reports the single point `[0, value]`.
    #[must_use]
    pub fn points(&self) -> Vec<[f32; 2]> {
        match &self.shape {
            Shape::Constant(value) => vec![[0.0, *value]],
            Shape::Knots { knots, .. } => knots.iter().map(|k| [k.x, k.y]).collect(),
        }
    }

    /// Smallest and largest value over `[0, 1]`. Exact for both interpolations, because
    /// neither overshoots its control points.
    #[must_use]
    pub fn bounds(&self) -> (f32, f32) {
        match &self.shape {
            Shape::Constant(value) => (*value, *value),
            Shape::Knots { knots, .. } => knots
                .iter()
                .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), k| {
                    (lo.min(k.y), hi.max(k.y))
                }),
        }
    }
}

fn validate_points(points: &[[f32; 2]]) -> Result<(), CurveError> {
    if points.is_empty() {
        return Err(CurveError::Empty);
    }
    for (index, &[x, y]) in points.iter().enumerate() {
        if !x.is_finite() || !y.is_finite() {
            return Err(CurveError::NotFinite { index });
        }
        if !(0.0..=1.0).contains(&x) {
            return Err(CurveError::OutOfRange { index, x });
        }
        if index > 0 && x <= points[index - 1][0] {
            return Err(CurveError::NotIncreasing { index });
        }
    }
    Ok(())
}

/// Fritsch-Carlson tangents: a weighted harmonic mean of the neighbouring secants where
/// they agree in sign, zero at local extrema, and the secant itself at both ends.
fn set_monotone_slopes(knots: &mut [Knot]) {
    let last = knots.len() - 1;
    let secant = |k: &[Knot], i: usize| (k[i + 1].y - k[i].y) / (k[i + 1].x - k[i].x);
    knots[0].slope = secant(knots, 0);
    knots[last].slope = secant(knots, last - 1);
    for i in 1..last {
        let (before, after) = (secant(knots, i - 1), secant(knots, i));
        knots[i].slope = if before * after <= 0.0 {
            0.0
        } else {
            let h_before = knots[i].x - knots[i - 1].x;
            let h_after = knots[i + 1].x - knots[i].x;
            let w_before = 2.0 * h_after + h_before;
            let w_after = h_after + 2.0 * h_before;
            (w_before + w_after) / (w_before / before + w_after / after)
        };
    }
}

#[inline]
fn eval_knots(knots: &[Knot], interpolation: Interpolation, x: f32) -> f32 {
    let last = knots.len() - 1;
    if x <= knots[0].x {
        return knots[0].y;
    }
    if x >= knots[last].x {
        return knots[last].y;
    }
    let mut segment = 0;
    while segment + 1 < last && knots[segment + 1].x < x {
        segment += 1;
    }
    let (start, end) = (knots[segment], knots[segment + 1]);
    let width = end.x - start.x;
    let t = (x - start.x) / width;
    match interpolation {
        Interpolation::Linear => start.y + t * (end.y - start.y),
        Interpolation::Smooth => {
            let (t2, t3) = (t * t, t * t * t);
            let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
            let h10 = t3 - 2.0 * t2 + t;
            let h01 = -2.0 * t3 + 3.0 * t2;
            let h11 = t3 - t2;
            h00 * start.y + h10 * width * start.slope + h01 * end.y + h11 * width * end.slope
        }
    }
}

/// Serialized form of a [`Curve`]: a number, a list of points, or a table.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    untagged,
    expecting = "a number, a list of [x, y] points, or { points, interp }"
)]
enum CurveRepr {
    Constant(f32),
    Points(Vec<[f32; 2]>),
    Table(CurveTable),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CurveTable {
    points: Vec<[f32; 2]>,
    #[serde(default)]
    interp: Interpolation,
}

impl TryFrom<CurveRepr> for Curve {
    type Error = CurveError;

    fn try_from(repr: CurveRepr) -> Result<Self, CurveError> {
        match repr {
            CurveRepr::Constant(value) if value.is_finite() => Ok(Self::constant(value)),
            CurveRepr::Constant(_) => Err(CurveError::NotFinite { index: 0 }),
            CurveRepr::Points(points) => Self::linear(&points),
            CurveRepr::Table(table) => Self::from_points(&table.points, table.interp),
        }
    }
}

impl From<Curve> for CurveRepr {
    fn from(curve: Curve) -> Self {
        match (curve.as_constant(), curve.interpolation()) {
            (Some(value), _) => Self::Constant(value),
            (None, Interpolation::Linear) => Self::Points(curve.points()),
            (None, interp) => Self::Table(CurveTable {
                points: curve.points(),
                interp,
            }),
        }
    }
}

/// A [`Curve`] per branching order.
///
/// Order 0 is the trunk, order 1 its lateral axes, and so on. Orders beyond the stored
/// list use the last curve, so a single curve applies to every order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "OrderCurveRepr", into = "OrderCurveRepr")]
pub struct OrderCurve {
    by_order: Vec<Curve>,
}

impl Default for OrderCurve {
    /// The constant 0 for every order.
    fn default() -> Self {
        Self::constant(0.0)
    }
}

impl OrderCurve {
    /// The same constant for every order and position.
    #[must_use]
    pub fn constant(value: f32) -> Self {
        Self::uniform(Curve::constant(value))
    }

    /// The same curve for every order.
    #[must_use]
    pub fn uniform(curve: Curve) -> Self {
        Self {
            by_order: vec![curve],
        }
    }

    /// One constant per order; the last one also serves deeper orders.
    ///
    /// # Errors
    ///
    /// Returns [`CurveError::NoOrders`] when `values` is empty.
    pub fn constants(values: &[f32]) -> Result<Self, CurveError> {
        Self::by_order(values.iter().copied().map(Curve::constant).collect())
    }

    /// One curve per order; the last one also serves deeper orders.
    ///
    /// # Errors
    ///
    /// Returns [`CurveError::NoOrders`] when `curves` is empty.
    pub fn by_order(curves: Vec<Curve>) -> Result<Self, CurveError> {
        if curves.is_empty() {
            return Err(CurveError::NoOrders);
        }
        Ok(Self { by_order: curves })
    }

    /// Curve used for `order`.
    #[inline]
    #[must_use]
    pub fn order(&self, order: u8) -> &Curve {
        let index = usize::from(order).min(self.by_order.len() - 1);
        &self.by_order[index]
    }

    /// Value for an axis of `order` at relative position `x`.
    #[inline]
    #[must_use]
    pub fn eval(&self, order: u8, x: f32) -> f32 {
        self.order(order).eval(x)
    }

    /// The curves with the values of order `o` multiplied by `factor(o)` and clamped to
    /// `[lo, hi]` (see [`Curve::scaled`]). `factor` is called once per stored curve, in
    /// order.
    #[must_use]
    pub fn scaled(&self, mut factor: impl FnMut(usize) -> f32, lo: f32, hi: f32) -> Self {
        let curves = self.by_order.iter().enumerate();
        Self {
            by_order: curves
                .map(|(order, curve)| curve.scaled(factor(order), lo, hi))
                .collect(),
        }
    }

    /// The stored curves, indexed by order.
    #[must_use]
    pub fn curves(&self) -> &[Curve] {
        &self.by_order
    }

    /// Smallest and largest value over all orders and positions.
    #[must_use]
    pub fn bounds(&self) -> (f32, f32) {
        self.by_order
            .iter()
            .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), curve| {
                let (curve_lo, curve_hi) = curve.bounds();
                (lo.min(curve_lo), hi.max(curve_hi))
            })
    }
}

/// Serialized form of an [`OrderCurve`]: one curve, or `{ by_order = [...] }`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged, expecting = "a curve, or { by_order = [curve, ...] }")]
enum OrderCurveRepr {
    Uniform(Curve),
    ByOrder(ByOrderTable),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ByOrderTable {
    by_order: Vec<Curve>,
}

impl TryFrom<OrderCurveRepr> for OrderCurve {
    type Error = CurveError;

    fn try_from(repr: OrderCurveRepr) -> Result<Self, CurveError> {
        match repr {
            OrderCurveRepr::Uniform(curve) => Ok(Self::uniform(curve)),
            OrderCurveRepr::ByOrder(table) => Self::by_order(table.by_order),
        }
    }
}

impl From<OrderCurve> for OrderCurveRepr {
    fn from(curve: OrderCurve) -> Self {
        let mut by_order = curve.by_order;
        if by_order.len() == 1 {
            Self::Uniform(by_order.remove(0))
        } else {
            Self::ByOrder(ByOrderTable { by_order })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Curve, CurveError, Interpolation, OrderCurve};
    use serde::{Deserialize, Serialize};

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-5
    }

    #[test]
    fn constant_is_flat() {
        let curve = Curve::constant(2.5);
        assert!(close(curve.eval(-1.0), 2.5) && close(curve.eval(0.3), 2.5));
        assert_eq!(curve.as_constant(), Some(2.5));
    }

    #[test]
    fn linear_interpolates_and_clamps() {
        let curve = Curve::linear(&[[0.2, 1.0], [0.6, 3.0], [1.0, 0.0]]).unwrap();
        assert!(close(curve.eval(0.0), 1.0));
        assert!(close(curve.eval(0.2), 1.0));
        assert!(close(curve.eval(0.4), 2.0));
        assert!(close(curve.eval(0.6), 3.0));
        assert!(close(curve.eval(0.8), 1.5));
        assert!(close(curve.eval(1.0), 0.0));
        assert!(close(curve.eval(7.0), 0.0));
        assert_eq!(curve.bounds(), (0.0, 3.0));
    }

    #[test]
    fn smooth_hits_points_and_does_not_overshoot() {
        let points = [[0.0, 0.0], [0.3, 0.1], [0.5, 2.0], [0.7, 2.0], [1.0, 0.5]];
        let curve = Curve::smooth(&points).unwrap();
        for [x, y] in points {
            assert!(close(curve.eval(x), y));
        }
        let (lo, hi) = curve.bounds();
        let mut previous = curve.eval(0.0);
        for step in 1..=500 {
            let x = step as f32 / 500.0;
            let y = curve.eval(x);
            assert!(y >= lo - 1e-5 && y <= hi + 1e-5);
            if x <= 0.5 {
                assert!(y >= previous - 1e-5, "not monotone at {x}");
            }
            previous = y;
        }
    }

    #[test]
    fn invalid_points_are_rejected() {
        assert_eq!(Curve::linear(&[]), Err(CurveError::Empty));
        assert_eq!(
            Curve::linear(&[[0.0, 1.0], [0.0, 2.0]]),
            Err(CurveError::NotIncreasing { index: 1 })
        );
        assert_eq!(
            Curve::linear(&[[0.0, 1.0], [1.5, 2.0]]),
            Err(CurveError::OutOfRange { index: 1, x: 1.5 })
        );
        assert_eq!(
            Curve::linear(&[[0.0, f32::NAN]]),
            Err(CurveError::NotFinite { index: 0 })
        );
        assert_eq!(OrderCurve::by_order(Vec::new()), Err(CurveError::NoOrders));
    }

    #[test]
    fn order_curve_reuses_the_last_order() {
        let curve = OrderCurve::constants(&[1.0, 2.0, 3.0]).unwrap();
        assert!(close(curve.eval(0, 0.5), 1.0));
        assert!(close(curve.eval(2, 0.5), 3.0));
        assert!(close(curve.eval(200, 0.5), 3.0));
        assert_eq!(curve.bounds(), (1.0, 3.0));
    }

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Holder {
        a: Curve,
        b: Curve,
        c: Curve,
        d: OrderCurve,
        e: OrderCurve,
    }

    const SAMPLE: &str = r#"
        a = 1
        b = [[0.0, 1.0], [0.5, 0.25], [1.0, 0.0]]
        c = { points = [[0.0, 1.0], [1.0, 0.0]], interp = "smooth" }
        d = { by_order = [60.0, [[0.0, 70.0], [1.0, 40.0]]] }
        e = [[0, 2], [1, 4]]
    "#;

    #[test]
    fn toml_forms_parse() {
        let holder: Holder = toml::from_str(SAMPLE).unwrap();
        assert_eq!(holder.a, Curve::constant(1.0));
        assert!(close(holder.b.eval(0.25), 0.625));
        assert_eq!(holder.c.interpolation(), Interpolation::Smooth);
        assert!(close(holder.c.eval(0.5), 0.5));
        assert!(close(holder.d.eval(0, 0.9), 60.0));
        assert!(close(holder.d.eval(5, 0.5), 55.0));
        assert!(close(holder.e.eval(3, 0.5), 3.0));
    }

    #[test]
    fn toml_round_trips() {
        let holder: Holder = toml::from_str(SAMPLE).unwrap();
        let text = toml::to_string(&holder).unwrap();
        let again: Holder = toml::from_str(&text).unwrap();
        assert_eq!(holder, again);
        assert!(
            text.contains("a = 1.0"),
            "constants stay bare numbers: {text}"
        );
    }

    #[test]
    fn toml_rejects_bad_curves() {
        #[derive(Debug, Deserialize)]
        struct One {
            a: OrderCurve,
        }
        let parse = |text: &str| toml::from_str::<One>(text).map(|one| one.a);
        assert!(parse("a = 2.0").is_ok());
        assert!(parse("a = [[0.5, 1.0], [0.2, 2.0]]").is_err());
        assert!(parse("a = { by_order = [] }").is_err());
        assert!(parse("a = { points = [[0.0, 1.0]], typo = 1 }").is_err());
        assert!(parse("a = \"high\"").is_err());
    }
}
