# GaussDB 统一 E2E 测试计划

> 目标：把当前已经推进的关键功能统一收敛到一套可重复执行的 E2E 回归矩阵，避免后续每次手工拼命令。

## 1. 适用范围

当前计划覆盖 5 条主线：

1. `PG <-> GaussDBPg`
2. `GaussDBPg -> PG` CDC 与稳定性
3. `MySQL -> GaussDBMySQL`
4. `GaussDBPg -> MySQL`（bootstrap）
5. `PG -> GaussDBOracle`（non-CDC basic）

当前不纳入本轮统一 E2E 主矩阵：

- `SHA256` 认证

## 2. 测试分层

### 2.1 Quick Gate

适合日常开发后快速确认“主路径没有被破坏”。

建议覆盖：

- `PG -> GaussDBPg` snapshot basic
- `PG -> GaussDBPg` struct basic
- `PG -> GaussDBPg` check basic
- `GaussDBPg -> PG` snapshot basic
- `GaussDBPg -> PG` check basic
- `GaussDBPg -> PG` CDC basic
- `GaussDBPg -> MySQL` snapshot basic
- `GaussDBPg -> MySQL` struct basic
- `GaussDBPg -> MySQL` check basic
- `GaussDBPg -> MySQL` CDC basic
- `MySQL -> GaussDBMySQL` smoke
- `MySQL -> GaussDBMySQL` struct basic
- `MySQL -> GaussDBMySQL` check basic
- `PG -> GaussDBOracle` snapshot smoke
- `PG -> GaussDBOracle` struct basic
- `PG -> GaussDBOracle` check basic
- 或一键回归（Quick）：`bash scripts/e2e/gaussdb_to_mysql_bootstrap.sh`

### 2.2 Full Functional Gate

适合阶段性合并前或一轮 spec 完成后做完整能力回归。

在 Quick Gate 基础上增加：

- `PG -> GaussDBPg` CDC basic
- `PG -> GaussDBPg` snapshot type matrix
- `PG -> GaussDBPg` struct view/routine
- `GaussDBPg -> PG` check type matrix
- `GaussDBPg -> PG` struct view/routine
- `GaussDBPg -> PG` CDC type matrix
- `GaussDBPg -> PG` CDC resume
- `GaussDBPg -> MySQL` struct advanced（default/index）

### 2.3 Resilience Gate

适合发布前或源端拓扑、CDC 逻辑发生变更后执行。

建议单独运行：

- `GaussDBPg -> PG` CDC failover（`dt-tests`）
- `scripts/e2e/gaussdb_to_pg_cdc.sh`
  - `TEST_RESUME=1`
  - `TEST_FAILOVER=1`
  - `TEST_NEG_SLOT_ACTIVE=1`
  - `TEST_NEG_NO_REPL_USER=1`

## 3. 前置环境

### 3.1 本地服务

- PostgreSQL 15：本机 Docker，默认 `5434`
- MySQL 8：本机 Docker
  - source：默认 `3311`
  - sink（GaussDB→MySQL bootstrap）：默认 `3308`

### 3.2 远端环境

- `GaussDBPg`：通过 `.env.local` 中 `gaussdb_pg_*` 配置
- `GaussDBPg` 候选主机：通过 `gaussdb_pg_candidate_hosts`
- `GaussDBMySQL`：当前为 `postgres://.../jyp_test_m` 这类 pg-wire + MySQL 兼容模式库
- `GaussDBOracle`：推荐通过 `dt-tests/tests/.env.local` 覆盖 `gaussdb_oracle_sinker_*` 指向远端 oracle-mode DB（并可配 `gaussdb_pg_candidate_hosts` 自动选 RW 主）；也可用本机替身（openGauss `sql_compatibility=A`）：`docker compose -f dt-tests/docker-compose.gaussdb_oracle.yml up -d`（端口 `55432`，变量见 `dt-tests/tests/.env` 的 `gaussdb_oracle_*`）

