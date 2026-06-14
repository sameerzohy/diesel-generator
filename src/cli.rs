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
        /// Skip the `cargo check` verification pass (faster, for iterating).
        #[arg(long)]
        no_verify: bool,
    },
}

/// Entry point called from main().
pub fn run() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Generate {
            spec,
            out,
            config,
            no_verify,
        } => generate(&spec, &out, config.as_deref(), no_verify),
    }
}

/// Derive a valid Cargo package name from the output directory's basename.
fn crate_name_from(out: &Path) -> String {
    let base = out
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("generated_diesel");
    let sanitized: String = base
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    if sanitized.is_empty() {
        "generated_diesel".to_string()
    } else {
        sanitized
    }
}

/// Commit 4: parse the spec, resolve types from the project config, and write
/// the Diesel schema files under `<out>/src/schema/`.
fn generate(spec: &Path, out: &Path, config_path: Option<&Path>, no_verify: bool) -> Result<()> {
    let text = fs::read_to_string(spec)
        .with_context(|| format!("failed to read spec file {}", spec.display()))?;
    let table = crate::parser::parse_spec(&text)?;
    let config = crate::config::Config::load(config_path)?;

    let tables = [table];
    let mut files = crate::codegen::generate_schema(&tables, &config)?;
    files.extend(crate::codegen::generate_models(&tables, &config)?);
    files.extend(crate::codegen::generate_types(&tables, &config)?);
    files.extend(crate::codegen::generate_migrations(&tables, &config)?);
    // The crate scaffold makes `<out>` a buildable, verifiable crate.
    let crate_name = crate_name_from(out);
    files.push(crate::codegen::generate_cargo_toml(&tables, &config, &crate_name));
    files.push(crate::codegen::generate_lib_rs(&tables));

    let mut rs_files = Vec::new();
    for file in &files {
        let dest = out.join(&file.path);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        fs::write(&dest, &file.contents)
            .with_context(|| format!("failed to write {}", dest.display()))?;
        println!("wrote {}", dest.display());
        if dest.extension().and_then(|e| e.to_str()) == Some("rs") {
            rs_files.push(dest);
        }
    }

    // Format (cosmetic) then verify (the safety net).
    let rs_refs: Vec<&Path> = rs_files.iter().map(PathBuf::as_path).collect();
    crate::verify::rustfmt_files(&rs_refs)?;
    if no_verify {
        println!("skipped cargo check (--no-verify)");
    } else {
        crate::verify::cargo_check(out)?;
        println!("cargo check passed ✓");
    }
    Ok(())
}
