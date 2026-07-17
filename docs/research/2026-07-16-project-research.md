# APE-DTS 项目调研报告

> 调研日期：2026-07-16
>
> 文档定位：这是该日期的静态调研快照，不是当前能力或缺陷的权威清单；后续提交可能已经修复或改变文中的结论
>
> 调研范围：项目整体架构、核心数据链路、数据库连接器、快照与 CDC、任务编排与监控、Web 控制台、测试体系、构建发布及已识别风险
>
> 调研方式：由 5 个只读子代理分别对架构、数据平面、编排监控、前端、测试交付进行并行代码调研，再统一汇总
>
> 说明：本报告未修改项目业务代码；报告中的“风险”是基于静态代码阅读得到的待验证结论，不等同于已复现缺陷

---

## 1. 执行摘要

APE-DTS 是一个以 Rust 实现的数据迁移与同步系统，支持快照迁移、CDC 增量同步、数据校验与修订、结构迁移等任务。项目同时包含一套基于 Vue 3 的管理控制台，以及负责持久化任务、启动引擎子进程、采集指标和管理运行生命周期的 Actix Web 后端。

系统可以用两种方式运行：

1. **独立引擎模式**：直接执行 `dt-main <task_config.ini>`。
2. **控制台托管模式**：浏览器访问 Vue SPA，由 `dt-console-server` 管理任务并为每次运行生成 INI 配置，再启动独立的 `dt-main` 子进程。

核心数据链路如下：

```text
数据源
  │
  ▼
Extractor（抽取器）
  │
  ▼
DtQueue（有界内存队列）
  │
  ▼
Pipeline + Parallelizer（流水线与并行策略）
  │
  ▼
Sinker（写入器）
  │
  ▼
目标端
```

控制台托管模式的运行拓扑如下：

```text
Vue 3 SPA
  │ HTTP / SSE，统一访问 /api
  ▼
dt-console-server（Actix Web）
  ├── SQLite：任务、运行、用户、告警、指标等
  ├── INI 渲染
  ├── 子进程管理
  ├── Prometheus 指标抓取
  ├── 日志与告警 SSE
  └── 运行恢复与孤儿进程处理
        │
        ▼
      dt-main（每次运行一个独立进程）
        │
        ▼
      数据迁移引擎
```

### 1.1 综合判断

项目已经形成较清晰的分层结构：

- `dt-common` 提供配置、元数据、位置、监控等基础能力；
- `dt-connector` 封装数据库读写；
- `dt-parallelizer` 负责数据分区和并行写入；
- `dt-pipeline` 负责队列消费、转换、写入和检查点推进；
- `dt-task` 负责组装并运行上述组件；
- `dt-console-server` 负责管理面和进程生命周期；
- `web-prototype` 提供用户界面。

当前项目的主要技术复杂度集中在以下方面：

1. 快照并行切分与保守检查点推进；
2. CDC 事务边界、LSN/位点确认和断线恢复；
3. GaussDB 多兼容模式、协议差异及 HA 切换；
4. 控制台与引擎子进程的信号、状态和指标协同；
5. 前端、真实后端与 MSW Mock 三套接口契约的一致性；
6. 数据库集成测试规模较大，但未进入常规 CI。

---

## 2. 项目定位与能力范围

项目根目录 `README.md` 将 APE-DTS 定义为支持任意源到任意目标数据传输的数据迁移工具，强调轻量、独立运行和云原生部署能力。

从代码、测试目录和现有文档综合看，项目覆盖的任务类型包括：

- 全量快照迁移；
- CDC 增量同步；
- 快照后继续 CDC 的两阶段任务；
- 数据检查、修订与复核；
- 表、索引、视图、例程等结构迁移；
- 数据过滤、库表列映射；
- Lua 数据处理；
- 日志或数据库检查点恢复；
- HTTP 拉取式流水线；
- 管理控制台、RBAC、告警、审计与许可证管理。

代码与测试中出现的数据库或数据系统包括：

- MySQL；
- PostgreSQL；
- GaussDB PostgreSQL/MySQL/Oracle 兼容模式；
- Oracle；
- MongoDB；
- Redis；
- Kafka；
- ClickHouse；
- StarRocks；
- Doris；
- Foxlake；
- 其他通过测试或目标端适配体现的系统。

需要注意，顶层 README 中的支持矩阵落后于当前代码和测试，不能作为完整的能力清单。

---

## 3. 仓库与模块结构

Cargo Workspace 在 `Cargo.toml` 中声明了 9 个成员。

| 模块 | 主要职责 |
|---|---|
| `dt-main` | 引擎 CLI 入口；读取配置并运行预检查或任务 |
| `dt-common` | 配置、错误、限流、日志、元数据、队列、位点、监控等共享能力 |
| `dt-connector` | 各类数据源抽取器、目标端写入器、元数据管理和 SQL 生成 |
| `dt-parallelizer` | 串行、快照、分区、合并、检查等并行策略 |
| `dt-pipeline` | 队列消费、Lua 处理、写入分发、检查点记录 |
| `dt-task` | 创建并编排连接器、并行器和流水线 |
| `dt-precheck` | 数据源和目标端预检查 |
| `dt-tests` | 数据库集成测试、Fixture 和辅助程序 |
| `dt-console-server` | 管理 API、SQLite 持久化、任务执行、监控和告警 |

前端独立位于 `web-prototype`，主要技术栈包括：

- Vue 3；
- TypeScript；
- Vite；
- Pinia；
- Vue Router；
- Element Plus；
- ECharts；
- vue-i18n；
- MSW；
- Vitest；
- Playwright。

### 3.1 模块依赖关系

核心 Rust 模块总体上按以下方向依赖：

