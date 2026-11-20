//! Fitting the parameters of a species to a target skeleton.
//!
//! The objective of a point `x` of the search space ([`Space`]) is the mean of the
//! [`Terms::total`] distance between the target and the plants of the base species with
//! the parameters of `x`, grown with `seeds` plant seeds: `plant_seed(seed, k)` for `k` in
//! `0..seeds`. The same seeds serve every candidate (common random numbers), so two
//! candidates are compared on the same draws, and the objective is a deterministic
//! function of `x`. Points outside the bounds are evaluated at the nearest point inside
//! and pay `penalty` times their squared distance to the box.
//!
//! [`Cmaes`] minimises it, one generation of `lambda` candidates at a time, every plant
//! of a generation grown by `arbrume_gen::run_batch` on the current `rayon` pool. The
//! search starts at the base species (all factors 1) at the middle of its
//! `variation.age` range, with the scale that matches the mean height of those plants to
//! the height of the target, and an initial step `0.3 ln(max_factor)`. It stops when the
//! next generation would exceed `budget` candidates, when the best distance has not
//! improved by more than `tolerance` for `patience` generations, or when the largest
//! standard deviation of the search falls below `min_std`.
//!
//! After the search, the best candidate and the base species are evaluated again on
//! `holdout` other seeds (`plant_seed(!seed, k)`), which the search never saw: the report
//! gives both distances, term by term.
//!
//! **Variation.** The candidates whose distance is within one seed-to-seed standard
//! deviation of the best (the standard deviation over its seeds of the best candidate's
//! distance) cannot be told apart from it by `seeds` plants. Their spread is written as
//! the `[variation]` of the fitted species: the range of their ages and scales, and for
//! each fitted parameter a jitter `sqrt(3)` times the standard deviation of their factor
//! relative to the best (the half-width of a uniform law of that standard deviation),
//! capped at 0.9 so that a jitter never brings a parameter near 0.
//!
//! **Checkpoints.** The whole state after a generation is a [`Checkpoint`]; resuming
//! from it gives the same result as a run that was not interrupted.

use std::fmt::Write as _;
use std::time::Instant;

use arbrume_core::rng::mix64;
use arbrume_core::species::Species;
use arbrume_gen::{plant_seed, run_batch};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::FitError;
use crate::cmaes::{Cmaes, CmaesSettings};
use crate::distance::{Descriptor, DistanceOptions, Terms, pruned_height, step_for};
use crate::skeleton::TreeSkeleton;
use crate::space::{Decoded, Parameter, Space, grow_skeleton, skeleton_parameters};

/// Name of the checkpoint format.
pub const CHECKPOINT_FORMAT: &str = "arbrume-fit-checkpoint";

/// Largest jitter written to the fitted `[variation]`.
const MAX_JITTER: f64 = 0.9;

/// Settings of a fit. The defaults are settings, not calibrated values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FitOptions {
    /// Names of the fitted parameters (`age`, `scale`, jitterable names); `None` for the
    /// jitterable parameters that change the skeleton of the base species, plus age and
    /// scale.
    pub parameters: Option<Vec<String>>,
    /// Plant seeds per candidate.
    pub seeds: usize,
    /// Largest number of candidates evaluated.
    pub budget: usize,
    /// Seed of the fit: of the search and of the plant seeds.
    pub seed: u64,
    /// Factors (and age and scale ratios) are searched in `[1 / max_factor, max_factor]`.
    pub max_factor: f64,
    /// Population of the evolution strategy; 0 for its default.
    pub lambda: usize,
    /// Generations without improvement before stopping.
    pub patience: usize,
    /// Smallest improvement of the best distance that counts.
    pub tolerance: f64,
    /// Stop when the largest standard deviation of the search is below this.
    pub min_std: f64,
    /// Weight of the squared distance to the bounds.
    pub penalty: f64,
    /// Seeds of the held-out evaluation.
    pub holdout: usize,
    /// Settings of the distance.
    pub distance: DistanceOptions,
}

impl Default for FitOptions {
    fn default() -> Self {
        Self {
            parameters: None,
            seeds: 4,
            budget: 1200,
            seed: 0,
            max_factor: 3.0,
            lambda: 0,
            patience: 30,
            tolerance: 1e-4,
            min_std: 1e-3,
            penalty: 1.0,
            holdout: 16,
            distance: DistanceOptions::default(),
        }
    }
}

