/// Pathos CLI — command-line interface for Pathos interactive stories.
///
/// Provides two subcommands:
/// - `run` — parses a story file and runs it in the terminal.
/// - `check` — parses a story file and reports diagnostics.

use std::path::PathBuf;

use clap::{Parser, Subcommand};
use pathos_core::{NarrativeRuntime, StoryState};
use pathos_parser::parse_file;
use pathos_tui::TuiBackend;

#[derive(Parser)]
#[command(name = "pathos", version, about = "Pathos interactive narrative engine")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run a story file interactively in the terminal.
    Run {
        /// Path to the story file (.pathos, .toml, .json, .yaml).
        file: PathBuf,
    },
    /// Parse a story file and print diagnostics without running.
    Check {
        /// Path to the story file.
        file: PathBuf,
    },
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Run { file } => {
            if let Err(e) = run_story(&file) {
                eprintln!("Error: {e}");
                std::process::exit(1);
            }
        }
        Commands::Check { file } => {
            if let Err(e) = check_story(&file) {
                eprintln!("Error: {e}");
                std::process::exit(1);
            }
        }
    }
}

fn run_story(path: &PathBuf) -> Result<(), String> {
    let source = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;

    let path_str = path.to_string_lossy();
    let output = parse_file(&path_str, &source);

    if output.diagnostics.iter().any(|d| {
        use pathos_parser::Severity;
        matches!(d.severity, Severity::Error)
    }) {
        eprintln!("Errors found in story file:");
        for d in &output.diagnostics {
            eprintln!("  [{:?}] {}", d.severity, d.message);
        }
        return Err("cannot run story with errors".into());
    }

    let mut runtime = NarrativeRuntime::new(
        output.config,
        output.graph,
        StoryState::default(),
    );

    let mut tui = TuiBackend::new()
        .map_err(|e| format!("cannot initialise terminal: {e}"))?;

    tui.run(&mut runtime)
        .map_err(|e| format!("runtime error: {e}"))
}

fn check_story(path: &PathBuf) -> Result<(), String> {
    let source = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;

    let path_str = path.to_string_lossy();
    let output = parse_file(&path_str, &source);

    println!("Story: {}", output.config.title);
    println!("Author: {}", output.config.author);
    println!("Start passage: {}", output.config.start);
    println!("Passages: {}", output.graph.nodes.len());
    println!();

    if output.diagnostics.is_empty() {
        println!("No issues found.");
    } else {
        for d in &output.diagnostics {
            let label = match d.severity {
                pathos_parser::Severity::Error => "ERROR",
                pathos_parser::Severity::Warning => "WARNING",
            };
            println!("[{label}] {}", d.message);
            if let Some(span) = &d.span {
                println!("  at line {}, column {}", span.line + 1, span.column + 1);
            }
        }

        let errors = output.diagnostics.iter().filter(|d| {
            matches!(d.severity, pathos_parser::Severity::Error)
        }).count();
        let warnings = output.diagnostics.len() - errors;
        println!();
        println!("{errors} error(s), {warnings} warning(s)");
    }

    Ok(())
}