```text
dt-main
  ├── dt-task
  └── dt-precheck

dt-task
  ├── dt-common
  ├── dt-connector
  ├── dt-parallelizer
  └── dt-pipeline

dt-pipeline
  ├── dt-common
  ├── dt-connector
  └── dt-parallelizer

dt-parallelizer
  ├── dt-common
  └── dt-connector

dt-connector
  └── dt-common
```

`dt-console-server` 复用 `dt-common`、`dt-connector` 和 `dt-precheck` 中的类型与能力，同时作为独立管理进程运行。

---

## 4. 引擎启动与任务编排

### 4.1 `dt-main` 入口

入口文件为 `dt-main/src/main.rs`，主要流程如下：

1. 设置 `RUST_BACKTRACE=1`；
2. 注册 Ctrl-C 超时退出逻辑；
3. 从第一个命令行参数读取 INI 路径；
4. 如果配置可解析为预检查配置，则执行预检查；
5. 否则创建 `TaskRunner` 并执行 `start_task()`；
6. 配置创建失败和任务执行失败使用不同退出码。

`dt-main` 本质上是**单任务、单进程**引擎，而不是长期驻留的任务调度服务。

### 4.2 `TaskRunner`

`dt-task/src/task_runner.rs` 中的 `TaskRunner` 是引擎核心编排器，负责：

- 加载 `TaskConfig`；
- 根据 extractor/sinker 类型推导任务类型；
- 创建全局、组件和任务级监控器；
- 构建路由器；
- 构建恢复器与位点记录器；
- 创建源端和目标端连接客户端；
- 创建入队和出队限流器；
- 创建抽取器、写入器、并行器和流水线；
- 启动并监督 extractor、pipeline 和 monitor 异步任务；
- 在任务结束时关闭连接并输出汇总。

### 4.3 单任务与多任务模式

`TaskRunner` 对不同任务采用两种运行模型。

#### 单任务模式

主要用于 CDC、结构迁移以及不需要按表拆分的任务：

1. 创建 `DtQueue`；
2. 创建共享的 `shut_down` 原子标志；
3. 创建记录接收位点和提交位点的 `Syncer`；
4. 创建 extractor、sinkers 和 pipeline；
5. 分别启动抽取、流水线、监控异步任务；
6. 任一核心任务失败时设置 shutdown，推动其他任务退出；
7. 等待任务结束并清理监控注册。

#### 多任务模式

主要用于按表并行的快照和检查任务：

- 先生成待执行的表任务；
- 使用 Semaphore 限制表级并发数；
- 使用 `JoinSet` 动态补充任务；
- 任一子任务失败会使整体任务失败；
- 全局监控任务在所有子任务结束后完成最终刷新。

### 4.4 异步任务泄漏防护

`TaskRunner` 使用 `AbortGuard` 防止外层 `start_task()` Future 被取消后，内部 extractor、pipeline 或 monitor 任务继续运行。Guard 在未解除武装时析构，会设置 shutdown 并 abort 已登记的 JoinHandle。

这是项目中较重要的生命周期安全设计，尤其适用于集成测试、超时和外层任务取消场景。

---

## 5. 统一数据模型与队列

核心数据类型定义在 `dt-common/src/meta/dt_data.rs`。

`DtItem` 包含：

- `dt_data`：具体数据；
- `position`：来源位点；
- `data_origin_node`：来源节点，用于标记与循环同步控制。

`DtData` 的主要变体包括：

- `Struct`：结构定义；
- `Ddl`：DDL；
- `Dcl`：DCL；
- `Dml`：行级变更；
- `Begin` / `Commit`：事务边界；
- `Heartbeat`：心跳；
- `Redis`；
- `Foxlake`。

所有 extractor 将数据库特有事件转换为统一的 `DtItem`，再交由公共流水线处理。

### 5.1 位点模型

`dt-common/src/meta/position.rs` 中的 `Position` 支持：

- RDB 快照位置；
- 快照完成标志；
- MySQL CDC 位点；
- PostgreSQL CDC 位点；
- MongoDB CDC 位点；
- Redis 位点；
- Foxlake S3 位点。

PostgreSQL 和 GaussDB CDC 共用 `PgCdc { lsn, timestamp }`，说明 GaussDB CDC 在位点层被视为 PostgreSQL 风格的 LSN 流。

---

## 6. 流水线与并行处理

### 6.1 `BasePipeline`

`dt-pipeline/src/base_pipeline.rs` 实现主流水线循环。

循环退出条件是：

```text
shutdown 已设置，并且队列已经排空
```

每轮主要执行：

1. 更新队列长度和容量监控；
2. 通过 parallelizer 从队列批量取数；
3. 根据数据类型选择 sink 方法；
4. 对 DML 应用可选 Lua 处理器；
5. 调用一个或多个 sinker；
6. 更新最后接收位点和最后提交位点；
7. 按时间间隔记录检查点；
8. 更新写入条数、字节和耗时等指标。

### 6.2 写入方法选择

流水线根据 `DtData` 类型选择：

- 结构数据：`sink_struct`；
- DDL：`sink_ddl`；
- DCL：`sink_dcl`；
- 普通关系型 DML：`sink_dml`；
- Redis/Foxlake 等原始数据：`sink_raw`；
- Begin、Commit、Heartbeat 通常不直接写入目标端，但参与位点推进。

### 6.3 并行策略

`dt-task/src/parallelizer_util.rs` 根据配置创建不同策略，包括：

- Snapshot；
- RDB Partition；
- RDB Merge；
- RDB Check；
- Serial；
- Table；
- Mongo；
- Redis；
- Foxlake。

`BaseParallelizer` 会尽量保持批次内数据同质，例如避免 DDL 与 DML 混杂，也会区分 SQL 类型和来源节点。

### 6.4 基于键的分区

`dt-parallelizer/src/rdb_partitioner.rs` 基于表元数据中的分区列计算哈希，将行分发到不同 sinker。