### 3.3 无污染要求

- `dt-tests` 用例必须各自负责 prepare / cleanup
- `scripts/e2e/gaussdb_to_pg_cdc.sh` 结束后必须清理 slot / schema / 临时用户，并 best-effort 切回原主
- 所有证据只保存脱敏日志，不提交凭据

## 4. 统一用例矩阵

| 套件 | 能力 | 命令 | 环境要求 | 预计用途 |
|---|---|---|---|---|
| Quick | `PG -> GaussDBPg` snapshot basic | `cargo test -p dt-tests --test integration_test -- pg_to_gaussdb::snapshot_tests::test::snapshot_basic_test --nocapture` | PG + GaussDBPg | 基础快照主路径 |
| Quick | `PG -> GaussDBPg` struct basic | `cargo test -p dt-tests --test integration_test -- pg_to_gaussdb::struct_tests::test::struct_basic_test --nocapture` | PG + GaussDBPg | 基础对象同步 |
| Quick | `PG -> GaussDBPg` check basic | `cargo test -p dt-tests --test integration_test -- pg_to_gaussdb::check_tests::test::check_basic_test --nocapture` | PG + GaussDBPg | 基础对账 |
| Quick | `GaussDBPg -> PG` snapshot basic | `cargo test -p dt-tests --test integration_test -- gaussdb_to_pg::snapshot_tests::test::snapshot_basic_test --nocapture` | GaussDBPg + PG | 反向快照主路径 |
| Quick | `GaussDBPg -> PG` check basic | `cargo test -p dt-tests --test integration_test -- gaussdb_to_pg::check_tests::test::check_basic_test --nocapture` | GaussDBPg + PG | 反向对账主路径 |
| Quick | `GaussDBPg -> PG` CDC basic | `cargo test -p dt-tests --test integration_test -- gaussdb_to_pg::cdc_tests::test::cdc_basic_test --nocapture` | GaussDBPg + PG | 基础 CDC 主路径 |
| Quick | `GaussDBPg -> MySQL` snapshot basic | `cargo test -p dt-tests --test integration_test -- gaussdb_to_mysql::snapshot_tests::test::snapshot_basic_test --nocapture` | GaussDBPg + MySQL8 | 反向快照主路径（bootstrap） |
| Quick | `GaussDBPg -> MySQL` struct basic | `cargo test -p dt-tests --test integration_test -- gaussdb_to_mysql::struct_tests::test::struct_basic_test --nocapture` | GaussDBPg + MySQL8 | 对象同步主路径（bootstrap） |
| Quick | `GaussDBPg -> MySQL` check basic | `cargo test -p dt-tests --test integration_test -- gaussdb_to_mysql::check_tests::test::check_basic_test --nocapture` | GaussDBPg + MySQL8 | 反向对账主路径（bootstrap） |
| Quick | `GaussDBPg -> MySQL` CDC basic | `cargo test -p dt-tests --test integration_test -- gaussdb_to_mysql::cdc_tests::test::cdc_basic_test --nocapture` | GaussDBPg + MySQL8 | 基础 CDC 主路径（bootstrap） |
| Quick | `GaussDBPg -> MySQL` precheck basic | `cargo test -p dt-tests --test integration_test -- gaussdb_to_mysql::precheck_tests::test::struct_supported_basic_test --nocapture` | GaussDBPg + MySQL8 | 预检查主路径（bootstrap） |
| Quick | `GaussDBPg -> MySQL` bootstrap batch（script） | `bash scripts/e2e/gaussdb_to_mysql_bootstrap.sh` | GaussDBPg + MySQL8 | 一键回归（snapshot/struct/check/cdc/precheck） |
| Quick | `MySQL -> GaussDBMySQL` smoke | `cargo test -p dt-tests --test integration_test -- mysql_to_gaussdb_mysql::snapshot_tests::test::smoke_test --nocapture` | MySQL8 + GaussDBMySQL | 最小连通性 |
| Quick | `MySQL -> GaussDBMySQL` struct basic | `cargo test -p dt-tests --test integration_test -- mysql_to_gaussdb_mysql::struct_tests::test::struct_basic_test --nocapture` | MySQL8 + GaussDBMySQL | 对象同步主路径 |
| Quick | `MySQL -> GaussDBMySQL` check basic | `cargo test -p dt-tests --test integration_test -- mysql_to_gaussdb_mysql::check_tests::test::check_basic_test --nocapture` | MySQL8 + GaussDBMySQL | 对账主路径 |
| Quick | `MySQL -> GaussDBMySQL` CDC basic | `cargo test -p dt-tests --test integration_test -- mysql_to_gaussdb_mysql::cdc_tests::test::cdc_basic_test --nocapture` | MySQL8 + GaussDBMySQL | 基础 CDC 主路径 |
| Quick | `PG -> GaussDBOracle` snapshot smoke | `cargo test -p dt-tests --test integration_test -- pg_to_gaussdb_oracle::snapshot_tests::test::smoke_test --nocapture` | PG + GaussDBOracle | 最小连通性（oracle-mode） |
| Quick | `PG -> GaussDBOracle` struct basic | `cargo test -p dt-tests --test integration_test -- pg_to_gaussdb_oracle::struct_tests::test::struct_basic_test --nocapture` | PG + GaussDBOracle | 基础对象同步（oracle-mode） |
| Quick | `PG -> GaussDBOracle` check basic | `cargo test -p dt-tests --test integration_test -- pg_to_gaussdb_oracle::check_tests::test::check_basic_test --nocapture` | PG + GaussDBOracle | 基础对账（oracle-mode） |
| Full | `PG -> GaussDBPg` CDC basic | `cargo test -p dt-tests --test integration_test -- pg_to_gaussdb::cdc_tests::test::cdc_basic_test --nocapture` | PG + GaussDBPg | 正向 CDC |
| Full | `PG -> GaussDBPg` type matrix | `cargo test -p dt-tests --test integration_test -- pg_to_gaussdb::snapshot_tests::test::type_matrix_test --nocapture` | PG + GaussDBPg | 特有类型快照兼容 |
| Full | `PG -> GaussDBPg` struct view/routine | `cargo test -p dt-tests --test integration_test -- pg_to_gaussdb::struct_tests::test::struct_view_routine_test --nocapture` | PG + GaussDBPg | 视图/函数/过程 |
| Full | `GaussDBPg -> PG` check type matrix | `cargo test -p dt-tests --test integration_test -- gaussdb_to_pg::check_tests::test::type_matrix_test --nocapture` | GaussDBPg + PG | 特有类型反向对账 |
| Full | `GaussDBPg -> PG` struct view/routine | `cargo test -p dt-tests --test integration_test -- gaussdb_to_pg::struct_tests::test::struct_view_routine_test --nocapture` | GaussDBPg + PG | 反向对象同步 |
| Full | `GaussDBPg -> PG` CDC type matrix | `cargo test -p dt-tests --test integration_test -- gaussdb_to_pg::cdc_tests::test::cdc_type_matrix_test --nocapture` | GaussDBPg + PG | 特有类型 CDC |
| Full | `GaussDBPg -> PG` CDC resume | `cargo test -p dt-tests --test integration_test -- gaussdb_to_pg::cdc_tests::test::cdc_resume_test --nocapture` | GaussDBPg + PG | checkpoint 恢复 |
| Full | `MySQL -> GaussDBMySQL` CDC type matrix | `cargo test -p dt-tests --test integration_test -- mysql_to_gaussdb_mysql::cdc_tests::test::cdc_type_matrix_test --nocapture` | MySQL8 + GaussDBMySQL | CDC 类型矩阵 |
| Full | `MySQL -> GaussDBMySQL` CDC resume | `cargo test -p dt-tests --test integration_test -- mysql_to_gaussdb_mysql::cdc_tests::test::cdc_resume_test --nocapture` | MySQL8 + GaussDBMySQL | checkpoint 恢复 |
| Full | `GaussDBPg -> MySQL` struct advanced | `cargo test -p dt-tests --test integration_test -- gaussdb_to_mysql::struct_tests::test::struct_advanced_test --nocapture` | GaussDBPg + MySQL8 | default/index 映射覆盖 |
| Resilience | `GaussDBPg -> PG` CDC failover | `ENABLE_GAUSSDB_FAILOVER_TEST=1 cargo test -p dt-tests --test integration_test -- gaussdb_to_pg::cdc_tests::test::cdc_failover_test --nocapture` | GaussDB 集群 + SSH + PG | 切主自愈 |
| Resilience | `MySQL -> GaussDBMySQL` CDC failover（target self-heal） | `ENABLE_GAUSSDB_FAILOVER_TEST=1 cargo test -p dt-tests --test integration_test -- mysql_to_gaussdb_mysql::cdc_tests::test::cdc_failover_test --nocapture` | MySQL8 + GaussDB 集群 + SSH | 目标端切主自愈 |
| Resilience | script basic | `bash scripts/e2e/gaussdb_to_pg_cdc.sh` | `.local/e2e/.env` | 基础 e2e |
| Resilience | script resume | `TEST_RESUME=1 bash scripts/e2e/gaussdb_to_pg_cdc.sh` | `.local/e2e/.env` | kill + restart |
| Resilience | script failover | `TEST_FAILOVER=1 bash scripts/e2e/gaussdb_to_pg_cdc.sh` | `.local/e2e/.env` + CM SSH | 主备切换 |
| Resilience | script slot active negative | `TEST_NEG_SLOT_ACTIVE=1 bash scripts/e2e/gaussdb_to_pg_cdc.sh` | `.local/e2e/.env` | slot 冲突 fail-fast |
| Resilience | script no repl user negative | `TEST_NEG_NO_REPL_USER=1 bash scripts/e2e/gaussdb_to_pg_cdc.sh` | `.local/e2e/.env` | 权限 fail-fast |

