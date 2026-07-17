# ape-dts 项目架构深度分析 —— 二次开发指南

## Context

ape-dts 是一个高性能、云原生的**异构数据库迁移与同步工具**，使用 Rust 编写。支持 MySQL、PostgreSQL、MongoDB、Redis、Kafka、StarRocks、ClickHouse、Doris、Foxlake、TiDB 等多种数据源之间的数据迁移和实时同步。本文档旨在帮助工程师快速理解项目架构，为二次开发提供完整指引。

---

## 一、项目总体架构

### 1.1 Workspace 结构（8 个 Crate）

```
ape-dts/
├── dt-main/          # 入口二进制，解析配置并启动任务
├── dt-common/        # 公共库：配置解析、元数据定义、监控、限流
├── dt-connector/     # 数据库连接器：Extractor（抽取）+ Sinker（写入）
├── dt-pipeline/      # 数据管道：Pipeline 编排、Lua 处理器
├── dt-parallelizer/  # 并行化策略：分区、合并、表级并行等
├── dt-task/          # 任务编排：组装 Extractor/Sinker/Parallelizer/Pipeline
├── dt-precheck/      # 预检查：连接校验、兼容性验证
└── dt-tests/         # 集成测试：多数据库对的端到端测试
```

### 1.2 依赖关系图

```
dt-main
  └─ dt-task
       ├─ dt-pipeline
       │    ├─ dt-parallelizer
       │    │    └─ dt-connector
       │    │         └─ dt-common
       │    └─ dt-connector
       └─ dt-connector
  └─ dt-precheck
       └─ dt-common
```

### 1.3 核心数据流（E-P-P-S 四层架构）

```
┌──────────────┐    ┌──────────┐    ┌───────────────┐    ┌──────────┐
│  Extractor   │───>│ DtQueue  │───>│ Parallelizer  │───>│  Sinker  │
│ (数据抽取)    │    │ (缓冲队列) │    │ (并行分发)     │    │ (数据写入) │
└──────────────┘    └──────────┘    └───────────────┘    └──────────┘
                                          │
                                    ┌─────┴─────┐
                                    │ Lua处理器  │
                                    │ (数据变换)  │
                                    └───────────┘
```

**编排层**：`BasePipeline` 是调度核心，循环从 DtQueue 取数据，按类型分发到 Parallelizer，再由 Parallelizer 路由到多个 Sinker 并行写入。

---

## 二、入口与启动流程

### 2.1 main.rs (`dt-main/src/main.rs`)

```
命令行: ape-dts <task_config.ini>
  │
  ├─ 如果配置含 [precheck] → dt_precheck::do_precheck()
  └─ 否则 → TaskRunner::new(config_file).start_task()
```

### 2.2 TaskRunner 启动流程 (`dt-task/src/task_runner.rs`)

```
TaskRunner::start_task()
  │
  ├─ 1. init_log4rs()            // 初始化日志系统
  ├─ 2. 解析 Router              // schema/table/column 映射
  ├─ 3. 构建 Recorder/Recovery   // 断点续传组件
  ├─ 4. 创建 ConnClient          // 数据库连接池
  ├─ 5. 构建 BufferLimiter       // 入队/出队限流器
  ├─ 6. 构建 TaskContext          // 共享上下文
  │
  ├─ 7. 按配置中的 schema.table 列表，为每个表创建子任务:
  │     ├─ ExtractorUtil::create_extractor()  // 工厂方法
  │     ├─ SinkerUtil::create_sinkers()       // 工厂方法（N个并行sinker）
  │     ├─ ParallelizerUtil::create()         // 工厂方法
  │     └─ 创建 BasePipeline
  │
  ├─ 8. tokio::select! {
  │       extractor.extract()    // 抽取协程
  │       pipeline.start()       // 管道协程
  │       monitor.flush()        // 监控协程
  │     }
  │
  └─ 9. pipeline.stop() + 资源清理
```

**关键文件**：
- `dt-task/src/task_runner.rs` — 主编排
- `dt-task/src/extractor_util.rs` — Extractor 工厂
- `dt-task/src/sinker_util.rs` — Sinker 工厂
- `dt-task/src/parallelizer_util.rs` — Parallelizer 工厂
- `dt-task/src/task_util.rs` — 连接池创建、Resumer 创建

---

## 三、核心 Trait 定义

### 3.1 Extractor（`dt-connector/src/lib.rs:57-63`）

