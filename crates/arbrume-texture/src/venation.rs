//! Veins and margin of one blade, in the frame of the blade.
//!
//! The frame of a blade has `t` along the midrib, from 0 at its base to 1 at its tip,
//! and `s` across it, both in blade lengths; the smooth half-width of the blade at `t`
//! is `0.5 width_ratio Foliage::blade_width(t)`.
//!
//! **Veins** are line segments with a width at each end:
//!
//! * pinnate: a midrib tapering to the tip, and on each side `veins` secondary veins.
//!   Following Runions et al. (2017), each secondary vein runs from a point of the margin
//!   to the midrib, which it meets at about `angle_deg`
//!   (`theta = arccos(nu / beta)` in their attachment rule); the margin points are evenly
//!   spaced along the blade, jittered, and a little offset from one side to the other.
//!   Each vein is a quadratic arc that leaves the midrib steeper than its chord and
//!   approaches the margin flatter, as secondary veins curve towards the tip.
//! * palmate: `2 veins + 1` primary veins fanning from the base over `+-angle_deg`,
//!   each to the margin.
//! * parallel: the midrib and `veins` veins on each side that follow the outline.
//! * open: the open venation model of Runions et al. (2005) on a static blade (no
//!   marginal growth): sources are scattered in the blade by dart throwing; at each step
//!   every source attracts its nearest vein node, every attracted node grows one step
//!   towards the mean direction of its sources, and sources within the kill distance of
//!   a node are removed, until none remain. Widths follow Murray's law with exponent 2
//!   (`r^2 = r1^2 + r2^2` at a branch point, as most models of that paper use).
//!
//! **Margin.** The ends of the secondary (pinnate) or primary (palmate) veins are where
//! lobes go, as the convergence points of Runions et al. (2017) are both vein ends and
//! the tips of teeth or lobes. Teeth (serrate, dentate, crenate) are evenly spaced along
//! the blade with jittered depth; this is a parametric stand-in for the margin model of
//! that paper, not its simulation.

use crate::noise::{random, smoothstep};
use arbrume_core::species::{BladeShape, Foliage, MarginKind, PAD_NOTCH_HALF_ANGLE, VenationKind};
use glam::Vec2;

/// A vein segment in the frame of a blade, widths in blade lengths.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Vein {
    pub a: Vec2,
    pub b: Vec2,
    pub width_a: f32,
    pub width_b: f32,
}

/// The outline of one blade with its margin and the vein ends lobes follow.
#[derive(Debug, Clone)]
pub(crate) struct Blade<'a> {
    foliage: &'a Foliage,
    /// Largest smooth half-width, in blade lengths.
    pub half: f32,
    /// `t` of the vein ends on the `+s` and `-s` sides, ascending.
    ends: [Vec<f32>; 2],
    /// Angles of the primary veins of palmate venation, ascending; empty otherwise.
    primaries: Vec<f32>,
    /// Radius of the smooth outline seen from the base, at `RAYS` angles over
    /// `[-pi/2, pi/2]`, for palmate lobes.
    radii: Vec<f32>,
    seed: u64,
}

/// Number of directions of [`Blade::radii`].
const RAYS: usize = 129;

impl<'a> Blade<'a> {
    /// The blade of `foliage` drawn with `seed` and its veins.
    pub fn new(foliage: &'a Foliage, seed: u64) -> (Self, Vec<Vein>) {
        let mut blade = Self {
            foliage,
            half: 0.5 * foliage.width_ratio,
            ends: [Vec::new(), Vec::new()],
            primaries: Vec::new(),
            radii: Vec::new(),
            seed,
        };
        let veins = match foliage.venation.kind {
            VenationKind::None => Vec::new(),
            VenationKind::Pinnate => blade.pinnate(),
            VenationKind::Palmate => blade.palmate(),
            VenationKind::Parallel => blade.parallel(),
            VenationKind::Open => crate::open::open_venation(&blade, seed),
        };
        for ends in &mut blade.ends {
            ends.sort_by(f32::total_cmp);
        }
        if !blade.primaries.is_empty() {
            blade.measure_radii();
        }
        (blade, veins)
    }

    /// Smooth half-width at `t`.
    pub fn smooth(&self, t: f32) -> f32 {
        self.half * self.foliage.blade_width(t)
    }

    /// Width of the midrib at the base of the blade.
    pub fn vein_width(&self) -> f32 {
        self.foliage.venation.width * 2.0 * self.half
    }

    /// Width of the midrib at `t`.
    fn midrib_width(&self, t: f32) -> f32 {
        self.vein_width() * (1.0 - 0.8 * t.clamp(0.0, 1.0))
    }

    /// The midrib, as eight segments, scaled in width by `scale`.
    fn midrib(&self, scale: f32) -> Vec<Vein> {
        const PIECES: usize = 8;
        let end = 0.97;
        (0..PIECES)
            .map(|i| {
                let (t0, t1) = (
                    end * i as f32 / PIECES as f32,
                    end * (i + 1) as f32 / PIECES as f32,
                );
                Vein {
                    a: Vec2::new(t0, 0.0),
                    b: Vec2::new(t1, 0.0),
                    width_a: scale * self.midrib_width(t0),
                    width_b: scale * self.midrib_width(t1),
                }
            })
            .collect()
    }

