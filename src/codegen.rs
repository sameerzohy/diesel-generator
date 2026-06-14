//! Code generators. Commit 4: the Diesel schema — one `diesel::table!` file per
//! table (`schema/<table>.rs`) plus a common `schema/mod.rs` that `include!`s
//! each table file and declares the cross-table rules (A1 layout).

use std::path::PathBuf;

use anyhow::{anyhow, Result};
use askama::Template;
use heck::ToSnakeCase;

use crate::config::Config;
use crate::ir::{TableDef, TypeDef};
use crate::typemap;

/// A file the generator produced: path relative to the output crate + contents.
#[derive(Debug, Clone)]
pub struct GeneratedFile {
    pub path: PathBuf, // e.g. "src/schema/ride.rs"
    pub contents: String,
}

/// One column row inside a `diesel::table!` block.
struct Column {
    name: String, // snake_case column name
    sql: String,  // Diesel SQL type, e.g. "Nullable<Numeric>"
}

#[derive(Template)]
#[template(path = "schema.rs.jinja", escape = "none")]
struct SchemaTemplate {
    table: String,
    pk: String, // primary key column(s), comma-joined for the table! header
    columns: Vec<Column>,
}

#[derive(Template)]
#[template(path = "schema_mod.rs.jinja", escape = "none")]
struct SchemaModTemplate {
    tables: Vec<String>, // sql table names, in spec order
}

/// One field inside a model struct.
struct ModelField {
    name: String, // snake_case column name
    rust: String, // Rust type, e.g. "Option<Decimal>"
}

#[derive(Template)]
#[template(path = "model.rs.jinja", escape = "none")]
struct ModelTemplate {
    imports: Vec<String>,        // `use <path>;` paths, deduped
    table: String,               // sql table name (for `table_name = ...`)
    struct_name: String,         // "Ride"
    new_struct_name: String,     // "NewRide"
    primary_key: Option<String>, // Some("a, b") only when the PK isn't the default `id`
    row_fields: Vec<ModelField>,
    insert_fields: Vec<ModelField>,
}

#[derive(Template)]
#[template(path = "enum.rs.jinja", escape = "none")]
struct EnumTemplate {
    name: String,          // "Status"
    variants: Vec<String>, // verbatim spec values; variant name == stored string
}

/// Generate the schema files for a set of tables (A1). The CLI passes one table
/// today; directory mode (Commit 9) will pass many. The common `mod.rs` is built
/// from all of them, so the multi-table case is exercised by passing a slice.
pub fn generate_schema(tables: &[TableDef], config: &Config) -> Result<Vec<GeneratedFile>> {
    let mut files = Vec::new();
    let mut module_names = Vec::new();

    for table in tables {
        // diesel::table! requires a primary key header — fail fast, never silent.
        if table.primary_key.is_empty() {
            return Err(anyhow!(
                "table `{}` has no primary key (diesel::table! requires one)",
                table.name
            ));
        }

        let mut columns = Vec::new();
        for field in &table.fields {
            let resolved = typemap::resolve(field, &table.types, config)?;
            columns.push(Column {
                name: resolved.column_name,
                sql: resolved.diesel_sql,
            });
        }

        // PK header uses the resolved (snake_case) column names, in PK order.
        // Supports composite keys: `ride (id, version)`.
        let pk = table
            .primary_key
            .iter()
            .map(|f| f.to_snake_case())
            .collect::<Vec<_>>()
            .join(", ");

        let module = table.sql_table.clone();
        let contents = SchemaTemplate {
            table: table.sql_table.clone(),
            pk,
            columns,
        }
        .render()?;

        files.push(GeneratedFile {
            path: PathBuf::from(format!("src/schema/{module}.rs")),
            contents,
        });
        module_names.push(module);
    }

    let contents = SchemaModTemplate {
        tables: module_names,
    }
    .render()?;
    files.push(GeneratedFile {
        path: PathBuf::from("src/schema/mod.rs"),
        contents,
    });

    Ok(files)
}

