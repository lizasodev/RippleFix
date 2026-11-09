//! `ripplefix` command-line interface.
//!
//! Exit codes: 0 success, 1 breaking changes found with `--fail-on-breaking`,
//! 2 usage or runtime error.

mod render;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use ripplefix_core::plugin::{LanguageAnalyzer, PLUGIN_API_VERSION};
use ripplefix_core::vcs::Repository;
use ripplefix_python::PydanticAnalyzer;
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser)]
#[command(
    name = "ripplefix",
    version,
    about = "Change code once. Handle the consequences."
)]
struct Cli {
    /// Repository to operate on (defaults to the current directory).
    #[arg(long, short = 'C', global = true, default_value = ".")]
    repo: PathBuf,
    /// Increase log verbosity on stderr (-v info, -vv debug).
    #[arg(long, short, global = true, action = clap::ArgAction::Count)]
    verbose: u8,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Detect structural changes between a revision and the working tree.
    Analyze {
        /// Base revision to compare against.
        #[arg(long, short, default_value = "HEAD")]
        base: String,
        #[arg(long, value_enum, default_value_t = Format::Human)]
        format: Format,
        /// Exit with status 1 if any change breaks a serialized API contract.
        #[arg(long)]
        fail_on_breaking: bool,
    },
    /// Trace breaking changes to routes, TypeScript consumers, and tests.
    Impact {
        /// Base revision to compare against.
        #[arg(long, short, default_value = "HEAD")]
        base: String,
        #[arg(long, value_enum, default_value_t = Format::Human)]
        format: Format,
    },
    /// Check that the environment and repository are usable.
    Doctor,
}

/// Files larger than this are not indexed (generated or vendored code).
const MAX_INDEXED_FILE_BYTES: u64 = 2 * 1024 * 1024;

fn build_index(repo: &Repository) -> Result<ripplefix_impact::ProjectIndex> {
    let mut sources = Vec::new();
    for path in repo.list_files()? {
        let p = std::path::Path::new(&path);
        if !(ripplefix_python::is_python(p) || ripplefix_typescript::is_typescript(p)) {
            continue;
        }
        let full = repo.root().join(p);
        match std::fs::metadata(&full) {
            Ok(m) if m.is_file() && m.len() <= MAX_INDEXED_FILE_BYTES => {}
            _ => {
                tracing::info!(%path, "skipped: missing, not a regular file, or too large");
                continue;
            }
        }
        if let Some(text) = repo.read_worktree(&path) {
            sources.push(ripplefix_impact::SourceFile { path, text });
        }
    }
    Ok(ripplefix_impact::ProjectIndex::build(sources))
}

#[derive(Clone, Copy, ValueEnum)]
enum Format {
    Human,
    Json,
}

fn analyzers() -> Vec<&'static dyn LanguageAnalyzer> {
    vec![&PydanticAnalyzer]
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    init_logging(cli.verbose);
    match run(cli) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::from(2)
        }
    }
}

fn init_logging(verbose: u8) {
    let level = match verbose {
        0 => "warn",
        1 => "info",
        _ => "debug",
    };
    let filter = tracing_subscriber::EnvFilter::try_from_env("RIPPLEFIX_LOG")
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(level));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .init();
}

fn run(cli: Cli) -> Result<ExitCode> {
    match cli.command {
        Command::Analyze {
            base,
            format,
            fail_on_breaking,
        } => {
            let repo = Repository::discover(&cli.repo).context("not inside a Git repository")?;
            let report = ripplefix_core::analyze_worktree(&repo, &base, &analyzers())?;
            match format {
                Format::Json => println!("{}", serde_json::to_string_pretty(&report)?),
                Format::Human => print!("{}", render::human(&report)),
            }
            Ok(if fail_on_breaking && report.has_breaking_changes() {
                ExitCode::from(1)
            } else {
                ExitCode::SUCCESS
            })
        }
        Command::Impact { base, format } => {
            let repo = Repository::discover(&cli.repo).context("not inside a Git repository")?;
            let report = ripplefix_core::analyze_worktree(&repo, &base, &analyzers())?;
            let index = build_index(&repo)?;
            let impact = ripplefix_impact::compute_impact(&index, &report);
            match format {
                Format::Json => println!("{}", serde_json::to_string_pretty(&impact)?),
                Format::Human => print!("{}", render::impact(&impact)),
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Doctor => {
            let mut ok = true;
            match Repository::discover(&cli.repo) {
                Ok(r) => println!("ok    git repository: {}", r.root().display()),
                Err(e) => {
                    ok = false;
                    println!("FAIL  git repository: {e}");
                }
            }
            println!("info  plugin API version: {PLUGIN_API_VERSION}");
            for a in analyzers() {
                let status = if a.api_version() == PLUGIN_API_VERSION {
                    "ok  "
                } else {
                    "FAIL"
                };
                println!("{status}  analyzer {} (API v{})", a.id(), a.api_version());
            }
            Ok(if ok {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(2)
            })
        }
    }
}
