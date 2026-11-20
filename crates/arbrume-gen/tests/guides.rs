//! Guided growth: crown, limb and erase strokes and freezes on self-organizing presets.
//!
//! The bounds on distances and counts below were measured on these plants and leave a
//! margin; they are regression bounds of this implementation, not properties of trees.

use arbrume_core::graph::PlantGraph;
use arbrume_core::species::Species;
use arbrume_gen::{
    GenParams, Generator, GuideReport, Guides, Stroke, StrokeKind, Surroundings, generate, preset,
};
use arbrume_grow::{EngineOptions, GrowParams, grow_guided, grow_guided_with, grow_in};
use glam::Vec3;

const SPECIES: &str = "open_grown_broadleaf";

fn params(age: u16) -> GrowParams {
    GrowParams {
        seed: 3,
        age,
        scale: 1.0,
    }
}

fn guides(strokes: Vec<Stroke>) -> Guides {
    Guides {
        strokes,
        freezes: Vec::new(),
    }
}

fn grow(species: &Species, age: u16, guides: &Guides) -> (PlantGraph, GuideReport) {
    grow_guided(species, &params(age), &Surroundings::none(), guides)
}

/// Live nodes of `graph` inside the balls of radius `radius` along `points`.
fn live_inside(graph: &PlantGraph, points: &[Vec3], radius: f32) -> usize {
    let balls = arbrume_grow::guides::stroke_balls(points, radius);
    let inside = |p: &Vec3| balls.iter().any(|c| c.distance(*p) <= radius);
    graph
        .positions()
        .iter()
        .zip(graph.flags())
        .filter(|(p, flags)| !flags.is_shed() && inside(p))
        .count()
}

fn largest_x(graph: &PlantGraph) -> f32 {
    let live = graph.positions().iter().zip(graph.flags());
    live.filter(|(_, f)| !f.is_shed())
        .map(|(p, _)| p.x)
        .fold(f32::MIN, f32::max)
}

/// An S curve from `start`, rising by `rise` while it goes `run` along X and swings
/// `swing` along Z.
fn s_curve(start: Vec3, run: f32, rise: f32, swing: f32) -> Vec<Vec3> {
    (0..=24)
        .map(|step| {
            let t = step as f32 / 24.0;
            let wave = (t * std::f32::consts::TAU).sin() * swing;
            start + Vec3::new(t * run, t * rise, wave)
        })
        .collect()
}

#[test]
fn without_strokes_the_plant_is_unchanged() {
    let species = preset(SPECIES).unwrap();
    let plain = GenParams::new(3, 14);
    let empty = plain.clone().with_guides(Guides::none());
    let (a, b) = (
        generate(&species, &plain).unwrap(),
        generate(&species, &empty).unwrap(),
    );
    assert_eq!(a.graph, b.graph);
    assert_eq!(a.mesh, b.mesh);
    assert!(b.guides.is_none());
    let meta = arbrume_gen::metadata(&species, &b);
    assert!(meta.get("guides").is_none());
    let (graph, report) = grow(&species, 14, &Guides::none());
    assert_eq!(graph, grow_in(&species, &params(14), &Surroundings::none()));
    assert_eq!(report.strokes.len(), 0);
}

#[test]
fn a_crown_stroke_fills_the_painted_side() {
    let species = preset(SPECIES).unwrap();
    let (plain, _) = grow(&species, 20, &Guides::none());
    let points = vec![Vec3::new(5.0, 8.0, 0.0), Vec3::new(8.0, 8.5, 0.0)];
    let painted = guides(vec![Stroke {
        strength: 2.0,
        reach: 6.0,
        ..Stroke::new(StrokeKind::Crown, points.clone(), 1.5)
    }]);
    let (graph, report) = grow(&species, 20, &painted);
    let stroke = report.strokes[0];
    let (before, after) = (
        live_inside(&plain, &points, 1.5),
        live_inside(&graph, &points, 1.5),
    );
    println!("{stroke:?}, nodes in the stroke {before} -> {after}");
    assert!(stroke.markers > 0);
    assert!(after > 3 * before, "{before} -> {after}");
    assert!(stroke.reached * 3 > 2 * stroke.markers, "{stroke:?}");
    let (wide, narrow) = (largest_x(&graph), largest_x(&plain));
    println!("crown reaches x = {wide} instead of {narrow}");
    assert!(wide > narrow + 1.5);
}

#[test]
fn a_limb_follows_its_stroke() {
    let species = preset(SPECIES).unwrap();
    let points = s_curve(Vec3::new(0.0, 3.0, 0.0), 5.0, 2.5, 1.0);
    let painted = guides(vec![Stroke::new(StrokeKind::Limb, points, 0.4)]);
    let (_, report) = grow(&species, 20, &painted);
    let limb = report.strokes[0];
    println!("{limb:?}");
    assert!(limb.claimed);
    assert!(limb.followed > 0.9 * limb.length, "{limb:?}");
    assert!(
        limb.mean_distance < 0.15 && limb.max_distance < 0.6,
        "{limb:?}"
    );
}

