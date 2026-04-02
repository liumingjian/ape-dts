# Spec

## Summary

Close out the first-wave `GaussDBMySQL` bootstrap docs after `snapshot + struct + check`
all passed in the real environment. The docs must reflect the corrected model:
`db_type=gaussdb_mysql` can still target a `postgres://.../<mysql-compatible-db>`
endpoint when GaussDB exposes MySQL compatibility at the database level.

## Scope

- update `docs/templates/mysql_to_gaussdb_mysql.md` to the validated env contract
- sync `docs/agent-summary/plan.md` and `docs/agent-summary/gaussdb-progress-tracker.md`
- record the child completion in the parent epic truth table

## Acceptance

- template doc no longer describes the target path as blocked
- snapshot / struct / check validated commands are documented
- tracker and plan show first-wave bootstrap as delivered while keeping CDC out of scope
