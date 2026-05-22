# Child Spec

## Title

MySQL -> GaussDBMySQL cdc basic

## Parent Epic

- `.codex-tasks/20260410-gaussdb-mysql-cdc-expansion/EPIC.md`

## Goal

在已验证的 `MySQL -> GaussDBMySQL` snapshot/struct/check/precheck 能力之上，打通
第一条 `cdc basic` 主路径：

1. 自动化跑通 `mysql_to_gaussdb_mysql::cdc_tests::test::cdc_basic_test`
2. 在真实环境中证明本机 MySQL 8 的 DML 可以增量同步到远端 `jyp_test_m`
3. 保持测试前后环境无污染，并把关键日志归档

## Constraints

- 只做 `MySQL -> GaussDBMySQL`
- 目标端继续使用 pg wire：`postgres://.../<mysql-compatible-db>`
- 先不做 DDL CDC、resume、failover
- 优先复用现有：
  - `MysqlCdcExtractor`
  - `GaussDBMySQL` 的 pg-wire sink path
  - `RdbTestRunner` / 现有 cdc harness

## Acceptance

- `cargo test -p dt-tests --test integration_test -- mysql_to_gaussdb_mysql::cdc_tests::test::cdc_basic_test --nocapture`
- 真实环境证据落到 child `raw/`
- 目标端验证 SQL 能证明最终一致结果

## Initial Hypothesis

- 最可能的首轮阻塞点不在 source extractor，而在 target sink / compare / cleanup 对
  `DbType::GaussDBMySQL + postgres://` 的 CDC 路径是否完整贯通。
- 因此首轮实现应优先：
  - 补最小 `cdc_tests.rs` 与 fixture
  - 复用 `mysql_to_mysql/cdc/basic_test` 的最小集
  - 通过真实运行暴露兼容性缺口，再迭代收敛
