# GaussDB (PG-compatible) -> Postgres CDC 手动 E2E 测试指南（无污染版）

本文档用于手动验证 **GaussDB（PG 兼容模式）作为源端**、**Postgres 作为目标端** 的 CDC 同步链路是否可用，并确保测试前后环境保持“无污染”（不遗留测试表/复制槽/容器数据）。

> CDC：ape-dts 在 GaussDB 侧通过逻辑解码插件 `mppdb_decoding`（JSON 输出）进行增量抽取。
>
> 适用 db_type：`gaussdb_pg`

## 0. 命名约定（强烈建议照做）

为降低污染风险，本文使用固定的测试对象名，并在测试前后做强制清理：

- 源端/目标端 schema：`ape_dts_manual`
- 源端/目标端 table：`gaussdb_to_pg_cdc_basic`
- 复制槽（slot）名：每次测试都生成一个新的，例如 `ape_manual_gaussdb_to_pg_20260330_153000`

如果你担心多人共用环境互相影响，建议将 `schema/table/slot_name` 都加上个人前缀或时间戳，但要同步修改：

- `task_config.ini` 里的 `slot_name` 和 `do_tbs`
- 源端/目标端建表 SQL

## 快速开始（推荐脚本模式）

如果你希望“一键跑通 + 自动清理（无污染）”，可直接使用仓库内置脚本：

```bash
# 1) GaussDB HA 候选（SQL 端口列表，如 8000）
export gaussdb_pg_candidate_hosts="10.0.0.1:8000,10.0.0.2:8000,10.0.0.3:8000"

# 2) 明确主库（脚本会优先使用该地址作为 extractor.url 的 base endpoint）
export SRC_GAUSS_PRIMARY_HOSTPORT="10.0.0.3:8000"

# 3) GaussDB 账号信息（只用于本机运行，不要提交到仓库）
export SRC_GAUSS_USERNAME="<gauss_user>"
export SRC_GAUSS_PASSWORD="<gauss_pwd>"

# 4) 目标端 PG（默认用本机 Docker Postgres15:5434，可按需覆盖）
export DST_PG_URL="postgres://127.0.0.1:5434/postgres?options[statement_timeout]=10s"

# 5) 运行（成功/失败都会清理测试 schema/table/slot；日志输出到 .local/e2e/）
bash scripts/e2e/gaussdb_to_pg_cdc.sh
```

说明：

- 如果你已经自行启动了 `127.0.0.1:5434` 的 Postgres，可设置 `SKIP_DOCKER_PG=1` 跳过容器管理。
- 脚本会 drop `TEST_SCHEMA/TEST_TABLE/SLOT_NAME`（默认沿用本文命名），请勿指向生产库。

## 验证记录

- 2026-03-30：已完成手动验证通过（GaussDB 源端 + 本地 Docker Postgres15 目标端）。

## 1. 前置条件

### 1.1 本地工具

- `docker`（用于启动本地 Postgres 15）
- `psql`（用于源端/目标端执行 SQL）
- `cargo`（用于运行 `dt-main`）

### 1.2 GaussDB（源端）必要条件

需要满足（至少）：

- 可通过 PG wire protocol 连接（`psql` 能连上）
- 逻辑复制/解码相关能力可用：`wal_level=logical`、`max_replication_slots > 0`、`max_wal_senders > 0`
- 逻辑解码插件 `mppdb_decoding` 可用
- 测试账号建议具备 replication/创建 slot 的权限（具体以 DBA 策略为准）

可选（HA/主备集群推荐）：

- 配置环境变量 `gaussdb_pg_candidate_hosts`，用于让 ape-dts 自动选择可写主库并优先 HA 复制端口（通常为 `port+1`）：

```bash
export gaussdb_pg_candidate_hosts="10.0.0.1:8000,10.0.0.2:8000"
```

## 2. 测试前清理（确保无污染起点）

