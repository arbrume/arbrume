//! Tests of the distance, the evolution strategy and the fit.

use arbrume_core::rng::{Rng, Stream};
use arbrume_fit::{
    Checkpoint, Cmaes, CmaesSettings, DistanceOptions, FitOptions, TreeSkeleton, distance, fit,
    fit_with_checkpoint, grow_skeleton,
};
use arbrume_gen::{GenParams, preset, preset_toml};

/// The skeleton of a small preset plant.
fn plant(name: &str, seed: u64, age: u16) -> TreeSkeleton {
    let species = preset(name).unwrap();
    grow_skeleton(&species, &GenParams::new(seed, age))
}

/// `skeleton` with every position moved by a uniform random offset of up to `amount`.
fn jittered(skeleton: &TreeSkeleton, amount: f32) -> TreeSkeleton {
    let mut out = skeleton.clone();
    for (i, p) in out.position.iter_mut().enumerate() {
        let mut rng = Rng::new(5, i as u32, Stream::custom(9));
        for c in p.iter_mut() {
            *c += amount * rng.signed();
        }
    }
    out
}

/// `skeleton` with its nodes in reverse order.
fn reversed(skeleton: &TreeSkeleton) -> TreeSkeleton {
    let n = skeleton.len();
    let new = |old: usize| i32::try_from(n - 1 - old).unwrap();
    TreeSkeleton {
        position: skeleton.position.iter().rev().copied().collect(),
        radius: skeleton.radius.iter().rev().copied().collect(),
        parent: skeleton
            .parent
            .iter()
            .rev()
            .map(|&p| usize::try_from(p).map_or(-1, new))
            .collect(),
    }
}

#[test]
fn identical_skeletons_are_at_distance_zero() {
    let options = DistanceOptions::default();
    for name in ["broadleaf", "shrub", "conifer"] {
        let tree = plant(name, 3, 12);
        let terms = distance(&tree, &tree, &options).unwrap();
        assert_eq!(terms.values(), [0.0; 6], "{name}");
    }
}

#[test]
fn the_distance_is_symmetric_and_ignores_node_order() {
    let options = DistanceOptions::default();
    let (a, b) = (plant("broadleaf", 1, 14), plant("maple_like", 2, 20));
    let ab = distance(&a, &b, &options).unwrap();
    let ba = distance(&b, &a, &options).unwrap();
    assert_eq!(ab, ba);
    assert!(ab.total > 0.05, "{ab:?}");
    let shuffled = distance(&reversed(&a), &b, &options).unwrap();
    assert_eq!(shuffled, ab);
    let self_shuffled = distance(&reversed(&a), &a, &options).unwrap();
    assert_eq!(self_shuffled.total, 0.0);
}

#[test]
fn the_distance_grows_with_the_perturbation() {
    let options = DistanceOptions::default();
    let tree = plant("broadleaf", 4, 16);
    let totals: Vec<f64> = [0.02, 0.1, 0.4]
        .iter()
        .map(|&amount| {
            distance(&tree, &jittered(&tree, amount), &options)
                .unwrap()
                .total
        })
        .collect();
    assert!(
        totals[0] > 0.0 && totals[0] < totals[1] && totals[1] < totals[2],
        "{totals:?}"
    );
    let species = preset("broadleaf").unwrap();
    let factors: Vec<f64> = [1.1, 1.4, 2.0]
        .iter()
        .map(|&f| {
            let varied = species.with_factor("growth.axis_length", f).unwrap();
            let other = grow_skeleton(&varied, &GenParams::new(4, 16));
            distance(&tree, &other, &options).unwrap().total
        })
        .collect();
    assert!(
        factors[0] < factors[1] && factors[1] < factors[2],
        "{factors:?}"
    );
}

#[test]
fn pruning_removes_thin_branches_from_both_trees() {
    let tree = plant("broadleaf", 4, 16);
    let thin: f32 = {
        let mut radii = tree.radius.clone();
        radii.sort_by(f32::total_cmp);
        radii[radii.len() / 2]
    };
    let mut noisy = tree.clone();
    for (i, radius) in noisy.radius.iter_mut().enumerate() {
        if *radius < thin && i % 3 == 0 {
            *radius = 0.0;
        }
    }
    let raw = distance(&tree, &noisy, &DistanceOptions::default()).unwrap();
    let options = DistanceOptions {
        min_radius: thin,
        ..DistanceOptions::default()
    };
    let pruned = distance(&tree, &noisy, &options).unwrap();
    assert!(pruned.total < raw.total, "{pruned:?} {raw:?}");
}

