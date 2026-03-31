# Progress Log

## 2026-03-31

### Init

- Initialized task artifacts.

### Progress

- Added `StructureType::Routine` (`do_structures=routine`).
- Added `PgCreateRoutineStatement` (header-only routing, `CREATE OR REPLACE ...` preserved).
- `PgStructFetcher` can now fetch routines via `pg_proc + pg_language` and `pg_get_functiondef(oid)`.
  - Only `plpgsql/sql` are kept; other languages are skipped with warn.
  - Prefers `pg_proc.prokind` (PG >= 11) to distinguish function/procedure; falls back when missing.
- `PgStructExtractor` now emits routine statements before view/matview.

### Validation (Unit)

```bash
cargo test -p dt-common -p dt-connector
```

Result: PASS

### Validation (E2E)

```bash
cargo test -p dt-tests --test integration_test -- pg_to_gaussdb::struct_tests::test::struct_view_routine_test --nocapture
cargo test -p dt-tests --test integration_test -- gaussdb_to_pg::struct_tests::test::struct_view_routine_test --nocapture
```

Result: PASS (both directions)
