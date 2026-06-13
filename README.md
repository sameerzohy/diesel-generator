# namma-diesel

> A code generator that reads YAML spec files and emits **Rust [Diesel](https://diesel.rs) ORM** code — the Rust counterpart to [`namma-dsl`](../namma-dsl), which reads the *same* YAML and emits Haskell [Beam](https://haskell-beam.github.io/beam/) code.

**One spec, two backends.** A storage spec written for NammaYatri today generates Haskell Beam tables, queries, and SQL migrations via `namma-dsl`. `namma-diesel` reads that identical YAML and generates a Rust Diesel schema, model structs, and migrations — no rewrite of the spec required.

```
                         ┌─────────────────────┐
   spec/Storage/Ride.yaml │  Storage YAML spec  │
                         └──────────┬──────────┘
                                    │
                 ┌──────────────────┴──────────────────┐
                 ▼                                      ▼
        ┌────────────────┐                     ┌──────────────────┐
        │   namma-dsl    │                     │  namma-diesel    │
        │   (Haskell)    │                     │     (Rust)       │
        └───────┬────────┘                     └────────┬─────────┘
                ▼                                       ▼
   Haskell Beam table + queries          Rust Diesel schema + models
   + SQL migrations                       + SQL up.sql / down.sql
```

---

## Why this exists

NammaYatri's backend is described declaratively: each database table is a YAML file in a `spec/Storage/` folder. `namma-dsl` turns those files into thousands of lines of correct, repetitive Haskell so engineers never hand-write Beam boilerplate.

If any part of that backend is ever rewritten in Rust — or a new Rust service needs to talk to the same database — the schema is already described. `namma-diesel` lets that Rust service reuse the existing specs instead of re-describing every table by hand. The spec stays the single source of truth; the generator fans out to a second language.

It is also a **first-class way to learn Rust**: building a code generator exercises parsing, the type system, traits, error handling, the filesystem, and process orchestration — without the distraction of async networking or a UI.

---

## What it generates (v1)

Given one Storage spec, `namma-diesel` emits, per table:

| Artifact | File | Diesel concept | namma-dsl analog |
|---|---|---|---|
| **Schema** | `schema.rs` | `diesel::table! { … }` | Beam `TableT f` type |
| **Read model** | `models/<table>.rs` | `#[derive(Queryable, Selectable, Identifiable)]` struct | domain type |
| **Insert model** | `models/<table>.rs` | `#[derive(Insertable)]` `New<Table>` struct | Beam `create` |
| **Custom types** | `types/<name>.rs` | Rust `enum` / `struct` (serde + Text-backed column) | `types:` block (`TypeObject`) |
| **Migration** | `migrations/<ts>_create_<table>/up.sql` + `down.sql` | Diesel migration pair | SQL `CREATE TABLE` |

Every generated file is run through `rustfmt`, and the whole output is verified with `cargo check` before the command reports success. **If the generated crate doesn't compile, the generator fails loudly** — invalid output never lands silently.

### Deferred to v2+

Query functions (`find_by_id`, `update_status`, …), domain↔DB conversion layer (the `ToTType`/`FromTType` analog), cached/Redis (`KV`) queries, relation extensions (`|WithId`), and git-hash-based incremental regeneration. These are designed for in [`PLAN.md`](./PLAN.md) but intentionally out of the first working version.

---

## Example

**Input** — an existing namma-dsl Storage spec (`spec/Storage/Ride.yaml`):

```yaml
Ride:
  tableName: ride
  fields:
    id: Id Ride
    status: Status
    fare: Maybe HighPrecMoney
    driverId: Id Person
    createdAt: UTCTime
  constraints:
    id: PrimaryKey
    driverId: SecondaryKey
  beamType:
    status: Text
  types:
    Status:
      - enum: "NEW,INPROGRESS,COMPLETED,CANCELLED"
      - derive: "Show,Eq"
```

**Output** — generated Rust:

`schema.rs`
```rust
diesel::table! {
    ride (id) {
        id -> Text,
        status -> Text,
        fare -> Nullable<Numeric>,
        driver_id -> Text,
        created_at -> Timestamptz,
    }
}
```

`models/ride.rs`
```rust
use crate::schema::ride;
use crate::types::status::Status;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;

#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = ride)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Ride {
    pub id: String,
    pub status: Status,
    pub fare: Option<Decimal>,
    pub driver_id: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = ride)]
pub struct NewRide {
    pub id: String,
    pub status: Status,
    pub fare: Option<Decimal>,
    pub driver_id: String,
    pub created_at: DateTime<Utc>,
}
```

`types/status.rs`
```rust
use diesel::{deserialize::FromSql, serialize::ToSql, sql_types::Text};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, AsExpression, FromSqlRow)]
#[diesel(sql_type = Text)]
pub enum Status {
    New,
    InProgress,
    Completed,
    Cancelled,
}
// + ToSql<Text, Pg> / FromSql<Text, Pg> impls mapping each variant to its string form
```

`migrations/20260613000000_create_ride/up.sql`
```sql
CREATE TABLE ride (
    id            TEXT PRIMARY KEY,
    status        TEXT NOT NULL,
    fare          NUMERIC,
    driver_id     TEXT NOT NULL,
    created_at    TIMESTAMPTZ NOT NULL
);
CREATE INDEX idx_ride_driver_id ON ride (driver_id);
```

---

## Usage

```bash
# Generate Rust for every spec in a folder, into an output crate
namma-diesel generate --spec ./spec/Storage --out ./generated-rust

# Generate a single spec
namma-diesel generate --spec ./spec/Storage/Ride.yaml --out ./generated-rust

# Skip the cargo check pass (faster, for iterating)
namma-diesel generate --spec ./spec/Storage --out ./generated-rust --no-verify
```

The output directory is a normal Cargo crate (`src/schema.rs`, `src/models/`, `src/types/`, `migrations/`) that you add `diesel` to and build.

---

## Type mapping (the heart of the framework)

`namma-diesel` resolves every field through one mapping table: **spec type → Rust type → Diesel SQL type → Postgres column type.**

| Spec type (namma-dsl) | Rust type | Diesel SQL type | Postgres column |
|---|---|---|---|
| `Text` | `String` | `Text` | `TEXT` |
| `Maybe T` | `Option<T>` | `Nullable<…>` | (nullable) |
| `Int` | `i64` | `BigInt` | `BIGINT` |
| `Bool` | `bool` | `Bool` | `BOOLEAN` |
| `Double` | `f64` | `Double` | `DOUBLE PRECISION` |
| `HighPrecMoney` | `rust_decimal::Decimal` | `Numeric` | `NUMERIC` |
| `UTCTime` | `chrono::DateTime<Utc>` | `Timestamptz` | `TIMESTAMPTZ` |
| `Id T` | `String` | `Text` | `TEXT` |
| custom enum + `beamType: Text` | generated `enum` | `Text` | `TEXT` |
| `[T]` | `Vec<T>` | `Array<…>` | `T[]` |

The map lives in one module and is overridable per field via the spec's `sqlType:` and `beamType:` blocks — the same override knobs namma-dsl exposes. Adding a new type is a one-line addition there.

---

## Architecture

A classic **parser → IR → generators** pipeline, deliberately mirroring namma-dsl so the two stay conceptually aligned.

```
 YAML spec ──▶ parser ──▶  IR (TableDef)  ──▶  type mapper  ──▶  generators  ──▶  files ──▶ verify
                          (Rust structs)                        (Askama templates)        (rustfmt + cargo check)
```

- **`config`** — CLI args + simple config (replaces namma-dsl's Dhall layer).
- **`ir`** — the intermediate representation: `TableDef`, `FieldDef`, `TypeDef`, `Constraint`, `IndexDef`. This is the contract between parsing and generation; everything downstream reads the IR, never the raw YAML.
- **`parser`** — `serde_yaml` → IR. Tolerates the existing namma-dsl format (old and new field syntax).
- **`typemap`** — the table above, as code.
- **`codegen`** — [Askama](https://github.com/rinja-rs/askama) templates (`templates/*.jinja`), one per artifact. Templates are compiled *into* the binary, so a broken template is a compile error of the generator, not a runtime surprise.
- **`verify`** — shells out to `rustfmt` and `cargo check`; non-zero exit = hard fail with the compiler's output.
- **`cli`** — the `namma-diesel` binary (clap).

Why **Askama** over `quote!`/`syn` or raw `format!`: templates read like the Diesel code being produced (best for learning), and Askama validates them at the generator's compile time. The real guarantee that output is *valid* Rust comes from the `cargo check` pass — that's engine-independent and always on.

Full rationale, phase-by-phase build order, and the Rust concepts each phase teaches are in **[`PLAN.md`](./PLAN.md)**.

---

## Status

Pre-implementation. This repository currently contains the design (`README.md`) and the build plan (`PLAN.md`). See the plan for the milestone breakdown.

## Relationship to namma-dsl

`namma-diesel` is a sibling, not a fork. It shares the **input format** and the **parser→IR→generator shape**, but none of the code — it's idiomatic Rust. The north star is: *any Storage spec that namma-dsl accepts, namma-diesel should eventually accept, producing the Diesel equivalent of what namma-dsl produces in Beam.*