更新操作存在专门的正确性保护：

- 如果更新修改了主键或唯一键，分区器会拒绝安全并行；
- 在没有键映射时，如果更新修改了分区列，也会拒绝分区。

该约束用于避免同一逻辑行被分发到不同 worker 后产生乱序或唯一约束冲突。

---

## 7. 快照迁移

### 7.1 PostgreSQL 快照抽取

`PgSnapshotExtractor` 的主要流程：

1. 从 `PgMetaManager` 获取表元数据；
2. 验证用户指定的分区列；
3. 创建 `PgSnapshotSplitter`；
4. 如果并发数较低且没有显式分区列，使用串行批量提取；
5. 否则进行并行切块提取；
6. 每个批次或切块携带快照位置；
7. 通过流水线写入并记录恢复点。

### 7.2 快照切分

`PgSnapshotSplitter` 支持：

- 单列切分；
- 检查列类型是否可切分；
- 使用 `pg_class.reltuples` 估算表行数；
- 整数列优先使用均匀区间；
- 均匀切分不适用时回退到非均匀切分；
- 对并发完成的切块按 `checkpoint_id` 顺序推进检查点。

这种设计偏向恢复正确性：后面的切块即使先完成，也要等前面的切块完成后才能推进全局安全检查点。

### 7.3 快照并行写入

`SnapshotParallelizer` 将批次按向量位置切分给多个 sinker。该层不是按业务主键进行分区，而是简单地将快照批次均衡切片。

---

## 8. PostgreSQL CDC

PostgreSQL CDC 由 `PgCdcClient` 和 `PgCdcExtractor` 实现。

### 8.1 复制连接准备

`PgCdcClient` 会：

- 解析 PostgreSQL URL；
- 以 `replication=database` 建立复制连接；
- 在缺失时创建 publication；
- 检查或创建使用 `pgoutput` 的逻辑复制槽；
- 执行 `START_REPLICATION SLOT ... LOGICAL ...`。

### 8.2 事件处理

`PgCdcExtractor` 处理：

- Relation；
- Begin；
- Commit；
- Insert；
- Update；
- Delete；
- Primary Keepalive。

Relation 消息用于建立表和列类型元数据；行事件被解码成统一 `RowData` 后推入队列。

### 8.3 DDL 捕获方式

PostgreSQL 原生逻辑复制流不直接提供完整 DDL。项目通过可配置的 `ddl_meta_tb` 元数据表间接捕获 DDL：对该表的插入事件进行特殊解码，转换为 DDL 数据。

### 8.4 LSN 确认

Extractor 向 PostgreSQL 发送的反馈 LSN 来自 `Syncer` 中已经提交的位点，而不是仅仅收到的位点。这确保源端确认不会越过目标端已安全写入的位置。

---

## 9. GaussDB 兼容与 CDC

### 9.1 三层兼容模型

项目没有简单地把 GaussDB 等同于 PostgreSQL，而是区分：

1. **任务语义类型 `DbType`**：如 `gaussdb_pg`、`gaussdb_mysql`、`gaussdb_oracle`；
2. **Wire Protocol**：MySQL 或 PostgreSQL；
3. **SQL 兼容模式**：P、M、A。

该模型允许例如“GaussDB MySQL 兼容模式，但通过 PostgreSQL Wire Protocol 访问”的组合。

### 9.2 GaussDB CDC 客户端

`GaussDBCdcClient` 具备以下能力：

- 使用 SQL 连接执行预检查与复制槽管理；
- 使用复制连接读取 CDC 流；
- 从环境变量读取候选主机；
- 记忆最近成功连接的节点；
- 拒绝连接只读或备节点；
- 检查 `wal_level=logical`；
- 使用 `protocolVersion=351`；
- 使用 SQL 端口建立管理连接；
- 使用 `SQL 端口 + 1` 作为 HA 复制端口；
- 创建使用 `mppdb_decoding` 的逻辑复制槽；
- 断线后执行指数退避重连。

### 9.3 GaussDB CDC 解码

`GaussDBJsonDecoder` 处理 `mppdb_decoding` 输出的 JSON 行，当前主要支持：

- BEGIN；
- COMMIT；
- INSERT；
- UPDATE；
- DELETE。

列信息通过 `columns_name`、`columns_type`、`columns_val` 和 `old_keys_*` 等字段恢复为统一行数据。

### 9.4 GaussDB 类型兼容

PostgreSQL 类型注册表会从 `pg_catalog.pg_type` 加载 OID 和类型名，并对部分 GaussDB 别名进行归一化，例如：

- `int1` / `tinyint` → `int2`；
- `smalldatetime` → `timestamp`；
- `nvarchar2` → `varchar`；
- `clob` → `text`；
- `blob` → `bytea`。

未知类型在部分路径中会回退为字符串表示，因此“可读取”不一定等于“可以按原生类型无损写入”。

---

## 10. 目标端写入与结构处理

### 10.1 `PgSinker`

PostgreSQL 和多个 GaussDB 目标模式复用 `PgSinker`。

DML 处理策略包括：

- Insert/Delete 尽量批量执行；
- 其他行类型串行执行；
- GaussDB 目标端遇到特定故障转移错误时重试；
- 通过共享连接池、重连锁和最近成功节点减少切换期间的竞争。

### 10.2 GaussDB 目标端自愈

GaussDB 写入端会：

- 根据候选主机重新建池；
- 检查 `pg_is_in_recovery`；
- 检查 `transaction_read_only` 与 `default_transaction_read_only`；
- 只选择可写节点；
- 对特定错误文本识别故障转移场景。

当前故障类型判断包含字符串匹配，因此对驱动错误文本和服务端版本存在一定依赖。

### 10.3 DDL 写入

`PgSinker` 对 DDL：