建议先准备一些变量，后续命令更好复制：

```bash
# 目标端（本地 Postgres 15）
# - DST_PG_URL: 用于 dt-main sinker（可包含 options[...] 等 URL query）
# - DST_PG_PSQL_URL: 用于 psql（建议不要带 URL query，避免部分客户端不识别）
export DST_PG_URL="postgres://127.0.0.1:5434/postgres?options[statement_timeout]=10s"
export DST_PG_PSQL_URL="postgres://postgres:postgres@127.0.0.1:5434/postgres"

# 源端（GaussDB PG 兼容模式）
# 注意：这里用示例占位，按你的真实环境替换
export SRC_GAUSS_URL="postgres://<gauss_user>:<gauss_pwd>@<gauss_host>:<gauss_port>/postgres"

export TEST_SCHEMA="ape_dts_manual"
export TEST_TABLE="gaussdb_to_pg_cdc_basic"
export SLOT_NAME="ape_manual_gaussdb_to_pg_$(date +%Y%m%d_%H%M%S)"
```

提示：

- 如果密码包含特殊字符，建议不要直接写进 URL，改用 `PGPASSWORD=... psql ...` 或 `.pgpass`。

### 2.1 清理源端（GaussDB）

1) 清理测试表/Schema：

```bash
psql "$SRC_GAUSS_URL" -v ON_ERROR_STOP=1 \
  -c "DROP TABLE IF EXISTS ${TEST_SCHEMA}.${TEST_TABLE};" \
  -c "DROP SCHEMA IF EXISTS ${TEST_SCHEMA} CASCADE;"
```

2) 如果你怀疑之前遗留了同名 slot，先检查并删除（如果不存在会跳过）：

```bash
psql "$SRC_GAUSS_URL" -v ON_ERROR_STOP=1 \
  -c "SELECT slot_name, active, restart_lsn, confirmed_flush_lsn FROM pg_replication_slots WHERE slot_name = '${SLOT_NAME}';"
```

说明：

- 本文推荐每次测试都用新的 `SLOT_NAME`，因此一般不需要“删旧 slot 才能跑”；
- 但测试结束时会按本文的 slot 名执行 drop，确保不残留。

### 2.2 清理目标端（本地 Postgres）

```bash
psql "$DST_PG_PSQL_URL" -v ON_ERROR_STOP=1 \
  -c "DROP TABLE IF EXISTS ${TEST_SCHEMA}.${TEST_TABLE};" \
  -c "DROP SCHEMA IF EXISTS ${TEST_SCHEMA} CASCADE;"
```

## 3. 启动目标端 Postgres 15（本机 Docker）

为了保证“无污染”，推荐用 **无数据卷** 的一次性容器（测试结束可直接删除容器清空数据）。

```bash
docker rm -f ape-dts-pg15 >/dev/null 2>&1 || true
docker run -d --name ape-dts-pg15 \
  -e POSTGRES_PASSWORD=postgres \
  -p 5434:5432 \
  postgres:15
```

等待数据库就绪后做一次连通性验证：

```bash
psql "$DST_PG_PSQL_URL" -c "SELECT version();"
```

## 4. 准备测试表（源端/目标端）

### 4.1 源端建表（GaussDB）

```bash
psql "$SRC_GAUSS_URL" -v ON_ERROR_STOP=1 \
  -c "DROP SCHEMA IF EXISTS ${TEST_SCHEMA} CASCADE;" \
  -c "CREATE SCHEMA ${TEST_SCHEMA};" \
  -c "CREATE TABLE ${TEST_SCHEMA}.${TEST_TABLE} (id INTEGER PRIMARY KEY, val TEXT);"
```

### 4.2 目标端建表（Postgres）

