//! Type registry, supplied by the consuming project via `--config`.
//! The generator ships NO custom-type knowledge: without a config only
//! primitives, `Id X`, and spec-defined enums resolve. Custom/imported types
//! (their Rust type, `use` import, SQL type) are declared in the project's
//! own repo, because that is where that policy belongs.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

/// How one spec type maps onto Rust + Diesel + Postgres. All fields are
/// optional so a config entry can override just one facet (e.g. only `pg`).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct TypeMapping {
    pub rust: Option<String>,   // "Decimal"               -> model field type
    pub diesel: Option<String>, // "Numeric"               -> diesel::table!
    pub pg: Option<String>,     // "numeric"               -> up.sql column
    pub import: Option<String>, // "rust_decimal::Decimal" -> `use` in the model; None = std
    #[serde(rename = "crate")]
    pub krate: Option<String>,  // "rust_decimal = \"1\""  -> generated Cargo.toml dep
}

#[derive(Debug, Default, Deserialize)]
pub struct Config {
    /// spec type name -> mapping. Comes entirely from the consuming project.
    #[serde(default)]
    pub types: HashMap<String, TypeMapping>,
}

impl Config {
    /// Load the project's config TOML, or an empty registry if none is given.
    pub fn load(path: Option<&Path>) -> Result<Config> {
        match path {
            Some(p) => {
                let text = std::fs::read_to_string(p)
                    .with_context(|| format!("failed to read config {}", p.display()))?;
                toml::from_str(&text)
                    .with_context(|| format!("invalid config TOML {}", p.display()))
            }
            None => Ok(Config::default()),
        }
    }
}