- 为每条 DDL 打开新的单连接池；
- 对大部分 schema 级对象设置 `search_path`；
- 执行转换后的 SQL；
- 执行后使相关目标表元数据缓存失效。

### 10.4 结构抽取

PostgreSQL/GaussDB 结构抽取器可处理：

- Schema；
- Table；
- Sequence 与 Owner；
- Constraint；
- Index；
- Comment；
- Routine；
- View；
- Materialized View。

需要注意，路由器目前主要重写对象头部的 schema/table 名，不会自动重写视图或例程定义内部引用的原始对象名。

---

## 11. 检查点与恢复机制

### 11.1 检查点语义

`BasePipeline` 同时跟踪：

- 最后接收位置；
- 最后提交位置。

对于 CDC：

- 普通 DML 推进接收位置；
- Commit/Heartbeat 推进提交位置；
- 持久化检查点优先使用提交位置；
- 没有提交位置时才使用当前接收位置。

这种设计使恢复点尽量落在安全事务边界。

### 11.2 数据库恢复

数据库记录器会创建默认元数据表，并按以下逻辑 Upsert：

```text
(task_id, resumer_type, position_key) → position_data
```

恢复器启动时将记录加载到内存映射中，用于判断：

- 某张表是否完成快照；
- 某张表从哪个位置继续快照；
- CDC 从哪个 LSN 或日志位置继续。

### 11.3 日志恢复

日志恢复读取：

- `position.log`；
- `finished.log`；
- 可选显式恢复配置。

快照恢复可以选择当前位置或检查点位置；CDC 优先采用检查点位置。

当前 `position.log` 只读取末尾 200 行。表数量较多或检查点输出频繁时，较早但仍需要的表位置可能不在读取范围内。

---

## 12. 控制台后端

### 12.1 启动流程

`dt-console-server/src/main.rs` 启动时会：

1. 初始化 tracing；
2. 读取监听地址和 SQLite 路径；
3. 建立数据库连接并执行迁移；
4. 创建默认管理员和默认资源组；
5. 处理上次异常退出留下的控制日志意图；
6. 创建限流器、活动运行表、指标抓取器、日志和告警状态；
7. 恢复上次服务进程存活期间正在运行的任务；
8. 启动指标抓取和保留策略循环；
9. 创建 Actix 应用和 Cookie Session；
10. 开始监听 HTTP 请求。

### 12.2 主要环境变量

代码中使用的运行参数包括：

| 环境变量 | 用途 | 默认值 |
|---|---|---|
| `CONSOLE_BIND_ADDR` | HTTP 监听地址 | `127.0.0.1:8080` |
| `CONSOLE_DB_PATH` | SQLite 文件路径 | `./data/console.db` |
| `CONSOLE_IDLE_TIMEOUT_SECS` | 会话空闲超时 | `3600` |
| `CONSOLE_SCRAPE_INTERVAL_SECS` | 指标抓取间隔 | `10` |
| `APE_DTS_BINARY_PATH` | `dt-main` 路径 | `target/release/dt-main` |
| `CONSOLE_STOP_GRACE_SECS` | SIGTERM 后等待时间 | `10` |
| `CONSOLE_RUN_DATA_DIR` | 每次运行的数据目录 | `./data/runs` |

现有控制台 README 未完整列出上述变量。

### 12.3 API 能力

API 统一位于 `/api`，主要包括：

- 登录、退出、当前用户；
- 用户与角色；
- 任务 CRUD；
- 任务启动、停止、暂停、恢复；
- 资源组；
- 连接测试与预检查；
- INI 预览；
- 运行状态和历史；
- 指标查询；
- 日志查询与 SSE；
- 告警、告警规则、告警通道和模板；
- 操作日志与控制日志；
- 系统主机、全局参数、许可证；
- 健康检查与就绪检查。

中间件包含：

- 请求日志；
- CORS；
- Cookie Session；
- CSRF；
- JSON 错误封装；
- 共享数据库与运行状态。

---

## 13. 任务运行生命周期

### 13.1 启动任务

控制台启动任务时执行：

1. RBAC 检查；
2. Idempotency-Key 重放检查；
3. 许可证检查；
4. 从 SQLite 读取任务；
5. 执行预检查；
6. 通过内存锁和数据库状态防止同一任务重复启动；
7. 对需要同步结构的快照任务先运行结构迁移；
8. 创建 Run 记录和运行目录；
9. 记录控制操作意图；
10. 将 Task 模型渲染成 INI；
11. 启动 `dt-main`；
12. 保存 PID 并将运行状态设为 `running`；
13. 注册指标抓取目标；
14. 启动后台 Supervisor。

### 13.2 子进程目录

每个 Run 使用独立目录：

```text
<data_dir>/<run_id>/
  ├── task_config.ini
  └── logs/
```

`LocalExecutor` 会将相对 `log4rs_file` 路径改成绝对路径，以避免子进程切换工作目录后找不到日志配置。

### 13.3 停止任务

停止流程：

1. 验证权限与幂等键；
2. 查询活动 Run；
3. 将状态改为 `stopping`；
4. 从活动进程表取出句柄；
5. 先发送 SIGTERM；
6. 在宽限期内轮询进程退出；
7. 超时后发送 SIGKILL；
8. 更新 Run、Task 和控制日志状态；
9. 删除指标抓取目标。

### 13.4 Supervisor

后台 Supervisor 每 2 秒检查一次子进程：

- 正常退出码 0：标记为 stopped；
- 非零退出或信号退出：标记为 failed；
- 两阶段任务的快照阶段正常完成：启动 CDC 阶段而不是结束 Run；
- 活动句柄丢失但数据库仍显示运行：标记为 orphaned/failed。

### 13.5 服务重启后的恢复

控制台重启后会加载 `pending`、`running`、`paused`、`stopping` 状态的 Run：

