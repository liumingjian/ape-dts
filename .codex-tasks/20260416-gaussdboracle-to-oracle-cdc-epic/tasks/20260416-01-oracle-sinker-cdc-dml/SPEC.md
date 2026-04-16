# Spec: OracleSinker 支持 CDC DML（UPDATE/DELETE）

## Goal

在现有 `OracleSinker`（当前仅支持 snapshot INSERT）基础上，扩展支持：

- `RowType::Update`
- `RowType::Delete`

用于承载 `GaussDBOracle -> Oracle` CDC basic。

## Constraints

- 继续使用 `sqlplus`（容器内 `docker exec`）作为执行方式。
- 不引入 OCI/JDBC 等新依赖。

## Acceptance / Validation

- `cargo test -p dt-connector --no-run` PASS

