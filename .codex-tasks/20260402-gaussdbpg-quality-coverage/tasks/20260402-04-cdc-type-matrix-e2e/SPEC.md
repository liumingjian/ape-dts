# Spec

## Summary

Open the CDC-focused child of `GaussDBPg Quality Coverage` and extend the first
GaussDB-specific type matrix into the `gaussdb_to_pg` CDC path.

This child has two linked goals:

- add a real `gaussdb_to_pg cdc_type_matrix_test`
- preserve / prove the current fail-fast contract for unsupported CDC events

## Initial Scope

- CDC matrix table uses the same first-wave alias set already validated in child 2/3:
  - `smalldatetime`
  - `tinyint`
  - `nvarchar2`
  - `clob`
  - `blob`
- runtime validation focuses on `GaussDBPg -> PG`
- fail-fast evidence starts with decoder-level proof for:
  - DDL / object-like `op_type`
  - unknown `op_type`

## Acceptance

- `dt-tests` contains `gaussdb_to_pg::cdc_tests::test::cdc_type_matrix_test`
- the CDC test passes in the real environment
- decoder/unit coverage proves unsupported CDC event types still fail fast with actionable errors
- taskmaster raw evidence records the key runtime logs