- 通过 `kill(pid, 0)` 判断 PID 是否仍存在；
- 存在则重建 `RunHandle`、恢复抓取目标并重新启动 Supervisor；
- 不存在则标记为 orphaned/failed。

该机制能够恢复管理状态，但仅通过 PID 存在性无法确认进程是否仍是原先的 `dt-main`，存在 PID 复用风险。

---

## 14. 两阶段 Snapshot + CDC

项目支持单个 Run 内依次运行两个 `dt-main` 子进程：

1. 在快照开始前捕获 CDC 起点；
2. 执行快照阶段；
3. 快照成功退出后，使用预先保存的起点启动 CDC；
4. 更新同一个 Run 的 PID；
5. 在控制日志中写入阶段切换事件。

这一模式避免快照期间发生的增量变更丢失，同时在管理面保持单个逻辑运行记录。

---

## 15. 监控、日志与告警

### 15.1 引擎监控

引擎内部包含：

- `Monitor`：组件级计数器；
- `GroupMonitor`：聚合多个组件；
- `TaskMonitor`：任务级指标；
- 可选 `PrometheusMetrics`。

指标包括：

- 抽取 RPS/BPS；
- 写入 RPS/BPS；
- 写入延迟；
- 队列长度；
- 任务进度；
- CDC 时间戳或延迟；
- DDL 计数等。

### 15.2 Log4rs 日志

根目录 `log4rs.yaml` 定义多个独立日志文件：

- `default.log`；
- `position.log`；
- `monitor.log`；
- `finished.log`；
- `task.log`；
- 数据检查与修订日志。

如果配置的 `log4rs_file` 不存在，`TaskRunner` 当前会直接继续运行，而不是启动失败。这样可能导致位置日志和监控日志静默缺失。

### 15.3 Prometheus

启用 `metrics` Feature 后，每个引擎进程启动 HTTP 服务，主要提供：

- `GET /metrics`；
- `GET /healthz`。

默认地址为 `0.0.0.0:9090`。

### 15.4 控制台指标抓取

控制台定期抓取活动 Run 的 Prometheus 指标，并将标量样本写入 SQLite。Histogram 和 Summary 当前被跳过。

连续三次抓取失败会创建 `metrics_unavailable` 告警，恢复成功后将告警标记为恢复。

---

## 16. Web 前端

### 16.1 启动与 API 接入

`web-prototype/src/main.ts`：

- 仅在 `VITE_USE_MOCK=true` 时启动 MSW；
- 初始化 Vue、Pinia、Router、i18n 和 Element Plus；
- 安装全局异常日志；
- 启用跨标签页退出同步。

Vite 开发服务器默认运行在 `127.0.0.1:5173`，并将 `/api` 代理到 `127.0.0.1:8080`。

共享 API Client：

- 默认 Base URL 为 `/api`；
- 请求超时 30 秒；
- 对写操作添加 XSRF Header；
- 统一解析 `{ code, message, details }` 错误；
- 收到 401 时清除本地用户并跳转登录页。

### 16.2 路由与权限

主要页面包括：

- 登录；
- Dashboard；
- Snapshot、CDC、Check、Struct 任务列表；
- 创建任务向导；
- 任务详情；
- 当前和历史告警；
- 告警规则、通道和模板；
- License；
- 用户和系统监控；
- 操作日志与控制日志；
- 全局参数。

角色包括：

- `admin`；
- `operator`；
- `viewer`。

路由守卫和权限矩阵共同控制页面访问与按钮展示。

### 16.3 创建任务向导

创建向导覆盖：

- 源端和目标端类型；
- GaussDB 子模式；
- 主机、端口、账号、数据库和 SSL；
- 资源组；
- 连接测试；
- 对象过滤；
- 库表列路由；
- Lua 处理；
- 并行器、流水线、恢复器和运行参数；
- 预检查；
- INI 预览；
- 创建后立即启动。

向导按任务类型动态调整步骤，使用 LocalStorage 保存草稿，并对脏数据离开页面进行确认。

### 16.4 Dashboard

Dashboard 每 5 秒轮询：

- 任务列表；
- 当前告警；
- 许可证；
- 活动任务的最新 Run；
- RPS 和延迟指标。

当前 Dashboard 并非调用单一 Summary API，而是在前端聚合多个接口结果。部分展示数据仍是推导值或占位值。

### 16.5 任务详情

任务详情包含：

- 配置；
- 对象；
- 日志；
- 监控；
- 告警；
- 运行历史。

日志支持 SSE、文件选择、级别过滤、暂停和自动跟随。监控支持 1 小时、6 小时、24 小时时间范围。

当前对象页的部分行数据由前端随机生成，不是来自真实对象进度接口。

### 16.6 告警

当前告警页每 8 秒轮询一次，支持筛选、单条清除和批量清除。

项目中已经存在 `useAlertStream` SSE Composable，但当前告警表没有使用它，因此“实时告警”目前主要通过轮询实现。

---

## 17. 测试体系

### 17.1 Rust 单元与集成测试