/// One evaluated candidate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Candidate {
    /// Point of the search space.
    pub x: Vec<f64>,
    /// Mean terms over the seeds.
    pub terms: Terms,
    /// Standard deviation of the total over the seeds.
    pub spread: f64,
    /// Objective: the mean total plus the bound penalty.
    pub value: f64,
    /// Generation that sampled it.
    pub generation: u64,
}

/// Summary of one generation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Generation {
    /// Generation index.
    pub generation: u64,
    /// Candidates evaluated so far.
    pub evaluations: usize,
    /// Best objective so far.
    pub best: f64,
    /// Mean objective of the generation.
    pub mean: f64,
    /// Step size after the generation.
    pub sigma: f64,
}

/// The state of a fit after a generation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Checkpoint {
    /// [`CHECKPOINT_FORMAT`].
    pub format: String,
    /// Hash of the target, the base species and the options; a resume must match it.
    pub fingerprint: String,
    /// Search space.
    pub space: Space,
    /// Jitterable parameters left out because they do not change the skeleton.
    pub dropped: Vec<String>,
    /// Resampling step of the distance, meters.
    pub step: f32,
    /// Evolution strategy.
    pub cmaes: Cmaes,
    /// Every candidate evaluated.
    pub archive: Vec<Candidate>,
    /// Index of the best candidate in `archive`.
    pub best: Option<usize>,
    /// One entry per generation.
    pub history: Vec<Generation>,
    /// Generations since the best improved by more than the tolerance.
    pub stall: usize,
    /// Why the search stopped, once it has.
    pub stopped: Option<String>,
    /// Wall time spent so far, seconds.
    pub seconds: f64,
}

/// The result of a fit.
#[derive(Debug, Clone, PartialEq)]
pub struct FitResult {
    /// The fitted species file: the base species with the fitted parameters and a fitted
    /// `[variation]`.
    pub toml: String,
    /// The fitted species, parsed from `toml`.
    pub species: Species,
    /// Age, scale and factors of the best candidate.
    pub fitted: Decoded,
    /// The best candidate.
    pub best: Candidate,
    /// The report: distances before and after, term by term, settings and history.
    pub report: Value,
    /// The final state.
    pub checkpoint: Checkpoint,
}

/// Fits `base_toml`, a species file labelled `label`, to `target`.
///
/// # Errors
///
/// As [`fit_with_checkpoint`].
pub fn fit(
    target: &TreeSkeleton,
    base_toml: &str,
    label: &str,
    options: &FitOptions,
) -> Result<FitResult, FitError> {
    fit_with_checkpoint(target, base_toml, label, options, None, &mut |_| Ok(()))
}

/// Everything a fit reads, prepared once.
struct Problem<'a> {
    base: Species,
    target: Descriptor,
    options: &'a FitOptions,
    train: Vec<u64>,
    step: f32,
}

/// Fits `base_toml` to `target`, resuming from `resume` when given, and calls
/// `on_generation` with the state after every generation (to write a checkpoint).
///
/// # Errors
///
/// Returns [`FitError::Gen`] for an invalid base species, [`FitError::Skeleton`] for an
/// invalid or empty target, [`FitError::Invalid`] for bad options, a parameter name that
/// is not known, or a checkpoint of another fit, and any error of `on_generation`.
pub fn fit_with_checkpoint(
    target: &TreeSkeleton,
    base_toml: &str,
    label: &str,
    options: &FitOptions,
    resume: Option<Checkpoint>,
    on_generation: &mut dyn FnMut(&Checkpoint) -> Result<(), FitError>,
) -> Result<FitResult, FitError> {
    if options.seeds == 0 || options.max_factor <= 1.0 || !options.max_factor.is_finite() {
        return Err(FitError::invalid("seeds must be >= 1 and max_factor > 1"));
    }
    let base = Species::from_toml_str(base_toml).map_err(arbrume_gen::GenError::from)?;
    base.validate().map_err(arbrume_gen::GenError::from)?;
    let step = step_for(
        pruned_height(target, options.distance.min_radius),
        &options.distance,
    );
    let target_descriptor = Descriptor::new(target, step, &options.distance)?;
    let problem = Problem {
        base,
        target: target_descriptor,
        options,
        train: (0..options.seeds as u32)
            .map(|k| plant_seed(options.seed, k))
            .collect(),
        step,
    };
    let fingerprint = fingerprint(target, base_toml, options)?;
    let mut state = match resume {
        Some(state) if state.fingerprint == fingerprint && state.format == CHECKPOINT_FORMAT => {
            state
        }
        Some(_) => return Err(FitError::invalid("the checkpoint belongs to another fit")),
        None => start(&problem, fingerprint)?,
    };
    while state.stopped.is_none() {
        let clock = Instant::now();
        generation(&problem, &mut state);
        state.seconds += clock.elapsed().as_secs_f64();
        on_generation(&state)?;
    }
    finish(&problem, label, base_toml, state)
}

