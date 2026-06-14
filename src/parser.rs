
use anyhow::{anyhow, Context, Result};
use serde_norway::Value;
use heck::ToSnakeCase;

use crate::ir::{Constraint, FieldDef, TableDef, TypeDef};

/// Parse the text of one spec file into a TableDef.
pub fn parse_spec(text: &str) -> Result<TableDef> {
    let root: Value = serde_norway::from_str(text).context("spec is not valid YAML")?;
    let root_map = root
        .as_mapping()
        .ok_or_else(|| anyhow!("top level of a spec must be a mapping"))?;

    // 1. The single top-level key is the table name, e.g. `Ride:`.
    let (name_key, body_val) = root_map.iter().next().ok_or_else(|| anyhow!("spec is empty"))?;
    let name = as_string(name_key, "table name")?;
    let body = body_val
        .as_mapping()
        .ok_or_else(|| anyhow!("body of `{name}` must be a mapping"))?;

    // 2. sql_table: explicit `tableName:`, else a lowercase fallback.
    //    (Real snake_case conversion arrives in Commit 3 with `heck`.)
    let sql_table = body
        .get("tableName")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| name.to_snake_case());

    // Sub-mappings we look fields up in (any may be absent).
    let beam_type = body.get("beamType").and_then(Value::as_mapping);
    let constraints = body.get("constraints").and_then(Value::as_mapping);
    let defaults = body.get("default").and_then(Value::as_mapping);

    // 3. Walk `fields:`.
    let mut fields = Vec::new();
    if let Some(fields_map) = body.get("fields").and_then(Value::as_mapping) {
        for (key, val) in fields_map {
            let fname = as_string(key, "field name")?;
            let raw_type = as_string(val, &format!("type of field `{fname}`"))?;

            // `Maybe X` -> optional, base type is X.
            let (optional, after_maybe) = match raw_type.strip_prefix("Maybe ") {
                Some(rest) => (true, rest),
                None => (false, raw_type.as_str()),
            };
            // `X|WithId` -> drop the relation suffix for v1.
            let spec_type = after_maybe.split('|').next().unwrap().trim().to_string();

            let db_type_override = beam_type
                .and_then(|m| m.get(fname.as_str()))
                .and_then(Value::as_str)
                .map(str::to_string);

            let field_constraints = constraints
                .and_then(|m| m.get(fname.as_str()))
                .and_then(Value::as_str)
                .map(parse_constraints)
                .unwrap_or_default();

            let default = defaults
                .and_then(|m| m.get(fname.as_str()))
                .and_then(Value::as_str)
                .map(str::to_string);

            fields.push(FieldDef {
                name: fname,
                spec_type,
                optional,
                db_type_override,
                default,
                constraints: field_constraints,
            });
        }
    }

    // 4. Inject created_at / updated_at if absent (A2).
    inject_timestamps(&mut fields);

    // 5. Walk `types:` (custom enums).
    let mut types = Vec::new();
    if let Some(types_map) = body.get("types").and_then(Value::as_mapping) {
        for (key, val) in types_map {
            let type_name = as_string(key, "type name")?;
            if let Some(variants) = enum_variants(val) {
                types.push(TypeDef::Enum { name: type_name, variants });
            }
        }
    }

    // 6. Derive PK / secondary-key lists from the constraints.
    let primary_key = fields_with(&fields, Constraint::PrimaryKey);
    let secondary_keys = fields_with(&fields, Constraint::SecondaryKey);

    Ok(TableDef { name, sql_table, fields, types, primary_key, secondary_keys })
}

// ---------- helpers ----------

fn as_string(v: &Value, what: &str) -> Result<String> {
    v.as_str()
        .map(str::to_string)
        .ok_or_else(|| anyhow!("{what} must be a string"))
}

/// "PrimaryKey" | "SecondaryKey" | "PrimaryKey|SecondaryKey"
fn parse_constraints(s: &str) -> Vec<Constraint> {
    s.split('|')
        .filter_map(|part| match part.trim() {
            "PrimaryKey" => Some(Constraint::PrimaryKey),
            "SecondaryKey" => Some(Constraint::SecondaryKey),
            _ => None,
        })
        .collect()
}

/// `- enum: "A,B,C"` (a list of single-key maps) -> ["A","B","C"].
fn enum_variants(val: &Value) -> Option<Vec<String>> {
    for item in val.as_sequence()? {
        if let Some(s) = item.get("enum").and_then(Value::as_str) {
            return Some(s.split(',').map(|v| v.trim().to_string()).collect());
        }
    }   
    None
}   

fn inject_timestamps(fields: &mut Vec<FieldDef>) {
    for ts in ["createdAt", "updatedAt"] {
        if !fields.iter().any(|f| f.name == ts) {
            fields.push(FieldDef {
                name: ts.to_string(),
                spec_type: "UTCTime".to_string(),
                optional: false,
                db_type_override: None,
                // self-default so inserts that omit timestamps still satisfy NOT NULL.
                default: Some("CURRENT_TIMESTAMP".to_string()),
                constraints: Vec::new(),
            });
        }
    }
}   

fn fields_with(fields: &[FieldDef], want: Constraint) -> Vec<String> {
    fields
        .iter()
        .filter(|f| f.constraints.contains(&want))
        .map(|f| f.name.clone())
        .collect()
}

// ---------- test ----------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ride_spec() {
        let text = include_str!("../examples/specs/Ride.yaml");
        let table = parse_spec(text).expect("Ride.yaml should parse");

        assert_eq!(table.name, "Ride");
        assert_eq!(table.sql_table, "ride");
        assert_eq!(table.fields.len(), 6); // 4 spec'd + created/updated
        assert_eq!(table.primary_key, vec!["id"]);
        assert_eq!(table.secondary_keys, vec!["driverId"]);

        let fare = table.fields.iter().find(|f| f.name == "fare").unwrap();
        assert!(fare.optional);
        assert!(table.fields.iter().any(|f| f.name == "createdAt"));
        
        assert_eq!(table.types.len(), 1);
        if let TypeDef::Enum { variants, .. } = &table.types[0] {
            assert_eq!(variants, &["NEW", "INPROGRESS", "COMPLETED", "CANCELLED"]);
        }
    }   
}
