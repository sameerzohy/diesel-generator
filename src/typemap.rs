//! Resolve a spec field type into the facets the generators need (Rust, Diesel,
//! Postgres, + the `use` import). The generator owns only language primitives and
//! structural rules; all custom/imported types come from the project's config.

use anyhow::{anyhow, bail, Result};
use heck::ToSnakeCase;

use crate::config::Config;
use crate::ir::{FieldDef, TypeDef};

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedType {
    pub column_name: String,
    pub rust_type: String,
    pub diesel_sql: String,
    pub pg_type: String,
    pub import: Option<String>, // `use` path the model needs; None for std primitives
}

/// Language-level primitives: no import, no external repo, universal.
/// (rust, diesel, pg). Everything else comes from the project's config.
fn primitive(spec: &str) -> Option<(&'static str, &'static str, &'static str)> {
    match spec {
        "Text" => Some(("String", "Varchar", "character varying(36)")),
        "Int" => Some(("i64", "Int8", "bigint")),
        "Bool" => Some(("bool", "Bool", "boolean")),
        "Double" => Some(("f64", "Float8", "double precision")),
        _ => None,
    }
}

pub fn resolve(field: &FieldDef, types: &[TypeDef], config: &Config) -> Result<ResolvedType> {
    let column_name = field.name.to_snake_case();
    let spec = &field.spec_type;

    // NOTE: `field.db_type_override` (from `beamType:`) is parsed into the IR but
    // intentionally NOT applied here in v1. Enums are already text-backed, which
    // covers the only `beamType` use in the demo. Storage-type override on a
    // non-enum field is a v2 feature (see the v2 table in PLAN.md).

    // Rule: a custom enum from this spec's `types:` block (we generate it).
    if types.iter().any(|t| matches!(t, TypeDef::Enum { name, .. } if name == spec)) {
        return Ok(wrap(field, ResolvedType {
            column_name,
            rust_type: spec.clone(),
            diesel_sql: "Varchar".into(),
            pg_type: "character varying(36)".into(),
            import: Some(format!("crate::types::{}::{}", spec.to_snake_case(), spec)),
        }));
    }

    // Rule: `Id X` -> a text id column (parametric).
    if spec.starts_with("Id ") {
        return Ok(wrap(field, ResolvedType {
            column_name,
            rust_type: "String".into(),
            diesel_sql: "Varchar".into(),
            pg_type: "character varying(36)".into(),
            import: None,
        }));
    }

    // Project config: custom/imported types (and primitive overrides) win.
    // A partial entry inherits any omitted facet from the primitive (when `spec`
    // is one); a custom type with no primitive base must specify all facets.
    if let Some(m) = config.types.get(spec) {
        let prim = primitive(spec);
        let rust_type = m.rust.clone()
            .or_else(|| prim.map(|p| p.0.to_string()))
            .ok_or_else(|| anyhow!("type `{spec}` in config has no `rust` (and is not a primitive)"))?;
        let diesel_sql = m.diesel.clone()
            .or_else(|| prim.map(|p| p.1.to_string()))
            .ok_or_else(|| anyhow!("type `{spec}` in config has no `diesel` (and is not a primitive)"))?;
        let pg_type = m.pg.clone()
            .or_else(|| prim.map(|p| p.2.to_string()))
            .ok_or_else(|| anyhow!("type `{spec}` in config has no `pg` (and is not a primitive)"))?;
        return Ok(wrap(field, ResolvedType {
            column_name,
            rust_type,
            diesel_sql,
            pg_type,
            import: m.import.clone(),
        }));
    }

    // Language primitive (no import needed).
    if let Some((rust, diesel, pg)) = primitive(spec) {
        return Ok(wrap(field, ResolvedType {
            column_name,
            rust_type: rust.into(),
            diesel_sql: diesel.into(),
            pg_type: pg.into(),
            import: None,
        }));
    }

    bail!("unknown type `{}` (field `{}`): not a primitive or a spec `types:` entry — \
           add it to [types] in your project config", spec, field.name);
}