    fn jitter(&self, index: u32, side: u32, salt: u64) -> f32 {
        random(self.seed, index, side, salt) - 0.5
    }

    fn pinnate(&mut self) -> Vec<Vein> {
        let mut veins = self.midrib(1.0);
        let venation = self.foliage.venation;
        let count = venation.veins.max(1);
        let slope = venation.angle_deg.to_radians().tan();
        for (side_index, side) in [1.0_f32, -1.0].into_iter().enumerate() {
            let offset = 0.2 * side_index as f32;
            for k in 0..venation.veins {
                let step = 0.82 / count as f32;
                let jitter = 0.35 * self.jitter(k, side_index as u32, 0x81);
                let t_end = 0.1 + step * (k as f32 + 0.5 + offset + jitter);
                if t_end >= 0.97 {
                    continue;
                }
                let reach = 0.9 * self.smooth(t_end);
                let t_start = (t_end - reach / slope).max(0.02);
                self.ends[side_index].push(t_end);
                let start = Vec2::new(t_start, 0.0);
                let end = Vec2::new(t_end, side * reach);
                let chord = end - start;
                let bulge = Vec2::new(-chord.y.abs(), chord.x * side).normalize_or_zero();
                let control = start + 0.5 * chord + bulge * (0.12 * chord.length());
                let width = 0.45 * self.midrib_width(t_start);
                veins.extend(arc(start, control, end, [width, 0.3 * width], 4));
            }
        }
        veins
    }

    fn palmate(&mut self) -> Vec<Vein> {
        let venation = self.foliage.venation;
        let count = venation.veins.cast_signed();
        let spread = venation.angle_deg.to_radians();
        let base = self.midrib_width(0.0);
        let mut veins = Vec::new();
        for j in -count..=count {
            let angle = if count == 0 {
                0.0
            } else {
                spread * j as f32 / count as f32
            };
            let direction = Vec2::new(angle.cos(), angle.sin());
            let mut reach = 0.0;
            while reach < 1.0 {
                let p = direction * (reach + 0.01);
                let outside = p.y.abs() > 0.9 * self.smooth(p.x) && p.x > 0.05;
                if p.x >= 1.0 || outside || p.y.abs() > self.half {
                    break;
                }
                reach += 0.01;
            }
            let end = direction * reach;
            if j != 0 {
                self.ends[usize::from(j < 0)].push(end.x);
            }
            self.primaries.push(angle);
            let width = base * (1.0 - 0.35 * j.unsigned_abs() as f32 / count.max(1) as f32);
            let pieces = 6;
            for i in 0..pieces {
                let (f0, f1) = (i as f32 / pieces as f32, (i + 1) as f32 / pieces as f32);
                veins.push(Vein {
                    a: end * f0,
                    b: end * f1,
                    width_a: width * (1.0 - 0.8 * f0),
                    width_b: width * (1.0 - 0.8 * f1),
                });
            }
        }
        veins
    }

    fn parallel(&self) -> Vec<Vein> {
        let mut veins = self.midrib(1.0);
        let count = self.foliage.venation.veins;
        let width = 0.3 * self.midrib_width(0.0);
        let pieces = 12;
        for side in [1.0_f32, -1.0] {
            for i in 1..=count {
                let fraction = 0.92 * i as f32 / (count + 1) as f32;
                let point = |t: f32| Vec2::new(t, side * fraction * self.smooth(t));
                for k in 0..pieces {
                    let (t0, t1) = (k as f32 / pieces as f32, (k + 1) as f32 / pieces as f32);
                    veins.push(Vein {
                        a: point(t0),
                        b: point(t1),
                        width_a: width,
                        width_b: width,
                    });
                }
            }
        }
        veins
    }

    /// Signed distance from the point `q` of the frame of the blade into the blade,
    /// margin included, in blade lengths (an approximation away from the edge).
    pub fn inside(&self, q: Vec2) -> f32 {
        if !self.primaries.is_empty() && self.foliage.margin.kind == MarginKind::Lobed {
            return self.palmate_inside(q);
        }
        let side = if q.y < 0.0 { -1.0 } else { 1.0 };
        let across = self.width(q.x.clamp(0.0, 1.0), side) - q.y.abs();
        let inside = across.min(q.x).min(1.0 - q.x);
        if self.foliage.shape == BladeShape::Pad {
            return inside.min(pad_notch(q));
        }
        inside
    }

