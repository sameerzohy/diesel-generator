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

/// Commit 2: read the spec, parse it into the IR, and print the IR.
fn generate(spec: &PathBuf, _out: &PathBuf) -> Result<()> {
    let text = fs::read_to_string(spec)
        .with_context(|| format!("failed to read spec file {}", spec.display()))?;
    let table = crate::parser::parse_spec(&text)?;
    println!("{table:#?}");
    Ok(())
}
