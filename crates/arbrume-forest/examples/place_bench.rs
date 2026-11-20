//! Times the placement of large forests on procedural hills.
//!
//! ```text
//! cargo run --release -p arbrume-forest --example place_bench -- 1000 2000 5000
//! ```
//!
//! Each argument is the side of a square terrain in meters; the forest mixes three
//! species (150 and 60 trees per hectare, spacings 3 and 4 m, and an understory of 600
//! per hectare, spacing 1.5 m) with slope and
//! altitude preferences, a river and ten clearings. Prints, per size and mode, the
//! trees, the time, the peak candidates of a band and the bytes of the result.

use std::time::Instant;

use arbrume_forest::{
    Exclusion, Exclusions, Hills, Instance, Mode, PlaceSettings, Shape, SpeciesRule, Terrain, place,
};

fn rules() -> Vec<SpeciesRule> {
    vec![
        SpeciesRule {
            density: 150.0,
            spacing: 3.0,
            variants: 4,
            altitude: [0.0, 70.0],
            altitude_falloff: 15.0,
            lean_deg: 3.0,
            tint: 0.08,
            ..SpeciesRule::default()
        },
        SpeciesRule {
            density: 60.0,
            spacing: 4.0,
            variants: 4,
            slope: [0.0, 25.0],
            slope_falloff: 10.0,
            ..SpeciesRule::default()
        },
        SpeciesRule {
            density: 600.0,
            spacing: 1.5,
            layer: 1,
            variants: 4,
            scale: [0.6, 1.0],
            ..SpeciesRule::default()
        },
    ]
}

fn exclusions(side: f32) -> Exclusions {
    let half = side / 2.0;
    let mut shapes = vec![Exclusion {
        shape: Shape::Path {
            points: vec![[-half, -half * 0.2], [0.0, half * 0.1], [half, -half * 0.1]],
            width: 15.0,
        },
        falloff: 6.0,
        carve: 4.0,
    }];
    for k in 0..10 {
        let t = k as f32 / 10.0;
        shapes.push(Exclusion {
            shape: Shape::Circle {
                center: [half * (2.0 * t - 0.9), half * 0.6 * (6.0 * t).sin()],
                radius: 20.0 + side * 0.01,
            },
            falloff: 8.0,
            carve: 0.0,
        });
    }
    Exclusions::new(shapes, None, None).expect("valid exclusions")
}

fn main() {
    let sides: Vec<f32> = std::env::args()
        .skip(1)
        .map(|a| a.parse().expect("a side in meters"))
        .collect();
    println!(
        "| side (m) | mode | trees | candidates (peak band) | time (s) | trees/s | result (MB) | saturated cells |"
    );
    println!("|---|---|---|---|---|---|---|---|");
    for side in sides {
        let hills = Hills {
            amplitude: 80.0,
            wavelength: 400.0,
            octaves: 5,
            resolution: (side / 2000.0).max(2.0),
            seed: 1,
        };
        let exclusions = exclusions(side);
        let mut terrain =
            Terrain::hills([-side / 2.0, -side / 2.0], [side, side], &hills).expect("terrain");
        exclusions.carve(&mut terrain);
        for mode in [Mode::Poisson, Mode::Competition] {
            let settings = PlaceSettings {
                seed: 7,
                mode,
                band_rows: 0,
            };
            let mut seconds = f64::INFINITY;
            let mut result = None;
            for _ in 0..3 {
                let start = Instant::now();
                result =
                    Some(place(&terrain, &exclusions, &rules(), &settings).expect("placement"));
                seconds = seconds.min(start.elapsed().as_secs_f64());
            }
            let (trees, report) = result.expect("one run");
            let bytes = trees.capacity() * std::mem::size_of::<Instance>();
            println!(
                "| {side} | {mode:?} | {} | {} | {seconds:.3} | {:.2e} | {:.1} | {} |",
                trees.len(),
                report.peak_candidates,
                trees.len() as f64 / seconds,
                bytes as f64 / 1e6,
                report
                    .species
                    .iter()
                    .map(|s| s.saturated_cells)
                    .sum::<usize>()
            );
        }
    }
}