/// Hash of everything that defines a fit.
fn fingerprint(
    target: &TreeSkeleton,
    base: &str,
    options: &FitOptions,
) -> Result<String, FitError> {
    let options = serde_json::to_string(options).map_err(|e| FitError::invalid(e.to_string()))?;
    let mut hash = 0x5EED_u64;
    for text in [target.to_csv().as_str(), base, options.as_str()] {
        for chunk in text.as_bytes().chunks(8) {
            let mut word = [0_u8; 8];
            word[..chunk.len()].copy_from_slice(chunk);
            hash = mix64(hash ^ u64::from_le_bytes(word));
        }
        hash = mix64(hash ^ text.len() as u64);
    }
    Ok(format!("{hash:016x}"))
}

/// The initial state: the space, its start point and the evolution strategy.
fn start(problem: &Problem<'_>, fingerprint: String) -> Result<Checkpoint, FitError> {
    let options = problem.options;
    let [young, old] = problem.base.variation.age;
    let base_age = f64::midpoint(f64::from(young), f64::from(old));
    let probe = Decoded {
        age: base_age.round() as u16,
        scale: 1.0,
        factors: Vec::new(),
    };
    let (names, dropped) = if let Some(names) = &options.parameters {
        (names.clone(), Vec::new())
    } else {
        let probe_params = probe.params(problem.train[0]);
        let (mut kept, dropped) = skeleton_parameters(&problem.base, &probe_params);
        kept.extend(["age".to_owned(), "scale".to_owned()]);
        (kept, dropped)
    };
    let parameters: Vec<Parameter> = names
        .iter()
        .map(|n| Parameter::parse(n))
        .collect::<Result<_, _>>()?;
    if parameters.is_empty() {
        return Err(FitError::invalid("no parameter to fit"));
    }
    let width = options.max_factor.ln();
    let heights = run_batch(problem.train.len(), |_, _, k| {
        let skeleton = grow_skeleton(&problem.base, &probe.params(problem.train[k]));
        pruned_height(&skeleton, options.distance.min_radius)
    });
    let mean_height = f64::from(heights.iter().sum::<f32>()) / heights.len() as f64;
    let scale = if mean_height > 0.0 {
        f64::from(problem.target.height) / mean_height
    } else {
        1.0
    };
    let scale = scale.clamp(0.01, 100.0).ln();
    let centre = |p: &Parameter| if *p == Parameter::Scale { scale } else { 0.0 };
    let space = Space {
        lower: parameters.iter().map(|p| centre(p) - width).collect(),
        upper: parameters.iter().map(|p| centre(p) + width).collect(),
        start: parameters.iter().map(centre).collect(),
        parameters,
        base_age,
        base_scale: 1.0,
    };
    let settings = CmaesSettings {
        lambda: options.lambda,
        sigma: 0.3 * width,
        seed: options.seed,
    };
    let mut state = Checkpoint {
        format: CHECKPOINT_FORMAT.to_owned(),
        fingerprint,
        cmaes: Cmaes::new(space.start.clone(), &settings),
        space,
        dropped,
        step: problem.step,
        archive: Vec::new(),
        best: None,
        history: Vec::new(),
        stall: 0,
        stopped: None,
        seconds: 0.0,
    };
    let points = [base_point(&state.space), state.space.start.clone()];
    let results = evaluate(problem, &state.space, &points, &problem.train);
    record(&mut state, &points, results, options.penalty);
    Ok(state)
}

