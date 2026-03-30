# Progress Log

## 2026-03-30

### Context

目标：将 `docs/zh/cdc/gaussdb_to_pg_manual_test.md` 的无污染手动步骤固化为可重复执行的 E2E 脚本，并在用户环境下直接跑通：

- `gaussdb_pg_candidate_hosts="10.250.0.52:8000,10.250.0.30:8000,10.250.0.51:8000"`（51 为主）
- `postgres://127.0.0.1:5434/postgres?options[statement_timeout]=10s`（本机 Docker Postgres15）

### Notes

- 脚本需保证失败时也执行清理（trap）。
- 凭据与运行配置落在 `.local/`（gitignored）避免污染仓库。

### Implementation

- 新增脚本：`scripts/e2e/gaussdb_to_pg_cdc.sh`
  - 默认启动 Docker Postgres15（容器名 `ape-dts-pg15`，端口 `5434->5432`），可用 `SKIP_DOCKER_PG=1` 跳过
  - 生成运行目录：`.local/e2e/gaussdb_to_pg_cdc_<ts>/`（含 task_config.ini 与 dt-main stdout/stderr）
  - 通过 `trap cleanup EXIT INT TERM` 强制清理：stop dt-main -> drop slot -> drop schema/table -> rm container
  - `SLOT_NAME` 默认生成并强制 sanitize（只保留 `[A-Za-z0-9_]`），避免 replication 命令语法问题
  - 支持从 `.local/manual/gaussdb_to_pg_cdc.ini` 读取 GaussDB 用户名/密码作为本机 fallback（不落库、不提交）
- 增强 slot_name 健壮性：
  - `dt-connector/src/extractor/gaussdb/gaussdb_cdc_client.rs`：`START_REPLICATION SLOT` 使用 quoted identifier
  - `dt-connector/src/extractor/pg/pg_cdc_client.rs`：`CREATE_REPLICATION_SLOT` / `START_REPLICATION` 使用 quoted identifier
- 文档入口补充：
  - `docs/zh/cdc/gaussdb_to_pg_manual_test.md` 顶部新增“脚本模式”快速开始

### Local Verification

- `bash -n scripts/e2e/gaussdb_to_pg_cdc.sh` ✅
- `cargo test -p dt-connector -q` ✅

### E2E Run (User Env)

- Command:
  - `gaussdb_pg_candidate_hosts="10.250.0.52:8000,10.250.0.30:8000,10.250.0.51:8000" bash scripts/e2e/gaussdb_to_pg_cdc.sh`
- Result: ✅ passed (2026-03-30 19:06 +0800)
- Run dir (local, gitignored): `.local/e2e/gaussdb_to_pg_cdc_20260330_190618/`
- Evidence: `raw/20260330_e2e_run1_snippet.log`

### Manual Verification (User)

- User reported manual verification succeeded after following the updated doc/script guidance (2026-03-30).
- Doc polish: clarified `DST_PG_URL` (dt-main) vs `DST_PG_PSQL_URL` (psql), and added troubleshooting for `unexpected character '-'`.
