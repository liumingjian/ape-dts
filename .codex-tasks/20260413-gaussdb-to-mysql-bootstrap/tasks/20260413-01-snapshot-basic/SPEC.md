# Task Specification

## Task Shape

- **Shape**: `single-full`

## Goals

- 新增 `gaussdb_to_mysql` 的 snapshot basic 自动化入口。
- 为该用例补齐最小 SQL 夹具与 `task_config.ini`。
- 通过编译或真实运行验证 `GaussDBPg -> Mysql` 的最小反向主路径。

## Non-Goals

- 不做 check / cdc。
- 不在本 child 内扩展类型矩阵、failover、resume。
- 不为尚未证明存在的问题提前改动大段运行时代码。

## Constraints

- 复用现有 `GaussDBPg` 源端配置契约和 MySQL sink 契约。
- 测试夹具保持最小，只覆盖一张小表和少量记录。
- 若首轮验证失败，优先保留失败证据并暴露 blocker，而不是静默扩大范围。

## Environment

- **Project root**: `/Users/lmj/projects/ai-project/ape-dts`
- **Language/runtime**: Rust / Cargo
- **Package manager**: Cargo
- **Test framework**: `cargo test -p dt-tests --test integration_test`
- **Build command**: `cargo test -p dt-tests --test integration_test --no-run`

## Risk Assessment

- [x] Existing runtime reuse path needs verification.
- [x] Schema-to-database mapping may be the first blocker.
- [x] Real GaussDB source availability confirmed for live validation.
- [x] Long-running test timeout may require adjustment.

## Deliverables

- `dt-tests/tests/gaussdb_to_mysql/` snapshot test module
- minimal snapshot fixture directory under `dt-tests/tests/gaussdb_to_mysql/snapshot/basic_test/`
- child progress artifacts documenting validation result and next blocker

## Done-When

- [x] `gaussdb_to_mysql::snapshot_tests::test::snapshot_basic_test` can be discovered and compiled
- [x] the task has a clear validation result (`PASS` or explicit blocker with evidence)

## Final Validation Command

```bash
cargo test -p dt-tests --test integration_test -- gaussdb_to_mysql::snapshot_tests::test::snapshot_basic_test --nocapture
```
