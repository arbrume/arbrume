//! `arbrume fit` and `arbrume distance`: fitting species parameters to a target skeleton,
//! and the distance between two skeletons ([`arbrume_fit`], `docs/fitting.md`).
//!
//! `fit` writes the fitted species file (`--out`), the report as JSON (`--report`) and,
//! after every generation, a checkpoint (`--checkpoint`, by default the species file
//! with the extension `.checkpoint.json`) from which `--resume` continues. A resumed fit
//! ends with the same species file as an uninterrupted one.

use std::path::{Path, PathBuf};
use std::time::Instant;

use arbrume_fit::{
    Checkpoint, DistanceOptions, FitOptions, Terms, TreeSkeleton, Up, distance, fit_with_checkpoint,
};
use clap::{Args, ValueEnum};

use crate::error::CliError;
use crate::source::resolve;

/// Vertical axis of a skeleton file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum UpChoice {
    /// +Y up (Arbrume, glTF).
    Y,
    /// +Z up (most scans).
    Z,
}

impl From<UpChoice> for Up {
    fn from(choice: UpChoice) -> Self {
        match choice {
            UpChoice::Y => Self::Y,
            UpChoice::Z => Self::Z,
        }
    }
}

/// Settings of the distance shared by both commands.
#[derive(Debug, Args)]
pub struct DistanceArgs {
    /// Remove nodes thinner than this radius, in meters, with their subtrees.
    #[arg(long, default_value_t = 0.0)]
    min_radius: f32,
    /// Resampling step in meters [default: the height over 100].
    #[arg(long)]
    step: Option<f32>,
    /// Smoothing passes over the resampled axes, against position noise.
    #[arg(long, default_value_t = 0)]
    smooth: u32,
    /// Up axis of CSV and JSON skeletons.
    #[arg(long, value_enum, default_value = "y")]
    up: UpChoice,
}

impl DistanceArgs {
    fn options(&self) -> DistanceOptions {
        DistanceOptions {
            min_radius: self.min_radius,
            step: self.step,
            smooth: self.smooth,
            ..DistanceOptions::default()
        }
    }
}

/// Arguments of `arbrume distance`.
#[derive(Debug, Args)]
pub struct DistanceCmdArgs {
    /// First skeleton: `.ann.npz`, `.glb`, `.csv` or `.json`.
    a: PathBuf,
    /// Second skeleton.
    b: PathBuf,
    #[command(flatten)]
    distance: DistanceArgs,
    /// Print the terms as JSON.
    #[arg(long)]
    json: bool,
}

/// Arguments of `arbrume fit`.
#[derive(Debug, Args)]
pub struct FitArgs {
    /// Target skeleton: `.ann.npz`, `.glb`, `.csv` or `.json`.
    #[arg(long)]
    target: PathBuf,
    /// Base species: a preset name or a species file.
    #[arg(long)]
    from: String,
    /// Fitted parameters: `age`, `scale` and jitterable names [default: the jitterable
    /// parameters that change the skeleton of the base species, age and scale].
    #[arg(long, value_delimiter = ',')]
    params: Vec<String>,
    /// Plant seeds per candidate.
    #[arg(long, default_value_t = 4)]
    seeds: usize,
    /// Largest number of candidates evaluated.
    #[arg(long, default_value_t = 1200)]
    budget: usize,
    /// Seed of the fit.
    #[arg(long, default_value_t = 0)]
    seed: u64,
    /// Held-out seeds of the final evaluation.
    #[arg(long, default_value_t = 16)]
    holdout: usize,
    /// Search factors in [1/F, F].
    #[arg(long, value_name = "F", default_value_t = 3.0)]
    max_factor: f64,
    #[command(flatten)]
    distance: DistanceArgs,
    /// Fitted species file to write.
    #[arg(long, default_value = "fitted.toml")]
    out: PathBuf,
    /// Report to write as JSON.
    #[arg(long)]
    report: Option<PathBuf>,
    /// Checkpoint file [default: the species file with `.checkpoint.json`].
    #[arg(long)]
    checkpoint: Option<PathBuf>,
    /// Continue from the checkpoint when it exists.
    #[arg(long)]
    resume: bool,
    /// Worker threads [default: all cores].
    #[arg(long)]
    jobs: Option<usize>,
    /// Print errors only.
    #[arg(long, short)]
    quiet: bool,
}

/// Reads a skeleton or fails with a usage error.
fn read(path: &Path, up: UpChoice) -> Result<TreeSkeleton, CliError> {
    TreeSkeleton::read(path, up.into()).map_err(CliError::usage)
}