/// Adds the evaluated `points` to the archive and updates the best; returns their
/// objective values.
fn record(
    state: &mut Checkpoint,
    points: &[Vec<f64>],
    results: Vec<(Terms, f64)>,
    penalty: f64,
) -> Vec<f64> {
    let mut values = Vec::with_capacity(points.len());
    for (x, (terms, spread)) in points.iter().zip(results) {
        let value = terms.total + penalty * state.space.outside(x);
        values.push(value);
        state.archive.push(Candidate {
            x: x.clone(),
            terms,
            spread,
            value,
            generation: state.cmaes.generation,
        });
        let index = state.archive.len() - 1;
        if state.best.is_none_or(|b| value < state.archive[b].value) {
            state.best = Some(index);
        }
    }
    values
}

/// Mean terms and seed-to-seed standard deviation of the total of every point of `xs`
/// over `seeds`; an invalid species gets the largest distance, 1 for every term.
fn evaluate(
    problem: &Problem<'_>,
    space: &Space,
    xs: &[Vec<f64>],
    seeds: &[u64],
) -> Vec<(Terms, f64)> {
    let decoded: Vec<Decoded> = xs.iter().map(|x| space.decode(x)).collect();
    let species: Vec<Option<Species>> = decoded
        .iter()
        .map(|d| d.species(&problem.base).ok())
        .collect();
    let options = &problem.options.distance;
    let worst = Terms {
        envelope: 1.0,
        density: 1.0,
        geometry: 1.0,
        counts: 1.0,
        features: 1.0,
        total: 1.0,
    };
    let per_seed = run_batch(xs.len() * seeds.len(), |_, _, job| {
        let (c, k) = (job / seeds.len(), job % seeds.len());
        let Some(species) = &species[c] else {
            return worst;
        };
        let skeleton = grow_skeleton(species, &decoded[c].params(seeds[k]));
        Descriptor::new(&skeleton, problem.step, options)
            .map_or(worst, |d| problem.target.compare(&d, options))
    });
    per_seed
        .chunks(seeds.len())
        .map(|terms| {
            let mean = Terms::mean(terms);
            let var = terms
                .iter()
                .map(|t| (t.total - mean.total).powi(2))
                .sum::<f64>()
                / terms.len() as f64;
            (mean, var.sqrt())
        })
        .collect()
}

/// Runs one generation: samples, evaluates, updates the strategy and the stop reason.
fn generation(problem: &Problem<'_>, state: &mut Checkpoint) {
    let options = problem.options;
    let lambda = state.cmaes.lambda();
    if state.archive.len() + lambda > options.budget {
        state.stopped = Some("budget".to_owned());
        return;
    }
    let xs = state.cmaes.ask();
    let results = evaluate(problem, &state.space, &xs, &problem.train);
    let previous = state.best.map_or(f64::INFINITY, |b| state.archive[b].value);
    let values = record(state, &xs, results, options.penalty);
    let best = state.best.map_or(f64::INFINITY, |b| state.archive[b].value);
    state.stall = if best < previous - options.tolerance {
        0
    } else {
        state.stall + 1
    };
    state.cmaes.tell(&xs, &values);
    state.history.push(Generation {
        generation: state.cmaes.generation - 1,
        evaluations: state.archive.len(),
        best,
        mean: values.iter().sum::<f64>() / values.len() as f64,
        sigma: state.cmaes.sigma,
    });
    if state.stall >= options.patience {
        state.stopped = Some("no improvement".to_owned());
    } else if state.cmaes.max_std() < options.min_std {
        state.stopped = Some("converged".to_owned());
    } else if state.archive.len() + lambda > options.budget {
        state.stopped = Some("budget".to_owned());
    }
}

/// The fitted `[variation]`: age and scale ranges and jitters of the good candidates.
#[derive(Debug, Clone, PartialEq, Serialize)]
struct FittedVariation {
    candidates: usize,
    age: [u16; 2],
    scale: [f32; 2],
    jitter: Vec<(String, f64)>,
}

