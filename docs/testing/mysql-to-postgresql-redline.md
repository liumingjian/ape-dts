# MySQL → PostgreSQL 真实迁移红线

## 用途

该红线用于重大引擎变更后的人工验证，证明真实 `dt-main` 能完成 MySQL 8 到 PostgreSQL 15 的 Snapshot 与后续 CDC 数据同步。它不是 `TaskRunner` 的进程内测试，也不经过 Console。

唯一入口：

```bash
bash scripts/e2e/mysql_to_postgresql_redline.sh
```

宿主机只需 Docker（含 Compose）、Cargo 和标准 Bash 工具。SQL 由容器内的 `mysql` 与 `psql` 客户端执行。

## 公开测试边界

- **源端 Seam**：通过真实 MySQL SQL 接口创建数据并执行增、改、删。
- **引擎 Seam**：Snapshot 与 CDC 分别运行一个真实 `target/debug/dt-main <ini>` 进程。
- **目标端 Seam**：通过真实 PostgreSQL SQL 接口逐阶段查询和验证。

v1 固定测试一张预建目标表，字段覆盖 `BIGINT`、`VARCHAR`、`DECIMAL/NUMERIC(12,2)` 与可空字段。Console、结构迁移、DDL CDC、恢复、故障切换、类型矩阵和性能不在范围内。

## 场景与成功标准

Snapshot 初始数据为：

```text
1|ORD-001|Alice|100.50|created|<NULL>
2|ORD-002|Bob|220.00|created|snapshot row
3|ORD-003|Carol|19.99|created|will be deleted
```

Snapshot 必须以退出码 0 结束，且源端、目标端和上述独立固定字面量三方一致。随后脚本从 MySQL 显式读取 binlog 文件名与 position，再启动独立 CDC 进程；禁止从空位置或位置 0 静默开始。

CDC 按顺序执行：

1. 插入并删除 `id=9001` 探针，证明 CDC 完整通路已就绪；
2. 插入 `id=4` 并等待两端与 INSERT 固定期望一致；
3. 更新 `id=1` 的金额、状态和备注，并等待旧值消失；
4. 删除 `id=3` 并等待目标端删除；
5. 重新查询两端全表，与最终固定期望比较。

最终数据必须为：

```text
1|ORD-001|Alice|188.80|paid|cdc update
2|ORD-002|Bob|220.00|created|snapshot row
4|ORD-004|David|88.80|created|cdc insert
```

任何阶段超时、`dt-main` 异常退出或数据不一致都以非零退出码失败。查询统一按主键排序、无 Header、`NULL` 显示为 `<NULL>`，小数固定两位。

## Deadline

| 阶段 | 默认上限 |
|---|---:|
| 数据库就绪 | 90 秒 |
| Snapshot | 120 秒 |
| CDC 探针 | 60 秒 |
| 每个 CRUD 阶段 | 30 秒 |
| 最终一致性 | 60 秒 |
| CDC 进程停止 | 15 秒 |

相应环境变量可临时调整：`DOCKER_TIMEOUT_SECS`、`SNAPSHOT_TIMEOUT_SECS`、`CDC_PROBE_TIMEOUT_SECS`、`CRUD_TIMEOUT_SECS`、`FINAL_TIMEOUT_SECS`、`STOP_TIMEOUT_SECS`。

## 数据库就绪判定

MySQL 官方镜像在初始化期间会先起一个临时服务端、随后重启它。只探测一次 `SELECT 1` 会撞进重启前的窗口：探测成功、下一条建表语句却挂在 `ERROR 2002 ... through socket`。因此就绪同时要求两件事：

1. Compose healthcheck（`mysqladmin ping` / `pg_isready`）报 `healthy`（镜像未声明 healthcheck 时该项自动跳过）；
2. 两端连续 `DB_READY_STREAK_REQUIRED` 次（默认 3 次，间隔 `DB_READY_PROBE_INTERVAL_SECS`，默认 1 秒）探测均成功——重启会打断连击，从而不会被误判为就绪。

超时失败会把最后一次未通过的原因带进 `summary.md`（healthcheck 未 healthy / MySQL 未接受连接 / PostgreSQL 未接受连接），而不是笼统的一句「就绪超时」。

## 失败原因兜底

`die` 之外，脚本还挂了 `ERR` trap：任何被 `set -e` 直接带走的语句（裸的 `mysql_sql` heredoc、`compose` 调用、命令替换赋值）都会记录「阶段 + 失败命令 + 退出码 + 行号」。`summary.md` 的 `Reason` 在失败运行中优先取 `die` 的显式原因，其次取 trap 记录，绝不再出现空白或 `none`。

## 隔离、产物与清理

每次运行使用唯一 Compose Project，MySQL 和 PostgreSQL 只绑定 `127.0.0.1` 动态端口，不声明固定容器名、网络名或持久卷。默认退出时先采集诊断并停止 CDC，再执行 `docker compose down -v --remove-orphans`。

产物保存在：

```text
.local/e2e/mysql-to-postgresql/<run-id>/
```

主要内容包括 Snapshot/CDC INI、进程 stdout/stderr、引擎日志、Compose 状态、数据库日志、各阶段两端 Dump、Diff、MySQL binlog 起点和 `summary.md`。

需要保留数据库环境排查时运行：

```bash
KEEP_ENV=1 bash scripts/e2e/mysql_to_postgresql_redline.sh
```

该模式仍会停止 `dt-main`，只保留数据库容器，并打印 Compose Project、动态端口和显式清理命令。不要把 `KEEP_ENV=1` 用作常规运行方式。

## 故障排查

1. 先查看产物目录中的 `summary.md`，确认失败阶段和进程退出码；
2. 查看对应阶段的 `dumps/*.tsv` 与 `diffs/*.diff`；
3. 检查 `snapshot.stderr.log`、`cdc.stderr.log` 和 `engine-logs/`；
4. 数据库启动或连接失败时检查 `docker/compose-ps.log`、`docker/mysql.log` 和 `docker/postgresql.log`；
5. CDC 未启动时确认 `mysql-master-status.tsv` 非空且包含合法文件名和 position。

如果首次运行需要拉取镜像，下载时间不计入脚本内部数据库就绪 Deadline，但会受调用方自己的命令执行超时影响。
