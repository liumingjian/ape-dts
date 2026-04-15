# Task Specification

## Task Shape

- **Shape**: `single-full`

## Goals

- 为 `GaussDBPg -> MySQL` 补齐 `dt-tests` 的 **precheck basic** 自动化入口。
- 固化最小 fixture：source 为 GaussDBPg schema，target 为 MySQL database（同名），确保过滤规则对两端都可解释。
- 更新 tracker / e2e matrix，将 `GaussDBPg -> MySQL` 的 `precheck` 从 `-` 关闭为 ✅。

## Non-Goals

- 不在本任务内实现 `GaussDBPg -> MySQL` 的 `struct` 对象同步（该项单独 epic/child 处理）。
- 不扩展 type-matrix / failover / resume。

## Constraints

- 不向仓库写入凭据：仅通过 `dt-tests/tests/.env.local` 注入连接信息。
- 用例必须 prepare/cleanup 幂等，避免污染共享 GaussDB / 本机 MySQL 环境。

## Environment

- **Project root**: `/Users/lmj/projects/ai-project/ape-dts`
- **Build command**: `cargo test -p dt-tests --test integration_test --no-run`
- **Validation command**:

```bash
set -a; source dt-tests/tests/.env.local; set +a
cargo test -p dt-tests --test integration_test -- \
  gaussdb_to_mysql::precheck_tests::test::struct_supported_basic_test --nocapture
```

## Done-When

- `gaussdb_to_mysql::precheck_tests::test::struct_supported_basic_test` 可被发现、可编译、并能在真实环境 PASS
- `docs/agent-summary/gaussdb-progress-tracker.md` / `docs/agent-summary/gaussdb-e2e-test-plan.md` 已反映 precheck 状态与入口

