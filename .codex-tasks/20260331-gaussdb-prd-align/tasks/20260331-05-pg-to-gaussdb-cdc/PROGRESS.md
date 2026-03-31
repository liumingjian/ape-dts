# Progress Log

## 2026-03-31

### Init

- Initialized task artifacts.

### Progress

- Added `dt-tests/tests/pg_to_gaussdb/cdc/basic_test` (minimal insert/update/delete).
- Wired `pg_to_gaussdb::cdc_tests::test::cdc_basic_test`.

### Validation (E2E)

```bash
cargo test -p dt-tests --test integration_test -- pg_to_gaussdb::cdc_tests::test::cdc_basic_test --nocapture
```

Result: PASS

Notes:

- Local PG source should be `wal_level=logical` (Docker `postgres:15`).
- Fixed `dt-parallelizer` to propagate sinker errors instead of panicking on transient I/O.
