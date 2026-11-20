//! `arbrume`: batch generation of plants from the command line.
//!
//! Subcommands:
//!
//! * `gen`: generate `.glb` files for one species (`--species`) or for a batch config
//!   (`--config`), with deterministic per-plant seeds, resume and a JSONL manifest; a
//!   config file can also list a stand of plants grown together (`[[stand]]`);
//! * `species list`, `species show <name>`: the built-in presets;
//! * `info <file.toml>`: validate a species file;
//! * `bench`: time each stage of the pipeline and the batch throughput, as a Markdown
//!   table;
//! * `metrics`: the `ICTree` features of skeletons, summarised per species;
//! * `fit`: fit species parameters to a target skeleton; `distance`: compare two
//!   skeletons;
//! * `forest`: place a forest of a config file over a terrain and write its variant
//!   library, an instanced file and instance tables;
//! * `pack`: author, validate, build, install and list species packs; their species
//!   are named `pack_id/name` wherever a preset name is accepted.
//!
//! Exit codes: 0 on success, 1 when generation or writing failed, 2 for invalid
//! arguments, configuration or species files. The user guide is `docs/cli.md`.

#![forbid(unsafe_code)]

mod annotate;
mod batch;
mod bench;
mod config;
mod error;
mod fit_cmd;
mod forest_cmd;
mod lods;
mod metrics_cmd;
mod pack_cmd;
mod progress;
mod roots;
mod scene;
mod sheet;
mod source;
mod species_cmd;
mod stand;
mod textures;
mod water;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};

use crate::error::CliError;

/// Command line of `arbrume`.
#[derive(Debug, Parser)]
#[command(
    name = "arbrume",
    version,
    about = "Procedural tree and plant generator"
)]
struct Cli {
    /// The subcommand to run.
    #[command(subcommand)]
    command: Command,
}

/// Subcommands of `arbrume`.
#[derive(Debug, Subcommand)]
enum Command {
    /// Generate plants and write them as .glb files.
    Gen(Box<batch::GenArgs>),
    /// Built-in species presets.
    #[command(subcommand)]
    Species(SpeciesCommand),
    /// Validate a species file and print a summary.
    Info(InfoArgs),
    /// Time the pipeline on the presets and print a Markdown table.
    Bench(bench::BenchArgs),
    /// Summarise the realism features of skeletons per species.
    Metrics(metrics_cmd::MetricsArgs),
    /// Fit the parameters of a species to a target skeleton.
    Fit(Box<fit_cmd::FitArgs>),
    /// Distance between two tree skeletons.
    Distance(fit_cmd::DistanceCmdArgs),
    /// Species packs: author, validate, build, install, list.
    #[command(subcommand)]
    Pack(pack_cmd::PackCommand),
    /// Render a contact sheet of thumbnails, for tuning species by eye.
    #[command(hide = true)]
    Sheet(sheet::SheetArgs),
    /// Place a forest over a terrain and write its variants, instances and tables.
    Forest(Box<forest_cmd::ForestArgs>),
}

/// Subcommands of `arbrume species`.
#[derive(Debug, Subcommand)]
enum SpeciesCommand {
    /// List the built-in presets.
    List,
    /// Print the TOML file of a preset.
    Show {
        /// Name of the preset.
        name: String,
    },
}

/// Arguments of `arbrume info`.
#[derive(Debug, Args)]
struct InfoArgs {
    /// Species file to validate.
    file: PathBuf,
}

fn run(cli: Cli) -> Result<(), CliError> {
    match cli.command {
        Command::Gen(args) => batch::run(&args),
        Command::Species(SpeciesCommand::List) => {
            species_cmd::list();
            Ok(())
        }
        Command::Species(SpeciesCommand::Show { name }) => species_cmd::show(&name),
        Command::Info(args) => species_cmd::info(&args.file),
        Command::Bench(args) => bench::run(&args),
        Command::Metrics(args) => metrics_cmd::run(&args),
        Command::Fit(args) => fit_cmd::run_fit(&args),
        Command::Distance(args) => fit_cmd::run_distance(&args),
        Command::Pack(command) => pack_cmd::run(&command),
        Command::Sheet(args) => sheet::run(&args),
        Command::Forest(args) => forest_cmd::run(&args),
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            error.exit_code()
        }
    }
}