```rust
#[async_trait]
pub trait Extractor {
    async fn extract(&mut self) -> anyhow::Result<()>;
    async fn close(&mut self) -> anyhow::Result<()>;
}
```

从数据源读取数据，转化为 `DtItem` 推入 `DtQueue`。

### 3.2 Sinker（`dt-connector/src/lib.rs:22-54`）

```rust
#[async_trait]
pub trait Sinker {
    async fn sink_dml(&mut self, data: Vec<RowData>, batch: bool) -> Result<()>;
    async fn sink_ddl(&mut self, data: Vec<DdlData>, batch: bool) -> Result<()>;
    async fn sink_dcl(&mut self, data: Vec<DclData>, batch: bool) -> Result<()>;
    async fn sink_raw(&mut self, data: Vec<DtItem>, batch: bool) -> Result<()>;
    async fn sink_struct(&mut self, data: Vec<StructData>) -> Result<()>;
    async fn refresh_meta(&mut self, data: Vec<DdlData>) -> Result<()>;
    async fn close(&mut self) -> Result<()>;
}
```

按数据类型分别处理：DML（行级变更）、DDL（结构变更）、DCL（权限变更）、Struct（结构迁移）。

### 3.3 Parallelizer（`dt-parallelizer/src/lib.rs`）

```rust
#[async_trait]
pub trait Parallelizer {
    fn get_name(&self) -> String;
    async fn drain(&mut self, buffer: &DtQueue) -> Result<Vec<DtItem>>;
    async fn sink_dml(data, sinkers) -> Result<DataSize>;
    async fn sink_ddl(data, sinkers) -> Result<DataSize>;
    // ...
    async fn close(&mut self) -> Result<()>;
}
```

从缓冲队列拉取数据，按策略分发到多个 Sinker。

### 3.4 Pipeline（`dt-pipeline/src/lib.rs`）

```rust
#[async_trait]
pub trait Pipeline {
    async fn start(&mut self) -> anyhow::Result<()>;
    async fn stop(&mut self) -> anyhow::Result<()>;
}
```

---

## 四、核心数据结构

### 4.1 DtItem & DtData（`dt-common/src/meta/dt_data.rs`）

所有流经 Pipeline 的数据统一封装为 `DtItem`：

```rust
pub struct DtItem {
    pub dt_data: DtData,         // 数据内容
    pub position: Position,      // 位点（用于断点续传）
    pub data_origin_node: String, // 数据来源节点（用于双向同步防环）
}

pub enum DtData {
    Struct { struct_data: StructData },  // 结构迁移
    Ddl { ddl_data: DdlData },          // DDL 变更
    Dcl { dcl_data: DclData },          // DCL 变更
    Dml { row_data: RowData },          // 行级 DML
    Begin {},                            // 事务开始
    Commit { xid: String },             // 事务提交
    Heartbeat {},                        // 心跳
    Redis { entry: RedisEntry },         // Redis 条目
    Foxlake { file_meta: S3FileMeta },   // Foxlake 文件
}
```

### 4.2 RowData（`dt-common/src/meta/row_data.rs`）

DML 操作的核心数据结构：

```rust
pub struct RowData {
    pub schema: String,                          // 库名
    pub tb: String,                              // 表名
    pub row_type: RowType,                       // Insert / Update / Delete
    pub before: Option<HashMap<String, ColValue>>, // 变更前数据
    pub after: Option<HashMap<String, ColValue>>,  // 变更后数据
    pub data_size: usize,                        // 内存占用
}
```

关键方法：
- `reverse()` — 反转操作（用于数据修复）
- `split_update_row_data()` — 拆分 Update 为 Delete + Insert（用于合并去重）
- `get_hash_code()` — 基于主键/唯一键计算哈希（用于分区和合并）
- `from_mysql_row()` / `from_pg_row()` — 从数据库行构造

### 4.3 ColValue（`dt-common/src/meta/col_value.rs`）

统一的列值类型系统，覆盖所有数据库类型：

```rust
pub enum ColValue {
    None, Bool, Tiny, Short, Long, LongLong, Float, Double,
    Decimal(String), Time(String), Date(String), DateTime(String),
    String(String), Blob(Vec<u8>), Json(Vec<u8>), MongoDoc(Document),
    // ... 30+ 变体
}
```

### 4.4 Position（`dt-common/src/meta/position.rs`）

不同数据源的断点位置信息：