## 5. 推荐执行顺序

### 5.1 日常开发回归

按 Quick Gate 顺序执行，优先看：

1. `GaussDBPg -> PG` CDC basic
2. `PG -> GaussDBPg` snapshot/struct/check basic
3. `GaussDBPg -> MySQL` snapshot/struct/check/cdc basic
4. `MySQL -> GaussDBMySQL` smoke/struct/check basic
5. `MySQL -> GaussDBMySQL` CDC basic

### 5.2 spec 合并前回归

执行 Quick Gate + Full Functional Gate。

重点看：

- type matrix 是否回归
- view/routine 双向 struct 是否被破坏
- `resume` 是否仍可恢复

### 5.3 发布前或 CDC 逻辑变更后

执行 Full Functional Gate + Resilience Gate。

重点看：

- `failover` 后是否自动重连
- `slot` / 权限负例是否仍 fail-fast
- e2e 脚本是否保持无污染

## 6. 证据归档建议

每次跑完整回归批次，建议至少保留：

- 执行命令清单
- 关键 PASS/FAIL 日志片段
- 若为 CDC：
  - `default.log`
  - `position.log`
  - failover / negative 专项日志
- 若为类型矩阵：
  - 差异前后的关键日志片段
  - 对应 fixture 说明

建议归档位置：

