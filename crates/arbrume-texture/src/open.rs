//! Open leaf venation (Runions et al. 2005) on a static blade.
//!
//! Distances are in blade lengths. Sources are placed by dart throwing at least
//! `SOURCE_SPACING` apart inside the blade (a uniform grid of that cell size answers the
//! spacing test), the veins start from one node at the base of the blade, and every step each live source
//! attracts its nearest node, each attracted node grows a new node `STEP` towards the
//! normalized sum of the unit vectors to its sources (their eq. 1), and sources closer
//! than `KILL_DISTANCE` to a node are removed. Each source caches its nearest node and
//! compares it with the nodes added in the last step only, so a step costs the number
//! of sources times the number of new nodes. Widths follow Murray's law with exponent 2
//! from tips of width `TIP_WIDTH` relative to the midrib.

use crate::noise::random;
use crate::venation::{Blade, Vein};
use glam::Vec2;

/// Smallest distance between two sources.
const SOURCE_SPACING: f32 = 0.022;
/// A source is removed once a vein node is this close.
const KILL_DISTANCE: f32 = 0.016;
/// Length of a new vein segment.
const STEP: f32 = 0.008;
/// Width of a vein tip relative to the width of the midrib.
const TIP_WIDTH: f32 = 0.12;
/// Largest number of growth steps.
const MAX_STEPS: usize = 600;

/// Dart-thrown sources inside `blade`.
fn sources(blade: &Blade, seed: u64) -> Vec<Vec2> {
    let rows = ((2.0 * blade.half / SOURCE_SPACING).ceil() as usize).max(1);
    let columns = (1.0 / SOURCE_SPACING).ceil() as usize;
    let mut grid: Vec<Vec<u32>> = vec![Vec::new(); rows * columns];
    let cell_of = |p: Vec2| {
        let x = ((p.x / SOURCE_SPACING) as usize).min(columns - 1);
        let y = (((p.y + blade.half) / SOURCE_SPACING) as usize).min(rows - 1);
        (x, y)
    };
    let mut placed: Vec<Vec2> = Vec::new();
    let tries = 12 * rows * columns;
    for attempt in 0..tries {
        let t = 0.02 + 0.96 * random(seed, attempt as u32, 0, 0xA1);
        let s = (2.0 * random(seed, attempt as u32, 1, 0xA1) - 1.0) * blade.half;
        let candidate = Vec2::new(t, s);
        if blade.inside(candidate) < 0.01 {
            continue;
        }
        let (x, y) = cell_of(candidate);
        let mut free = true;
        for gy in y.saturating_sub(1)..=(y + 1).min(rows - 1) {
            for gx in x.saturating_sub(1)..=(x + 1).min(columns - 1) {
                let near = |&i: &u32| placed[i as usize].distance(candidate) < SOURCE_SPACING;
                free &= !grid[gy * columns + gx].iter().any(near);
            }
        }
        if free {
            grid[y * columns + x].push(placed.len() as u32);
            placed.push(candidate);
        }
    }
    placed
}

/// The veins of `blade` grown by the open venation model; the main vein emerges as the
/// widest path from the base.
pub(crate) fn open_venation(blade: &Blade, seed: u64) -> Vec<Vein> {
    let mut nodes: Vec<Vec2> = vec![Vec2::ZERO];
    let midrib = nodes.len();
    let mut parents: Vec<u32> = vec![u32::MAX; midrib];
    let mut live: Vec<(Vec2, u32, f32)> = sources(blade, seed)
        .into_iter()
        .map(|p| {
            let (index, distance) = nearest(&nodes, 0..midrib, p);
            (p, index, distance)
        })
        .collect();
    let mut fresh = 0..0;
    for _ in 0..MAX_STEPS {
        for source in &mut live {
            let (index, distance) = nearest(&nodes, fresh.clone(), source.0);
            if distance < source.2 {
                (source.1, source.2) = (index, distance);
            }
        }
        live.retain(|source| source.2 >= KILL_DISTANCE);
        if live.is_empty() {
            break;
        }
        let mut pull = vec![Vec2::ZERO; nodes.len()];
        for &(p, index, _) in &live {
            pull[index as usize] += (p - nodes[index as usize]).normalize_or_zero();
        }
        let start = nodes.len();
        for (index, sum) in pull.iter().enumerate() {
            if sum.length_squared() > 1e-8 {
                nodes.push(nodes[index] + sum.normalize() * STEP);
                parents.push(index as u32);
            }
        }
        if nodes.len() == start {
            break;
        }
        fresh = start..nodes.len();
    }
    widths(&nodes, &parents, midrib, blade)
}

/// Index of and distance to the node of `range` nearest to `p`.
fn nearest(nodes: &[Vec2], range: std::ops::Range<usize>, p: Vec2) -> (u32, f32) {
    range
        .map(|i| (i as u32, nodes[i].distance(p)))
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .unwrap_or((0, f32::MAX))
}

/// Segments from every grown node to its parent, with Murray's-law widths.
fn widths(nodes: &[Vec2], parents: &[u32], midrib: usize, blade: &Blade) -> Vec<Vein> {
    let scale = blade.vein_width() * TIP_WIDTH;
    let mut squared = vec![0.0_f32; nodes.len()];
    for index in (midrib..nodes.len()).rev() {
        if squared[index] == 0.0 {
            squared[index] = scale * scale;
        }
        let parent = parents[index] as usize;
        if parent >= midrib {
            squared[parent] += squared[index];
        }
    }
    let widest = blade.vein_width();
    (midrib..nodes.len())
        .map(|index| {
            let parent = parents[index] as usize;
            let own = squared[index].sqrt().min(widest);
            let up = if parent >= midrib {
                squared[parent].sqrt().min(widest)
            } else {
                own
            };
            Vein {
                a: nodes[index],
                b: nodes[parent],
                width_a: own,
                width_b: up,
            }
        })
        .collect()
}