/// One line with every term.
fn terms_line(terms: &Terms) -> String {
    Terms::NAMES
        .iter()
        .zip(terms.values())
        .map(|(name, value)| format!("{name} {value:.4}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Runs `arbrume distance`.
pub fn run_distance(args: &DistanceCmdArgs) -> Result<(), CliError> {
    let a = read(&args.a, args.distance.up)?;
    let b = read(&args.b, args.distance.up)?;
    let clock = Instant::now();
    let terms = distance(&a, &b, &args.distance.options()).map_err(CliError::usage)?;
    let elapsed = clock.elapsed();
    if args.json {
        let text = serde_json::to_string_pretty(&terms).map_err(CliError::failed)?;
        println!("{text}");
    } else {
        println!("{}", terms_line(&terms));
        println!(
            "{} and {} nodes, {:.0} us",
            a.len(),
            b.len(),
            elapsed.as_secs_f64() * 1e6
        );
    }
    Ok(())
}

/// Writes `text` to `path`, through a temporary file so that a crash leaves the old one.
fn write_atomic(path: &Path, text: &str) -> Result<(), CliError> {
    let temporary = path.with_extension("tmp");
    std::fs::write(&temporary, text)
        .and_then(|()| std::fs::rename(&temporary, path))
        .map_err(|e| CliError::failed(format!("{}: {e}", path.display())))
}

/// The fit options of `args`.
fn fit_options(args: &FitArgs) -> FitOptions {
    FitOptions {
        parameters: (!args.params.is_empty()).then(|| args.params.clone()),
        seeds: args.seeds,
        budget: args.budget,
        seed: args.seed,
        holdout: args.holdout,
        max_factor: args.max_factor,
        distance: args.distance.options(),
        ..FitOptions::default()
    }
}

/// Runs `arbrume fit`.
pub fn run_fit(args: &FitArgs) -> Result<(), CliError> {
    let target = read(&args.target, args.distance.up)?;
    let base = resolve(&args.from, Path::new("."))?;
    let options = fit_options(args);
    let checkpoint_path = args
        .checkpoint
        .clone()
        .unwrap_or_else(|| args.out.with_extension("checkpoint.json"));
    let resume = if args.resume && checkpoint_path.exists() {
        let text = std::fs::read_to_string(&checkpoint_path).map_err(CliError::usage)?;
        let state: Checkpoint = serde_json::from_str(&text)
            .map_err(|e| CliError::usage(format!("{}: {e}", checkpoint_path.display())))?;
        Some(state)
    } else {
        None
    };
    let quiet = args.quiet;
    let mut save = |state: &Checkpoint| {
        let text =
            serde_json::to_string(state).map_err(|e| arbrume_fit::FitError::Io(e.to_string()))?;
        write_atomic(&checkpoint_path, &text)
            .map_err(|e| arbrume_fit::FitError::Io(e.to_string()))?;
        if !quiet && let Some(last) = state.history.last() {
            eprintln!(
                "generation {:>3}: {:>4} candidates, best {:.4}, mean {:.4}, sigma {:.3}",
                last.generation, last.evaluations, last.best, last.mean, last.sigma
            );
        }
        Ok(())
    };
    let run = || {
        fit_with_checkpoint(
            &target,
            &base.text,
            &base.label,
            &options,
            resume,
            &mut save,
        )
    };
    let result = match args.jobs {
        Some(jobs) => rayon::ThreadPoolBuilder::new()
            .num_threads(jobs.max(1))
            .build()
            .map_err(CliError::failed)?
            .install(run),
        None => run(),
    }
    .map_err(CliError::usage)?;
    write_atomic(&args.out, &result.toml)?;
    if let Some(path) = &args.report {
        let text = serde_json::to_string_pretty(&result.report).map_err(CliError::failed)?;
        write_atomic(path, &text)?;
    }
    if !quiet {
        let report = &result.report;
        let terms = |key: &str| serde_json::from_value::<Terms>(report[key]["holdout"].clone());
        let (before, after) = (terms("before"), terms("after"));
        if let (Ok(before), Ok(after)) = (before, after) {
            println!("before: {}", terms_line(&before));
            println!("after:  {}", terms_line(&after));
        }
        println!(
            "age {}, scale {:.4}, {} candidates in {:.1} s, stopped: {}; wrote {}",
            result.fitted.age,
            result.fitted.scale,
            report["evaluations"],
            result.checkpoint.seconds,
            report["stopped"].as_str().unwrap_or("?"),
            args.out.display()
        );
    }
    Ok(())
}
