# GaussDB PG 兼容模式联调 Runbook（MVP）

> 适用范围：`DbType::GaussDBPg`（配置值 `gaussdb_pg`），MVP 主路径为 **MD5 认证**。

## 1. 环境准备（GaussDB 侧）

### 1.1 认证（MD5）

- GaussDB 默认可能启用非标准 SHA256 认证，标准 PG 驱动无法连接。
- MVP 要求 GaussDB 侧启用 MD5 认证：
  - `password_encryption_type=1`
- 重新设置用户密码使其按 MD5 规则存储（若环境变更需以 DBA 指令为准）。

### 1.2 CDC 前置条件（逻辑复制）

CDC（`GaussDB -> PG`）要求：

- `wal_level=logical`
- `max_replication_slots > 0`
- `max_wal_senders > 0`
- `mppdb_decoding` 可用（扩展/插件存在且允许创建逻辑复制槽）
- 建议使用具备 replication 权限的账号

可参考 precheck 输出项（`dt-precheck` 会检查关键参数，并对 GaussDBPg 增加 `mppdb_decoding` 可用性检查）。

### 1.3 CDC 协议参考驱动（官方）

- replication 子协议（keepalive/status update 端序与布局）以官方驱动 `resources/gsjdbc4.jar` 为准对齐。
- 说明：仅作为“行为参考/回归对照”，ape-dts 运行时不依赖该 JAR。

## 2. 配置模板

- `PG -> GaussDB`：`docs/templates/pg_to_gaussdb.md`
- `GaussDB -> PG`：`docs/templates/gaussdb_to_pg.md`

重点差异：

- GaussDB PG 兼容模式统一使用 `db_type=gaussdb_pg`
- 连接 URL 仍使用 `postgres://...`（PG wire protocol）
- GaussDB CDC 不走 PG 的 `publication + pgoutput`，而是走 `mppdb_decoding`（JSON）
- `PG <-> GaussDBPg` struct/check 的验收口径为“逻辑结构等价”（见 2.1），不要求 `pg_catalog` 物理字段逐项一致。

### 2.1 Struct 验收口径（PG <-> GaussDBPg）

- **原则**：只比较 DDL 直接相关且 GaussDB 可稳定提供的逻辑元数据；不比较 PostgreSQL 专有/物理差异字段。
- **归一化**：
  - indexdef 中 `USING ubtree` 归一为 `USING btree`；
  - 不比较底层 access method / tablespace / replica identity / row security 等物理差异。
- **保底**：当两端都是 `pg` 时仍保持严格对比语义（不因 GaussDB 兼容而降级）。

## 3. dt-tests 联调（推荐）

### 3.1 配置环境变量

`dt-tests` 会从 `dt-tests/tests/.env` 读取 URL，占位可通过 `dt-tests/tests/.env.local` 覆盖。

建议在 `.env.local` 设置（示例变量名）：

- `gaussdb_pg_extractor_without_auth_url`
- `gaussdb_pg_extractor_username`
- `gaussdb_pg_extractor_password`
- `gaussdb_pg_sinker_without_auth_url`
- `gaussdb_pg_sinker_username`
- `gaussdb_pg_sinker_password`

如 GaussDB 为主备/集群环境，建议额外设置候选节点（用于自动选择可写主库，避免 VIP/LB 混连导致的 `read-only transaction` / EOF 波动）：

```bash
export gaussdb_pg_candidate_hosts="10.0.0.1:8000,10.0.0.2:8000,10.0.0.3:8000"
```

说明：

- 一旦设置候选列表，将**优先使用候选**逐个探测 `pg_is_in_recovery=false` 并选择 read-write 主库端点；
  extractor URL 中的 base host/port 仅作为“全部候选失败”的最后兜底，避免 VIP/LB 漂移导致 SQL/replication 跨节点抖动。
