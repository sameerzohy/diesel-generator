#[derive(Debug, Clone)]
pub struct TableDef {
    pub name: String,             // "Ride"
    pub sql_table: String,        // "ride"
    pub fields: Vec<FieldDef>,
    pub types: Vec<TypeDef>,      // custom enums under `types:`
    pub primary_key: Vec<String>, // field names
    pub secondary_keys: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct FieldDef {
    pub name: String,                     // "driverId"
    pub spec_type: String,                // "Id Person" (Maybe + |relation stripped)
    pub optional: bool,
    pub db_type_override: Option<String>, // from `beamType:`
    pub constraints: Vec<Constraint>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Constraint {
    PrimaryKey,
    SecondaryKey,
}

#[derive(Debug, Clone)]
pub enum TypeDef {
    Enum { name: String, variants: Vec<String> },
}