/// Runs CMA-ES on `f` from `start` and returns the best value found.
fn minimise(f: impl Fn(&[f64]) -> f64, start: Vec<f64>, generations: usize) -> f64 {
    let settings = CmaesSettings {
        lambda: 0,
        sigma: 0.5,
        seed: 3,
    };
    let mut cmaes = Cmaes::new(start, &settings);
    let mut best = f64::INFINITY;
    for _ in 0..generations {
        let xs = cmaes.ask();
        let values: Vec<f64> = xs.iter().map(|x| f(x)).collect();
        best = values.iter().copied().fold(best, f64::min);
        cmaes.tell(&xs, &values);
    }
    best
}

#[test]
fn cmaes_minimises_the_sphere_and_rosenbrock() {
    let sphere = |x: &[f64]| x.iter().map(|v| v * v).sum::<f64>();
    assert!(minimise(sphere, vec![1.0; 8], 300) < 1e-12);
    let rosenbrock = |x: &[f64]| {
        x.windows(2)
            .map(|w| 100.0 * (w[1] - w[0] * w[0]).powi(2) + (1.0 - w[0]).powi(2))
            .sum::<f64>()
    };
    assert!(minimise(rosenbrock, vec![0.0; 6], 1500) < 1e-8);
}

#[test]
fn cmaes_state_round_trips_through_json() {
    let mut cmaes = Cmaes::new(
        vec![0.3, -0.2, 1.0],
        &CmaesSettings {
            lambda: 0,
            sigma: 0.4,
            seed: 1,
        },
    );
    let xs = cmaes.ask();
    let values: Vec<f64> = xs.iter().map(|x| x.iter().sum::<f64>().sin()).collect();
    cmaes.tell(&xs, &values);
    let back: Cmaes = serde_json::from_str(&serde_json::to_string(&cmaes).unwrap()).unwrap();
    assert_eq!(back, cmaes);
}

/// Small fit options for the tests.
fn small_options() -> FitOptions {
    FitOptions {
        parameters: Some(vec![
            "growth.axis_length".to_owned(),
            "growth.branching_angle".to_owned(),
            "scale".to_owned(),
        ]),
        seeds: 2,
        budget: 4 * 7,
        seed: 11,
        holdout: 2,
        ..FitOptions::default()
    }
}

/// A target: the sapling preset with a longer axis length, at age 6.
fn sapling_target() -> TreeSkeleton {
    let species = preset("sapling").unwrap();
    let varied = species.with_factor("growth.axis_length", 1.3).unwrap();
    grow_skeleton(&varied, &GenParams::new(99, 6))
}

#[test]
fn a_fit_does_not_depend_on_the_thread_count() {
    let target = sapling_target();
    let text = preset_toml("sapling").unwrap();
    let options = small_options();
    let run = |threads: usize| {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap();
        pool.install(|| fit(&target, text, "sapling", &options).unwrap())
    };
    let (one, many) = (run(1), run(5));
    assert_eq!(one.toml, many.toml);
    assert_eq!(one.best, many.best);
    assert_eq!(one.report["after"], many.report["after"]);
    assert!(one.best.terms.total <= one.report["start"]["train"]["total"].as_f64().unwrap());
    let reparsed = arbrume_core::species::Species::from_toml_str(&one.toml).unwrap();
    assert_eq!(reparsed, one.species);
}

#[test]
fn a_resumed_fit_ends_where_an_uninterrupted_one_does() {
    let target = sapling_target();
    let text = preset_toml("sapling").unwrap();
    let options = small_options();
    let whole = fit(&target, text, "sapling", &options).unwrap();
    let mut saved: Vec<String> = Vec::new();
    let mut keep = |state: &Checkpoint| {
        saved.push(serde_json::to_string(state).unwrap());
        Ok(())
    };
    fit_with_checkpoint(&target, text, "sapling", &options, None, &mut keep).unwrap();
    assert!(saved.len() >= 3);
    let middle: Checkpoint = serde_json::from_str(&saved[1]).unwrap();
    let resumed = fit_with_checkpoint(
        &target,
        text,
        "sapling",
        &options,
        Some(middle),
        &mut |_| Ok(()),
    )
    .unwrap();
    assert_eq!(resumed.toml, whole.toml);
    assert_eq!(resumed.best, whole.best);
    assert_eq!(resumed.checkpoint.cmaes, whole.checkpoint.cmaes);
    let other = FitOptions {
        seed: 12,
        ..options
    };
    let first: Checkpoint = serde_json::from_str(&saved[0]).unwrap();
    assert!(
        fit_with_checkpoint(&target, text, "sapling", &other, Some(first), &mut |_| Ok(
            ()
        ))
        .is_err()
    );
}