- 连接成功后会记录“上次成功端点（host, sql_port）”，后续重连优先尝试该端点以减少反复探测与 standby 噪音。
- CDC replication 连接会使用 HA 端口（通常为 `port+1`），并默认 `sslmode=disable`（NoTLS）；
  仅当服务端明确要求 SSL 时才回退到 TLS。

CDC 解码与失败策略（MVP）：

- CDC MVP 只支持 DML（`INSERT/UPDATE/DELETE`）事件；若遇到 DDL/对象事件或未知 `op_type`，会 **fail fast** 并在错误信息中提示可能原因与建议动作（先做 struct 同步/避免在线 DDL/提供 raw 样本扩展 decoder）。
- JSON 解析失败/字段缺失等错误会在日志中打印 `LSN + category + raw_snippet(<=200)` 便于定位，但不会无限制输出整行原始内容。

### 3.2 运行用例

已新增用例骨架：

- `PG -> GaussDB`：`dt-tests/tests/pg_to_gaussdb/`（snapshot/struct/check）
- `GaussDB -> PG`：`dt-tests/tests/gaussdb_to_pg/`（snapshot/cdc/check）

示例：

```bash
# 仅示例：需要真实可连通的 GaussDB/PG 实例
cargo test -p dt-tests --test integration_test -- pg_to_gaussdb::snapshot_tests::test::snapshot_basic_test --nocapture
cargo test -p dt-tests --test integration_test -- gaussdb_to_pg::cdc_tests::test::cdc_basic_test --nocapture
ENABLE_GAUSSDB_FAILOVER_TEST=1 cargo test -p dt-tests --test integration_test -- gaussdb_to_pg::cdc_tests::test::cdc_failover_test --nocapture
ENABLE_GAUSSDB_FAILOVER_TEST=1 cargo test -p dt-tests --test integration_test -- mysql_to_gaussdb_mysql::cdc_tests::test::cdc_failover_test --nocapture
```

说明：

- `cdc_failover_test` 会在测试内部完成 `cm_ctl switchover`、等待 CDC 自动重连到新主 HA 端口，并在结束时 best-effort 恢复原主。
- 若 GaussDB 使用 `gaussdb_pg_candidate_hosts`，测试侧源端读写会按候选解析当前 RW 主库，避免比较连接在切主后卡在旧主导致误报。
- 对 `MySQL -> GaussDBMySQL`，failover 发生在**目标端**（pg-wire GaussDB MySQL-compatible DB），需要配置 `gaussdb_pg_candidate_hosts` 才能在无 VIP/LB 的情况下实现写入池切换与自愈。

### 3.3 CDC P1 演练（Resume + Failover + 负例，真实环境）

为验证“切主不应破坏实时同步任务，可允许短暂异常但需自愈”的稳定性目标，我们提供脚本化演练入口：

- `scripts/e2e/gaussdb_to_pg_cdc.sh`（无污染：结束自动清理 slot/schema/table，并 best-effort 切回原主）

建议通过本地 gitignored 环境文件提供参数：

- `.local/e2e/.env`（严禁提交到 git）
  - `gaussdb_pg_candidate_hosts`：GaussDB SQL 端口候选（例：`host:8000,...`）
  - `SRC_GAUSS_URL`：源端连接串（可含 userinfo；脚本会自动解析）
  - `DST_PG_URL`：目标 PG（建议使用本机 docker 5434）
  - `GAUSSDB_CM_SSH_PASSWORD`：failover 需要（仅本地环境变量；不要写入任何可提交文件）

可选开关：

- `GAUSSDB_CM_REQUIRE_HEALTHY=1`：要求集群完全健康（否则默认只要存在健康 standby 就允许演练）
- `GAUSSDB_CM_ENV_FILE=~/gauss_env_file`（默认如此）
- `GAUSSDB_CM_RUBY_USER=Ruby`（默认如此）

运行方式（每次只开一个开关，避免多阶段歧义）：