常规 CI 执行：

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --exclude dt-tests -- -D warnings
cargo check --workspace --all-targets --all-features --exclude dt-tests
cargo nextest run --workspace --exclude dt-tests --all-features --lib --bins --no-fail-fast
cargo test --workspace --doc --all-features --exclude dt-tests
```

覆盖较多的模块包括：

- 配置解析；
- 过滤和路由；
- DDL 解析；
- SQL 生成；
- 快照语句和切分；
- 控制台 INI 渲染；
- 参数验证；
- 运行生命周期；
- 用户、RBAC、License 和审计。

`dt-console-server/tests` 包含较完整的 API 和仓储测试，例如：

- Session/Auth；
- RBAC；
- 用户 CRUD；
- 任务 CRUD；
- INI Golden；
- Run 生命周期；
- License；
- Operate Log。

### 17.2 数据库集成测试

`dt-tests` 使用 Fixture 驱动：

1. 执行源端准备 SQL；
2. 执行目标端准备 SQL；
3. 启动同步任务；
4. 等待初始化；
5. 执行源端测试 SQL；
6. 可选执行目标端测试 SQL；
7. 等待同步；
8. 比较源端与目标端数据。

覆盖内容包括：

- Snapshot；
- CDC；
- Check/Revise/Review；
- Precheck；
- Struct；
- 数据类型矩阵；
- Resume；
- Failover；
- 双向和循环同步；
- Lua 转换；
- DDL、字符集、路由等数据库特性。

但 `dt-tests` 被常规 Rust CI 明确排除，因此关键数据链路的回归不能只依赖标准 CI。

### 17.3 前端测试

前端测试包括：

- Vitest 单元测试；
- Playwright E2E；
- MSW Mock；
- 可选真实后端 E2E。

常规前端 CI 执行：

```bash
pnpm install --frozen-lockfile
pnpm lint
pnpm test
pnpm build
pnpm test:e2e
```

默认 Playwright 使用 MSW，而不是 Rust 控制台和真实数据库。真实后端模式需要设置：

```bash
E2E_REAL_BACKEND=1
```

项目仍有若干 `it.todo`，涉及：

- 告警 SSE 清理；
- 向导高级验证；
- 登录审计和 Session 超时；
- INI Golden 与凭据转义；
- 指标保留和并发写入。

---

## 18. 构建、部署与发布

### 18.1 Rust 构建

Rust 工具链固定为 1.85.0。Release Profile 使用：

- `opt-level = 3`；
- LTO；
- 单 codegen unit；
- Strip Debug Info；
- Panic unwind。

### 18.2 Docker

根目录 Dockerfile：

- 使用 Rust 1.85 Builder；
- 安装 CMake 和 libclang；
- Release 构建默认开启 `metrics`；
- 将引擎和 `log4rs.yaml` 复制到 Distroless 镜像；
- 默认入口是 `/ape-dts`。

默认容器主要面向引擎，不是控制台，除非显式更改构建模块。

### 18.3 发布工作流

仓库包含手动触发的工作流：

- 构建并上传 GitHub Release 压缩包；
- 构建并推送多架构 Docker 镜像；
- 构建并上传二进制到 S3。

这些发布流程均为 `workflow_dispatch`，不是自动发布。

### 18.4 前端部署

前端为 History 模式 SPA，生产部署需要：

- 将未知路由回退至 `index.html`；
- 将 `/api/` 反向代理至控制台服务；
- 对 SSE 路由关闭代理缓冲。

---

## 19. 已识别风险与待验证问题

本节内容来自静态阅读，建议通过针对性测试进一步确认。

### 19.1 高优先级

#### R-01：停止信号与引擎优雅退出机制可能不一致

**代码现象：**

- 控制台停止任务时向子进程发送 SIGTERM，超时后发送 SIGKILL；
- `dt-main` 显式监听的是 Ctrl-C；
- 未发现 SIGTERM 与内部 `shut_down` 标志的直接桥接。

**潜在影响：**

- 流水线可能来不及排空队列；
- 最终检查点和监控刷新可能无法完成；
- CDC 恢复位置可能落后或不完整。

**建议验证：**

- 启动带积压的 CDC 任务；
- 通过控制台停止；
- 检查队列排空、最终位点和下次恢复结果。

#### R-02：暂停状态可能只发生在控制台侧

**代码现象：**

- Pause Handler 发送 SIGUSR1，并将 Run 标记为 paused；
- 未发现引擎注册 SIGUSR1 Handler。

**潜在影响：**

- UI 显示暂停；
- 指标抓取停止；
- 实际数据同步可能仍在继续。

**建议验证：**

- 运行持续写入的 CDC；
- 执行暂停；
- 观察目标端是否继续收到数据。

#### R-03：多个引擎进程可能争用默认指标端口

**代码现象：**

- 每个引擎默认监听 `0.0.0.0:9090`；
- 控制台允许不同任务并行运行；
- 抓取器默认访问 `127.0.0.1:9090`。

**潜在影响：**

- 第二个任务启动指标服务时端口冲突；
- 控制台可能抓取到错误任务的指标；
- 指标服务启动中的 `unwrap` 可能导致任务异常。

**建议：**

- 为每个 Run 分配唯一端口；
- 将端口写入渲染后的 INI；
- 抓取目标按 Run 注册；
- 避免指标服务 Bind 失败直接 panic。

#### R-04：数据库集成测试未进入常规 CI

**代码现象：**

- `dt-tests` 包含大量核心迁移测试；
- Clippy、Check、Nextest、Doc Test 均排除 `dt-tests`。

**潜在影响：**

- 连接器、SQL 生成、CDC 和恢复回归可通过常规 CI；
- 合并前质量依赖人工选择并运行测试。

**建议：**

- 至少建立每日或按标签触发的数据库集成 CI；
- 选择 MySQL/PostgreSQL 快照与 CDC 作为最小冒烟集；
- GaussDB/Oracle 保留环境依赖型手动或专用 Runner 验证。

### 19.2 中优先级

#### R-05：PostgreSQL CDC 断流恢复能力弱于 GaussDB CDC

GaussDB CDC 有显式重连和指数退避循环；PostgreSQL CDC 在流错误或结束路径中存在 panic 行为。

建议统一 CDC 连接生命周期抽象，并对可恢复错误执行重连，对不可恢复错误返回结构化失败。

#### R-06：PostgreSQL Relation 类型查找存在 `unwrap`

未知扩展类型或未加载 OID 可能导致 panic。建议返回带表名、列名和 OID 的结构化错误，或按配置回退到文本解码。

#### R-07：GaussDB CDC 当前只支持 DML

DDL 类操作会被识别为不支持。在线 DDL 可能使任务失败或要求业务侧禁止 DDL。

建议明确产品边界，并在预检查、文档和 UI 中暴露这一限制。

#### R-08：GaussDB UPDATE 缺少旧键时存在语义风险

当 `old_keys_*` 缺失时，解码器可能用 After Image 作为 Before Image。主键变化时，目标端 WHERE 条件可能错误。

建议：

- 对缺失旧键且主键发生变化的情况直接失败；
- 或要求复制插件提供完整旧键；
- 增加针对键变更的集成测试。

#### R-09：日志恢复只读取末尾 200 条位置记录

多表、高频检查点任务可能丢失较早但仍需恢复的位置信息。

建议使用按任务/表索引的恢复日志、可配置 Tail 大小，或优先推荐数据库恢复器。

#### R-10：重启恢复仅通过 PID 判断进程身份

`kill(pid, 0)` 只能确认 PID 存在，不能确认进程属于该 Run，且可能受 PID 复用影响。

建议同时校验：

- 进程启动时间；
- 命令行参数中的 Run INI；
- 子进程写入的身份文件；
- 或采用长期 Supervisor/IPC 管理。

#### R-11：子进程 stdout/stderr 管道未见持续消费

Executor 将 stdout/stderr 设置为 piped，但在已调研路径中未看到持续 drain。大量输出可能造成管道背压并阻塞子进程。

建议明确选择：

- 继承父进程输出；
- 重定向文件；
- 或启动异步消费任务。

### 19.3 前端与契约风险

#### R-12：MSW Mock 与当前接口存在漂移

已观察到以下差异：

- Mock 生命周期使用旧的 `/tasks/:id/action`；
- 当前 UI 使用 `/tasks/:id/start|pause|resume|stop`；
- Mock 连接测试和预检查路径与当前向导路径不同；
- Mock Dashboard Summary 已不被当前 Dashboard 使用。

默认 Playwright 依赖 MSW，因此 Mock 漂移可能同时造成：

- Mock E2E 误报；
- 真实后端契约问题未被发现。

#### R-13：Dashboard 时间范围未完整接入查询

UI 有时间范围选择器，但数据 Composable 的指标范围固定为最近 1 小时。Top Running Tasks 的部分 RPS、延迟和 Sparkline 仍是占位值。

#### R-14：任务详情对象数据为随机生成

Objects Tab 当前不是后端真实对象进度。容易让用户误认为存在精确的对象级监控。

#### R-15：部分导入导出和日志功能为本地合成

包括：

- 任务导出为当前前端列表 JSON；
- INI 模板部分为硬编码；
- License 文件在浏览器端生成；
- Control Log 页面合成“虚拟日志文件”及随机大小；
- Operate Log 的 Level 展示和过滤未完整连接后端。

建议在产品层明确“演示功能”和“真实管理能力”的边界。

---

## 20. 文档一致性问题

调研中发现：

1. 架构文档仍称 8 个 Crate，当前 Workspace 已有 9 个；
2. 顶层模块列表未完整体现 `dt-console-server`；
3. README 连接器矩阵未覆盖代码中的 Oracle、GaussDB 和 Foxlake 等能力；
4. 控制台 README 未列出所有运行环境变量；
5. 前端测试 README 对 CI 和 Playwright 的描述落后于当前工作流；
6. 镜像构建文档引用的 Workflow 文件名与实际文件不一致；
7. README 中仍有 `TBD` 内容；
8. `.gitmodules` 为空，但部分文档仍提及 Submodule。

建议将顶层 README、控制台 README 和配置文档设为权威入口，历史 Agent Summary 和任务文档明确标记时间与适用版本。

---

## 21. 建议的验证矩阵

### 21.1 普通 Rust 变更

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --exclude dt-tests -- -D warnings
cargo nextest run --workspace --exclude dt-tests --all-features --lib --bins --no-fail-fast
cargo test --workspace --doc --all-features --exclude dt-tests
```

