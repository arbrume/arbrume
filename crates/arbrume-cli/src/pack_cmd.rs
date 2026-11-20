//! `arbrume pack`: author, validate, build, install and list species packs
//! (`docs/packs.md`).
//!
//! An invalid pack exits with code 2, like an invalid species file; a file that cannot
//! be written exits with code 1.

use std::path::{Path, PathBuf};

use arbrume_pack::{
    EXTENSION, Pack, PackError, PackStore, Report, THUMBNAIL_SIZE, add_species, build_archive,
    new_pack, validate_path, write_thumbnails,
};
use clap::Subcommand;

use crate::error::CliError;

/// Subcommands of `arbrume pack`.
#[derive(Debug, Subcommand)]
pub enum PackCommand {
    /// Create a pack directory with a pack.toml and a placeholder licence file.
    New {
        /// Directory of the new pack; must not exist or be empty.
        dir: PathBuf,
        /// Pack id [default: from the directory name].
        #[arg(long)]
        id: Option<String>,
        /// Display name [default: the id].
        #[arg(long)]
        name: Option<String>,
    },
    /// Add a species to a pack directory, from a preset or a species file.
    AddSpecies {
        /// Pack directory.
        dir: PathBuf,
        /// Preset name or species TOML file.
        source: String,
        /// Name in the pack [default: the preset name or file stem].
        #[arg(long)]
        name: Option<String>,
        /// Name shown to users.
        #[arg(long)]
        display_name: Option<String>,
        /// Category shown to users, such as "broadleaf tree".
        #[arg(long)]
        category: Option<String>,
    },
    /// Validate a pack directory or archive: files, species, growth, images.
    Validate {
        /// Pack directory or .arbpack file.
        path: PathBuf,
    },
    /// Render the missing thumbnails of a pack directory.
    Thumbs {
        /// Pack directory.
        dir: PathBuf,
        /// Side of the thumbnails in pixels.
        #[arg(long, default_value_t = THUMBNAIL_SIZE)]
        size: u32,
        /// Render every thumbnail, replacing existing ones.
        #[arg(long)]
        force: bool,
    },
    /// Build a deterministic .arbpack archive of a pack directory.
    Build {
        /// Pack directory.
        dir: PathBuf,
        /// Archive to write [default: ID-VERSION.arbpack].
        #[arg(long, short)]
        out: Option<PathBuf>,
    },
    /// Validate a pack and install it into the pack directory.
    Install {
        /// .arbpack file or pack directory.
        file: PathBuf,
        /// Directory of installed packs to install into [default: the user pack
        /// directory].
        #[arg(long)]
        dir: Option<PathBuf>,
    },
    /// List the installed packs and their species.
    List {
        /// Directory of installed packs [default: the user pack directory].
        #[arg(long)]
        dir: Option<PathBuf>,
    },
    /// Remove an installed pack.
    Remove {
        /// Id of the pack.
        id: String,
        /// Directory of installed packs [default: the user pack directory].
        #[arg(long)]
        dir: Option<PathBuf>,
    },
}

/// A pack error as a command line error: write failures exit with 1, the rest with 2.
fn cli_error(error: PackError) -> CliError {
    match error {
        PackError::Io { .. } => CliError::failed(error),
        other => CliError::usage(other),
    }
}

/// The store in `dir`, or the default one.
fn store(dir: Option<&Path>) -> Result<PackStore, CliError> {
    match dir {
        Some(dir) => Ok(PackStore::new(dir)),
        None => PackStore::open_default().map_err(cli_error),
    }
}

/// Runs `arbrume pack`.
pub fn run(command: &PackCommand) -> Result<(), CliError> {
    match command {
        PackCommand::New { dir, id, name } => {
            let id = new_pack(dir, id.as_deref(), name.as_deref()).map_err(cli_error)?;
            println!("created pack `{id}` in {}", dir.display());
            println!(
                "next: arbrume pack add-species {} PRESET|FILE",
                dir.display()
            );
            Ok(())
        }
        PackCommand::AddSpecies {
            dir,
            source,
            name,
            display_name,
            category,
        } => {
            let added = add_species(
                dir,
                source,
                name.as_deref(),
                display_name.as_deref(),
                category.as_deref(),
            )
            .map_err(cli_error)?;
            println!("added species `{added}` to {}", dir.display());
            Ok(())
        }
        PackCommand::Validate { path } => validate(path),
        PackCommand::Thumbs { dir, size, force } => thumbs(dir, *size, *force),
        PackCommand::Build { dir, out } => build(dir, out.as_deref()),
        PackCommand::Install { file, dir } => install(file, dir.as_deref()),
        PackCommand::List { dir } => list(&store(dir.as_deref())?),
        PackCommand::Remove { id, dir } => {
            let path = store(dir.as_deref())?.remove(id).map_err(cli_error)?;
            println!("removed pack `{id}` from {}", path.display());
            Ok(())
        }
    }
}