```bash
# basic
bash scripts/e2e/gaussdb_to_pg_cdc.sh

# resume (kill + restart from checkpoint LSN)
TEST_RESUME=1 bash scripts/e2e/gaussdb_to_pg_cdc.sh

# negative: slot active (2nd dt-main + precheck)
TEST_NEG_SLOT_ACTIVE=1 bash scripts/e2e/gaussdb_to_pg_cdc.sh

# negative: no replication privilege user (precheck fail-fast)
TEST_NEG_NO_REPL_USER=1 bash scripts/e2e/gaussdb_to_pg_cdc.sh

# failover: cm_ctl switchover + dt-main 自愈
TEST_FAILOVER=1 bash scripts/e2e/gaussdb_to_pg_cdc.sh
```

Failover 执行细节（CM，供脚本/人工对照）：

- switchover **必须在当前主 DN 的主机上执行**（否则可能失败）。
- 远端执行环境（示例）：
  - `ssh root@<primary_host>`
  - `su - Ruby && source ~/gauss_env_file`
- 切换命令（以 dn instance 为准）：
  - 切换到 n1：`cm_ctl switchover -n 1 -D/data/cluster/var/lib/engine/data1/data/dn_6001`
  - 切换到 n2：`cm_ctl switchover -n 2 -D/data/cluster/var/lib/engine/data1/data/dn_6002`
  - 切换到 n3：`cm_ctl switchover -n 3 -D/data/cluster/var/lib/engine/data1/data/dn_6003`
- 验证（推荐与脚本一致做法）：
  - `cm_ctl query -Cv | grep -A5 "Datanode State"`
  - 输出中包含 `"Primary Normal"` 的节点即为当前主节点。
- 常见阻断（环境问题）：
  - `cluster_state: Degraded` 或存在 `Down` 节点时，switchover 可能失败（如 “candidate to be promoted timeout” / “another command ... is running”）。
  - 建议先恢复集群到 `Normal` 再执行 `TEST_FAILOVER=1`。

证据位置（本地，不提交）：

- `.local/e2e/gaussdb_to_pg_cdc_<timestamp>/`
  - `logs/default.log`：候选选主、HA 端口、NoTLS、切主后重连证据
  - `logs/position.log`：checkpoint_position LSN（resume）
  - `precheck_*.log`：负例的 fail-fast 报错证据

## 4. 手工联调建议（证据归档）

建议对 MVP 的 4 组联调证据分别归档（日志/样本/对比结果）：

1. `PG -> GaussDB snapshot`
2. `PG -> GaussDB struct/check`
3. `GaussDB -> PG snapshot/check`
4. `GaussDB -> PG cdc`

建议至少保留：

- 任务 `task_config.ini`
- 任务日志（`log_dir` 下）
- CDC 场景：
  - 复制槽信息（slot name / restart_lsn / confirmed_flush_lsn）
  - 原始 `mppdb_decoding` 输出样本（JSON 行）
  - 若出现解析失败，截取导致失败的原始行（用于扩展 decoder 兼容层）

## 5. 常见问题排查（速查）

### 5.1 连接认证失败（疑似 SHA256）

现象：

- 客户端报错认证方式不兼容/无法握手

处理：

- 确认 GaussDB 侧已启用 MD5：`password_encryption_type=1`
- 重新设置用户密码后重试连接

### 5.2 Precheck 提示 `mppdb_decoding` 不可用

处理：

- 确认扩展/插件存在且对当前用户可见
- 确认实例允许创建 logical replication slot

### 5.3 复制槽已存在 / 无法创建

处理：

- 优先复用 slot：更换 `slot_name`，或保持 `recreate_slot_if_exists=false` 并继续运行（推荐）
- 仅在确认需要重置 slot（可能导致从新位点开始，通常需要重做 snapshot）时，才设置 `recreate_slot_if_exists=true`
- 若需要手工清理，按 DBA 规范 drop 对应 slot

### 5.4 CDC 无数据 / 位点不推进

处理：

- 确认 `wal_level=logical` 且写入发生在被同步的表集合中（filter 生效）
- 确认账号具备 replication 权限
- 打开日志，检查复制流是否持续收到 `mppdb_decoding` 输出