```rust
pub enum Position {
    None,
    RdbSnapshot { schema, tb, order_key },           // 快照位点
    MysqlCdc { binlog_filename, position, gtid_set }, // MySQL binlog 位点
    PgCdc { lsn, timestamp },                         // PG WAL 位点
    MongoCdc { resume_token, operation_time },         // Mongo Change Stream
    Redis { repl_id, repl_offset },                   // Redis PSYNC 位点
    Kafka { topic, partition, offset },                // Kafka 偏移量
    // ...
}
```

### 4.5 DtQueue（`dt-common/src/meta/dt_queue.rs`）

Extractor 和 Pipeline 之间的有界缓冲队列：

```
特性：
- 基于 ConcurrentQueue 的线程安全队列
- 内存感知：按字节数和记录数双重限制
- 支持入队/出队限流（BufferLimiter）
- push() 时若队列满，异步等待通知
```

---

## 五、各数据库实现详解

### 5.1 Extractor 实现矩阵

| 数据库       | Snapshot | CDC | Struct | Check | 特殊模式 |
|-------------|----------|-----|--------|-------|---------|
| MySQL       | ✅       | ✅  | ✅     | ✅    | —       |
| PostgreSQL  | ✅       | ✅  | ✅     | ✅    | —       |
| MongoDB     | ✅       | ✅  | —      | ✅    | —       |
| Redis       | —        | ✅  | —      | —     | Scan/Reshard/SnapshotFile |
| Kafka       | ✅       | —   | —      | —     | —       |
| Foxlake     | —        | —   | —      | —     | FoxlakeS3 |

**关键文件位置**：`dt-connector/src/extractor/{mysql,pg,mongo,redis,kafka,foxlake}/`

#### BaseExtractor（`dt-connector/src/extractor/base_extractor.rs`）

所有 Extractor 的公共逻辑：
- `push_dt_data()` — 路由 + 推入缓冲 + 更新监控
- `parse_ddl()` / `parse_dcl()` — DDL/DCL 解析与过滤
- `refresh_and_check_data_marker()` — 双向同步标记检测

#### MySQL CDC Extractor

通过 `mysql-binlog-connector-rust`（自定义 fork）连接 MySQL binlog：
- 支持 GTID 和 binlog position 两种定位
- 事件流：BinlogEvent → DtData::Dml/Ddl/Dcl → DtQueue

#### PG CDC Extractor

通过逻辑复制槽（Logical Replication Slot）读取 WAL：
- 基于 LSN（Log Sequence Number）定位
- 使用自定义 fork 的 `tokio-postgres`

#### MySQL/PG Snapshot Extractor

- 使用 Splitter 按主键/唯一键将表拆分为多个 Chunk
- 每个 Chunk 独立查询，支持多线程并行抽取
- 支持通过 `partition_cols` 配置自定义分区列

### 5.2 Sinker 实现矩阵

| 数据库       | Write | Check | Struct | Statistic | 特殊模式 |
|-------------|-------|-------|--------|-----------|---------|
| MySQL       | ✅    | ✅    | ✅     | —         | —       |
| PostgreSQL  | ✅    | ✅    | ✅     | —         | —       |
| MongoDB     | ✅    | ✅    | —      | —         | —       |
| Redis       | ✅    | —     | —      | ✅        | —       |
| Kafka       | ✅    | —     | —      | —         | —       |
| ClickHouse  | ✅    | —     | ✅     | —         | —       |
| StarRocks   | ✅    | —     | ✅     | —         | —       |
| Doris       | ✅    | —     | —      | —         | —       |
| Foxlake     | ✅    | —     | ✅     | —         | Push/Merge |

**关键文件位置**：`dt-connector/src/sinker/{mysql,pg,mongo,redis,kafka,clickhouse,starrocks,foxlake}/`

#### MySQL Sinker（`dt-connector/src/sinker/mysql/mysql_sinker.rs`）

- `sink_dml()` — 批量 INSERT/DELETE，串行 UPDATE
- 使用 `REPLACE INTO` 或 `INSERT IGNORE` 处理冲突
- DDL/DCL 使用临时连接执行（避免影响主连接池）

#### PG Sinker

类似 MySQL，使用 PostgreSQL 特有的 SQL 语法（ON CONFLICT DO UPDATE 等）

### 5.3 Parallelizer 实现