/// Generate the model structs for a set of tables (Commit 5): a `Queryable`/
/// `Selectable`/`Identifiable` row struct + an `Insertable` `NewX` struct per
/// table under `src/models/<table>.rs`, plus a `src/models/mod.rs`.
pub fn generate_models(tables: &[TableDef], config: &Config) -> Result<Vec<GeneratedFile>> {
    let mut files = Vec::new();
    let mut module_names = Vec::new();

    for table in tables {
        if table.primary_key.is_empty() {
            return Err(anyhow!("table `{}` has no primary key", table.name));
        }

        // `use diesel::prelude::*` brings the four derives into scope; the schema
        // import lets `table_name = <t>` resolve.
        let mut imports = vec![
            "diesel::prelude::*".to_string(),
            format!("crate::schema::{}", table.sql_table),
        ];
        let mut row_fields = Vec::new();
        let mut insert_fields = Vec::new();

        for field in &table.fields {
            let resolved = typemap::resolve(field, &table.types, config)?;
            if let Some(import) = &resolved.import {
                if !imports.contains(import) {
                    imports.push(import.clone());
                }
            }
            row_fields.push(ModelField {
                name: resolved.column_name.clone(),
                rust: resolved.rust_type.clone(),
            });
            // The Insert struct excludes the auto-injected timestamps (DB-defaulted).
            if field.name != "createdAt" && field.name != "updatedAt" {
                insert_fields.push(ModelField {
                    name: resolved.column_name,
                    rust: resolved.rust_type,
                });
            }
        }

        // Identifiable defaults its primary key to `id`; only emit the attribute
        // when the PK differs (composite, or a non-`id` column).
        let pk_cols: Vec<String> = table.primary_key.iter().map(|f| f.to_snake_case()).collect();
        let primary_key = if pk_cols == ["id"] {
            None
        } else {
            Some(pk_cols.join(", "))
        };

        let contents = ModelTemplate {
            imports,
            table: table.sql_table.clone(),
            struct_name: table.name.clone(),
            new_struct_name: format!("New{}", table.name),
            primary_key,
            row_fields,
            insert_fields,
        }
        .render()?;

        files.push(GeneratedFile {
            path: PathBuf::from(format!("src/models/{}.rs", table.sql_table)),
            contents,
        });
        module_names.push(table.sql_table.clone());
    }

    let mod_contents = module_names
        .iter()
        .map(|m| format!("pub mod {m};"))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    files.push(GeneratedFile {
        path: PathBuf::from("src/models/mod.rs"),
        contents: mod_contents,
    });

    Ok(files)
}