/// The variation of the candidates indistinguishable from the best.
fn fitted_variation(state: &Checkpoint, best: &Candidate) -> FittedVariation {
    let limit = best.terms.total + best.spread;
    let mut good: Vec<&Candidate> = state
        .archive
        .iter()
        .filter(|c| c.value <= limit && state.space.outside(&c.x) == 0.0)
        .collect();
    if good.is_empty() {
        good.push(best);
    }
    let decoded: Vec<Decoded> = good.iter().map(|c| state.space.decode(&c.x)).collect();
    let ages: Vec<u16> = decoded.iter().map(|d| d.age).collect();
    let scales: Vec<f32> = decoded.iter().map(|d| d.scale).collect();
    let best_x = state.space.clamp(&best.x);
    let mut jitter = Vec::new();
    for (i, parameter) in state.space.parameters.iter().enumerate() {
        if let Parameter::Factor(name) = parameter {
            let ratios: Vec<f64> = good
                .iter()
                .map(|c| (state.space.clamp(&c.x)[i] - best_x[i]).exp())
                .collect();
            let mean = ratios.iter().sum::<f64>() / ratios.len() as f64;
            let var = ratios.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / ratios.len() as f64;
            jitter.push((name.clone(), (3.0_f64.sqrt() * var.sqrt()).min(MAX_JITTER)));
        }
    }
    FittedVariation {
        candidates: good.len(),
        age: [
            ages.iter().copied().min().unwrap_or(1),
            ages.iter().copied().max().unwrap_or(1),
        ],
        scale: [
            scales.iter().copied().fold(f32::INFINITY, f32::min),
            scales.iter().copied().fold(0.0, f32::max),
        ],
        jitter,
    }
}

/// `value` with its floats rounded to six significant digits.
fn rounded(value: &toml::Value) -> toml::Value {
    match value {
        toml::Value::Float(f) if *f != 0.0 && f.is_finite() => {
            let digits = 5 - f.abs().log10().floor() as i32;
            let factor = 10_f64.powi(digits);
            toml::Value::Float((f * factor).round() / factor)
        }
        toml::Value::Array(items) => toml::Value::Array(items.iter().map(rounded).collect()),
        toml::Value::Table(table) => {
            toml::Value::Table(table.iter().map(|(k, v)| (k.clone(), rounded(v))).collect())
        }
        other => other.clone(),
    }
}

/// The table at `key` of `table`, created (or replacing another value) when needed.
fn subtable<'t>(table: &'t mut toml::Table, key: &str) -> &'t mut toml::Table {
    let entry = table
        .entry(key)
        .or_insert_with(|| toml::Value::Table(toml::Table::new()));
    if !entry.is_table() {
        *entry = toml::Value::Table(toml::Table::new());
    }
    match entry {
        toml::Value::Table(inner) => inner,
        _ => unreachable!("the entry was just made a table"),
    }
}

/// Sets the dotted `path` of `table` to `value`, creating tables on the way.
fn set_path(table: &mut toml::Table, path: &str, value: toml::Value) {
    let mut segments: Vec<&str> = path.split('.').collect();
    let leaf = segments.pop().unwrap_or(path);
    let target = segments.into_iter().fold(table, subtable);
    target.insert(leaf.to_owned(), value);
}

/// The value at the dotted `path` of `value`.
fn get_path<'v>(value: &'v toml::Value, path: &str) -> Option<&'v toml::Value> {
    path.split('.')
        .try_fold(value, |value, segment| value.get(segment))
}

/// The fitted species file: `base_toml` with the fitted values and variation.
fn species_toml(
    base_toml: &str,
    fitted: &Species,
    decoded: &Decoded,
    variation: &FittedVariation,
    header: &str,
) -> Result<String, FitError> {
    let invalid = |e: &dyn std::fmt::Display| FitError::invalid(format!("species file: {e}"));
    let mut table: toml::Table = toml::from_str(base_toml).map_err(|e| invalid(&e))?;
    let full = toml::Value::try_from(fitted).map_err(|e| invalid(&e))?;
    for (name, _) in &decoded.factors {
        let value = get_path(&full, name).ok_or_else(|| invalid(&name))?;
        set_path(&mut table, name, rounded(value));
    }
    let name = format!("{} (fitted)", fitted.name);
    table.insert("name".to_owned(), toml::Value::String(name));
    let age = variation.age.map(|a| toml::Value::Integer(i64::from(a)));
    set_path(
        &mut table,
        "variation.age",
        toml::Value::Array(age.to_vec()),
    );
    let scale = variation
        .scale
        .map(|s| rounded(&toml::Value::Float(f64::from(s))));
    set_path(
        &mut table,
        "variation.scale",
        toml::Value::Array(scale.to_vec()),
    );
    let jitters = subtable(subtable(&mut table, "variation"), "jitter");
    for (name, amount) in &variation.jitter {
        jitters.insert(name.clone(), rounded(&toml::Value::Float(*amount)));
    }
    let body = toml::to_string(&table).map_err(|e| invalid(&e))?;
    Ok(format!("{header}{body}"))
}

