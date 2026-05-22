# Task Specification

## Task Shape

- **Shape**: `single-full`

## Goals

- 用**本机 Docker**拉起一套可复用的 `GaussDBOracle`（Oracle 兼容模式）测试环境（优先采用 `openGauss` 镜像，`sql_compatibility=A`）。
- 在代码中补齐 `DbType::GaussDBOracle`（`gaussdb_oracle`）最小可用骨架：配置解析、连接/路由、基础元数据/SQL 工具链可编译运行。
- 新增 `dt-tests` 的 `pg_to_gaussdb_oracle` 最小 smoke（snapshot basic）用例，并在本机环境 PASS。
- 更新 docs/tracker，使 `GaussDBOracle` 不再停留在 `BLOCKED`，并给出本机 docker 环境与回归命令入口。

## Non-Goals

- 不在本任务内交付 `GaussDBOracle` 的完整对象同步/类型矩阵/CDC/resume/failover。
- 不实现 Oracle wire-protocol（OCI/ODBC/JDBC）；本轮以 **Postgres wire + Oracle compatibility mode** 为前提（与现有 GaussDB 接入模型一致）。

## Constraints

- 必须遵守仓库约束：不提交真实环境凭据（`.env.local`、`.local/`、带口令 URL 等）。
- 复用现有 `sqlx(Postgres)`/`tokio-postgres` 连接栈与 `Pg*` 运行时，减少新增依赖面。
- Docker 端口/容器名需避免与现有 `dt-tests` 本机端口冲突。

## Environment

- **Project root**: `/Users/lmj/projects/ai-project/ape-dts`
- **Language/runtime**: Rust (cargo workspace)
- **Test framework**: `cargo test` + `dt-tests` integration tests
- **Docker**: local Docker Desktop/Engine

## Risk Assessment

- [ ] openGauss docker 镜像初始化较慢，healthcheck/等待策略需稳健
- [ ] 新增 `DbType` 变体将触发大量 match 语句补齐，需靠编译驱动逐个修复
- [ ] Oracle compatibility mode 的 SQL 细节差异可能导致 struct/check 在后续扩展时出现 break（本轮只做 smoke）

## Deliverables

- `dt-tests/docker-compose.gaussdb_oracle.yml`（本机 oracle-mode gaussdb 环境）
- `dt-tests/tests/.env` 新增 `gaussdb_oracle_*` 连接变量
- 代码补齐：`DbType::GaussDBOracle` 与必要的路由/工具分支
- `dt-tests/tests/pg_to_gaussdb_oracle/**` + `integration_test.rs` 模块入口
- docs：`docs/agent-summary/gaussdb-progress-tracker.md`、`docs/agent-summary/gaussdb-e2e-test-plan.md`（如需）与 `docs/agent-summary/gaussdb-oracle-roadmap.md` 状态更新

## Done-When

- [ ] `docker compose -f dt-tests/docker-compose.gaussdb_oracle.yml up -d` 后可连通并 `SHOW sql_compatibility` 返回 `A`
- [ ] `cargo test -p dt-tests --test integration_test -- pg_to_gaussdb_oracle::snapshot_tests::test::smoke_test --nocapture` PASS
- [ ] tracker/docs 中 `GaussDBOracle` 不再是纯 `BLOCKED`，并提供清晰的本机回归入口

## Final Validation Command

```bash
docker compose -f dt-tests/docker-compose.gaussdb_oracle.yml up -d
cargo test -p dt-tests --test integration_test -- pg_to_gaussdb_oracle::snapshot_tests::test::smoke_test --nocapture
```

