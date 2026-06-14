//! Code generators. Commit 4: the Diesel schema — one `diesel::table!` file per
//! table (`schema/<table>.rs`) plus a common `schema/mod.rs` that `include!`s
//! each table file and declares the cross-table rules (A1 layout).

use std::path::PathBuf;

use anyhow::{anyhow, Result};
use askama::Template;
use heck::ToSnakeCase;

use crate::config::Config;
use crate::ir::TableDef;
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

[types.HighPrecMoney]
rust = "Decimal"
diesel = "Numeric"
pg = "numeric"
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
}