| 策略              | 适用场景                     | 关键特性 |
|------------------|----------------------------|---------|
| Serial           | 低并发、严格有序              | 单 Sinker 串行处理 |
| Snapshot         | 全量快照                     | 按表分发到不同 Sinker |
| Table            | CDC 多表                     | 按表路由并行写入 |
| RdbPartition     | 单大表 CDC                   | 按 hash(主键) 分区，多 Sinker 并行 |
| RdbMerge         | CDC 高频更新                  | 合并同一行的多次操作（去重优化） |
| RdbCheck         | 数据校验                     | 专用校验并行策略 |
| Mongo / Redis    | 对应数据库专用                | 数据库特定逻辑 |
| Foxlake          | S3 文件模式                   | 批量文件处理 |

**RdbMerge 核心逻辑**：
```
多条变更 → 按 hash_code 分组 → 合并同一行：
  - Insert + Delete = 无操作
  - Insert + Update = Insert（新值）
  - Update + Update = Update（最终值）
  - Update + Delete = Delete
→ 输出合并后的最小变更集
```

**关键文件位置**：`dt-parallelizer/src/`

---

## 六、配置系统

### 6.1 配置文件格式（INI）

```ini
[extractor]
db_type=mysql
extract_type=cdc
url=mysql://user:pass@host:3306?ssl-mode=disabled
server_id=123

[sinker]
db_type=mysql
sink_type=write
url=mysql://user:pass@host:3306?ssl-mode=disabled
batch_size=200

[filter]
do_tbs=test_db.*
ignore_tbs=test_db.tmp_*
do_events=insert,update,delete

[router]
tb_map=src_db.src_tb:dst_db.dst_tb

[parallelizer]
parallel_type=rdb_merge
parallel_size=8

[pipeline]
buffer_size=16000
checkpoint_interval_secs=10

[processor]
lua_code_file=/path/to/transform.lua
```

### 6.2 配置解析入口

- **TaskConfig**：`dt-common/src/config/task_config.rs` — 总配置解析
- **ExtractorConfig**：`dt-common/src/config/extractor_config.rs` — 各数据库的抽取配置
- **SinkerConfig**：`dt-common/src/config/sinker_config.rs` — 各数据库的写入配置
- **FilterConfig**：`dt-common/src/config/filter_config.rs` — 过滤规则
- **RouterConfig**：`dt-common/src/config/router_config.rs` — 路由映射
- **枚举定义**：`dt-common/src/config/config_enums.rs` — DbType/ExtractType/SinkType/ParallelType 等

---

## 七、关键子系统

### 7.1 路由系统（RdbRouter）

**文件**：`dt-connector/src/rdb_router.rs`

支持 schema、table、column 三级映射：
```rust
pub struct RdbRouter {
    pub schema_map: HashMap<String, String>,
    pub tb_map: HashMap<(String, String), (String, String)>,
    pub col_map: HashMap<(String, String), HashMap<String, String>>,
    pub topic_map: HashMap<(String, String), String>,  // Kafka topic
}
```

关键方法：`route_row()`, `route_ddl()`, `route_struct()`, `reverse()`

### 7.2 过滤系统（RdbFilter）

**文件**：`dt-common/src/rdb_filter.rs`

多维过滤能力：
- schema 级：`do_schemas` / `ignore_schemas`
- table 级：`do_tbs` / `ignore_tbs`（支持通配符 `*`）
- column 级：`ignore_cols`
- 事件级：`do_events`（insert/update/delete）
- DDL/DCL 级：`do_ddls` / `do_dcls`
- WHERE 条件：`where_conditions`
- Redis 命令：`ignore_cmds`
- 时间过滤：`TimeFilter`

### 7.3 断点续传系统（Resumer）

**文件目录**：`dt-connector/src/extractor/resumer/`

| 组件        | 职责 |
|------------|------|
| `Recorder` trait | 持久化当前位点 |
| `Recovery` trait | 启动时恢复位点 |

支持的续传模式：
- `FromLog` — 从日志文件恢复
- `FromTarget` — 从目标数据库状态恢复
- `FromDB` — 从专用元数据表恢复
- `Dummy` — 不续传

### 7.4 Lua 数据变换

**文件**：`dt-pipeline/src/lua_processor.rs`

在 Pipeline 的 DML 处理阶段执行 Lua 脚本，可实现：
- 数据清洗、格式转换
- 字段映射和计算
- 条件过滤和路由
- 使用嵌入式 Lua 5.4（mlua 库）

### 7.5 双向同步防环（DataMarker）