```bash
psql "$DST_PG_PSQL_URL" -v ON_ERROR_STOP=1 \
  -c "DROP SCHEMA IF EXISTS ${TEST_SCHEMA} CASCADE;" \
  -c "CREATE SCHEMA ${TEST_SCHEMA};" \
  -c "CREATE TABLE ${TEST_SCHEMA}.${TEST_TABLE} (id INTEGER PRIMARY KEY, val TEXT);"
```

## 5. 配置并启动 CDC 任务（ape-dts）

### 5.1 准备 `task_config.ini`

建议将配置放到本地目录，避免误提交。以下以仓库根目录下 `./.local/` 为例（目录不存在就创建）：

```bash
mkdir -p .local/manual
```

创建文件：`.local/manual/gaussdb_to_pg_cdc.ini`，内容示例（把 `<...>` 替换成真实值）：

```ini
[extractor]
db_type=gaussdb_pg
extract_type=cdc
url=postgres://<gauss_host>:<gauss_port>/postgres?options[statement_timeout]=10s
username=<gauss_user>
password=<gauss_pwd>

# 每次测试建议使用新的 slot_name，避免读取旧 WAL 历史造成“环境污染”
slot_name=<slot_name>
start_lsn=
recreate_slot_if_exists=false
keepalive_interval_secs=10

# 无污染建议：关闭 heartbeat（不额外创建/写入心跳表）
heartbeat_interval_secs=0
heartbeat_tb=

[filter]
do_dbs=
ignore_dbs=
do_tbs=ape_dts_manual.gaussdb_to_pg_cdc_basic
ignore_tbs=
do_events=insert,update,delete

[sinker]
db_type=pg
sink_type=write
url=postgres://127.0.0.1:5434/postgres?options[statement_timeout]=10s
username=postgres
password=postgres
batch_size=2

[router]
db_map=
tb_map=
col_map=

[parallelizer]
parallel_type=rdb_merge
parallel_size=1

[pipeline]
buffer_size=4
checkpoint_interval_secs=1

[runtime]
log_level=info
log4rs_file=./log4rs.yaml
log_dir=./logs/manual/gaussdb_to_pg_cdc
```

把上面的两处改成你实际使用的值：

- `slot_name=<slot_name>`：替换为你生成的 `SLOT_NAME`（建议按本文第 2 节的方式生成）
- `do_tbs=ape_dts_manual.gaussdb_to_pg_cdc_basic`：与本文创建的表保持一致

### 5.2 启动任务

在仓库根目录执行：

```bash
# 收到 SIGINT/SIGTERM 后允许排空缓冲并落位点的时间上限，默认 8 秒；
# 设得太小（例如 3）会让忙碌任务来不及收敛，进程以退出码 4 硬退且最后位点可能没落盘；
# 设成 0 表示不给任何窗口，任何信号都直接变成退出码 4。
export SHUTDOWN_TIMEOUT_SECS=8
cargo run -p dt-main -- .local/manual/gaussdb_to_pg_cdc.ini
```

启动后建议观察日志里是否出现：

- 复制槽创建/复用成功
- 开始 streaming（持续收到 `XLogData` 或开始消费 JSON 变更）
- sinker 持续写入目标端

## 6. 执行源端增删改（测试数据）

保持 `dt-main` 在运行，另开一个终端执行以下 SQL（源端 GaussDB）：

### 6.1 插入（INSERT）

```bash
psql "$SRC_GAUSS_URL" -v ON_ERROR_STOP=1 \
  -c "INSERT INTO ${TEST_SCHEMA}.${TEST_TABLE} (id, val) VALUES (1, 'a'), (2, 'b');"
```

### 6.2 更新（UPDATE）

```bash
psql "$SRC_GAUSS_URL" -v ON_ERROR_STOP=1 \
  -c "UPDATE ${TEST_SCHEMA}.${TEST_TABLE} SET val = 'c' WHERE id = 2;"
```

### 6.3 删除（DELETE）

```bash
psql "$SRC_GAUSS_URL" -v ON_ERROR_STOP=1 \
  -c "DELETE FROM ${TEST_SCHEMA}.${TEST_TABLE} WHERE id = 1;"
```