/// Prints the warnings and errors of `report`.
fn print_report(report: &Report) {
    for warning in &report.warnings {
        eprintln!("warning: {warning}");
    }
    for error in &report.errors {
        eprintln!("error: {error}");
    }
}

/// `arbrume pack validate`.
fn validate(path: &Path) -> Result<(), CliError> {
    let report = validate_path(path);
    print_report(&report);
    if !report.is_ok() {
        return Err(CliError::usage(format!(
            "{}: invalid pack ({} errors)",
            path.display(),
            report.errors.len()
        )));
    }
    let pack = Pack::load(path).map_err(cli_error)?;
    let manifest = pack.manifest();
    println!(
        "{}: valid pack `{}` {}, {} species, {} warnings",
        path.display(),
        manifest.id,
        manifest.version,
        manifest.species.len(),
        report.warnings.len()
    );
    Ok(())
}

/// `arbrume pack thumbs`.
fn thumbs(dir: &Path, size: u32, force: bool) -> Result<(), CliError> {
    let written = write_thumbnails(dir, size, force).map_err(cli_error)?;
    for path in &written {
        println!("{}", dir.join(path).display());
    }
    println!("{} thumbnails written", written.len());
    Ok(())
}

/// `arbrume pack build`.
fn build(dir: &Path, out: Option<&Path>) -> Result<(), CliError> {
    let pack = Pack::load(dir).map_err(cli_error)?;
    let mut report = arbrume_pack::validate(&pack);
    report
        .warnings
        .retain(|w| !w.file.starts_with("thumbnails/"));
    print_report(&report);
    let bytes = build_archive(&pack).map_err(cli_error)?;
    let manifest = pack.manifest();
    let out = out.map_or_else(
        || PathBuf::from(format!("{}-{}.{EXTENSION}", manifest.id, manifest.version)),
        Path::to_path_buf,
    );
    let temporary = out.with_extension(format!("{EXTENSION}.tmp"));
    std::fs::write(&temporary, &bytes)
        .and_then(|()| std::fs::rename(&temporary, &out))
        .map_err(|e| CliError::failed(format!("{}: {e}", out.display())))?;
    println!(
        "{}: pack `{}` {}, {} bytes",
        out.display(),
        manifest.id,
        manifest.version,
        bytes.len()
    );
    Ok(())
}

/// `arbrume pack install`.
fn install(file: &Path, dir: Option<&Path>) -> Result<(), CliError> {
    let store = store(dir)?;
    let outcome = store.install(file).map_err(cli_error)?;
    let replaced = outcome
        .replaced
        .map(|version| format!(", replacing {version}"))
        .unwrap_or_default();
    println!(
        "installed pack `{}` {}{replaced} in {}",
        outcome.id,
        outcome.version,
        outcome.path.display()
    );
    Ok(())
}

/// `arbrume pack list`: one line per pack, then one indented line per species.
fn list(store: &PackStore) -> Result<(), CliError> {
    let packs = store.list().map_err(cli_error)?;
    if packs.is_empty() {
        println!("no pack installed in {}", store.dir().display());
    }
    for pack in packs {
        let pack = match pack {
            Ok(pack) => pack,
            Err(error) => {
                eprintln!("warning: {error}");
                continue;
            }
        };
        let manifest = pack.manifest();
        println!(
            "{}\t{}\t{}\t{}",
            manifest.id, manifest.version, manifest.name, manifest.content_licence
        );
        for entry in &manifest.species {
            println!(
                "  {}/{}\t{}\t{}",
                manifest.id, entry.name, entry.display_name, entry.category
            );
        }
    }
    Ok(())
}