**文件**：`dt-connector/src/data_marker.rs`

通过在目标库写入特殊标记行标识数据来源，CDC 时检测并过滤掉从对端同步过来的数据，防止循环复制。

### 7.6 监控系统

**文件目录**：`dt-common/src/monitor/`

```
Monitor
  ├─ NoWindow Counter — 累计计数（总记录数、总字节数）
  ├─ TimeWindow Counter — 时间窗口统计（QPS、吞吐率）
  └─ CounterType 枚举：
       ExtractedRecordCount, ExtractedDataSize,
       BufferSize, QueuedRecordCurrent, QueuedByteCurrent,
       SinkedRecordTotal, SinkedByteTotal,
       RtPerQuery, SerialWrites, DDLRecordTotal, ...
```

支持可选的 Prometheus Metrics 导出（`--features metrics`）。

### 7.7 限流系统

**文件目录**：`dt-common/src/limiter/`

```
BufferLimiter
  ├─ RateLimiter — 令牌桶限流（governor 库）
  └─ CapacityLimiter — 信号量式容量限制
```

分为入队限流（extractor 端）和出队限流（sinker 端），可分别配置。

---

## 八、二次开发指南

### 8.1 新增数据库源（Extractor）

1. **定义配置**：在 `dt-common/src/config/config_enums.rs` 的 `DbType` 枚举中添加新类型
2. **添加 ExtractorConfig 变体**：在 `dt-common/src/config/extractor_config.rs` 中添加配置解析
3. **实现 Extractor trait**：在 `dt-connector/src/extractor/` 下新建目录，实现 `Extractor` trait
4. **注册工厂方法**：在 `dt-task/src/extractor_util.rs` 的 `create_extractor()` 中添加 match 分支
5. **定义元数据**：如需要，在 `dt-common/src/meta/` 下新建对应的元数据结构
6. **添加测试**：在 `dt-tests/tests/` 下新建测试目录

### 8.2 新增数据库目标（Sinker）

1. **添加 SinkerConfig 变体**：在 `dt-common/src/config/sinker_config.rs`
2. **实现 Sinker trait**：在 `dt-connector/src/sinker/` 下新建目录
3. **注册工厂方法**：在 `dt-task/src/sinker_util.rs` 的 `create_sinkers()` 中添加 match 分支
4. **添加测试**

### 8.3 新增并行化策略

1. **添加 ParallelType 枚举值**：在 `config_enums.rs`
2. **实现 Parallelizer trait**：在 `dt-parallelizer/src/`
3. **注册工厂方法**：在 `dt-task/src/parallelizer_util.rs`

### 8.4 新增数据变换能力

两种方式：
- **Lua 脚本**：编写 Lua 脚本，通过 `[processor]` 配置加载
- **Rust 原生**：在 `dt-pipeline/src/` 中新增 Processor 组件，在 `BasePipeline.sink_dml()` 中调用

### 8.5 新增任务类型

1. 在 `config_enums.rs` 的 `ExtractType` / `SinkType` / `TaskType` 中添加枚举
2. 更新 `build_task_type()` 函数的匹配逻辑
3. 实现对应的 Extractor + Sinker 组合
4. 在 `TaskRunner::start_task()` 中处理新任务类型的启动流程

---

## 九、测试体系

### 9.1 测试结构

```
dt-tests/tests/
├── test_runner/              # 测试框架核心
│   ├── test_base.rs          # 测试入口
│   ├── rdb_test_runner.rs    # RDB 通用测试运行器
│   ├── mongo_test_runner.rs  # MongoDB 测试运行器
│   ├── redis_test_runner.rs  # Redis 测试运行器
│   └── mock_utils/           # 模拟数据生成器
├── mysql_to_mysql/           # MySQL→MySQL 测试用例
│   ├── snapshot/             # 快照测试
│   ├── cdc/                  # CDC 测试
│   ├── struct/               # 结构迁移测试
│   └── check/                # 数据校验测试
├── pg_to_pg/                 # PG→PG
├── mysql_to_starrocks/       # MySQL→StarRocks
├── redis_to_redis/           # Redis→Redis
└── ...                       # 更多数据库对
```

### 9.2 单个测试用例结构

```
tests/mysql_to_mysql/cdc/basic_test/
├── task_config.ini    # 任务配置（URL 使用模板变量如 {mysql_extractor_url}）
├── src_prepare.sql    # 源端建表
├── dst_prepare.sql    # 目标端建表
├── src_test.sql       # 源端测试数据
└── dst_test.sql       # 目标端验证（可选）
```

