# Spec

## Summary

Open the first executable child of `GaussDBPg Quality Coverage` by hardening the
type contract for PRD-listed GaussDB-specific aliases. Keep scope intentionally
small and foundational so later type-matrix e2e work has a stable codec layer.

## Initial Scope

- `smalldatetime` -> timestamp-like `ColValue::DateTime`
- `tinyint` -> `int2`-like `ColValue::Short`
- `nvarchar2` -> varchar-like `ColValue::String`
- `clob` -> text-like `ColValue::String`
- `blob` -> bytea-like `ColValue::Blob`

## Acceptance

- alias normalization is explicit in pg type handling
- unknown/custom OIDs can fall back through alias resolution for the above types
- focused unit tests cover alias mapping and value conversion