/// Apply `optional`: wrap the Rust and Diesel sides.
fn wrap(field: &FieldDef, mut r: ResolvedType) -> ResolvedType {
    if field.optional {
        r.rust_type = format!("Option<{}>", r.rust_type);
        r.diesel_sql = format!("Nullable<{}>", r.diesel_sql);
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{FieldDef, TypeDef};

    fn field(name: &str, spec_type: &str, optional: bool) -> FieldDef {
        FieldDef {
            name: name.into(),
            spec_type: spec_type.into(),
            optional,
            db_type_override: None,
            default: None,
            constraints: vec![],
        }
    }

    /// A consuming project supplies this in ITS repo and passes `--config`.
    /// Inlined here only as test data — the generator ships no real config.
    fn project() -> Config {
        toml::from_str(
            r#"
[types.UTCTime]
rust = "DateTime<Utc>"
diesel = "Timestamptz"
pg = "timestamptz"
import = "chrono::{DateTime, Utc}"
crate = 'chrono = { version = "0.4", features = ["serde"] }'

[types.HighPrecMoney]
rust = "Decimal"
diesel = "Numeric"
pg = "numeric"
import = "rust_decimal::Decimal"
crate = 'rust_decimal = "1"'
"#,
        )
        .unwrap()
    }

    #[test]
    fn primitives_need_no_config() {
        let r = resolve(&field("x", "Text", false), &[], &Config::default()).unwrap();
        assert_eq!(r.rust_type, "String");
        assert_eq!(r.diesel_sql, "Varchar");
        assert_eq!(r.pg_type, "character varying(36)");
        assert_eq!(r.import, None);
        assert_eq!(resolve(&field("n", "Int", false), &[], &Config::default()).unwrap().rust_type, "i64");
    }

    #[test]
    fn id_is_a_structural_rule() {
        let r = resolve(&field("driverId", "Id Person", false), &[], &Config::default()).unwrap();
        assert_eq!(r.column_name, "driver_id");
        assert_eq!(r.diesel_sql, "Varchar");
    }

    #[test]
    fn custom_types_come_from_the_project_config() {
        let r = resolve(&field("fare", "HighPrecMoney", true), &[], &project()).unwrap();
        assert_eq!(r.rust_type, "Option<Decimal>");
        assert_eq!(r.diesel_sql, "Nullable<Numeric>");
        assert_eq!(r.import.as_deref(), Some("rust_decimal::Decimal"));
        assert_eq!(resolve(&field("t", "UTCTime", false), &[], &project()).unwrap().import.as_deref(),
                   Some("chrono::{DateTime, Utc}"));
    }

    #[test]
    fn generator_owns_no_custom_types() {
        // HighPrecMoney is unknown without a project config — by design.
        assert!(resolve(&field("fare", "HighPrecMoney", false), &[], &Config::default()).is_err());
    }

    #[test]
    fn yaml_enum_imports_from_generated_crate() {
        let types = vec![TypeDef::Enum { name: "Status".into(), variants: vec!["NEW".into()] }];
        let r = resolve(&field("status", "Status", false), &types, &Config::default()).unwrap();
        assert_eq!(r.rust_type, "Status");
        assert_eq!(r.diesel_sql, "Varchar");
        assert_eq!(r.import.as_deref(), Some("crate::types::status::Status"));
    }

    #[test]
    fn unknown_type_errors() {
        assert!(resolve(&field("x", "Wibble", false), &[], &Config::default()).is_err());
    }

    #[test]
    fn partial_override_of_a_primitive_inherits() {
        // Only `pg` set — `rust`/`diesel` inherit from the Text primitive.
        let cfg: Config = toml::from_str("[types.Text]\npg = \"text\"").unwrap();
        let r = resolve(&field("x", "Text", false), &[], &cfg).unwrap();
        assert_eq!(r.pg_type, "text"); // overridden
        assert_eq!(r.rust_type, "String"); // inherited
        assert_eq!(r.diesel_sql, "Varchar"); // inherited
    }

    #[test]
    fn partial_entry_for_a_custom_type_errors() {
        // No primitive base to inherit from -> all facets are required.
        let cfg: Config = toml::from_str("[types.Money]\npg = \"numeric\"").unwrap();
        assert!(resolve(&field("x", "Money", false), &[], &cfg).is_err());
    }
}
