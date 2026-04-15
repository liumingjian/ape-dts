# Task Specification

## Task Shape

- **Shape**: `single-full`

## Goals

- 新增 `gaussdb_to_mysql` 的 cdc basic 自动化入口。
- 为该用例补齐最小 SQL 夹具与 `task_config.ini`，验证 `GaussDBCdcExtractor -> Mysql` 的主路径可运行。
- 复用既有 slot 策略与 retry 策略，在共享 GaussDB 环境下拿到稳定证据。

## Non-Goals

- 不做 resume / failover / type matrix。
- 不在本 child 内扩展 struct/DDL 能力。
- 不在未证明缺口前改动运行时代码。

## Constraints

- 复用现有 `GaussDBPg` CDC 连接契约（slot_name 等）与 MySQL write sink 契约。
- 夹具保持最小：1 张表 + insert/update/delete，最终状态可对比。
- source cleanup 采用幂等 prepare（避免共享 GaussDB cleanup 抖动）。

## Environment

- **Project root**: `/Users/lmj/projects/ai-project/ape-dts`
- **Language/runtime**: Rust / Cargo
- **Package manager**: Cargo
- **Test framework**: `cargo test -p dt-tests --test integration_test`
- **Build command**: `cargo test -p dt-tests --test integration_test --no-run`

## Risk Assessment

- [x] GaussDB logical replication 启动（slot + START_REPLICATION）已通过 `start_millis=60000` 验证可跑通。
- [x] 共享 GaussDB 连接抖动可能触发 transient error，但 `TestBase::run_with_retry` 已覆盖。
- [x] MySQL 目标端已验证可写且 CDC 写入/对比通过。

## Deliverables

- `dt-tests/tests/gaussdb_to_mysql/cdc_tests.rs`
- minimal fixture directory under `dt-tests/tests/gaussdb_to_mysql/cdc/basic_test/`
- child progress artifacts documenting live validation result

## Done-When

- [x] `gaussdb_to_mysql::cdc_tests::test::cdc_basic_test` can be discovered and compiled
- [x] live validation passes

## Final Validation Command

```bash
cargo test -p dt-tests --test integration_test -- gaussdb_to_mysql::cdc_tests::test::cdc_basic_test --nocapture
```
