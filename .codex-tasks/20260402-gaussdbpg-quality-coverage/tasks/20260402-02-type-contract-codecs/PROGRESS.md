# Progress Log

## Context Recovery Block

- **Task**: `GaussDBPg type contract and codec coverage`
- **Shape**: `single-full`
- **Truth file**: `.codex-tasks/20260402-gaussdbpg-quality-coverage/tasks/20260402-02-type-contract-codecs/TODO.csv`

## 2026-04-02

- Child opened from the quality-coverage epic after the `GaussDBMySQL` first-wave bootstrap closed.
- Initial scope intentionally narrowed to the PRD-listed GaussDB-specific aliases:
  - `smalldatetime`
  - `tinyint`
  - `nvarchar2`
  - `clob`
  - `blob`
- Early code reading shows the current gap:
  - `TypeRegistry::parse_col_meta()` derives `value_type` from OID first
  - unknown/custom OIDs currently fall back to `PgValueType::String`
  - that means GaussDB-specific aliases with non-standard OIDs can miss numeric / datetime / bytea semantics unless alias fallback is applied
- Planned fix:
  - add explicit alias normalization in `type_registry`
  - resolve `value_type` from alias when the OID path is unknown
  - add focused unit tests in `pg_value_type.rs`, `type_registry.rs`, and `pg_col_value_convertor.rs`
- Implementation landed:
  - `dt-common/src/meta/pg/type_registry.rs`
    - normalize PRD aliases:
      - `smalldatetime -> timestamp`
      - `tinyint -> int2`
      - `nvarchar2 -> varchar`
      - `clob -> text`
      - `blob -> bytea`
    - add unknown/custom OID fallback through alias resolution
  - `dt-common/src/meta/pg/pg_value_type.rs`
    - teach `from_alias()` the five PRD aliases
  - `dt-common/src/meta/adaptor/pg_col_value_convertor.rs`
    - add focused unit coverage proving the converted `ColValue` semantics:
      - `smalldatetime -> DateTime`
      - `tinyint -> Short`
      - `nvarchar2/clob -> String`
      - `blob -> Blob`
- Validation:
  - `cargo test -p dt-common gaussdb_type_matrix -- --nocapture` PASS
  - `cargo test -p dt-common -p dt-connector gaussdb_type_matrix -- --nocapture` PASS
- Evidence:
  - `raw/20260402_gaussdb_type_matrix.log`
- Child 2 is now closed.