- taskmaster child 的 `raw/`
- 或 `.local/e2e/<timestamp>/`

## 7. 当前建议的第一轮统一回归批次

如果我们下一步要做“当前关键能力统一验证”，我建议分两批：

### Batch A：主路径回归

- `pg_to_gaussdb::snapshot_tests::test::snapshot_basic_test`
- `pg_to_gaussdb::struct_tests::test::struct_basic_test`
- `pg_to_gaussdb::check_tests::test::check_basic_test`
- `gaussdb_to_pg::snapshot_tests::test::snapshot_basic_test`
- `gaussdb_to_pg::check_tests::test::check_basic_test`
- `gaussdb_to_pg::cdc_tests::test::cdc_basic_test`
- `mysql_to_gaussdb_mysql::snapshot_tests::test::smoke_test`
- `mysql_to_gaussdb_mysql::struct_tests::test::struct_basic_test`
- `mysql_to_gaussdb_mysql::check_tests::test::check_basic_test`
- `mysql_to_gaussdb_mysql::cdc_tests::test::cdc_basic_test`

### Batch B：增强能力回归

- `pg_to_gaussdb::snapshot_tests::test::type_matrix_test`
- `pg_to_gaussdb::struct_tests::test::struct_view_routine_test`
- `gaussdb_to_pg::check_tests::test::type_matrix_test`
- `gaussdb_to_pg::struct_tests::test::struct_view_routine_test`
- `gaussdb_to_pg::cdc_tests::test::cdc_type_matrix_test`
- `gaussdb_to_pg::cdc_tests::test::cdc_resume_test`
- `mysql_to_gaussdb_mysql::cdc_tests::test::cdc_type_matrix_test`
- `mysql_to_gaussdb_mysql::cdc_tests::test::cdc_resume_test`
- `ENABLE_GAUSSDB_FAILOVER_TEST=1 cargo test -p dt-tests --test integration_test -- gaussdb_to_pg::cdc_tests::test::cdc_failover_test --nocapture`
- `ENABLE_GAUSSDB_FAILOVER_TEST=1 cargo test -p dt-tests --test integration_test -- mysql_to_gaussdb_mysql::cdc_tests::test::cdc_failover_test --nocapture`

