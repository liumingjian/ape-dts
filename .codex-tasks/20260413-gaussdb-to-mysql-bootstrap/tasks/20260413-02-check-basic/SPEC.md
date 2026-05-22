# Task Specification

## Task Shape

- **Shape**: `single-full`

## Goals

- 新增 `gaussdb_to_mysql` 的 check basic 自动化入口。
- 为该用例补齐最小 SQL 夹具与 `task_config.ini`，验证 `GaussDBPg -> Mysql` 的基础对账主路径。
- 使用确定性的 1 行 diff 期望日志，保证 check 与 router 映射确实被执行（而不是空跑）。

## Non-Goals

- 不做 cdc / resume / failover。
- 不在本 child 内扩展 type matrix。
- 不对运行时做非必要改动（先用 dt-tests 证明缺口，再修运行时）。

## Constraints

- 复用现有 `GaussDBPg` 源端配置契约和 MySQL sink 契约。
- 夹具保持最小：1 张表 + 2 行数据。
- source cleanup 继续采用幂等 prepare 策略（避免共享 GaussDB 的读写/连接抖动导致 cleanup 失败）。

## Environment

- **Project root**: `/Users/lmj/projects/ai-project/ape-dts`
- **Language/runtime**: Rust / Cargo
- **Package manager**: Cargo
- **Test framework**: `cargo test -p dt-tests --test integration_test`
- **Build command**: `cargo test -p dt-tests --test integration_test --no-run`

## Risk Assessment

- [x] Router `public -> MySQL database` 映射已被 check 主路径复用验证。
- [x] 共享 GaussDB 连接抖动可能触发 transient error，但已由 `TestBase::run_with_retry` 覆盖。
- [x] check log 路径与期望日志已匹配（`expect_check_log` 对比通过）。

## Deliverables

- `dt-tests/tests/gaussdb_to_mysql/check_tests.rs`
- minimal fixture directory under `dt-tests/tests/gaussdb_to_mysql/check/basic_test/`
- expected check logs under `dt-tests/tests/gaussdb_to_mysql/check/basic_test/expect_check_log/`
- child progress artifacts documenting validation result

## Done-When

- [x] `gaussdb_to_mysql::check_tests::test::check_basic_test` can be discovered and compiled
- [x] live validation passes with expected check logs

## Final Validation Command

```bash
cargo test -p dt-tests --test integration_test -- gaussdb_to_mysql::check_tests::test::check_basic_test --nocapture
```