## 7. 目标端验证（Postgres）

在目标端查询并验证最终结果（允许有轻微延迟，可重试几次）：

```bash
psql "$DST_PG_PSQL_URL" -v ON_ERROR_STOP=1 \
  -c "SELECT * FROM ${TEST_SCHEMA}.${TEST_TABLE} ORDER BY id;"
```

期望结果：

- 只剩一行：`(2, 'c')`
- `id=1` 不存在

你也可以额外验证行数：

```bash
psql "$DST_PG_PSQL_URL" -v ON_ERROR_STOP=1 -c "SELECT count(*) FROM ${TEST_SCHEMA}.${TEST_TABLE};"
```

## 8. 测试后清理（确保无污染终点）

### 8.1 停止任务

回到运行 `dt-main` 的终端，按 `Ctrl+C` 退出。

> 若 slot 仍显示 active，通常说明还有连接未断开；稍等几秒或确认进程已退出后再执行 drop slot。

### 8.2 清理源端（drop table/schema + drop slot）

```bash
psql "$SRC_GAUSS_URL" -v ON_ERROR_STOP=1 \
  -c "DROP TABLE IF EXISTS ${TEST_SCHEMA}.${TEST_TABLE};" \
  -c "DROP SCHEMA IF EXISTS ${TEST_SCHEMA} CASCADE;" \
  -c "SELECT pg_drop_replication_slot(slot_name) FROM pg_replication_slots WHERE slot_name = '${SLOT_NAME}';"
```

验证 slot 不存在：

```bash
psql "$SRC_GAUSS_URL" -v ON_ERROR_STOP=1 \
  -c "SELECT slot_name, active FROM pg_replication_slots WHERE slot_name = '${SLOT_NAME}';"
```

### 8.3 清理目标端（drop table/schema）

```bash
psql "$DST_PG_PSQL_URL" -v ON_ERROR_STOP=1 \
  -c "DROP TABLE IF EXISTS ${TEST_SCHEMA}.${TEST_TABLE};" \
  -c "DROP SCHEMA IF EXISTS ${TEST_SCHEMA} CASCADE;"
```

### 8.4 清理目标端容器（可选，但推荐保证无污染）

```bash
docker rm -f ape-dts-pg15
```

## 9. 常见问题排查

### 9.1 目标端一直没数据

优先检查：

- `task_config.ini` 的 `do_tbs` 是否与实际表名一致
- 是否在启动 `dt-main` 之后才执行源端 DML（如果 DML 发生在 slot 创建之前，通常不会被 CDC 捕获）
- 源端是否满足逻辑复制条件（`wal_level=logical` 等）
- 账号是否具备创建/使用复制槽的权限

### 9.2 drop slot 失败（slot active 或权限不足）

- 确认 `dt-main` 进程已退出并断开连接
- 若仍 active，查询 `pg_stat_activity` 定位连接（需要权限）
- 权限不足则按 DBA 流程处理

### 9.3 HA/主备环境偶发 read-only / EOF / 连接波动

- 推荐设置 `gaussdb_pg_candidate_hosts`（见 1.2），让 ape-dts 自动挑选可写主库并优先复制端口
- 避免频繁 drop/recreate 同一个 slot；本文推荐“一次测试一个新 slot，结束即 drop”

### 9.4 replication 报错 `unexpected character '-'`

通常出现在 `START_REPLICATION SLOT <slot_name>` 阶段：当 `slot_name` 包含 `-` 等特殊字符且未被正确 quoting 时，会被服务端解析为语法错误。

建议：

- 优先使用仅包含字母/数字/下划线的 `slot_name`（例如 `ape_manual_gaussdb_to_pg_20260330_153000`）。
- 如果你在使用较新的 ape-dts 版本：slot_name 已在 `START_REPLICATION` 中进行 quoting，通常不会再触发该问题。
