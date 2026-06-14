use std::fs;
use std::path::{Path, PathBuf};

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
        /// Optional project type-registry config (TOML). Custom/imported types
        /// (e.g. HighPrecMoney, UTCTime) are resolved from here.
        #[arg(long)]
        config: Option<PathBuf>,
    },
}

/// Entry point called from main().
pub fn run() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Generate { spec, out, config } => generate(&spec, &out, config.as_deref()),
    }
}

/// Commit 4: parse the spec, resolve types from the project config, and write
/// the Diesel schema files under `<out>/src/schema/`.
fn generate(spec: &Path, out: &Path, config_path: Option<&Path>) -> Result<()> {
    let text = fs::read_to_string(spec)
        .with_context(|| format!("failed to read spec file {}", spec.display()))?;
    let table = crate::parser::parse_spec(&text)?;
    let config = crate::config::Config::load(config_path)?;

    let tables = [table];
    let mut files = crate::codegen::generate_schema(&tables, &config)?;
    files.extend(crate::codegen::generate_models(&tables, &config)?);
    files.extend(crate::codegen::generate_types(&tables, &config)?);
    for file in &files {
        let dest = out.join(&file.path);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        fs::write(&dest, &file.contents)
            .with_context(|| format!("failed to write {}", dest.display()))?;
        println!("wrote {}", dest.display());
    }
    Ok(())
}
