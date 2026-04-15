# SPEC — dt-tests Oracle Runner Support

## Goal

让 `dt-tests` 的 `RdbTestRunner` 具备 Oracle 基础能力，从而支持新增的 Oracle ↔ GaussDBOracle snapshot 用例：

- 能在 Oracle 侧执行 `src_prepare.sql/src_test.sql/src_clean.sql`（当 Oracle 是 src）
- 能在 Oracle 侧执行 `dst_prepare.sql/dst_test.sql/dst_clean.sql`（当 Oracle 是 dst）
- `fetch_data` 能从 Oracle 拉取表数据并转成 `RowData` 供 compare 复用现有逻辑（router `col_map` 支持大小写差异）

## Constraints

- 仍以 `sqlplus`（docker exec 模式）执行；不引入 OCI/JDBC。
- 必须避免把 Oracle 凭据写入 git；通过 `dt-tests/tests/.env.local` 覆盖。

## Acceptance Criteria

- `cargo test -p dt-tests --test integration_test --no-run` 通过
- Oracle 执行 SQL 与 fetch_data 在 smoke 用例中可用（由子任务 3/4 的 E2E PASS 证明）