### 21.2 控制台后端变更

```bash
cargo nextest run -p dt-console-server --lib --bins
cargo test -p dt-console-server --tests -- --nocapture
cargo test -p dt-console-server --test ini_golden_export -- --nocapture
```

### 21.3 数据链路变更

启动本地数据库：

```bash
cd dt-tests
docker compose \
  -f docker-compose.ci.yml \
  -f docker-compose.override.local.yml \
  up -d
```

运行目标用例：

```bash
cargo test \
  --package dt-tests \
  --test integration_test \
  -- mysql_to_mysql::cdc_tests::test::cdc_basic_test \
  --nocapture
```

### 21.4 前端变更

```bash
cd web-prototype
pnpm install --frozen-lockfile
pnpm lint
pnpm test
pnpm build
pnpm test:e2e
```

### 21.5 真实端到端验证

```bash
cd web-prototype

E2E_REAL_BACKEND=1 \
E2E_DB_SOURCE_DSN='mysql://<user>:<password>@127.0.0.1:3307/src_db' \
E2E_DB_TARGET_DSN='mysql://<user>:<password>@127.0.0.1:3308/dst_db' \
pnpm exec playwright test e2e/full-happy-path.spec.ts --timeout=300000
```

---

## 22. 建议的后续工作优先级

### P0：运行正确性

1. 验证并修复 SIGTERM 优雅退出；
2. 验证 Pause/Resume 是否真正控制引擎；
3. 为每个 Run 分配独立指标端口；
4. 建立最小数据库集成冒烟 CI。

### P1：CDC 稳定性

