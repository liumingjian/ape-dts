# Progress Log

## 2026-03-31

### Init

- Initialized task artifacts.

### Progress

- `PgStructFetcher.get_privileges()` now includes routine grants derived from `pg_proc.proacl` via `aclexplode`.
- Generates:
  - `GRANT EXECUTE ON FUNCTION schema.name(args) TO role [WITH GRANT OPTION]`
  - `GRANT EXECUTE ON PROCEDURE schema.name(args) TO role [WITH GRANT OPTION]`
- Uses `pg_get_function_identity_arguments(oid)` to build a stable signature for overloaded routines.
- Fallback when `pg_proc.prokind` is unavailable: infer procedure/function from `pg_get_functiondef(oid)` header.

### Validation (Unit)

```bash
cargo test -p dt-common -p dt-connector
```

Result: PASS