    /// [`Blade::inside`] for palmate lobes: in polar coordinates about the base, the
    /// outline radius is cut by the margin depth between primary veins, with a parabolic
    /// profile from a lobe tip on a primary vein to the sinus halfway to the next.
    fn palmate_inside(&self, q: Vec2) -> f32 {
        use std::f32::consts::FRAC_PI_2;
        if self.radii.is_empty() || q.x <= 0.0 {
            return -q.length().max(q.x.abs()).max(1e-4);
        }
        let angle = q.y.atan2(q.x);
        let slot = ((angle + FRAC_PI_2) / std::f32::consts::PI * (RAYS - 1) as f32).round();
        let radius = self.radii[(slot as usize).min(RAYS - 1)];
        let gap = if self.primaries.len() > 1 {
            (self.primaries[1] - self.primaries[0]).abs()
        } else {
            1.0
        };
        let nearest = self
            .primaries
            .iter()
            .map(|&p| (angle - p).abs())
            .fold(f32::MAX, f32::min);
        let x = (nearest / (0.5 * gap)).min(1.6);
        let cut = (x * x).min(1.0);
        let margin = self.foliage.margin;
        let jitter = 1.0 + 0.3 * margin.irregularity * self.jitter(slot as u32 / 9, 0, 0x93);
        radius * (1.0 - margin.depth * cut * jitter) - q.length()
    }

    /// Fills [`Blade::radii`] by marching each ray out of the smooth outline.
    fn measure_radii(&mut self) {
        use std::f32::consts::{FRAC_PI_2, PI};
        self.radii = (0..RAYS)
            .map(|i| {
                let angle = -FRAC_PI_2 + PI * i as f32 / (RAYS - 1) as f32;
                let direction = Vec2::new(angle.cos(), angle.sin());
                let mut reach = 0.0_f32;
                while reach < 1.0 {
                    let p = direction * (reach + 0.004);
                    if p.x > 1.0 || p.y.abs() > self.smooth(p.x) {
                        break;
                    }
                    reach += 0.004;
                }
                reach
            })
            .collect();
    }

    /// Half-width of the blade at `t` on the side of sign `side`, margin included.
    pub fn width(&self, t: f32, side: f32) -> f32 {
        let smooth = self.smooth(t);
        let margin = self.foliage.margin;
        if margin.kind == MarginKind::Entire || margin.depth <= 0.0 {
            return smooth;
        }
        let side_index = usize::from(side < 0.0);
        let envelope = smoothstep(0.03, 0.2, t) * smoothstep(1.0, 0.85, t);
        let cut = if margin.kind == MarginKind::Lobed {
            self.lobe_cut(t, side_index)
        } else {
            self.tooth_cut(t, side_index)
        };
        smooth * (1.0 - margin.depth * cut * envelope)
    }

    /// Cut of the teeth at `t`, in `[0, 1]`: 0 at a tooth tip.
    fn tooth_cut(&self, t: f32, side: usize) -> f32 {
        let margin = self.foliage.margin;
        let teeth = margin.teeth.max(1) as f32;
        let phase = (t - 0.05) / 0.93 * teeth + 0.5 * side as f32 * margin.irregularity;
        let index = phase.floor();
        let f = phase - index;
        let scale = 1.0
            + margin.irregularity * self.jitter((index as i32).cast_unsigned(), side as u32, 0x91);
        let cut = match margin.kind {
            MarginKind::Serrate => f,
            MarginKind::Dentate => (2.0 * f - 1.0).abs(),
            _ => 1.0 - (4.0 * f * (1.0 - f)).sqrt(),
        };
        (cut * scale).clamp(0.0, 1.0)
    }

    /// Cut of the lobes at `t`, in `[0, 1]`: 0 at a lobe tip, 1 at the bottom of a sinus.
    fn lobe_cut(&self, t: f32, side: usize) -> f32 {
        let ends = &self.ends[side];
        if ends.is_empty() {
            return self.tooth_cut(t, side);
        }
        let gap = if ends.len() > 1 {
            (ends[ends.len() - 1] - ends[0]) / (ends.len() - 1) as f32
        } else {
            0.5
        };
        let nearest = ends
            .iter()
            .map(|&end| (t - end).abs())
            .fold(f32::MAX, f32::min);
        let x = (nearest / (0.5 * gap)).min(1.0);
        x * x
    }
}

/// A quadratic Bezier arc from `a` through control `c` to `b` as `pieces` segments,
/// its width going from `widths[0]` to `widths[1]`.
fn arc(a: Vec2, c: Vec2, b: Vec2, widths: [f32; 2], pieces: u32) -> Vec<Vein> {
    let point = |f: f32| a * (1.0 - f) * (1.0 - f) + c * 2.0 * f * (1.0 - f) + b * f * f;
    (0..pieces)
        .map(|i| {
            let (f0, f1) = (i as f32 / pieces as f32, (i + 1) as f32 / pieces as f32);
            Vein {
                a: point(f0),
                b: point(f1),
                width_a: widths[0] + (widths[1] - widths[0]) * f0,
                width_b: widths[0] + (widths[1] - widths[0]) * f1,
            }
        })
        .collect()
}

/// Signed distance from the point `q` of a pad (blade frame, centre at `x = 0.5`) out
/// of its notch: negative inside the wedge that runs from the centre to the base edge.
fn pad_notch(q: Vec2) -> f32 {
    let back = 0.5 - q.x;
    if back <= 0.0 {
        return q.distance(Vec2::new(0.5, 0.0));
    }
    let (sin, cos) = PAD_NOTCH_HALF_ANGLE.sin_cos();
    q.y.abs() * cos - back * sin
}
