# namma-diesel — Build Plan

A phase-by-phase plan to build `namma-diesel` (see [`README.md`](./README.md) for what it is). Written for someone **new to Rust**: each phase names the Rust concepts it forces you to learn, so the project doubles as a Rust curriculum. Build in order — each phase produces something runnable before the next begins.

---

## Decisions already made

These are locked (decided during design). Everything below assumes them.

| Decision | Choice | Why |
|---|---|---|
| **Input format** | Reuse namma-dsl's existing Storage YAML, drop-in | One spec, two backends. No rewrite of NammaYatri specs. |
| **Codegen engine** | [Askama](https://github.com/rinja-rs/askama) templates | Templates look like the output (learnable); validated at the generator's compile time. |
| **Correctness gate** | Post-gen `rustfmt` + `cargo check` on the output | The only true guarantee the emitted Rust is valid. Mirrors namma-dsl's reliance on `cabal build all`. |
| **v1 artifacts** | custom types/enums + `table!` schema + model structs + SQL up/down migration | Smallest end-to-end useful slice; teaches Diesel fundamentals. |
| **Config layer** | CLI flags + optional TOML | Replaces namma-dsl's Dhall layer with something simpler. |
| **Target DB** | Postgres first | What NammaYatri uses; Diesel's Postgres backend is the most complete. |

### Locked by eng review (2026-06-13)

| # | Decision |
|---|---|
| **A1 — schema layout** | One file per table at `src/schema/<table>.rs`, plus a generated `src/schema/mod.rs` that declares each module and emits `allow_tables_to_appear_in_same_query!(...)` (and `joinable!` in v2) so tables can be queried together. The generator parses **all** specs first, then writes the per-table files and the single `mod.rs` — no per-spec overwrite. |
| **A2 — auto-fields** | Inject `createdAt`/`updatedAt` (`TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP`) during IR construction, since namma-dsl adds them too (Storage.hs:1323). `merchantId`/`merchantOperatingCityId` injection + exact column parity are **v2** — documented, not silent. |
| **A3 — type map** | *(Superseded by A6.)* The SQL type map lives in the optional TOML config; defaults mirror namma-dsl's out-of-box values. A6 generalizes this into the full type registry and moves the custom-type mappings out of the generator entirely. |
| **A4 — enums** | Keep Text-backed hand-rolled `ToSql<Text,Pg>`/`FromSql<Text,Pg>` (matches namma-dsl's `beamType: Text` storage — do **not** use `diesel-derive-enum`, which would switch to a PG native enum type). `FromSql` returns a Diesel error (never panics) on an unrecognized string. Every enum gets a generated round-trip test. |
| **A5 — Askama** | Use the `askama` crate **0.13+** (the project renamed back from `rinja`). Templates live in `templates/`. |
| **A6 — type registry** | Three type categories: std primitives (code), YAML `types:` (generated, `import = crate::types::…`), and external/imported (config registry). `HighPrecMoney`/`UTCTime` are external (carry a `use` import + `crate` dep), **not** primitives. Registry = `Config.types: HashMap<String, TypeMapping{rust?,diesel?,pg?,import?,crate?}>` seeded with built-in defaults, overridable per project. Rust paths live in the config, never the shared YAML. Unknown type → hard error. |

### NOT in scope (v1)

- **merchantId / merchantOperatingCityId injection** and exact column-type parity with the live DB — deferred to v2 (A2). v1 injects only created/updated.
- **Query functions, domain↔DB conversions, cached/KV queries, relations (`|WithId`), incremental git-hash regen, schema-diff `ALTER` migrations** — all v2 (see the v2 table).
- **Distribution** (how users install the `namma-diesel` binary — `cargo install --git`, release binaries, brew). Not built in v1; revisit once the tool works. Flagged so it doesn't silently drop.
- **`joinable!` / foreign-key join wiring** — `mod.rs` gets `allow_tables_to_appear_in_same_query!` now, but `joinable!` arrives with v2 relations.

### What already exists (reused, not rebuilt)

- **namma-dsl's Storage YAML format** — reused drop-in; no new spec format.
- **namma-dsl's parser→IR→generator shape** — mirrored structurally (the IR-is-the-contract rule), though the Rust code is independent.
- **namma-dsl's auto-field + type-mapping rules** — referenced as the source of truth for A2/A3 defaults rather than reinvented.

---

## Commit Roadmap (v1) — the working checklist

**This is the execution plan.** Build in this order. Each commit leaves the repo
green (compiles, runs, tests pass) and is independently reviewable. Cut the commit
only when its **Done when** check passes. Commits 1–5 are the table + schema target;
6–9 finish v1.

> Granularity chosen: **4 small commits to first schema** (one Rust concept each),
> then continue the same way. Commit message uses the title verbatim.

### Target A: schema + models (the immediate goal)

- [ ] **Commit 1 — `chore: scaffold cargo project`**
  - *Goal:* an empty-but-real CLI that reads a spec file and prints it.
  - *Touch:* `cargo init`; `Cargo.toml` (+ `clap` derive); `.gitignore` (`/target`);
    `src/main.rs`, `src/cli.rs`; empty module stubs `src/ir.rs`, `src/parser.rs`,
    `src/typemap.rs`, `src/codegen/mod.rs`; `examples/specs/Ride.yaml` (a real spec).
  - *Done when:* `cargo run -- generate --spec examples/specs/Ride.yaml --out ./out`
    prints the YAML and exits 0. `cargo build` is clean.
  - *Learn:* cargo, `Cargo.toml`, modules, `clap` derive, `std::fs`, `Result`/`?`.

- [ ] **Commit 2 — `feat: parse storage spec into IR`**
  - *Goal:* turn the YAML into Rust structs. The IR is the contract everything reads.
  - *Touch:* `src/ir.rs` (`TableDef`, `FieldDef`, `Constraint`); `src/parser.rs`
    (`serde_norway::Value` walk → `TableDef`: name, `sql_table`, fields, `optional`
    from `Maybe`, `primary_key` from `constraints`). **Auto-field injection (A2):**
    after parsing, append `created_at`/`updated_at` `FieldDef`s
    (`TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP`) unless already present. Add
    `serde`, `serde_norway`, `anyhow`. One `#[test]` parsing `Ride.yaml`.
  - *Done when:* `cargo test` passes; printing the IR with `{:#?}` shows correct
    field count (including injected created/updated), PK, and `optional` flags.
  - *Learn:* structs, enums, `Vec`, `Option`, `match`, `serde_norway::Value` navigation,
    writing tests.

- [ ] **Commit 3 — `feat: type mapper (spec type to diesel sql type)`**
  - *Goal:* resolve every field into all facets the generators need, in one `ResolvedType`
    (single source of truth — Commits 4/5/7 just read it):
    ```
    struct ResolvedType {
        column_name: String,        // driverId -> driver_id  (heck)
        rust_type:   String,        // String / Option<Decimal> / Status
        diesel_sql:  String,        // Varchar / Nullable<Numeric>
        pg_type:     String,        // character varying(36) / numeric
        import:      Option<String>,// `use` path the model needs; None for std primitives
    }
    ```
  - **Three type categories** (A6): (1) **std primitives** — `Text`/`Id _` → `String`/`Varchar`/
    `varchar(36)`, `Int` → `i64`/`Int8`/`bigint`, `Bool`, `Double` — `import: None`. (2) **YAML
    `types:`** (enums) — we generate them; `import = crate::types::<snake>::<Name>`. (3)
    **external/imported** — `HighPrecMoney`, `UTCTime`, domain types — resolved via a **config
    type registry**; `import` = the `use` path, plus a `crate` dep for the generated Cargo.toml.
  - **Type registry (A6, supersedes A3):** `Config.types: HashMap<String, TypeMapping>` where
    `TypeMapping { rust?, diesel?, pg?, import?, crate? }` (all optional → override one facet,
    inherit the rest). Built-in defaults seed the registry (incl. `UTCTime` →
    `chrono::DateTime<Utc>` and `HighPrecMoney` → `rust_decimal::Decimal`, each with its import +
    crate). Config entries override/extend. The shared YAML stays Rust-free — Rust paths live in
    the config, so one spec still feeds both backends.
  - *Rules:* `optional` wraps Rust `Option<…>` / Diesel `Nullable<…>`; custom enum → `rust_type`
    is the enum name, Text-backed; **unknown type → hard `anyhow` error** ("add to `[types]` or
    declare in spec `types:`") — never emit code that won't compile.
  - *Touch:* `src/typemap.rs` (`ResolvedType`, `builtin_types()`, `resolve(field, &types, &config)
    -> Result<ResolvedType>`); `src/config.rs` (`Config`, `TypeMapping`, TOML load); add `heck`,
    `serde`, `toml`. Commit 2's lowercase `sql_table` already fixed to `heck` snake_case.
  - *Done when:* `cargo test` green — base types, `Id`, `Option` wrapping, a yaml enum
    (import = `crate::types::…`), an external type (`HighPrecMoney`, import set), an **unknown
    type → Err**, and a config entry overriding `pg`.
  - *Learn:* `HashMap`, `match`/`matches!`, `Option` merge, `serde` derive, TOML, `heck`, error paths.
  - *Note:* the generated crate's `Cargo.toml` (Commit 8) must include the `crate` deps of every
    imported type actually used, or `cargo check` fails. Collect used imports in Commit 5.

- [ ] **Commit 4 — `feat: generate per-table schema + global mod.rs`** ← **first generated Rust** (implementation-ready)
  - *Goal:* turn `&[TableDef]` into Diesel schema files (**A1 layout**): one
    `schema/<table>.rs` per table (`diesel::table!`) + one common `schema/mod.rs` that holds
    the cross-table rules.
  - **Codegen shape (locked):** `codegen::generate_schema(tables: &[TableDef], config: &Config)
    -> Result<Vec<GeneratedFile>>` operates on a **slice**. CLI feeds a `Vec` of 1 now;
    Commit 9 (directory mode) feeds N. The multi-table `mod.rs` is snapshot-tested in Commit 4
    by passing `&[ride, booking]` directly — directory mode is NOT pulled forward.
  - **Per-table file** `schema/<t>.rs`:
    ```
    diesel::table! {
        ride (id) {
            id -> Varchar,
            status -> Varchar,
            fare -> Nullable<Numeric>,
            driver_id -> Varchar,
            created_at -> Timestamptz,
            updated_at -> Timestamptz,
        }
    }
    ```
  - **Common file** `schema/mod.rs` (the "rules" file): `pub mod <t>;` for each table +
    `diesel::allow_tables_to_appear_in_same_query!(<all tables>);`. (`joinable!` → v2.)
  - **Touch:** `Cargo.toml` (+`askama` 0.13 **A5**, +`insta` dev); `templates/schema.rs.jinja` +
    `templates/schema_mod.rs.jinja`; `src/codegen.rs` (`generate_schema`, Askama context structs,
    file writing); `src/cli.rs` (wire optional `--config` — deferred from C3; `generate` →
    parse → `Config::load` → `generate_schema` → write under `<out>/src/schema/`);
    `examples/specs/Booking.yaml` (2nd spec for the multi-table snapshot).
  - **Footguns / locked details:**
    - **`#[template(escape = "none")]` on every template** — Askama's default HTML escaper
      would mangle the `<`/`>` in `Nullable<Numeric>` into `&lt;`/`&gt;`. Critical.
    - **No primary key → hard `anyhow` error** (`diesel::table!` requires a PK header). Never silent.
    - **Composite PK** → header `ride (id, version)`; PK field names mapped to their resolved
      `column_name`.
    - **Dumb templates** — resolve/snake-case/join in Rust; context structs carry finished strings.
    - **No config shipped** — snapshot tests build `Config` inline (like `typemap`'s `project()`).
  - *Done when:* `cargo test` green — snapshots for single-table `schema/ride.rs`, two-table
    `schema/mod.rs` (both `pub mod` + the `allow_tables!` macro), a no-PK spec → `Err`, and a
    composite-PK header. (`cargo run` end-to-end on `Ride.yaml` now needs *your* `--config`
    because of `HighPrecMoney` + the decoupling — by design; the snapshot tests are the gate.)
  - *Sub-steps:* **4a** Askama + `schema.rs.jinja` + single-table render/snapshot → **4b**
    `schema_mod.rs.jinja` + multi-table `mod.rs` + `Booking.yaml` → **4c** no-PK error +
    composite PK → **4d** wire `--config` + write files to `<out>/src/schema/`.
  - *Learn:* traits + derive macros (Askama `Template`), `escape="none"`, lifetimes (clone to
    `String` if the borrow checker fights), iterators, writing files, snapshot testing (`insta`).

- [ ] **Commit 5 — `feat: generate Queryable/Insertable model structs`** (implementation-ready)
  - *Goal:* per table, a row struct + a `NewX` insert struct under `src/models/<table>.rs`,
    plus a `src/models/mod.rs`. First commit that uses `resolve`'s `rust_type` and `import`.
  - **Codegen:** `codegen::generate_models(tables: &[TableDef], config: &Config) ->
    Result<Vec<GeneratedFile>>` (same slice + `GeneratedFile` shape as `generate_schema`).
  - **Row struct** (all columns):
    ```
    #[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
    #[diesel(table_name = crate::schema::ride)]
    #[diesel(check_for_backend(diesel::pg::Pg))]
    pub struct Ride {
        pub id: String,
        pub status: Status,
        pub fare: Option<Decimal>,
        pub driver_id: String,
        pub created_at: DateTime<Utc>,
        pub updated_at: DateTime<Utc>,
    }
    ```
    Composite PK → add `#[diesel(primary_key(a, b))]`.
  - **Insert struct** (decision locked): `NewRide` has every column **except** the
    auto-injected `created_at`/`updated_at` (excluded by IR field name `createdAt`/`updatedAt`;
    the DB defaults them). Keeps `id` (app-supplied).
    ```
    #[derive(Debug, Clone, Insertable)]
    #[diesel(table_name = crate::schema::ride)]
    pub struct NewRide { pub id: String, pub status: Status, pub fare: Option<Decimal>, pub driver_id: String }
    ```
    *(Generalizing to "exclude any field with a SQL `default:`" is a v2 refinement.)*
  - **Imports:** `use diesel::prelude::*;` (brings the four derives into scope) + the deduped
    `import` paths from `resolve` (`crate::types::status::Status`, `chrono::{DateTime, Utc}`,
    `rust_decimal::Decimal`). The Insert struct excludes timestamps, so it may not need chrono;
    collect imports per actually-used field set.
  - **Touch:** `templates/model.rs.jinja` (`escape = "none"`); `src/codegen.rs`
    (`generate_models`, Row/New context structs — share the field-list rendering to stay DRY,
    CQ1); `src/cli.rs` (`generate` also writes models). `src/models/mod.rs` lists `pub mod <t>;`.
  - **Footguns:** `escape = "none"` again (`Option<…>` has `<>`); struct name is the IR
    `name` (CamelCase `Ride`), table_name is `crate::schema::<sql_table>`; `Selectable` needs
    `check_for_backend` or it won't verify field-vs-schema types.
  - *Done when:* `cargo test` green — snapshots for `Ride` row + `NewRide` (matches README),
    a `Booking` case (no custom types), and an assertion that `NewRide` omits `created_at`.
  - *Learn:* multiple derives, dedup/collect, Diesel attribute macros, mapping Rust types onto
    struct fields.
  - *Note:* the **UTCTime** parked decision shows up here — the row struct's `created_at:
    DateTime<Utc>` needs the chrono import, which today comes from the project `--config`.

### Target B: complete v1

- [ ] **Commit 6 — `feat: generate custom types and enums`** (implementation-ready) ← **crate compiles after this**
  - *Goal:* the spec `types:` block → a Rust enum per type under `src/types/<snake>.rs`
    (matching the `crate::types::<snake>::<Name>` import Commit 3 emits) + `src/types/mod.rs`.
    Makes the model fields (`status: Status`) resolve.
  - **Codegen:** `codegen::generate_types(tables: &[TableDef], _config: &Config) ->
    Result<Vec<GeneratedFile>>` (same slice/`GeneratedFile` shape). Iterate each table's
    `TypeDef::Enum`.
  - **Variant naming (locked):** variant name = the spec value **verbatim** (stored DB string
    == variant name, 1:1). Emit `#[allow(non_camel_case_types)]` on the enum to cover
    SCREAMING values.
  - **Generated enum** (A4 — Text-backed, **not** `diesel-derive-enum`):
    ```
    #[allow(non_camel_case_types)]
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, AsExpression, FromSqlRow)]
    #[diesel(sql_type = Text)]
    pub enum Status { NEW, INPROGRESS, COMPLETED, CANCELLED }
    ```
    plus `fn as_str(&self) -> &'static str` (variant → string) and
    `fn from_str(s: &str) -> deserialize::Result<Self>` (string → variant, else `Err`).
    `ToSql<Text, Pg>` writes `as_str()`; `FromSql<Text, Pg>` calls `from_str` — **never panics**
    on an unknown value.
  - **Round-trip test (A4 / T1):** generate an inline `#[cfg(test)]` in the output crate:
    `from_str(as_str(v)) == v` for every variant, and `from_str("bogus")` is `Err`. Pure (no
    DB) because it tests the helpers, not the Diesel impls. The generator's own tests snapshot
    the file + assert the `Err` arm is present.
  - **Touch:** `templates/enum.rs.jinja` (`escape = "none"` — types have `<>` via generics/derives);
    `src/codegen.rs` (`generate_types`, context struct with `{ name, allow_lint, variants:
    Vec<{ ident, value }> }`); `src/cli.rs` (`generate` also writes types). `src/types/mod.rs`
    lists `pub mod <snake>;`. Sync the README enum example to verbatim variants.
  - **Footguns:** `as_str`/`from_str` arms must be 1:1 with the variants; `from_str`'s `_ =>`
    arm returns `Err`, not `unreachable!`.
  - *Done when:* `cargo test` green — snapshot of `types/status.rs` (enum + derives + impls +
    the round-trip test text), and assertions that the `Err` arm and `#[allow(...)]` are present.
  - *Learn:* how Diesel maps a custom Rust type to a column (`ToSql`/`FromSql`, `AsExpression`/
    `FromSqlRow`), `match` arms, generating test code.
  - *NOT in scope:* **records** (non-enum `types:`) — the IR's `TypeDef` is `Enum`-only; records
    are a v2 item (would add `TypeDef::Record` parsing in the IR first).
  - *After this commit:* schema + models + types all exist. A manual `cargo check` on the output
    needs only a one-line `lib.rs` (`mod schema; mod models; mod types;`) + the deps — which
    Commit 8 generates automatically.

- [x] **Commit 7 — `feat: generate SQL up/down migrations`** (done)
  - *Goal:* a Diesel migration pair per table — `migrations/<NNNN>_create_<table>/up.sql` +
    `down.sql`.
  - **Dir naming (locked):** zero-padded **sequence number** by spec order
    (`0001_create_ride`, `0002_create_booking`). Deterministic, unique, sortable, no flag.
    (Renumbering on table insert is acceptable in v1's regenerate-fresh model; incremental
    `ALTER` diffs are v2.)
  - **Codegen:** `codegen::generate_migrations(tables: &[TableDef], config: &Config) ->
    Result<Vec<GeneratedFile>>` (same slice/`GeneratedFile` shape). Index `tables` for the NNNN.
  - **up.sql** — `CREATE TABLE <sql_table> ( <cols> );` then a `CREATE INDEX` per secondary key:
    - column line: `<column> <pg_type>` + ` NOT NULL` (when `!optional`) + ` DEFAULT <default>` (when set).
    - **PK:** single `id` → inline `PRIMARY KEY` on the column; composite → table-level
      `PRIMARY KEY (a, b)`.
    - **index:** each `secondary_keys` entry → `CREATE INDEX idx_<table>_<col> ON <table> (<col>);`
      (finally consumes `secondary_keys`).
  - **down.sql** — `DROP TABLE <sql_table>;`.
  - **IR change (locked):** add `default: Option<String>` to `FieldDef`. `inject_timestamps`
    sets `created_at`/`updated_at` → `Some("CURRENT_TIMESTAMP")` (required — `NewRide` omits
    them, so the column must self-default). The parser also reads the spec's `default:` block
    into it (one mechanism, covers spec defaults too).
  - **Touch:** `templates/migration_up.sql.jinja` (`escape = "none"` not needed for SQL, but set
    it anyway for consistency — SQL has no `<>` issue); `src/codegen.rs` (`generate_migrations`,
    context struct `{ table, columns: Vec<{name, ty, not_null, default?}>, pk_inline?/pk_table?,
    indexes }`); `src/ir.rs` (`default`); `src/parser.rs` (`default:` parse + timestamp defaults);
    `src/cli.rs` (write migrations). down.sql can be built inline (one line) — no template needed.
  - *Done when:* `cargo test` green — `up.sql` snapshot (CREATE TABLE + the `NOT NULL`, the
    `created_at … DEFAULT CURRENT_TIMESTAMP`, the `CREATE INDEX idx_ride_driver_id`), `down.sql`
    snapshot (`DROP TABLE ride;`), and a composite-PK case emitting table-level `PRIMARY KEY (…)`.
  - *Learn:* `std::fs::create_dir_all`, `PathBuf`, SQL formatting, extending the IR + parser.

- [x] **Commit 8 — `feat: post-gen rustfmt + cargo check verification`** ← **the safety net** (done)
  - *Goal:* make `<out>` a crate that actually compiles, verified automatically; fail loudly if not.
  - **Crate scaffold (locked):**
    - `<out>/Cargo.toml` — `[package]` `edition = "2021"` (diesel-stable; the generator's own
      crate stays 2024), name = sanitized out-dir basename (fallback `generated_diesel`).
    - `[dependencies]` assembly: take `[cargo.dependencies]` from the project config **verbatim**
      (each `name = '<toml fragment>'`), then append each **used** type's `crate` line, deduped by
      crate name (the token before `=`; base wins on collision). If `diesel` isn't present, inject
      `diesel = { version = "2", features = ["postgres_backend"] }`. **Only deps for types actually
      resolved are emitted** — collect crate lines during the resolve loop, no unused `rust_decimal`.
    - `<out>/src/lib.rs` — `pub mod schema;` + `pub mod models;` (+ `pub mod types;` only when any
      enum was generated).
  - **Config change (locked):** add a `cargo` section — `Config { types, cargo: CargoConfig }`,
    `CargoConfig { dependencies: HashMap<String, String> }` (value = raw TOML dep fragment as a
    string). Absent section = empty (the diesel default still kicks in).
  - **`src/verify.rs` (new, locked):** `std::process::Command`.
    - `rustfmt(path)` — run `rustfmt --edition 2021 <file>` per generated `.rs` (skip `.sql`).
      Always runs (cosmetic). `rustfmt` missing → warn once and skip.
    - `cargo_check(out)` — `cargo check` in `<out>`, capture stdout+stderr; non-zero exit ⇒
      `Err` carrying the captured compiler output. `cargo` missing ⇒ hard error.
  - **`--no-verify` (locked):** skips only the `cargo check` step (matches the README usage line);
    rustfmt + the scaffold still run.
  - **CLI order:** write schema/models/types/migrations → write `Cargo.toml` + `lib.rs` → rustfmt
    the `.rs` files → unless `--no-verify`, `cargo check`.
  - *Done when:* `cargo test` green; a real end-to-end `generate` of `Ride.yaml` (with a project
    config that has `[cargo.dependencies]`) ends with a passing `cargo check`, and deliberately
    breaking a template makes `generate` exit non-zero with the compiler's message. **This is also
    the commit that ends the bare-`cargo run` friction** — the generator now owns the whole loop
    and supplies the config-driven deps, so a configured run is turnkey.
  - *Touch:* `src/verify.rs` (new); `src/codegen.rs` (`generate_cargo_toml`, `generate_lib_rs`);
    `src/config.rs` (`cargo` section); `src/cli.rs` (write scaffold, `--no-verify`, call verify);
    a sample `examples/namma-diesel.toml` so the example is runnable.
  - *Learn:* `std::process::Command`, capturing stderr, exit codes, error propagation.

- [ ] **Commit 9 — `feat: directory mode + error messages`**
  - *Goal:* point `--spec` at a folder of specs; friendly failures.
  - *Touch:* walk the dir, generate per `.yaml`; `thiserror` for "which file/field
    failed". Document an `examples/` run in the README.
  - *Done when:* a folder of real NammaYatri specs produces a `cargo check`-clean crate.
  - *Learn:* directory walking, custom error types, `From` impls.

**v1 done** = commits 1–9 merged; the demo in the README works end to end.

### How to work each commit
1. Read the matching Phase section below for detail and pitfalls.
2. Write the code + its test. Run the **Done when** check.
3. Commit with the exact title. Tick the box here in the same commit.
4. Next commit. Never start N+1 with N red.

---

## Mental model: how a code generator works

Three stages, always separated:

1. **Parse** — turn the YAML text into Rust data structures (the *IR*, intermediate representation). After this stage, the YAML is gone; you only work with structs.
2. **Generate** — walk the IR and produce source text. Templates do the writing.
3. **Verify** — prove the produced text is valid by compiling it.

The golden rule, inherited from namma-dsl: **the IR is the contract.** The parser's only job is to fill the IR. The generators' only job is to read the IR. They never know about each other. This is what lets one spec drive Haskell *and* Rust — same IR shape, different generators.

---

## Project layout

Start as a **single binary crate with modules** (simpler than a workspace while learning). Refactor into a workspace later if it grows.

```
namma-diesel/
├── Cargo.toml
├── src/
│   ├── main.rs          # CLI entry (clap) — thin
│   ├── cli.rs           # arg parsing + command dispatch
│   ├── config.rs        # config struct
│   ├── ir.rs            # TableDef, FieldDef, TypeDef, Constraint, IndexDef
│   ├── parser.rs        # serde_norway -> IR
│   ├── typemap.rs       # spec type -> (Rust type, SQL type)
│   ├── codegen/
│   │   ├── mod.rs       # orchestrates generators
│   │   ├── schema.rs    # diesel::table! generator
│   │   ├── model.rs     # Queryable/Insertable struct generator
│   │   ├── types.rs     # enum/struct generator
│   │   └── migration.rs # up.sql / down.sql generator
│   └── verify.rs        # rustfmt + cargo check
├── templates/           # Askama .jinja templates (askama 0.13+)
│   ├── schema.rs.jinja       # one diesel::table! block
│   ├── schema_mod.rs.jinja   # mod decls + allow_tables_to_appear_in_same_query!
│   ├── model.rs.jinja
│   ├── enum.rs.jinja
│   └── migration_up.sql.jinja
├── examples/
│   └── specs/Ride.yaml  # a real spec to test against
└── tests/
    └── snapshots/       # golden expected output (insta)
```

---

## Dependencies

Add these as the phases that need them arrive — don't front-load.

| Crate | Purpose | Phase |
|---|---|---|
| `clap` (derive feature) | CLI parsing | 0 |
| `serde`, `serde_norway` | YAML → structs | 1 |
| `anyhow` | easy error handling (`Result` + `?`) | 1 |
| `thiserror` | typed errors (later, nicer messages) | 7 |
| `heck` | case conversion (`CamelCase` ↔ `snake_case`) | 2 |
| `askama` | templates | 3 |
| `insta` (dev-dependency) | snapshot/golden tests | 3 |

The **generated** crate (separate from this tool) depends on `diesel`, `chrono`, `rust_decimal`, `serde`. You don't add those here.

---

## Phase 0 — Rust setup + a CLI that reads a file

**Goal:** `namma-diesel generate --spec examples/specs/Ride.yaml --out ./out` runs, reads the file, and prints its raw contents.

**Steps**
1. `cargo new namma-diesel` (or `cargo init` in this repo).
2. Add `clap` with the `derive` feature. Define a `Cli` struct and a `generate` subcommand with `--spec` and `--out`.
3. In `main.rs`, read the spec file with `std::fs::read_to_string` and `println!` it.

**Rust concepts learned:** `cargo` and `Cargo.toml`, the module system, `clap` derive macros, `std::fs`, `Result` and the `?` operator, ownership basics (passing `&str` vs `String`).

**Done when:** the command prints the YAML. No parsing yet.

---

## Phase 1 — Define the IR + parse YAML into it

This is the most important phase. Get the IR right and everything downstream is easy.

**Steps**
1. In `ir.rs`, define the structs. Start small — only what v1 needs:

   ```rust
   pub struct TableDef {
       pub name: String,            // "Ride"
       pub sql_table: String,       // "ride"
       pub fields: Vec<FieldDef>,
       pub types: Vec<TypeDef>,     // custom enums/records
       pub primary_key: Vec<String>,
       pub secondary_keys: Vec<Vec<String>>,
       pub indexes: Vec<IndexDef>,
   }

   pub struct FieldDef {
       pub name: String,            // domain field name, "driverId"
       pub spec_type: String,       // raw from YAML, "Id Person"
       pub optional: bool,          // was it `Maybe ...`?
       pub db_type_override: Option<String>, // from beamType:
       pub sql_type_override: Option<String>,// from sqlType:
       pub default: Option<String>,
       pub constraints: Vec<Constraint>,
   }

   pub enum Constraint { PrimaryKey, SecondaryKey, NotNull }

   pub enum TypeDef {
       Enum { name: String, variants: Vec<String> },
       Record { name: String, fields: Vec<(String, String)> },
   }
   ```

2. In `parser.rs`, parse the YAML. The namma-dsl format is a top-level map (`Ride: {...}`). Use `serde_norway::Value` for a **forgiving** parse — walk the `Value` tree by hand rather than deriving `Deserialize` on the IR directly. Reason: the spec format has irregularities (old vs new field syntax, list-of-maps for `types`) that don't map cleanly onto derive. Walking `Value` is more code but far more robust, and it's how namma-dsl's parser works too (it walks the YAML with lenses).
3. Handle the two field syntaxes namma-dsl supports (ordered list-of-maps, and the legacy plain map).
4. Strip the `Maybe ` prefix → set `optional = true`. Strip the `|WithId`-style relation suffix for now (record it, ignore it in v1).
5. Write a unit test: parse `examples/specs/Ride.yaml`, assert the `TableDef` has the right field count, PK, and one enum type.

**Rust concepts learned:** `struct` and `enum` definitions, `Vec`, `Option`, deriving `Debug`, pattern matching with `match`, borrowing while iterating, `serde_norway::Value` navigation, writing `#[test]` functions.

**Done when:** `cargo test` parses a real spec into a correct IR. **Print the IR with `{:#?}` and eyeball it.**

> **Pitfall:** don't try to make `serde` derive do everything. The spec format wasn't designed for serde. Walk the `Value`. You'll thank yourself when you hit `types:` (a list of single-key maps).

---

## Phase 2 — The type mapper

**Goal:** given a `spec_type` string and the table's known custom types, resolve `(rust_type, diesel_sql_type, pg_column_type)`.

**Steps**
1. In `typemap.rs`, write a function `resolve(spec_type, optional, types, overrides) -> ResolvedType`.
2. Implement the table from the README as a `match` on the base type string.
3. Custom types: if `spec_type` names a generated enum/record, the Rust type is that enum's name; the SQL type follows `beamType`/`sqlType` overrides (default `Text` for enums).
4. `Maybe T` / `optional` wraps Rust in `Option<…>` and Diesel in `Nullable<…>`.
5. Case conversion with `heck`: field `driverId` → column `driver_id`; type `Ride` → table `ride`.
6. Tests for each row of the table, plus `Option` wrapping and one custom enum.

**Rust concepts learned:** `match` exhaustiveness, returning structs, `HashMap` (if you table-drive it), string manipulation, the `heck` crate, more testing.

**Done when:** every mapping-table row has a passing test.

---

## Phase 3 — First generator: schema + models via Askama

Now you produce real Rust. Pick the schema and model generators first because they're the ORM core.

**Steps**
1. Add `askama`. Create `templates/schema.rs.jinja` and `templates/model.rs.jinja`.
2. Define an Askama context struct per template (a struct holding exactly the strings the template needs — already type-mapped). Keep templates dumb; do all logic in Rust, pass finished strings in.

   ```rust
   #[derive(Template)]
   #[template(path = "schema.rs.jinja")]
   struct SchemaTemplate<'a> {
       table: &'a str,
       pk: &'a str,
       columns: Vec<Column<'a>>, // { name, sql_type }
   }
   ```

3. `templates/schema.rs.jinja`:

   ```jinja
   diesel::table! {
       {{ table }} ({{ pk }}) {
       {%- for c in columns %}
           {{ c.name }} -> {{ c.sql_type }},
       {%- endfor %}
       }
   }
   ```

4. `model.rs.jinja`: emit the `Queryable`/`Selectable`/`Identifiable` struct and the `NewX` `Insertable` struct.
5. In `codegen/mod.rs`, wire: IR → type-mapped context → render → write file under `--out`.
6. Add `insta` snapshot tests: render for `Ride.yaml`, snapshot the output. Review the snapshot once, then it guards against regressions.

**Rust concepts learned:** traits and `derive` macros (Askama's `Template`), lifetimes (`&'a str` in context structs — your first real encounter; keep it simple by cloning to `String` if lifetimes fight you), iterators, writing files, snapshot testing.

**Done when:** running the tool on `Ride.yaml` writes a `schema.rs` and `models/ride.rs` you'd be happy to commit by hand.

> **Lifetime tip:** if `&'a str` borrow-checker errors slow you down, use owned `String` in the context structs at first. Performance is irrelevant for a code generator. Optimize for getting it working.

---

## Phase 4 — SQL migration generator

**Steps**
1. `templates/migration_up.sql.jinja` and a `down.sql` (just `DROP TABLE`).
2. Emit `CREATE TABLE` with columns (pg types from the type mapper), `PRIMARY KEY`, `NOT NULL` for non-optional fields, defaults, and `CREATE INDEX` for each secondary key / index.
3. Write to `migrations/<timestamp>_create_<table>/{up,down}.sql` (Diesel's migration layout). Use a fixed/derived timestamp so snapshots are stable (e.g. derive from spec name, or pass `--migration-ts`).
4. Snapshot test.

**Rust concepts learned:** more templating, `std::fs::create_dir_all`, path building with `std::path::PathBuf`, formatting.

**Done when:** a `psql` could run the `up.sql` and create the table the schema describes.

---

## Phase 5 — Custom type / enum generation

Fields referencing custom types won't compile without this, so it's part of v1.

**Steps**
1. `templates/enum.rs.jinja`: a Rust `enum` with `Serialize`/`Deserialize` and the Diesel `AsExpression`/`FromSqlRow` derives plus `#[diesel(sql_type = Text)]`.
2. Generate the `ToSql<Text, Pg>` / `FromSql<Text, Pg>` impls that map each variant to its string form (the variant name as it appears in the enum spec).
3. Records (non-enum custom types): generate a plain serde struct. (If a record is stored as a single JSON column, map it through `Jsonb` later; for v1, a struct + Text/JSON is enough.)
4. Snapshot test for an enum.

**Rust concepts learned:** how Diesel maps custom Rust types to SQL columns (`ToSql`/`FromSql`), enum variants, more derives. This is the deepest Diesel-specific phase — take your time here.

**Done when:** the generated `Status` enum compiles inside the output crate and round-trips to `TEXT`.

---

## Phase 6 — The verification pass (the safety net)

This is what makes the tool trustworthy.

**Steps**
1. In `verify.rs`, after all files are written, shell out with `std::process::Command`:
   - `rustfmt` each generated `.rs` file (formats + catches gross syntax errors).
   - `cargo check` in the `--out` crate.
2. If either exits non-zero, print its stderr and return an error — the `generate` command fails.
3. Add `--no-verify` to skip it while iterating.
4. Make the `--out` crate a real, minimal Cargo crate (generate its `Cargo.toml` with `diesel` deps + a `lib.rs` that `mod`-declares the generated files) so `cargo check` has something to check.

**Rust concepts learned:** `std::process::Command`, capturing stdout/stderr, exit codes, propagating errors up to `main`.

**Done when:** deliberately breaking a template makes `generate` fail with the compiler's complaint, not a silent bad file.

---

## Phase 7 — CLI polish + directory mode

**Steps**
1. `--spec` accepts a directory: walk it, generate for every `.yaml`.
2. Nice error messages with `thiserror` (which file, which field failed).
3. `--all` flag groundwork (real incremental regen is v2).
4. A short `examples/` run documented in the README.

**Rust concepts learned:** directory walking (`std::fs::read_dir` or the `walkdir` crate), custom error types with `thiserror`, `From` impls for error conversion.

**Done when:** pointing `--spec` at a folder of NammaYatri specs produces a buildable Rust crate.

---

## v1 done = the demo

> `namma-diesel generate --spec ./spec/Storage --out ./generated` reads real NammaYatri storage specs and writes a Rust Diesel crate that **`cargo check` passes**, containing schema, models, custom enums, and migrations.

That's the "whoa": the same YAML that builds the Haskell backend now builds a Rust one.

---

## v2 and beyond (designed, deferred)

Build these only after v1 is solid. Each maps to a namma-dsl feature.

| Feature | namma-dsl analog | Notes |
|---|---|---|
| **Query functions** | `BeamQueries.hs` / `queries:` block | `find_by_id`, `create`, `update_<x>` as Diesel DSL fns. Parse the `where:` clause tree into Diesel `.filter(...)`. |
| **Domain ↔ DB conversion** | `ToTType` / `FromTType` | A domain struct distinct from the row struct + `From` impls. Needed when the DB shape ≠ domain shape. |
| **Relations** | `\|WithId`, `\|WithCachedId` | Foreign-key fields, joins, `Identifiable`/`Associations`. |
| **Cached / KV queries** | `CachedQueries.hs` | Redis-backed wrappers. Rust side: a cache trait + generated key construction. |
| **Incremental regen** | git-hash file-state (`App.hs`) | `git hash-object` vs `HEAD` to skip unchanged specs. Straightforward port. |
| **Schema-diff migrations** | `SQL/Table.hs` `ALTER` generation | Compare new spec to last `up.sql`; emit `ALTER TABLE` instead of `CREATE`. The hardest v2 item. |
| **`beamType` storage override** | `beamType:` block | Apply `db_type_override` to a non-enum field's storage (override Diesel/PG type, keep the domain Rust type). Parsed into the IR in v1 but not yet applied (`resolve` has a NOTE). Enums are already text-backed. |
| **Workspace split** | namma-dsl's lib layout | Split into `nd-core` / `nd-parser` / `nd-codegen` / `nd-cli` crates when it grows. |

---

## Testing strategy

1. **Snapshot tests (`insta`)** — every generator snapshots its output for a fixed input spec. Reviewing a snapshot diff is how you review a generator change.
2. **The compile gate** — keep one `examples/` spec and assert in CI that the generated crate `cargo check`s. This catches semantic breakage snapshots can't.
3. **Parser unit tests** — table of `(yaml fragment → expected IR)`, including the injected `created_at`/`updated_at` (A2).
4. **Type-mapper unit tests** — one per mapping row, plus a config-override case (A3).
5. **Multi-spec test (guards A1)** — generate from two specs; assert both `schema/<t>.rs` files exist and `schema/mod.rs` lists both modules with `allow_tables_to_appear_in_same_query!`. Without this, the per-spec-overwrite bug can silently return.
6. **Enum round-trip test (guards A4/T1)** — per generated enum: every variant → string → variant; an unknown string → `Err`, not panic.

Snapshots guard *shape*; `cargo check` guards *correctness*; the multi-spec and round-trip tests guard the two bugs most likely to regress. You need all of them.

---

## How to learn Rust *through* this project

You don't need to learn Rust before starting — learn it phase by phase. Order of concepts this plan walks you through:

1. **Phases 0–1:** ownership, `Result`/`?`, structs, enums, `Vec`, `Option`, `match`, modules, tests. (This is ~60% of day-to-day Rust.)
2. **Phases 2–3:** traits and derive macros, iterators, lifetimes (gently), the borrow checker in anger.
3. **Phases 5–6:** trait impls (`ToSql`/`FromSql`), `std::process`, error propagation.

Keep open while building: [The Rust Book](https://doc.rust-lang.org/book/) (chs. 1–10 cover everything in Phases 0–3), [Diesel guides](https://diesel.rs/guides/), and [Askama docs](https://rinja.dev/). When the borrow checker fights you, the escape hatch is almost always *clone to an owned `String`* — do that, keep moving, optimize never (a code generator's speed doesn't matter).

**First action:** Phase 0 — `cargo init`, add `clap`, make `namma-diesel generate --spec <file>` print the file. One sitting.