#[test]
fn a_stroke_from_the_ground_steers_the_trunk() {
    let species = preset(SPECIES).unwrap();
    let points: Vec<Vec3> = (0..=24)
        .map(|step| {
            let t = step as f32 / 24.0;
            Vec3::new((t * std::f32::consts::TAU).sin() * 1.2, t * 8.0, 0.0)
        })
        .collect();
    let painted = guides(vec![Stroke::new(StrokeKind::Limb, points, 0.5)]);
    let (_, report) = grow(&species, 20, &painted);
    let trunk = report.strokes[0];
    println!("{trunk:?}");
    assert!(
        trunk.claimed && trunk.followed > 0.9 * trunk.length,
        "{trunk:?}"
    );
    assert!(
        trunk.mean_distance < 0.1 && trunk.max_distance < 0.3,
        "{trunk:?}"
    );
}

#[test]
fn an_erase_stroke_leaves_its_volume_empty() {
    let species = preset(SPECIES).unwrap();
    let points = vec![Vec3::new(3.0, 5.0, 0.0), Vec3::new(3.0, 10.0, 0.0)];
    let (plain, _) = grow(&species, 20, &Guides::none());
    let erased = guides(vec![Stroke::new(StrokeKind::Erase, points.clone(), 1.5)]);
    let (graph, _) = grow(&species, 20, &erased);
    let before = live_inside(&plain, &points, 1.5);
    println!("nodes in the erased volume {before} -> 0");
    assert!(before > 50);
    assert_eq!(live_inside(&graph, &points, 1.5), 0);
}

#[test]
fn a_freeze_keeps_the_grown_part_and_adds_growth() {
    let species = preset(SPECIES).unwrap();
    let first = guides(vec![Stroke::new(
        StrokeKind::Crown,
        vec![Vec3::new(-4.0, 7.0, 0.0)],
        1.5,
    )]);
    let (frozen, _) = grow(&species, 16, &first);
    let mut later = first.clone();
    later.freezes.push(16);
    let crown = vec![Vec3::new(5.0, 8.0, 2.0), Vec3::new(7.0, 9.0, 2.0)];
    later
        .strokes
        .push(Stroke::new(StrokeKind::Crown, crown.clone(), 1.5).from_cycle(16));
    let erase = vec![Vec3::new(-3.0, 6.0, -3.0)];
    later
        .strokes
        .push(Stroke::new(StrokeKind::Erase, erase.clone(), 1.2).from_cycle(16));
    let (grown, report) = grow(&species, 22, &later);
    println!("{:?}", report.strokes);
    let old = |graph: &PlantGraph, kept: bool| {
        let mut nodes: Vec<_> = (0..graph.node_count())
            .filter(|&n| graph.births()[n] < 16 && (kept || !graph.flags()[n].is_shed()))
            .map(|n| {
                let p = graph.positions()[n];
                let bits = [p.x.to_bits(), p.y.to_bits(), p.z.to_bits()];
                (graph.births()[n], graph.orders()[n], bits)
            })
            .collect();
        nodes.sort_unstable();
        nodes
    };
    assert_eq!(old(&frozen, true), old(&grown, true));
    let pruned = report.strokes[2].pruned as usize;
    assert!(pruned > 0);
    assert_eq!(old(&frozen, false).len(), old(&grown, false).len() + pruned);
    assert_eq!(live_inside(&grown, &erase, 1.2), 0);
    let added = (0..grown.node_count()).filter(|&n| grown.births()[n] >= 16);
    let near = |n: &usize| {
        let p = grown.positions()[*n];
        crown.iter().any(|c| c.distance(p) < 4.0)
    };
    let (added, near): (Vec<_>, Vec<_>) = added.partition(near);
    println!(
        "{} new nodes near the stroke, {} elsewhere",
        added.len(),
        near.len()
    );
    assert!(added.len() > 20 && near.is_empty());
}

#[test]
fn guided_growth_does_not_depend_on_threads() {
    let species = preset(SPECIES).unwrap();
    let painted = guides(vec![
        Stroke::new(StrokeKind::Crown, vec![Vec3::new(4.0, 8.0, 0.0)], 2.0),
        Stroke::new(
            StrokeKind::Limb,
            s_curve(Vec3::new(0.0, 3.0, 0.0), -4.0, 2.0, 1.0),
            0.4,
        ),
    ]);
    let open = Surroundings::none();
    let run = |parallel_markers| {
        let options = EngineOptions {
            parallel_markers,
            ..EngineOptions::default()
        };
        grow_guided_with(&species, &params(18), &open, &painted, options)
    };
    assert_eq!(run(true), run(false));
    assert_eq!(run(true), grow(&species, 18, &painted));
}

#[test]
fn the_last_growth_frame_is_the_guided_plant() {
    let species = preset(SPECIES).unwrap();
    let painted = guides(vec![Stroke::new(
        StrokeKind::Limb,
        s_curve(Vec3::new(0.0, 3.0, 0.0), 4.0, 2.0, 1.0),
        0.4,
    )]);
    let params = GenParams::new(3, 12).with_quality(0.3).with_guides(painted);
    let mut generator = Generator::new();
    let timeline = generator.timeline(&species, &params).unwrap();
    let mut frame = arbrume_gen::Plant::default();
    generator
        .frame(&species, &timeline, 11, &mut frame)
        .unwrap();
    let plant = generate(&species, &params).unwrap();
    assert_eq!(frame.graph, plant.graph);
    assert_eq!(frame.mesh, plant.mesh);
    assert_eq!(frame.guides, plant.guides);
    assert!(
        arbrume_gen::metadata(&species, &plant)
            .get("guides")
            .is_some()
    );
}