这样拆分的好处是：

- Batch A 适合日常高频跑
- Batch B 适合阶段性收口
- failover 单独放在 Batch B，避免拖慢所有普通回归

## 8. 最近一次执行记录

- 2026-04-02：已执行 **Batch A**（9 条主路径）并全部 PASS。
  - 证据：`.codex-tasks/20260402-gaussdbpg-quality-coverage/tasks/20260402-05-quality-gate-evidence/raw/batch-a/summary.tsv`
- 2026-04-03：已执行 **Batch B + Resilience Gate**（混合结果）。
  - Batch B：`6/6 PASS`
  - Resilience：
    - `dt-tests cdc_failover_test`：`FAIL`
    - script matrix（`basic/resume/slot-active/no-repl-user/failover`）：`5/5 PASS`
  - 关键结论：
    - `GaussDBPg -> PG` 的增强回归（type matrix / struct view-routine / CDC type matrix / CDC resume）已全部通过。
    - 真实 e2e 脚本已经证明 failover 期间 CDC 可重连到新主并继续同步，且脚本收尾能把主库恢复回 `node 2 / 10.250.0.30`。
    - 当前红点集中在 `dt-tests` 的 failover restore 校验：真实切主成功，但测试内 restore 回原主阶段仍会因 `cm_ctl busy / convergence timeout` 失败。
  - 证据：
    - `.codex-tasks/20260403-gaussdb-gate-batchb-resilience/PROGRESS.md`
    - `.codex-tasks/20260403-gaussdb-gate-batchb-resilience/raw/batch-b/summary.tsv`
    - `.codex-tasks/20260403-gaussdb-gate-batchb-resilience/raw/resilience/summary.tsv`
- 2026-04-13：已在真实 HA 环境执行 `MySQL -> GaussDBMySQL` 的目标端 failover 自愈回归并 PASS。
  - 证据：`.codex-tasks/20260413-gaussdb-target-selfheal/PROGRESS.md`