/// The point of the base species: every factor 1, the base age, scale 1.
fn base_point(space: &Space) -> Vec<f64> {
    vec![0.0; space.parameters.len()]
}

/// Held-out evaluation, variation, species file and report.
fn finish(
    problem: &Problem<'_>,
    label: &str,
    base_toml: &str,
    state: Checkpoint,
) -> Result<FitResult, FitError> {
    let options = problem.options;
    let best_index = state
        .best
        .ok_or_else(|| FitError::invalid("the budget allows no generation"))?;
    let best = state.archive[best_index].clone();
    let space = &state.space;
    let fitted = space.decode(&best.x);
    let holdout: Vec<u64> = (0..options.holdout.max(1) as u32)
        .map(|k| plant_seed(!options.seed, k))
        .collect();
    let points = [
        base_point(space),
        space.start.clone(),
        best.x.clone(),
        state.cmaes.mean.clone(),
    ];
    let held = evaluate(problem, space, &points, &holdout);
    let trained = evaluate(problem, space, &points[..2], &problem.train);
    let variation = fitted_variation(&state, &best);
    let mut header = String::new();
    let _ = writeln!(
        header,
        "# Fitted by `arbrume fit` from `{label}`: age {}, scale {:.4}.",
        fitted.age, fitted.scale
    );
    let _ = writeln!(
        header,
        "# Distance to the target on {} held-out seeds: {:.4} for the base, {:.4} fitted.",
        holdout.len(),
        held[0].0.total,
        held[2].0.total
    );
    let species = fitted.species(&problem.base)?;
    let toml = species_toml(base_toml, &species, &fitted, &variation, &header)?;
    let parsed = Species::from_toml_str(&toml)
        .map_err(|e| FitError::invalid(format!("fitted species: {e}")))?;
    let names: Vec<&str> = space.parameters.iter().map(Parameter::name).collect();
    let factors: serde_json::Map<String, Value> = fitted
        .factors
        .iter()
        .map(|(n, f)| (n.clone(), json!(f)))
        .collect();
    let report = json!({
        "format": "arbrume-fit-report",
        "base": label,
        "target": {
            "height": problem.target.height,
            "reach": problem.target.reach,
            "resampled_nodes": problem.target.nodes,
            "axes_per_order": problem.target.counts,
        },
        "step": problem.step,
        "parameters": names,
        "dropped_parameters": state.dropped,
        "options": options,
        "seeds": { "train": problem.train, "holdout": holdout },
        "before": {
            "train": trained[0].0, "holdout": held[0].0, "holdout_spread": held[0].1,
            "age": space.decode(&points[0]).age, "scale": 1.0,
        },
        "start": {
            "train": trained[1].0, "holdout": held[1].0,
            "scale": space.decode(&space.start).scale,
        },
        "after": {
            "train": best.terms, "train_spread": best.spread,
            "holdout": held[2].0, "holdout_spread": held[2].1,
        },
        "search_mean": { "holdout": held[3].0, "point": space.decode(&state.cmaes.mean) },
        "fitted": { "age": fitted.age, "scale": fitted.scale, "factors": factors },
        "variation": variation,
        "generations": state.history.len(),
        "evaluations": state.archive.len(),
        "growths": state.archive.len() * problem.train.len(),
        "stopped": state.stopped,
        "seconds": state.seconds,
        "history": state.history,
    });
    Ok(FitResult {
        toml,
        species: parsed,
        fitted,
        best,
        report,
        checkpoint: state,
    })
}
