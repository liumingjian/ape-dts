# Spec: Oracle -> GaussDBOracle Struct Basic (bootstrap)

## Goal

补齐 `Oracle -> GaussDBOracle` 的 **struct basic**（结构同步最小闭环）能力，用于能力矩阵收口：

- `dt-task/dt-connector`: 新增 `OracleStructExtractor`（显式 `extract_type=struct`）。
- `dt-tests`: 新增 `oracle_to_gaussdb_oracle::struct_basic_test` 回归入口。
- docs：更新 tracker/e2e-plan/roadmap，标记 `Oracle -> GaussDBOracle` struct 已覆盖。

## Constraints / Assumptions

- 仍使用 `sqlplus`（本机 Oracle XE docker）作为 Oracle 连接方式。
- 结构输出以 `StructStatement::PgCreateSchema/PgCreateTable` 形式下发，目标端通过 pg-wire 执行。
- Debug-First：遇到不支持的 Oracle column type 直接显式失败（不做静默降级）。

## Acceptance / Validation

```bash
docker compose -f dt-tests/docker-compose.oracle_xe.yml up -d
cargo test -p dt-tests --test integration_test -- oracle_to_gaussdb_oracle::struct_tests::test::struct_basic_test --nocapture
```

