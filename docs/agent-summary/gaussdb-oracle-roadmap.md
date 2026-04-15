# GaussDB Oracle 兼容模式路线图（已解锁：本机 Docker Smoke）

> 状态：`ACTIVE (local docker smoke)`

本轮已用 **本机 Docker** 拉起 `openGauss`（`sql_compatibility=A`）作为 `GaussDBOracle` 的可回归环境，
并完成最小闭环：`DbType + route + smoke(snapshot)`。

## 1. 本机环境（Docker）

- 启动：
  - `docker compose -f dt-tests/docker-compose.gaussdb_oracle.yml up -d`
- 连通性验证（容器内）：
  - `SHOW sql_compatibility;` 必须为 `A`
- 默认端口与账号（见 compose 与 dt-tests/.env）：
  - host: `127.0.0.1:55432`
  - user: `gaussdb`

## 2. 当前已交付（Smoke）

- 代码：`DbType::GaussDBOracle`（`gaussdb_oracle`）已接入配置与运行时骨架
- 自动化：新增 `dt-tests` 用例
  - `pg_to_gaussdb_oracle::snapshot_tests::test::smoke_test`
- 证据：
  - `.codex-tasks/20260415-gaussdb-oracle-bootstrap/PROGRESS.md`

## 3. 下一步建议（按优先级）

1. `PG -> GaussDBOracle` 扩到 `struct/check/precheck`（仍保持 target-first，先把目标端语义跑通）
2. 增加 Oracle mode 的对象/类型差异用例（逐步从“能跑”走向“可用”）
3. 在统一 e2e 计划中纳入 `GaussDBOracle` 的 Quick Gate（至少 smoke）

## 4. 明确不做（当前阶段）

- 不实现 Oracle wire-protocol（OCI/ODBC/JDBC）
- 不承诺 `GaussDBOracle` 的 CDC/resume/failover（后续单开 Epic）