/// Generate the custom enum types (Commit 6): each `types:` enum becomes a
/// Text-backed Rust enum under `src/types/<snake>.rs` (matching the
/// `crate::types::<snake>::<Name>` import the type mapper emits), plus a
/// `src/types/mod.rs`. Variant names are the spec values verbatim (A4).
pub fn generate_types(tables: &[TableDef], _config: &Config) -> Result<Vec<GeneratedFile>> {
    let mut files = Vec::new();
    let mut module_names = Vec::new();

    for table in tables {
        for ty in &table.types {
            // TypeDef is enum-only in v1; records are a v2 item.
            let TypeDef::Enum { name, variants } = ty;
            let module = name.to_snake_case();
            let contents = EnumTemplate {
                name: name.clone(),
                variants: variants.clone(),
            }
            .render()?;
            files.push(GeneratedFile {
                path: PathBuf::from(format!("src/types/{module}.rs")),
                contents,
            });
            if !module_names.contains(&module) {
                module_names.push(module);
            }
        }
    }

    if !module_names.is_empty() {
        let mod_contents = module_names
            .iter()
            .map(|m| format!("pub mod {m};"))
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        files.push(GeneratedFile {
            path: PathBuf::from("src/types/mod.rs"),
            contents: mod_contents,
        });
    }

    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_spec;

    /// A consuming project's config (inline test data — generator ships none).
    fn config() -> Config {
        toml::from_str(
            r#"
[types.UTCTime]
rust = "DateTime<Utc>"
diesel = "Timestamptz"
pg = "timestamptz"
import = "chrono::{DateTime, Utc}"

[types.HighPrecMoney]
rust = "Decimal"
diesel = "Numeric"
pg = "numeric"
import = "rust_decimal::Decimal"
"#,
        )
        .unwrap()
    }

    fn file_ending<'a>(files: &'a [GeneratedFile], suffix: &str) -> &'a GeneratedFile {
        files
            .iter()
            .find(|f| f.path.to_string_lossy().ends_with(suffix))
            .unwrap_or_else(|| panic!("no generated file ending in {suffix}"))
    }

    #[test]
    fn renders_single_table_schema() {
        let ride = parse_spec(include_str!("../examples/specs/Ride.yaml")).unwrap();
        let files = generate_schema(&[ride], &config()).unwrap();
        insta::assert_snapshot!(file_ending(&files, "schema/ride.rs").contents);
    }

    #[test]
    fn renders_common_mod_with_rules() {
        let ride = parse_spec(include_str!("../examples/specs/Ride.yaml")).unwrap();
        let booking = parse_spec(include_str!("../examples/specs/Booking.yaml")).unwrap();
        let files = generate_schema(&[ride, booking], &config()).unwrap();

        // both per-table files exist
        assert!(files.iter().any(|f| f.path.ends_with("ride.rs")));
        assert!(files.iter().any(|f| f.path.ends_with("booking.rs")));
        insta::assert_snapshot!(file_ending(&files, "schema/mod.rs").contents);
    }

    #[test]
    fn no_primary_key_errors() {
        // No `constraints:` -> empty primary_key -> hard error (before any field resolve).
        let table = parse_spec("NoPk:\n  tableName: no_pk\n  fields:\n    name: Text\n").unwrap();
        assert!(generate_schema(&[table], &Config::default()).is_err());
    }

    #[test]
    fn composite_primary_key_in_header() {
        let spec = "Ledger:\n  tableName: ledger\n  fields:\n    accountId: Id Account\n    version: Int\n  constraints:\n    accountId: PrimaryKey\n    version: PrimaryKey\n";
        let table = parse_spec(spec).unwrap();
        // injected created_at/updated_at are UTCTime -> need the project config.
        let files = generate_schema(&[table], &config()).unwrap();
        let schema = file_ending(&files, "schema/ledger.rs");
        assert!(schema.contents.contains("ledger (account_id, version)"));
    }

    #[test]
    fn renders_ride_model() {
        let ride = parse_spec(include_str!("../examples/specs/Ride.yaml")).unwrap();
        let files = generate_models(&[ride], &config()).unwrap();
        let model = file_ending(&files, "models/ride.rs");
        insta::assert_snapshot!(model.contents);

        assert!(model.contents.contains("pub struct Ride {"));
        assert!(model.contents.contains("pub struct NewRide {"));
        // NewRide excludes the auto-injected timestamps; the row struct keeps them.
        let new_struct = model.contents.split("pub struct NewRide").nth(1).unwrap();
        assert!(!new_struct.contains("created_at"));
        assert!(!new_struct.contains("updated_at"));
        assert!(model.contents.contains("created_at"));
        // mod.rs declares the module
        assert!(files
            .iter()
            .any(|f| f.path.ends_with("models/mod.rs") && f.contents.contains("pub mod ride;")));
    }

    #[test]
    fn renders_booking_model_no_custom_types() {
        let booking = parse_spec(include_str!("../examples/specs/Booking.yaml")).unwrap();
        let files = generate_models(&[booking], &config()).unwrap();
        insta::assert_snapshot!(file_ending(&files, "models/booking.rs").contents);
    }

    #[test]
    fn renders_enum_type() {
        let ride = parse_spec(include_str!("../examples/specs/Ride.yaml")).unwrap();
        let files = generate_types(&[ride], &Config::default()).unwrap();
        let status = file_ending(&files, "types/status.rs");
        insta::assert_snapshot!(status.contents);

        // A4: FromSql errors (never panics) on an unknown value; the Err arm is present.
        assert!(status.contents.contains("unknown Status value"));
        assert!(status.contents.contains("#[allow(non_camel_case_types)]"));
        assert!(status.contents.contains("impl FromSql<Text, Pg> for Status"));
        // mod.rs declares the module
        assert!(files
            .iter()
            .any(|f| f.path.ends_with("types/mod.rs") && f.contents.contains("pub mod status;")));
    }

    #[test]
    fn no_types_block_produces_no_files() {
        // Booking has no `types:` block -> no type files at all.
        let booking = parse_spec(include_str!("../examples/specs/Booking.yaml")).unwrap();
        let files = generate_types(&[booking], &Config::default()).unwrap();
        assert!(files.is_empty());
    }
}