1. 为 PostgreSQL CDC 增加断流重连；
2. 移除类型 OID 路径中的 panic/unwrap；
3. 明确 GaussDB DDL CDC 边界；
4. 对 GaussDB 主键变更 UPDATE 增加保护和测试；
5. 扩展或替代仅尾读 200 行的日志恢复机制。

### P2：控制台可信度

1. 持续消费子进程 stdout/stderr；
2. 加强重启后的进程身份确认；
3. 对日志配置缺失给出明确失败或告警；
4. 将运行配置、指标端口和候选节点变成可观察的 Run 元数据。

### P3：前端产品化

1. 对齐前端、后端和 MSW 契约；
2. 让 Dashboard 时间范围真正影响查询；
3. 替换任务详情对象页随机数据；
4. 选择 SSE 或轮询作为统一告警实时机制；
5. 将任务导出、License 下载、控制日志下载切换到真实后端能力；
6. 完成高风险 `it.todo` 测试。

### P4：文档治理

1. 更新 Workspace 和连接器清单；
2. 完整记录环境变量；
3. 修复 Workflow 文件名和陈旧测试说明；
4. 为 GaussDB 兼容模式、协议和 HA 参数提供统一配置文档；
5. 标明历史规划文档和当前实现文档的权威等级。

---

## 23. 关键文件索引

### 引擎与编排

- `dt-main/src/main.rs`
- `dt-task/src/task_runner.rs`
- `dt-task/src/extractor_util.rs`
- `dt-task/src/sinker_util.rs`
- `dt-task/src/parallelizer_util.rs`
- `dt-task/src/task_util.rs`

### 公共数据模型与配置

- `dt-common/src/config/task_config.rs`
- `dt-common/src/config/config_enums.rs`
- `dt-common/src/config/resumer_config.rs`
- `dt-common/src/meta/dt_data.rs`
- `dt-common/src/meta/position.rs`
- `dt-common/src/meta/pg/pg_meta_manager.rs`
- `dt-common/src/meta/pg/type_registry.rs`
- `dt-common/src/meta/pg/pg_value_type.rs`

### 流水线与并行器

- `dt-pipeline/src/base_pipeline.rs`
- `dt-pipeline/src/http_server_pipeline.rs`
- `dt-parallelizer/src/base_parallelizer.rs`
- `dt-parallelizer/src/snapshot_parallelizer.rs`
- `dt-parallelizer/src/rdb_partitioner.rs`

### PostgreSQL/GaussDB

- `dt-connector/src/extractor/pg/pg_snapshot_extractor.rs`
- `dt-connector/src/extractor/pg/pg_snapshot_splitter.rs`
- `dt-connector/src/extractor/pg/pg_cdc_client.rs`
- `dt-connector/src/extractor/pg/pg_cdc_extractor.rs`
- `dt-connector/src/extractor/gaussdb/gaussdb_cdc_client.rs`
- `dt-connector/src/extractor/gaussdb/gaussdb_cdc_extractor.rs`
- `dt-connector/src/extractor/gaussdb/gaussdb_json_decoder.rs`
- `dt-connector/src/sinker/pg/pg_sinker.rs`
- `dt-connector/src/rdb_query_builder.rs`
- `dt-connector/src/rdb_router.rs`

### 恢复机制

- `dt-connector/src/extractor/resumer/mod.rs`
- `dt-connector/src/extractor/resumer/recorder/to_database.rs`
- `dt-connector/src/extractor/resumer/recovery/from_database.rs`
- `dt-connector/src/extractor/resumer/recovery/from_log.rs`

### 控制台后端

- `dt-console-server/src/main.rs`
- `dt-console-server/src/lib.rs`
- `dt-console-server/src/run_handlers.rs`
- `dt-console-server/src/executor.rs`
- `dt-console-server/src/ini_renderer.rs`
- `dt-console-server/src/metrics_scraper.rs`
- `dt-console-server/README.md`

### 前端

- `web-prototype/src/main.ts`
- `web-prototype/src/router/index.ts`
- `web-prototype/src/api/client.ts`
- `web-prototype/src/stores/auth.ts`
- `web-prototype/src/views/tasks/CreateTaskWizard.vue`
- `web-prototype/src/views/tasks/TaskDetail.vue`
- `web-prototype/src/components/TaskListView.vue`
- `web-prototype/src/composables/useDashboardData.ts`
- `web-prototype/src/components/AlertTableView.vue`
- `web-prototype/src/mock/handlers/`

### 测试与交付

- `.github/workflows/ci.yml`
- `.github/workflows/frontend.yml`
- `.github/workflows/build_and_release.yml`
- `.github/workflows/build_and_push_images.yml`
- `.github/workflows/build_and_upload_to_s3.yml`
- `dt-tests/README.md`
- `dt-tests/docker-compose.ci.yml`
- `web-prototype/playwright.config.ts`
- `web-prototype/vitest.config.ts`
- `QUICKSTART.md`

---

## 24. 结论

APE-DTS 已经具备较完整的数据迁移引擎、数据库适配层、恢复机制和管理控制台。核心代码采用统一事件模型和可插拔的 Extractor/Pipeline/Parallelizer/Sinker 架构，便于扩展新的数据源、目标端和处理策略。GaussDB 相关实现也已经从简单 PostgreSQL 兼容，发展为区分任务语义、Wire Protocol 和 SQL 兼容模式的模型，并包含 CDC HA 与目标端故障转移逻辑。

项目当前最需要优先确认的不是功能数量，而是运行生命周期和验证闭环：

- 控制台信号是否能让引擎安全停止和暂停；
- 多任务指标服务是否相互隔离；
- CDC 断线和未知类型是否会导致 panic；
- 核心数据库集成测试能否进入稳定、可持续的自动化流程；
- 前端、后端与 Mock 是否共享同一套契约。

如果先完成这些高优先级验证与治理，项目的生产可用性、故障可诊断性和后续开发效率都会明显提升。
