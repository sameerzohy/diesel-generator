use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

/// namma-diesel: generate Rust Diesel code from namma-dsl storage specs.
#[derive(Parser)]
#[command(name = "namma-diesel", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}   

#[derive(Subcommand)]
enum Command {
    /// Generate Diesel code from a storage spec.
    Generate {
        /// Path to a spec YAML file.
        #[arg(long)]
        spec: PathBuf,
        /// Output directory for the generated crate.
        #[arg(long)]
        out: PathBuf,
    },  
}   

/// Entry point called from main().
pub fn run() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Generate { spec, out } => generate(&spec, &out),
    }   
}   

/// Commit 1: just read the spec and print it. Parsing comes in Commit 2.
fn generate(spec: &PathBuf, out: &PathBuf) -> Result<()> {
    let text = fs::read_to_string(spec)
        .with_context(|| format!("failed to read spec file {}", spec.display()))?;
    println!("--- spec: {}  (out: {}) ---", spec.display(), out.display());
    println!("{text}");
    Ok(())
}
