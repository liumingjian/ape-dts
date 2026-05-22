# Progress Log

## 2026-03-31

### Init

- Initialized task artifacts.

### Progress

- Added `StructureType::View` to enable `do_structures=view`.
- Added `PgCreateViewStatement` and integrated it into struct routing + sink SQL generation.
- `PgStructFetcher` can now fetch `pg_views` and `pg_matviews` and emit view statements.
- `PgStructExtractor` now pushes view/matview statements before RBAC.

### Validation (E2E)

Commands:

```bash
cargo test -p dt-tests --test integration_test -- pg_to_gaussdb::struct_tests::test::struct_view_routine_test --nocapture
cargo test -p dt-tests --test integration_test -- gaussdb_to_pg::struct_tests::test::struct_view_routine_test --nocapture
```

Result: PASS (both directions)

Notes:

- For targets that do not support materialized views (e.g. ustore-enabled GaussDB), matview creation falls back to a normal view to keep the object queryable.
