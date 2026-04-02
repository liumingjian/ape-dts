# Spec

## Summary

Build the first non-CDC type-matrix e2e layer for `GaussDBPg` in a directional
way:

- `PG -> GaussDBPg snapshot type_matrix_test`
- `GaussDBPg -> PG check type_matrix_test`

This child depends on the codec baseline from child 2 and turns it into actual
dt-tests entry points and fixtures.

## Directional Rule

- `PG -> GaussDBPg` should use canonical PG types to validate target
  compatibility.
- `GaussDBPg -> PG` should use the GaussDB-specific aliases validated in child 2
  to exercise source-side metadata/codec behavior.

## Acceptance

- dt-tests entry points exist for the two non-CDC matrix directions
- fixture SQL covers the first alias set and matching canonical target types
- compile/no-run passes; real-env execution is attempted when sandbox allows
