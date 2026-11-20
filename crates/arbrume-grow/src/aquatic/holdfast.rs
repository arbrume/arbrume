//! The holdfast of a kelp: claw-like haptera from the base of each stem down to the
//! rock.
//!
//! Kelp has no roots; its holdfast is a cone of branched haptera that grip the rock
//! rather than enter it. The root system of `[roots]` grows into the soil, below the
//! ground, which is the wrong place for them, so haptera are built here instead:
//! `haptera` claws arch outwards and down to the bottom, evenly spread around the base
//! of the plant (the mean of the bases of its stems), each reaching `reach` meters from
//! it (both jittered by `variation`); claw `k` leaves the first node of the stem whose
//! base lies furthest towards its azimuth, so the stems of a kelp share one holdfast. Near its end a claw forks `forks` times, each fork a shorter claw
//! that leaves at an angle and ends on the bottom beside its parent. A claw is a
//! quadratic arc from the stem to the bottom, tapering to half its radius.
//!
//! Haptera are lateral axes flagged as roots, so they are labelled `root` in the
//! annotations, keep the wind data of the base of their stem, and are not bent by the
//! water. They carry the birth of the first node of their stem.

use crate::stage::Context;
use arbrume_core::graph::{ChainKind, Metamer, NodeFlags, NodeId, PlantGraph};
use arbrume_core::rng::{Rng, Stream};
use arbrume_core::species::Holdfast;
use glam::Vec3;
use std::f32::consts::TAU;

/// Metamers of one claw.
const CLAW_METAMERS: u32 = 5;

/// Angle between a fork and the claw it leaves, in radians.
const FORK_ANGLE: f32 = 0.5;

/// Length of a fork relative to the claw it leaves.
const FORK_LENGTH: f32 = 0.45;

/// Stream of the random reach and azimuth of the haptera.
const HAPTERA: Stream = Stream::custom(0x5A02);

/// One claw to build: a quadratic arc from `from` through `control` to `to`.
#[derive(Debug, Clone, Copy)]
struct Claw {
    attach: NodeId,
    from: Vec3,
    control: Vec3,
    to: Vec3,
    radius: f32,
    forks: u32,
}

impl Claw {
    fn point(&self, t: f32) -> Vec3 {
        let u = 1.0 - t;
        self.from * (u * u) + self.control * (2.0 * u * t) + self.to * (t * t)
    }
}

/// Builds `claw` in `graph` and its forks, alternating sides with `side`.
fn build(graph: &mut PlantGraph, claw: Claw, birth: u16, side: f32) {
    let metamer = |position: Vec3, radius: f32| Metamer {
        flags: NodeFlags::ROOT,
        ..Metamer::new(position, radius, birth)
    };
    let taper = |t: f32| claw.radius * (1.0 - 0.5 * t);
    let step = 1.0 / CLAW_METAMERS as f32;
    let first = graph.add_chain(
        claw.attach,
        ChainKind::Lateral,
        metamer(claw.point(step), taper(step)),
    );
    let chain = graph.chain_of(first);
    let mut fork_at = first;
    for index in 2..=CLAW_METAMERS {
        let t = index as f32 * step;
        let node = graph.extend_chain(chain, metamer(claw.point(t), taper(t)));
        if index == CLAW_METAMERS - 2 {
            fork_at = node;
        }
    }
    if claw.forks == 0 {
        return;
    }
    let start = graph.position(fork_at);
    let reach = claw.to - start;
    let turned = glam::Quat::from_axis_angle(Vec3::Y, FORK_ANGLE * side) * reach;
    let mut to = start + turned * (1.0 + FORK_LENGTH);
    to.y = claw.to.y;
    let control = start + turned * 0.5 + Vec3::Y * (0.25 * reach.length());
    let fork = Claw {
        attach: fork_at,
        from: start,
        control,
        to,
        radius: taper(1.0 - 2.0 * step) * 0.7,
        forks: claw.forks - 1,
    };
    build(graph, fork, birth, -side);
}

/// Adds the holdfast to the stems of `graph`. See the [module documentation](self).
pub(super) fn add_holdfast(graph: &mut PlantGraph, cx: &Context, holdfast: &Holdfast) {
    let stems: Vec<(NodeId, Vec3)> = graph
        .chains()
        .iter()
        .filter(|chain| chain.attach.is_none() && !graph.flags()[chain.first.index()].is_root())
        .map(|chain| (chain.first, chain.origin))
        .collect();
    if stems.is_empty() {
        return;
    }
    let center = stems.iter().map(|&(_, origin)| origin).sum::<Vec3>() / stems.len() as f32;
    let mut rng = Rng::new(cx.seed, 0, HAPTERA);
    let radius = holdfast.radius * cx.scale;
    let count = holdfast.haptera;
    for index in 0..count {
        let jitter = holdfast.variation * rng.signed();
        let azimuth = TAU * (index as f32 + 0.5 * jitter) / count as f32;
        let reach = holdfast.reach * cx.scale * (1.0 + holdfast.variation * rng.signed());
        let outward = Vec3::new(azimuth.cos(), 0.0, azimuth.sin());
        let toward = |origin: Vec3| (origin - center).dot(outward);
        let &(first, _) = stems
            .iter()
            .max_by(|a, b| toward(a.1).total_cmp(&toward(b.1)))
            .unwrap_or(&stems[0]);
        let from = graph.position(first);
        let mut to = center + outward * reach;
        to.y = center.y + 0.5 * radius;
        let mut control = from + outward * (0.8 * reach);
        control.y = f32::midpoint(from.y, to.y) + 0.25 * (from.y - to.y);
        let claw = Claw {
            attach: first,
            from,
            control,
            to,
            radius,
            forks: holdfast.forks,
        };
        let side = if index % 2 == 0 { 1.0 } else { -1.0 };
        build(graph, claw, graph.births()[first.index()], side);
    }
}

#[cfg(test)]
mod tests {
    use super::add_holdfast;
    use crate::stage::Context;
    use arbrume_core::graph::{Metamer, PlantGraph};
    use arbrume_core::species::{Holdfast, Species};
    use glam::Vec3;

    #[test]
    fn haptera_are_roots_that_reach_the_bottom_around_the_stem() {
        let mut graph = PlantGraph::new();
        let first = graph.add_root_chain(Vec3::ZERO, Metamer::new(Vec3::Y * 0.15, 0.02, 0));
        let chain = graph.chain_of(first);
        graph.extend_chain(chain, Metamer::new(Vec3::Y * 1.0, 0.015, 1));
        let species = Species::default();
        let cx = Context {
            species: &species,
            seed: 3,
            age: 4,
            scale: 1.0,
        };
        let holdfast = Holdfast {
            haptera: 6,
            forks: 1,
            ..Holdfast::default()
        };
        add_holdfast(&mut graph, &cx, &holdfast);
        graph.check_invariants().unwrap();
        assert!(graph.is_contiguous());
        assert_eq!(graph.chain_count(), 1 + 6 * 2);
        for chain in graph.chains().iter().skip(1) {
            assert!(graph.flags()[chain.first.index()].is_root());
            let end = graph.position(chain.last);
            assert!(end.y.abs() < 0.02, "{end}");
            assert!(Vec3::new(end.x, 0.0, end.z).length() > 0.08);
        }
    }
}
