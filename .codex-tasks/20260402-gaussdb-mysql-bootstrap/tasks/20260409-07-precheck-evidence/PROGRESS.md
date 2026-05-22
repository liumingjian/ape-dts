# Progress Log

## Context Recovery Block

- **Task**: `MySQL -> GaussDBMySQL precheck + real-environment evidence`
- **Shape**: `single-full`
- **Truth file**: `.codex-tasks/20260402-gaussdb-mysql-bootstrap/tasks/20260409-07-precheck-evidence/TODO.csv`

## 2026-04-09

- Child opened to close the remaining `PARTIAL` gap in `GaussDBMySQL` first-wave bootstrap.
- Initial facts confirmed:
  - the validated target contract is `DbType::GaussDBMySQL + postgres://.../jyp_test_m`
  - `dt-precheck` still routed `DbType::GaussDBMySQL` only through `MySqlPrechecker + MysqlFetcher`
  - this means precheck could not consume the real target over pg wire
- Active fix direction:
  - keep `MySqlPrechecker` semantics for MySQL-compatible metadata checks
  - replace the fetch side with protocol-aware selection:
    - mysql wire -> `MysqlFetcher`
    - pg wire -> `PgCompatibleMysqlFetcher`
- Test plan for this child:
  - add `mysql_to_gaussdb_mysql::precheck_tests`
  - cover one positive case and one missing-struct negative case
  - make precheck tests clean up after themselves to preserve shared-environment hygiene
- Implementation landed:
  - `dt-precheck`
    - `DbType::GaussDBMySQL` precheck is now protocol-aware:
      - mysql wire keeps using `MysqlFetcher`
      - pg wire now uses new `PgCompatibleMysqlFetcher`
    - `MySqlPrechecker` version validation now accepts GaussDB/OpenGauss style version strings for `DbType::GaussDBMySQL`
  - `dt-tests`
    - added `mysql_to_gaussdb_mysql::precheck_tests`
    - added basic positive and missing-struct negative fixtures
    - `TestBase::run_precheck_test` now uses the same retry pattern as other shared-environment tests
    - `PrecheckTestRunner` now performs best-effort cleanup through `RdbTestRunner::execute_clean_sqls()`
- Validation completed:
  - `cargo test -p dt-precheck --no-run` PASS
  - `cargo test -p dt-tests --test integration_test --no-run` PASS
  - `cargo test -p dt-tests --test integration_test -- mysql_to_gaussdb_mysql::precheck_tests::test::struct_supported_basic_test --nocapture` PASS
  - `cargo test -p dt-tests --test integration_test -- mysql_to_gaussdb_mysql::precheck_tests::test::db_not_exists_test --nocapture` PASS
- Real-environment notes:
  - positive case confirmed `gaussdb_mysql_precheck_basic` can be created, checked by precheck, and then cleaned from both source and target
  - negative case confirmed missing source struct still reports `CheckIfStructExisted=false` while target remains valid under `do_struct_init=true`
  - post-run no-pollution checks:
    - target `information_schema.schemata` count for `gaussdb_mysql_precheck_basic/gaussdb_mysql_precheck_missing` = `0`
    - source `information_schema.schemata` count for the same names = `0`
- Evidence written to:
  - `raw/struct_supported_basic_test.log`
  - `raw/db_not_exists_test.log`
