# Spec: Oracle -> GaussDBOracle Check Basic (bootstrap)

## Goal

补齐 `Oracle -> GaussDBOracle` 的 **check basic** 回归入口（对账主路径），用于能力矩阵收口。

## Constraints / Assumptions

- Oracle 连接继续使用 `sqlplus`（本机 Oracle XE docker 内执行 `docker exec`）。
- 对账能力复用现有 `PgCheck`（GaussDBOracle 通过 pg-wire 连接）。
- 不新增 Oracle MetaManager；对账链路允许 extractor meta 缺失（按 routed row + dst meta 计算主键）。

## Deliverables

- `dt-task`: `PgCheck/MysqlCheck` sinker 允许 `extractor_meta_manager` 缺失（不再 `unwrap()`）。
- `dt-tests`: 新增 `oracle_to_gaussdb_oracle/check/basic_test` fixture + `check_basic_test`。
- `docs`: tracker/e2e-plan/roadmap 更新能力矩阵与入口。

## Acceptance / Validation

```bash
docker compose -f dt-tests/docker-compose.oracle_xe.yml up -d
cargo test -p dt-tests --test integration_test -- oracle_to_gaussdb_oracle::check_tests::test::check_basic_test --nocapture
```