### 9.3 测试执行流程

1. 执行 `src_prepare.sql`（源端准备）
2. 执行 `dst_prepare.sql`（目标端准备）
3. 启动同步任务
4. 等待初始化（`start_millis`）
5. 执行 `src_test.sql`（注入测试数据）
6. 执行 `dst_test.sql`（可选）
7. 等待同步完成（`parse_millis`）
8. 对比源端和目标端数据

---

## 十、构建与部署

### 10.1 本地开发

```bash
# 环境要求
rustup install 1.85.0            # MSRV
# 需要 cmake, libclang-dev (for rdkafka)

make init                        # 初始化 git submodules
make build                       # cargo build --release
make build-debug                 # cargo build (debug)
make lint                        # cargo clippy
make test                        # 运行测试
```

### 10.2 Docker 构建

```bash
make docker-build                # 构建 Docker 镜像
# 产物：~71.4 MB distroless 镜像
```

### 10.3 运行

```bash
# 直接运行
./target/release/dt-main task_config.ini

# Docker 运行
docker run apecloud/ape-dts:2.0.25 /path/to/task_config.ini
```

---

## 十一、关键设计模式总结

| 模式 | 应用位置 | 说明 |
|-----|---------|------|
| **Trait 多态** | Extractor/Sinker/Parallelizer/Pipeline | 所有组件通过 trait 定义接口，impl 实现具体逻辑 |
| **工厂模式** | ExtractorUtil/SinkerUtil/ParallelizerUtil | 根据配置枚举创建具体实现 |
| **生产者-消费者** | Extractor → DtQueue → Pipeline | 异步有界队列解耦读写 |
| **策略模式** | Parallelizer | 不同并行策略可插拔替换 |
| **观察者模式** | Monitor/Counter | 各组件上报指标，Monitor 聚合统计 |
| **路由模式** | RdbRouter | schema/table/column 三级映射 |
| **令牌桶** | RateLimiter | 控制读写速率 |
| **断点续传** | Recorder/Recovery | Position 持久化与恢复 |

---

## 十二、关键文件速查表

| 功能 | 文件路径 |
|------|---------|
| 程序入口 | `dt-main/src/main.rs` |
| 任务编排 | `dt-task/src/task_runner.rs` |
| 核心 Trait 定义 | `dt-connector/src/lib.rs` |
| Pipeline 循环 | `dt-pipeline/src/base_pipeline.rs` |
| 配置枚举 | `dt-common/src/config/config_enums.rs` |
| 总配置解析 | `dt-common/src/config/task_config.rs` |
| 数据结构 DtItem | `dt-common/src/meta/dt_data.rs` |
| 行数据 RowData | `dt-common/src/meta/row_data.rs` |
| 列值 ColValue | `dt-common/src/meta/col_value.rs` |
| 位点 Position | `dt-common/src/meta/position.rs` |
| 缓冲队列 DtQueue | `dt-common/src/meta/dt_queue.rs` |
| 路由 | `dt-connector/src/rdb_router.rs` |
| 过滤 | `dt-common/src/rdb_filter.rs` |
| Lua 变换 | `dt-pipeline/src/lua_processor.rs` |
| 监控 | `dt-common/src/monitor/monitor.rs` |
| 限流 | `dt-common/src/limiter/buffer_limiter.rs` |
| 错误类型 | `dt-common/src/error.rs` |
| Extractor 工厂 | `dt-task/src/extractor_util.rs` |
| Sinker 工厂 | `dt-task/src/sinker_util.rs` |
| Parallelizer 工厂 | `dt-task/src/parallelizer_util.rs` |
| MySQL CDC | `dt-connector/src/extractor/mysql/mysql_cdc_extractor.rs` |
| PG CDC | `dt-connector/src/extractor/pg/pg_cdc_extractor.rs` |
| MySQL Sinker | `dt-connector/src/sinker/mysql/mysql_sinker.rs` |
| PG Sinker | `dt-connector/src/sinker/pg/pg_sinker.rs` |
| 合并并行器 | `dt-parallelizer/src/merge_parallelizer.rs` |
| 分区并行器 | `dt-parallelizer/src/partition_parallelizer.rs` |
| 断点续传 | `dt-connector/src/extractor/resumer/` |
| 双向同步标记 | `dt-connector/src/data_marker.rs` |
