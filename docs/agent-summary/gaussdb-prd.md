# GaussDB 数据同步二次开发 PRD

> **版本**: v1.0
> **日期**: 2026-03-16
> **受众**: 内部研发团队
> **项目**: ape-dts GaussDB 扩展

---

## 一、背景与目标

### 1.1 业务背景

在国产化替代、混合云架构和数据库升级迁移的大背景下，华为 GaussDB 作为国产企业级数据库正在被越来越多的客户采用。ape-dts 作为异构数据库迁移与同步工具，需要扩展 GaussDB 支持能力，覆盖以下核心场景：

- **国产化替代**: 客户从 PG/MySQL/Oracle 迁移到 GaussDB，满足信创合规要求
- **混合云架构**: 华为云栈（HCS）环境中 GaussDB 与其他数据库的数据同步
- **数据库升级迁移**: 零停机从现有数据库迁移到 GaussDB
- **产品能力扩展**: ape-dts 支持更多数据库类型，增强竞争力

### 1.2 目标数据库版本

- **华为 GaussDB**: HCS 25.1.30 版本（华为云栈托管版）
- 参考文档: https://doc.hcs.huawei.com/db/en-us/gaussdbqlh/25.1.30/productdesc/qlh_03_0001.html
- GaussDB 基于 openGauss（源自 PostgreSQL 9.2/Postgres-XC），支持三种 SQL 兼容模式

### 1.3 产品目标

支持以下数据库与 GaussDB 之间的**数据同步**和**对象同步**：

| 方向 | MVP (2-4周) | 完整版 |
|------|-------------|--------|
| PostgreSQL → GaussDB | ✅ | ✅ |
| GaussDB → PostgreSQL | ✅ | ✅ |
| MySQL → GaussDB | — | ✅ |
| GaussDB → MySQL | — | ✅ |
| Oracle → GaussDB | — | ✅ |
| GaussDB → Oracle | — | ✅ |

---

## 二、功能需求

### 2.1 同步模式矩阵

| 同步模式 | 说明 | MVP | 完整版 |
|---------|------|-----|--------|
| **全量快照迁移 (Snapshot)** | 一次性全量数据迁移 | ✅ | ✅ |
| **增量实时同步 (CDC)** | 基于日志的实时增量同步 | ✅ | ✅ |
| **对象结构同步 (Struct)** | 数据库对象的创建和迁移 | ✅ | ✅ |
| **数据校验 (Check)** | 源端与目标端数据一致性校验 | ✅ | ✅ |
| **预检查 (Precheck)** | 同步前的环境兼容性检查 | ✅ | ✅ |

> **注意**: 本期同步模式为**分别单向同步**，即 PG→GaussDB 和 GaussDB→PG 是两个独立任务，非双活实时同步（不涉及 DataMarker 防环）。

### 2.2 对象同步范围

需支持以下所有数据库对象的迁移：

| 对象类型 | MVP | 完整版 | 说明 |
|---------|-----|--------|------|
| **表结构 (Table)** | ✅ | ✅ | 列定义、数据类型、默认值、NOT NULL 约束 |
| **索引 (Index)** | ✅ | ✅ | B-tree、Hash、GIN、GiST 等 |
| **约束 (Constraint)** | ✅ | ✅ | 主键、唯一键、外键、CHECK 约束 |
| **序列 (Sequence)** | ✅ | ✅ | 序列定义及当前值 |
| **视图 (View)** | ✅ | ✅ | 普通视图和物化视图 |
| **存储过程/函数 (Function)** | ✅ | ✅ | PL/pgSQL 函数和存储过程 |
| **用户/角色 (User/Role)** | ✅ | ✅ | DCL 层面的用户、角色定义 |
| **权限 (Permission)** | ✅ | ✅ | GRANT/REVOKE 权限迁移 |
| **注释 (Comment)** | ✅ | ✅ | 表和列的注释 |
| **触发器 (Trigger)** | — | ✅ | 表级触发器 |
| **自定义类型 (Custom Type)** | — | ✅ | ENUM、复合类型、域类型 |
| **GaussDB 分布式表特性** | ✅ | ✅ | distribute by 分布键（参见 2.5） |

### 2.3 GaussDB 兼容模式支持

GaussDB 支持三种 SQL 兼容模式，本项目需全部支持。架构设计采用**多枚举值**方案：

| 兼容模式 | DbType 枚举值 | 复用策略 | MVP |
|---------|--------------|---------|-----|
| PostgreSQL 兼容 | `DbType::GaussDBPg` | 复用 PG Sinker/Extractor（扩展） | ✅ |
| MySQL 兼容 | `DbType::GaussDBMySQL` | 复用 MySQL Sinker（扩展） | — |
| Oracle 兼容 | `DbType::GaussDBOracle` | 新建 Oracle 兼容 Sinker | — |

**设计参考**: 类似 TiDB 模式（`DbType::Mysql | DbType::Tidb`），GaussDB 各兼容模式复用对应数据库的实现：
- `DbType::Pg | DbType::GaussDBPg` → 共享 PG Sinker/Snapshot Extractor
- `DbType::Mysql | DbType::GaussDBMySQL` → 共享 MySQL Sinker
- `DbType::GaussDBOracle` → 需新建（Oracle 兼容语法的 Sinker）

### 2.4 认证方式

需同时支持两种认证方式：

| 认证方式 | 说明 | 实现策略 |
|---------|------|---------|
| **MD5** | 标准 PG 认证，tokio-postgres 原生支持 | 零改动 |
| **SHA256** | 华为自研认证协议（非 SCRAM-SHA-256） | 需修改 `apecloud/rust-postgres` fork 添加 SHA256 支持 |

配置方式：通过连接 URL 或配置参数指定认证方式。

### 2.5 GaussDB 分布式表支持

GaussDB 作为分布式数据库，表创建时需要指定分布策略。参考 TiDB 的设计模式（不在 ape-dts 中处理分布式特性，交给数据库自身处理）：

- **PG → GaussDB**: 同步表结构时，不主动添加 `DISTRIBUTE BY` 子句，让 GaussDB 使用默认分布策略（通常默认按主键 hash 分布）
- **GaussDB → PG**: 同步时忽略 GaussDB 特有的分布式语法，仅同步标准 PG 兼容的 DDL
- **可配置扩展**: 预留配置项允许用户自定义分布策略映射，但 MVP 阶段不实现

### 2.6 CDC 实时同步

#### 2.6.1 PG → GaussDB CDC

- **Extractor**: 复用现有 `PgCdcExtractor`，基于 pgoutput 插件
- **Sinker**: 复用 PG Sinker（`DbType::Pg | DbType::GaussDBPg` 模式）
- **无需新开发**: PG 侧的 CDC 能力完全复用

#### 2.6.2 GaussDB → PG CDC（核心新开发）

- **Extractor**: 需全新实现 `GaussDBCdcExtractor`
- **逻辑复制插件**: `mppdb_decoding`（已 POC 验证）
- **输出格式**: **JSON 格式**（非 pgoutput 二进制格式）
- **Sinker**: 复用现有 `PgSinker`

**GaussDB CDC 关键技术点**:

| 特性 | GaussDB (mppdb_decoding) | PG (pgoutput) | 差异 |
|------|------------------------|---------------|------|
| 复制槽创建 | `pg_create_logical_replication_slot(name, 'mppdb_decoding')` | `CREATE_REPLICATION_SLOT ... LOGICAL "pgoutput"` | 不兼容 |
| Publication 模型 | 不支持 Publication/Subscription | `CREATE PUBLICATION ... FOR ALL TABLES` | 不兼容 |
| 输出格式 | JSON 文本 | pgoutput 二进制 | 不兼容 |
| LSN 格式 | 兼容 X/Y 格式 | X/Y 格式 | 兼容 |
| StandbyStatusUpdate | 兼容 | 标准 PG 协议 | 兼容 |

### 2.7 数据校验 (Data Check)

同步完成后支持数据一致性校验：

- **全量校验**: 对比源端和目标端所有数据行
- **抽样校验**: 按比例或随机抽样校验
- **结构校验**: 对比表结构、索引、约束定义
- **校验报告**: 输出差异记录和修复 SQL
- **复用现有框架**: PG 的 Check Extractor/Sinker 模式，添加 GaussDB 支持

### 2.8 性能要求

- 性能不低于现有 PG→PG 同步的基准水平
- 具体指标参考现有 PG→PG 的 QPS、延迟、吞吐量测试结果

---

## 三、架构设计

### 3.1 整体架构

遵循 ape-dts 的 E-P-P-S 四层架构（Extractor → Pipeline → Parallelizer → Sinker）：

```
                    GaussDB CDC (mppdb_decoding / JSON)
                              │
┌──────────────────────┐     ▼      ┌──────────────────────┐
│  GaussDBCdcExtractor │ ────────── │  PG/MySQL Sinker     │
│  (新建)              │            │  (复用)               │
└──────────────────────┘            └──────────────────────┘

┌──────────────────────┐            ┌──────────────────────┐
│  PG/MySQL Extractor  │ ────────── │  GaussDB Sinker      │
│  (复用)              │            │  (复用PG/MySQL Sinker) │
└──────────────────────┘            └──────────────────────┘
```

### 3.2 组件复用策略

| 组件 | GaussDB PG 模式 | GaussDB MySQL 模式 | GaussDB Oracle 模式 |
|------|-----------------|-------------------|-------------------|
| **Sinker** | 复用 PG Sinker | 复用 MySQL Sinker | 新建 |
| **Snapshot Extractor** | 复用 PG Snapshot | 复用 MySQL Snapshot | 新建 |
| **Struct Extractor** | 复用 PG Struct（微调）| 复用 MySQL Struct（微调）| 新建 |
| **CDC Extractor** | **新建** GaussDBCdcExtractor | **新建**（共享 mppdb_decoding 解析器）| **新建** |
| **Precheck** | 扩展 PG Prechecker | 扩展 MySQL Prechecker | 新建 |
| **MetaManager** | 复用 PG（扩展 OID）| 复用 MySQL | 新建 |
| **Type Registry** | 复用 PG（增加 GaussDB 类型）| 复用 MySQL | 新建 |
| **Query Builder** | 复用 PG | 复用 MySQL | 新建 |
| **Connection Pool** | 复用 PG（sqlx::PgPool）| 复用 MySQL | 复用 PG |

### 3.3 DbType 枚举设计

```rust
// dt-common/src/config/config_enums.rs
pub enum DbType {
    // 现有类型...
    Mysql, Pg, Mongo, Redis, Kafka, StarRocks, ClickHouse, Foxlake, Tidb, Doris,

    // 新增 GaussDB 类型
    #[strum(serialize = "gaussdb_pg")]
    GaussDBPg,          // PostgreSQL 兼容模式

    #[strum(serialize = "gaussdb_mysql")]
    GaussDBMySQL,       // MySQL 兼容模式

    #[strum(serialize = "gaussdb_oracle")]
    GaussDBOracle,      // Oracle 兼容模式
}
```

### 3.4 GaussDB CDC Extractor 架构

```
GaussDBCdcExtractor
├── GaussDBCdcClient          // 连接管理、复制槽管理
│   ├── connect()             // 建立复制连接
│   ├── prepare_slot()        // 创建 mppdb_decoding 逻辑复制槽
│   ├── start_replication()   // 启动 WAL 流
│   └── send_keepalive()      // 心跳回复
│
├── GaussDBJsonDecoder        // JSON 消息解析器
│   ├── decode_insert()       // 解析 INSERT 事件
│   ├── decode_update()       // 解析 UPDATE 事件（含 before/after）
│   ├── decode_delete()       // 解析 DELETE 事件
│   ├── decode_begin()        // 解析事务开始
│   └── decode_commit()       // 解析事务提交
│
├── PgMetaManager (复用)       // 元数据管理
├── PgColValueConvertor (复用)  // 列值转换（JSON→ColValue）
└── BaseExtractor (复用)        // 公共逻辑（push、filter、route）
```

**关键差异点**:
- PG CDC 使用 pgoutput 二进制格式 → GaussDB 使用 JSON 文本格式
- PG CDC 需要 CREATE PUBLICATION → GaussDB 不需要
- PG CDC 通过 Relation 消息获取表结构 → GaussDB 通过 JSON 字段名直接映射
- 复制槽创建语法不同，但 LSN 位点格式兼容

---

## 四、配置设计

### 4.1 GaussDB 作为 Sinker（PG 兼容模式）

```ini
[extractor]
db_type=pg
extract_type=cdc
url=postgres://user:pass@pg_host:5432/mydb
slot_name=ape_dts_slot

[sinker]
db_type=gaussdb_pg
sink_type=write
url=postgres://user:pass@gaussdb_host:5432/mydb
batch_size=200

[filter]
do_tbs=public.*

[parallelizer]
parallel_type=rdb_merge
parallel_size=8
```

### 4.2 GaussDB 作为 Extractor（CDC）

```ini
[extractor]
db_type=gaussdb_pg
extract_type=cdc
url=postgres://user:pass@gaussdb_host:5432/mydb
slot_name=ape_dts_gaussdb_slot
heartbeat_interval_secs=10
heartbeat_tb=heartbeat_db.ape_dts_heartbeat

[sinker]
db_type=pg
sink_type=write
url=postgres://user:pass@pg_host:5432/mydb
batch_size=200

[filter]
do_tbs=public.*

[parallelizer]
parallel_type=rdb_merge
parallel_size=8
```

### 4.3 GaussDB 结构同步

```ini
[extractor]
db_type=pg
extract_type=struct
url=postgres://user:pass@pg_host:5432/mydb

[sinker]
db_type=gaussdb_pg
sink_type=struct
url=postgres://user:pass@gaussdb_host:5432/mydb
conflict_policy=interrupt

[filter]
do_tbs=public.*
do_structures=database,table,constraint,sequence,comment,index
```

### 4.4 GaussDB 数据校验

```ini
[extractor]
db_type=gaussdb_pg
extract_type=snapshot
url=postgres://user:pass@gaussdb_host:5432/mydb

[sinker]
db_type=pg
sink_type=check
url=postgres://user:pass@pg_host:5432/mydb

[parallelizer]
parallel_type=rdb_check
```

---

## 五、详细实现方案

### 5.1 Phase 1: 基础框架 + GaussDB Sinker（第1周）

**目标**: 支持 PG → GaussDB 的全量迁移和结构同步

#### 5.1.1 需修改文件清单

| 文件路径 | 修改内容 |
|---------|---------|
| `dt-common/src/config/config_enums.rs` | 添加 `GaussDBPg`、`GaussDBMySQL`、`GaussDBOracle` 三个 DbType 枚举变体 |
| `dt-common/src/config/task_config.rs` | Sinker 配置分支添加 `DbType::GaussDBPg`（合并到 PG 分支）；Extractor 配置添加 GaussDB Snapshot/Struct 分支 |
| `dt-common/src/config/sinker_config.rs` | 无需修改（复用 `SinkerConfig::Pg`） |
| `dt-common/src/config/extractor_config.rs` | 无需修改（复用 `ExtractorConfig::PgSnapshot` / `PgStruct`） |
| `dt-common/src/system_dbs.rs` | `is_system_db()` 添加 `DbType::GaussDBPg => Self::POSTGRES` |
| `dt-common/src/utils/sql_util.rs` | `get_escape_pairs()` 添加 `DbType::GaussDBPg` 到 PG 分支 |
| `dt-task/src/task_util.rs` | `ConnClient::from_config()` 中 GaussDB 复用 PG 连接池创建 |
| `dt-task/src/sinker_util.rs` | `create_sinkers()` 中 GaussDBPg 映射到 PG sinker |
| `dt-task/src/extractor_util.rs` | `create_extractor()` 中 GaussDBPg Snapshot/Struct 映射到 PG extractor |
| `dt-precheck/src/builder/prechecker_builder.rs` | 添加 `DbType::GaussDBPg` 分支 |
| `dt-precheck/src/prechecker/pg_prechecker.rs` | 版本检查逻辑适配 GaussDB 版本号格式 |
| `dt-connector/src/rdb_query_builder.rs` | GaussDBPg 复用 `new_for_pg()` |
| `dt-common/src/meta/ddl_meta/ddl_parser.rs` | DDL 解析中 `DbType::Pg` 分支扩展为 `DbType::Pg \| DbType::GaussDBPg` |
| `dt-common/src/meta/struct_meta/structure/constraint.rs` | 约束类型映射支持 GaussDB |
| `dt-connector/src/data_marker.rs` | DataMarker 支持 GaussDBPg（预留，本期非必需） |

#### 5.1.2 关键实现模式

参考 TiDB 复用模式，核心改动为在 match 分支中添加 GaussDB 枚举值：

```rust
// task_config.rs - Sinker 配置
DbType::Pg | DbType::GaussDBPg => match sink_type {
    SinkType::Write => SinkerConfig::Pg { ... },
    SinkType::Check => SinkerConfig::PgCheck { ... },
    SinkType::Struct => SinkerConfig::PgStruct { ... },
}

// sinker_util.rs - Sinker 创建
SinkerConfig::Pg { url, batch_size, replace, .. } => {
    // 完全复用 PgSinker 创建逻辑
}
```

### 5.2 Phase 2: GaussDB CDC Extractor（第2-3周）

**目标**: 支持 GaussDB → PG 的增量实时同步

#### 5.2.1 新建文件清单

| 文件路径 | 说明 |
|---------|------|
| `dt-connector/src/extractor/gaussdb/mod.rs` | GaussDB extractor 模块入口 |
| `dt-connector/src/extractor/gaussdb/gaussdb_cdc_client.rs` | mppdb_decoding 复制槽管理和流连接 |
| `dt-connector/src/extractor/gaussdb/gaussdb_cdc_extractor.rs` | CDC 事件解析、JSON 解码、DtData 转换 |
| `dt-connector/src/extractor/gaussdb/gaussdb_json_decoder.rs` | mppdb_decoding JSON 格式解析器 |

#### 5.2.2 需修改文件清单

| 文件路径 | 修改内容 |
|---------|---------|
| `dt-connector/src/extractor/mod.rs` | 添加 `pub mod gaussdb;` |
| `dt-common/src/config/extractor_config.rs` | 添加 `GaussDBCdc` 变体（独立配置，不复用 PgCdc） |
| `dt-common/src/config/task_config.rs` | GaussDB CDC extractor 配置解析 |
| `dt-task/src/extractor_util.rs` | 注册 `GaussDBCdcExtractor` 工厂方法 |
| `dt-common/src/meta/position.rs` | 可复用 `Position::PgCdc`（LSN 格式兼容）；如需区分则添加 `Position::GaussDBCdc` |
| `dt-connector/src/extractor/resumer/utils.rs` | 断点续传支持 GaussDB CDC 位点恢复 |

#### 5.2.3 GaussDB CDC Client 核心逻辑

```rust
// gaussdb_cdc_client.rs - 关键差异点

impl GaussDBCdcClient {
    /// 创建逻辑复制槽（不需要 Publication）
    async fn prepare_slot(&mut self) -> Result<PgLsn> {
        // GaussDB: 使用 SQL 函数创建槽
        // SELECT * FROM pg_create_logical_replication_slot('slot_name', 'mppdb_decoding');
        // 注意：不需要 CREATE PUBLICATION
    }

    /// 启动复制流
    async fn start_replication(&self, lsn: PgLsn) -> Result<LogicalReplicationStream> {
        // START_REPLICATION SLOT <name> LOGICAL <lsn>
        // 注意：不需要 publication_names 参数
        // mppdb_decoding 可能需要特定的选项参数
    }
}
```

#### 5.2.4 JSON 解码器设计

```rust
// gaussdb_json_decoder.rs

/// mppdb_decoding JSON 事件结构（示例，以 POC 验证结果为准）
/// INSERT: {"table":"schema.table","op_type":"INSERT","columns_name":["id","name"],"columns_type":["int4","text"],"columns_val":["1","'hello'"]}
/// UPDATE: {"table":"schema.table","op_type":"UPDATE","columns_name":[...],"columns_type":[...],"columns_val":[...],"old_keys_name":[...],"old_keys_type":[...],"old_keys_val":[...]}
/// DELETE: {"table":"schema.table","op_type":"DELETE","old_keys_name":[...],"old_keys_type":[...],"old_keys_val":[...]}

pub struct GaussDBJsonDecoder {
    meta_manager: Arc<PgMetaManager>,
}

impl GaussDBJsonDecoder {
    pub fn decode_message(&self, json_str: &str) -> Result<Vec<DtData>> {
        // 解析 JSON → 转换为 RowData/DdlData → 封装为 DtData
    }
}
```

### 5.3 Phase 3: 数据校验 + 对象同步完善（第3-4周）

**目标**: 完善数据校验、对象同步、预检查

#### 5.3.1 数据校验

复用现有 PG Check 框架：
- Check Extractor: GaussDBPg 复用 PG Snapshot Extractor（`extract_type=snapshot`）
- Check Sinker: GaussDBPg 复用 PG Check Sinker（`sink_type=check`）
- 需修改 `task_config.rs` 确保 GaussDBPg 的 Check 模式正确路由

#### 5.3.2 对象同步完善

**结构提取器增强**（`dt-connector/src/extractor/pg/pg_struct_extractor.rs`）:

| 对象 | 提取方式 | GaussDB 适配 |
|------|---------|-------------|
| 视图 | `pg_catalog.pg_views` | 兼容，直接复用 |
| 序列 | `information_schema.sequences` | 兼容，直接复用 |
| 函数/存储过程 | `pg_catalog.pg_proc` | 需适配 GaussDB 特有函数语法 |
| 用户/角色 | `pg_catalog.pg_roles` | 兼容，直接复用 |
| 权限 | `information_schema.role_table_grants` | 兼容，直接复用 |

**结构写入器增强**（`dt-connector/src/sinker/pg/pg_struct_sinker.rs`）:
- GaussDB 特有 DDL 语法适配（如 `DISTRIBUTE BY` 默认行为处理）
- GaussDB 不支持的 PG 特性过滤（如某些 PG 扩展语法）

#### 5.3.3 GaussDB 特有类型支持

需在 Type Registry 中增加 GaussDB 特有类型的 OID 映射：

| GaussDB 类型 | 对应 PG 类型 | ColValue 映射 |
|-------------|-------------|-------------|
| `smalldatetime` | `timestamp` | `ColValue::DateTime` |
| `tinyint` | `int2` | `ColValue::Short` |
| `nvarchar2` | `varchar` | `ColValue::String` |
| `clob` | `text` | `ColValue::String` |
| `blob` | `bytea` | `ColValue::Blob` |

修改文件：
- `dt-common/src/meta/pg/type_registry.rs` — 添加 GaussDB OID
- `dt-common/src/meta/adaptor/pg_col_value_convertor.rs` — 添加类型转换逻辑
- `dt-common/src/meta/pg/pg_value_type.rs` — 添加 GaussDB 特有 PgValueType 变体

#### 5.3.4 预检查增强

`dt-precheck/src/prechecker/pg_prechecker.rs` 需要的适配：

| 检查项 | 现有 PG 逻辑 | GaussDB 适配 |
|--------|-------------|-------------|
| 版本检查 | `server_version_num >= 120000` | GaussDB 版本号格式不同，需独立检查逻辑 |
| WAL 级别 | `wal_level = 'logical'` | 兼容 |
| 复制槽数量 | `max_replication_slots > 0` | 兼容 |
| WAL 发送器 | `max_wal_senders > 0` | 兼容 |
| 系统 Schema 过滤 | `pg_catalog, information_schema` | 需增加 GaussDB 特有系统 schema（如 `cstore`, `db4ai` 等） |

### 5.4 Phase 4: 完整版扩展（MVP 之后）

#### 5.4.1 MySQL → GaussDB（GaussDB MySQL 兼容模式）

- `DbType::GaussDBMySQL` 复用 MySQL Sinker
- 连接池：使用 PG 连接池但通过 MySQL 兼容端口
- 类型映射：MySQL 类型 → GaussDB MySQL 兼容类型（基本一致）
- 关键：需验证 GaussDB MySQL 兼容模式的 SQL 语法兼容性

#### 5.4.2 Oracle → GaussDB

- Oracle Extractor: 需全新开发（LogMiner/OGG 接入）
- GaussDB Oracle 兼容模式 Sinker: 需新建
- 工作量最大，建议作为独立项目规划

#### 5.4.3 GaussDB → MySQL / Oracle

- GaussDB CDC Extractor 已在 Phase 2 完成
- MySQL Sinker / Oracle Sinker: 复用现有（如有）或新建
- 类型映射：GaussDB 类型 → MySQL/Oracle 类型

---

## 六、SHA256 认证支持方案

### 6.1 问题描述

GaussDB 默认使用华为自研 SHA256 认证（非标准 SCRAM-SHA-256），标准 `tokio-postgres` / `sqlx` 不支持。

### 6.2 实现方案

修改 `apecloud/rust-postgres` fork，在认证握手阶段添加 SHA256 支持：

1. **识别认证类型**: 在 `startup.rs` 的认证流程中，检测 GaussDB 返回的 SHA256 认证请求
2. **实现 SHA256 握手**: 按照 GaussDB/openGauss 的 SHA256 协议规范实现密码哈希和响应
3. **配置选择**: 通过连接参数或环境变量选择认证方式
4. **向后兼容**: 不影响标准 PG 的 MD5/SCRAM 认证

### 6.3 测试验证

- MD5 认证连接 GaussDB 实例
- SHA256 认证连接 GaussDB 实例
- 混合场景：同一任务源端 MD5、目标端 SHA256

---

## 七、测试计划

### 7.1 测试用例结构

```
dt-tests/tests/
├── pg_to_gaussdb/              # PG → GaussDB (PG兼容模式)
│   ├── snapshot/               # 全量快照测试
│   │   ├── basic_test/
│   │   ├── type_test/          # 数据类型兼容性
│   │   └── large_table_test/   # 大表测试
│   ├── cdc/                    # CDC 增量同步
│   │   ├── basic_test/
│   │   ├── ddl_test/           # DDL 事件同步
│   │   └── multi_table_test/   # 多表并行
│   ├── struct/                 # 结构同步
│   │   ├── basic_test/         # 表/索引/约束
│   │   ├── view_test/          # 视图
│   │   ├── sequence_test/      # 序列
│   │   ├── function_test/      # 函数/存储过程
│   │   └── rbac_test/          # 用户/角色/权限
│   ├── check/                  # 数据校验
│   │   ├── basic_test/
│   │   └── struct_test/
│   └── precheck/               # 预检查
│
├── gaussdb_to_pg/              # GaussDB → PG
│   ├── snapshot/
│   ├── cdc/                    # 核心：mppdb_decoding CDC
│   │   ├── basic_test/
│   │   ├── ddl_test/
│   │   ├── type_test/          # GaussDB 特有类型
│   │   └── resume_test/        # 断点续传
│   ├── struct/
│   ├── check/
│   └── precheck/
│
└── gaussdb_to_gaussdb/         # GaussDB → GaussDB (自回环)
    ├── snapshot/
    └── cdc/
```

### 7.2 测试环境要求

| 组件 | 版本 | 配置 |
|------|------|------|
| PostgreSQL | 14+ | wal_level=logical |
| GaussDB | HCS 25.1.30 | wal_level=logical, MD5/SHA256 认证均可 |
| Rust | 1.85.0 | MSRV |

### 7.3 核心测试场景

| 测试场景 | 优先级 | 说明 |
|---------|--------|------|
| PG→GaussDB 全量快照（基本类型） | P0 | 验证 Sinker 基本功能 |
| PG→GaussDB 结构同步（表+索引+约束） | P0 | 验证对象同步 |
| PG→GaussDB CDC 增量同步 | P0 | 复用 PG Extractor |
| GaussDB→PG 全量快照 | P0 | 验证 Snapshot Extractor |
| GaussDB→PG CDC（mppdb_decoding） | P0 | 核心新功能验证 |
| 数据校验（双向） | P0 | 数据一致性验证 |
| GaussDB 特有类型同步 | P1 | smalldatetime/tinyint/nvarchar2 等 |
| GaussDB→PG CDC 断点续传 | P1 | 故障恢复验证 |
| 预检查（双向） | P1 | 环境兼容性检查 |
| SHA256 认证连接 | P1 | 认证方式验证 |
| 视图/序列/函数同步 | P1 | 高级对象同步 |
| 用户/角色/权限同步 | P2 | DCL 同步验证 |
| 大表并行快照 | P2 | 性能验证 |
| 长时间 CDC 稳定性 | P2 | 7x24 稳定性 |

---

## 八、里程碑规划

### MVP: 2-4 周

| 周次 | 目标 | 交付物 |
|------|------|--------|
| **W1** | 基础框架 + GaussDB Sinker | PG→GaussDB 全量快照 + 结构同步可用 |
| **W2** | GaussDB CDC Extractor | GaussDB→PG CDC 增量同步可用 |
| **W3** | 数据校验 + 对象同步完善 + SHA256 | 双向校验可用，视图/序列/函数/权限同步 |
| **W4** | 测试完善 + 性能调优 + 文档 | 全部 P0 测试用例通过，性能达标 |

### MVP 验收标准

1. PG → GaussDB: 全量快照 + CDC 增量 + 结构同步 + 数据校验全部可用
2. GaussDB → PG: 全量快照 + CDC 增量（mppdb_decoding）+ 结构同步 + 数据校验全部可用
3. 所有 P0 测试用例通过
4. 性能不低于 PG→PG 同步基准水平
5. 支持 MD5 和 SHA256 两种认证方式
6. 对象同步覆盖：表、索引、约束、序列、视图、函数、用户/角色、权限、注释

### 完整版: MVP 后 4-8 周

| 阶段 | 目标 |
|------|------|
| 完整版 Phase 1 | MySQL → GaussDB（MySQL 兼容模式） |
| 完整版 Phase 2 | GaussDB → MySQL |
| 完整版 Phase 3 | Oracle → GaussDB（Oracle 兼容模式） |
| 完整版 Phase 4 | GaussDB → Oracle |

---

## 九、风险与缓解

| 风险 | 严重度 | 概率 | 缓解方案 |
|------|--------|------|---------|
| SHA256 认证 fork 改动量大 | 高 | 中 | 先用 MD5 认证推进开发，SHA256 并行开发 |
| mppdb_decoding JSON 格式与 POC 有差异 | 高 | 低 | POC 已验证，保留格式适配的灵活性 |
| GaussDB 版本间 CDC 行为不一致 | 中 | 中 | 锁定 HCS 25.1.30 版本，其他版本增量适配 |
| GaussDB 特有类型 OID 未知 | 中 | 中 | 连接 GaussDB 实例查询 pg_type 获取 |
| 视图/函数语法不完全兼容 | 中 | 中 | 跳过不兼容语法，记录日志告警 |
| 分布式表 DDL 影响结构同步 | 低 | 低 | 采用 TiDB 模式，不主动处理分布式特性 |

---

## 十、关键文件索引

### 需修改的现有文件

| 文件 | Phase | 修改概述 |
|------|-------|---------|
| `dt-common/src/config/config_enums.rs` | 1 | 添加 3 个 GaussDB DbType 枚举 |
| `dt-common/src/config/task_config.rs` | 1 | Sinker/Extractor 配置路由 |
| `dt-common/src/system_dbs.rs` | 1 | GaussDB 系统库过滤 |
| `dt-common/src/utils/sql_util.rs` | 1 | GaussDB 转义规则 |
| `dt-task/src/task_util.rs` | 1 | 连接池创建 |
| `dt-task/src/sinker_util.rs` | 1 | Sinker 工厂 |
| `dt-task/src/extractor_util.rs` | 1,2 | Extractor 工厂 |
| `dt-precheck/src/builder/prechecker_builder.rs` | 1 | 预检查构建 |
| `dt-precheck/src/prechecker/pg_prechecker.rs` | 3 | 版本检查适配 |
| `dt-connector/src/rdb_query_builder.rs` | 1 | 查询构建 |
| `dt-common/src/meta/ddl_meta/ddl_parser.rs` | 1 | DDL 解析 |
| `dt-common/src/meta/pg/type_registry.rs` | 3 | GaussDB 类型 OID |
| `dt-common/src/meta/adaptor/pg_col_value_convertor.rs` | 3 | 类型转换 |
| `dt-connector/src/data_marker.rs` | 1 | DataMarker 支持（预留） |

### 需新建的文件

| 文件 | Phase | 说明 |
|------|-------|------|
| `dt-connector/src/extractor/gaussdb/mod.rs` | 2 | 模块入口 |
| `dt-connector/src/extractor/gaussdb/gaussdb_cdc_client.rs` | 2 | CDC 连接管理 |
| `dt-connector/src/extractor/gaussdb/gaussdb_cdc_extractor.rs` | 2 | CDC 事件处理 |
| `dt-connector/src/extractor/gaussdb/gaussdb_json_decoder.rs` | 2 | JSON 解码器 |
| `dt-tests/tests/pg_to_gaussdb/**` | 1-3 | PG→GaussDB 测试套件 |
| `dt-tests/tests/gaussdb_to_pg/**` | 2-3 | GaussDB→PG 测试套件 |

---

## 十一、监控与可观测性

### 11.1 监控指标

复用 ape-dts 现有的 Monitor 框架（`dt-common/src/monitor/`），为 GaussDB 同步任务提供以下监控指标：

#### 11.1.1 核心指标

| 指标类型 | 指标名称 | 说明 | 单位 |
|---------|---------|------|------|
| **吞吐量** | `ExtractedRecordCount` | 已抽取的记录总数 | 条 |
| **吞吐量** | `ExtractedDataSize` | 已抽取的数据总量 | 字节 |
| **吞吐量** | `SinkedRecordTotal` | 已写入的记录总数 | 条 |
| **吞吐量** | `SinkedByteTotal` | 已写入的数据总量 | 字节 |
| **性能** | `QPS` | 每秒处理记录数（时间窗口统计） | 条/秒 |
| **性能** | `RtPerQuery` | 单次查询响应时间 | 毫秒 |
| **延迟** | `CDC Lag` | CDC 同步延迟（源端 LSN - 当前消费 LSN） | 秒 |
| **缓冲** | `QueuedRecordCurrent` | 当前队列中的记录数 | 条 |
| **缓冲** | `QueuedByteCurrent` | 当前队列中的数据量 | 字节 |
| **错误** | `ErrorCount` | 错误次数 | 次 |
| **DDL** | `DDLRecordTotal` | DDL 事件总数 | 条 |

#### 11.1.2 GaussDB 特有指标

| 指标名称 | 说明 | 采集方式 |
|---------|------|---------|
| `GaussDB_Replication_Slot_Lag` | GaussDB 复制槽延迟 | 查询 `pg_replication_slots` 视图 |
| `GaussDB_WAL_LSN` | 当前 WAL LSN 位点 | 查询 `pg_current_wal_lsn()` |
| `GaussDB_Slot_Active` | 复制槽是否活跃 | 查询 `pg_replication_slots.active` |
| `GaussDB_JSON_Decode_Time` | JSON 解码耗时 | 内部计时器 |
| `GaussDB_Connection_Pool_Size` | 连接池大小 | sqlx Pool metrics |

### 11.2 日志规范

#### 11.2.1 日志级别

| 级别 | 使用场景 |
|------|---------|
| **ERROR** | 致命错误，任务无法继续（连接失败、复制槽丢失、数据类型不兼容） |
| **WARN** | 警告信息，任务可继续但需关注（DDL 跳过、类型转换降级、重试） |
| **INFO** | 关键节点信息（任务启动、复制槽创建、checkpoint、任务完成） |
| **DEBUG** | 调试信息（每条 DML 记录、JSON 解析详情） |

#### 11.2.2 关键日志点

| 日志内容 | 级别 | 示例 |
|---------|------|------|
| GaussDB 连接建立 | INFO | `Connected to GaussDB at gaussdb_host:5432, auth=SHA256` |
| mppdb_decoding 槽创建 | INFO | `Created replication slot 'ape_dts_slot' with mppdb_decoding at LSN 0/12345678` |
| JSON 解码失败 | ERROR | `Failed to decode mppdb_decoding JSON: invalid format at line 42` |
| GaussDB 特有类型转换 | DEBUG | `Converted GaussDB smalldatetime to ColValue::DateTime` |
| CDC 位点更新 | INFO | `Checkpoint saved: LSN=0/12345678, timestamp=2026-03-16T07:25:44Z` |
| 复制槽断开重连 | WARN | `Replication connection lost, retrying in 5s...` |

### 11.3 Prometheus Metrics（可选）

如果启用 `--features metrics`，导出以下 Prometheus 指标：

```
# 吞吐量
ape_dts_extracted_records_total{db_type="gaussdb_pg",task="pg_to_gaussdb"} 1000000
ape_dts_sinked_records_total{db_type="gaussdb_pg",task="pg_to_gaussdb"} 999950

# QPS
ape_dts_qps{db_type="gaussdb_pg",task="pg_to_gaussdb"} 5000

# CDC 延迟
ape_dts_cdc_lag_seconds{db_type="gaussdb_pg",task="gaussdb_to_pg"} 2.5

# 队列深度
ape_dts_queue_depth{db_type="gaussdb_pg",task="gaussdb_to_pg"} 1500

# 错误计数
ape_dts_errors_total{db_type="gaussdb_pg",task="gaussdb_to_pg",error_type="json_decode"} 3
```

### 11.4 告警规则

建议配置以下告警规则：

| 告警名称 | 条件 | 严重度 | 说明 |
|---------|------|--------|------|
| GaussDB CDC 延迟过高 | `cdc_lag > 60s` | Warning | CDC 同步延迟超过 1 分钟 |
| GaussDB CDC 延迟严重 | `cdc_lag > 300s` | Critical | CDC 同步延迟超过 5 分钟 |
| 复制槽断开 | `slot_active == false` | Critical | GaussDB 复制槽不活跃 |
| 队列积压 | `queue_depth > 10000` | Warning | 缓冲队列积压严重 |
| 错误率过高 | `error_rate > 10/min` | Critical | 每分钟错误超过 10 次 |
| QPS 异常下降 | `qps < 100 && lag > 10s` | Warning | QPS 低于 100 且有延迟 |

---

## 十二、故障排查指南

### 12.1 常见问题与解决方案

#### 12.1.1 连接问题

**问题**: `FATAL: password authentication failed for user "xxx"`

**原因**:
- GaussDB 使用 SHA256 认证，但 tokio-postgres 不支持
- 密码错误

**解决方案**:
1. 配置 GaussDB 使用 MD5 认证：`ALTER SYSTEM SET password_encryption_type = 1;`
2. 或使用支持 SHA256 的 rust-postgres fork
3. 检查密码是否正确

---

**问题**: `connection refused` 或 `timeout`

**原因**:
- GaussDB 端口未开放
- 防火墙阻止
- GaussDB 未启动

**解决方案**:
1. 检查 GaussDB 是否运行：`ps aux | grep gaussdb`
2. 检查端口监听：`netstat -tuln | grep 5432`
3. 检查防火墙规则：`iptables -L`
4. 检查 `pg_hba.conf` 是否允许远程连接

#### 12.1.2 CDC 问题

**问题**: `ERROR: logical decoding requires wal_level >= logical`

**原因**: GaussDB 的 `wal_level` 未设置为 `logical`

**解决方案**:
```sql
-- 检查当前配置
SHOW wal_level;

-- 修改配置（需要重启）
ALTER SYSTEM SET wal_level = 'logical';
ALTER SYSTEM SET max_replication_slots = 10;
ALTER SYSTEM SET max_wal_senders = 10;

-- 重启 GaussDB
```

---

**问题**: `ERROR: replication slot "ape_dts_slot" does not exist`

**原因**: 复制槽被删除或未创建

**解决方案**:
```sql
-- 查看现有复制槽
SELECT * FROM pg_replication_slots;

-- 手动创建复制槽
SELECT * FROM pg_create_logical_replication_slot('ape_dts_slot', 'mppdb_decoding');
```

---

**问题**: `ERROR: could not find plugin "mppdb_decoding"`

**原因**: GaussDB 未安装 mppdb_decoding 插件

**解决方案**:
1. 检查插件是否存在：`SELECT * FROM pg_available_extensions WHERE name = 'mppdb_decoding';`
2. 如果不存在，联系 GaussDB 管理员安装插件
3. 确认 GaussDB 版本支持逻辑复制

#### 12.1.3 JSON 解码问题

**问题**: `Failed to decode mppdb_decoding JSON: missing field 'columns_name'`

**原因**: mppdb_decoding 输出格式与预期不符

**解决方案**:
1. 抓取实际的 JSON 输出：启用 DEBUG 日志查看原始 JSON
2. 对比 POC 验证的格式，调整 `GaussDBJsonDecoder`
3. 检查 GaussDB 版本是否与 POC 环境一致

---

**问题**: `JSON parse error: invalid escape sequence`

**原因**: JSON 中包含特殊字符未正确转义

**解决方案**:
1. 在 JSON 解析前进行预处理，转义特殊字符
2. 使用更宽松的 JSON 解析器（如 `serde_json` 的 `from_str_lenient`）

#### 12.1.4 类型转换问题

**问题**: `Unsupported GaussDB type: smalldatetime (OID=xxxx)`

**原因**: GaussDB 特有类型未在 Type Registry 中注册

**解决方案**:
1. 连接 GaussDB 查询类型 OID：
   ```sql
   SELECT oid, typname FROM pg_type WHERE typname = 'smalldatetime';
   ```
2. 在 `dt-common/src/meta/pg/type_registry.rs` 中添加 OID 映射
3. 在 `pg_col_value_convertor.rs` 中添加转换逻辑

---

**问题**: `Type mismatch: expected int4, got smalldatetime`

**原因**: 源端和目标端类型不匹配

**解决方案**:
1. 使用 Lua Processor 进行类型转换
2. 修改目标端表结构以兼容源端类型
3. 在 Router 配置中添加列级映射和转换规则

#### 12.1.5 性能问题

**问题**: CDC 延迟持续增长

**原因**:
- Sinker 写入速度慢于 Extractor 抽取速度
- 网络带宽不足
- 目标端数据库负载过高

**解决方案**:
1. 增加 `parallel_size`（并行 Sinker 数量）
2. 增大 `batch_size`（批量写入大小）
3. 使用 `rdb_merge` 并行策略减少写入次数
4. 检查目标端数据库性能（CPU、磁盘 IO、锁等待）
5. 优化网络带宽

---

**问题**: QPS 很低，但 CPU 使用率不高

**原因**:
- 单条记录处理时间过长
- 频繁的小批量写入
- 锁竞争

**解决方案**:
1. 增大 `batch_size`
2. 检查是否有外键约束导致的锁等待
3. 临时禁用触发器：`SET session_replication_role = 'replica';`
4. 检查是否有慢查询

### 12.2 调试技巧

#### 12.2.1 启用详细日志

```bash
# 设置环境变量启用 DEBUG 日志
export RUST_LOG=dt_connector::extractor::gaussdb=debug,dt_task=info

# 运行任务
./dt-main task_config.ini
```

#### 12.2.2 抓取 mppdb_decoding 原始输出

```sql
-- 手动消费复制槽，查看 JSON 格式
SELECT * FROM pg_logical_slot_get_changes('ape_dts_slot', NULL, NULL);
```

#### 12.2.3 检查复制槽状态

```sql
-- 查看复制槽延迟
SELECT
    slot_name,
    active,
    pg_wal_lsn_diff(pg_current_wal_lsn(), restart_lsn) AS lag_bytes,
    pg_wal_lsn_diff(pg_current_wal_lsn(), confirmed_flush_lsn) AS confirmed_lag_bytes
FROM pg_replication_slots
WHERE slot_name = 'ape_dts_slot';
```

#### 12.2.4 性能分析

```bash
# 使用 perf 分析 CPU 热点
perf record -g ./dt-main task_config.ini
perf report

# 使用 flamegraph 生成火焰图
cargo flamegraph --bin dt-main -- task_config.ini
```

---

## 十三、性能基准与调优

### 13.1 性能目标

GaussDB 同步性能应不低于现有 PG→PG 同步的基准水平：

| 场景 | 指标 | 目标值 | 说明 |
|------|------|--------|------|
| **全量快照** | 吞吐量 | ≥ 50,000 行/秒 | 单表快照抽取速度 |
| **CDC 增量** | QPS | ≥ 10,000 行/秒 | 实时 CDC 处理速度 |
| **CDC 延迟** | Lag | < 3 秒 | 正常负载下的同步延迟 |
| **内存占用** | Memory | < 500 MB | 单任务内存占用 |
| **CPU 占用** | CPU | < 200% | 单任务 CPU 占用（多核） |

### 13.2 性能测试场景

#### 13.2.1 全量快照性能测试

**测试环境**:
- 源端：PostgreSQL 14，表大小 1000 万行
- 目标端：GaussDB HCS 25.1.30
- 网络：1 Gbps
- 硬件：8 核 CPU，16 GB RAM

**测试配置**:
```ini
[extractor]
db_type=pg
extract_type=snapshot
parallel_workers=4

[sinker]
db_type=gaussdb_pg
batch_size=500

[parallelizer]
parallel_type=snapshot
parallel_size=8
```

**预期结果**:
- 吞吐量：50,000 - 80,000 行/秒
- 总耗时：1000 万行约 2-3 分钟
- 内存占用：< 500 MB

#### 13.2.2 CDC 增量性能测试

**测试环境**:
- 源端：GaussDB HCS 25.1.30，TPS = 5000
- 目标端：PostgreSQL 14
- 网络：1 Gbps

**测试配置**:
```ini
[extractor]
db_type=gaussdb_pg
extract_type=cdc

[sinker]
db_type=pg
batch_size=200

[parallelizer]
parallel_type=rdb_merge
parallel_size=8
```

**预期结果**:
- QPS：10,000 - 15,000 行/秒
- CDC 延迟：< 3 秒（正常负载）
- CPU 占用：150% - 200%

### 13.3 性能调优指南

#### 13.3.1 Extractor 调优

| 参数 | 默认值 | 调优建议 | 影响 |
|------|--------|---------|------|
| `parallel_workers` | 1 | 设置为 CPU 核心数的 50%-100% | 提升快照抽取并行度 |
| `batch_size` | 100 | 增大到 500-1000 | 减少网络往返次数 |
| `buffer_size` | 16000 | 增大到 32000-64000 | 增加缓冲队列容量 |

#### 13.3.2 Sinker 调优

| 参数 | 默认值 | 调优建议 | 影响 |
|------|--------|---------|------|
| `batch_size` | 100 | 增大到 200-500 | 批量写入，减少事务开销 |
| `parallel_size` | 4 | 增大到 8-16 | 提升并行写入能力 |
| `replace` | false | 根据场景选择 | `true` 使用 REPLACE/ON CONFLICT，性能更好 |

#### 13.3.3 Parallelizer 调优

| 策略 | 适用场景 | 性能特点 |
|------|---------|---------|
| `serial` | 严格有序要求 | 性能最低，单线程 |
| `table` | 多表 CDC | 按表并行，适合多表场景 |
| `rdb_partition` | 单大表 CDC | 按主键 hash 分区，高并发 |
| `rdb_merge` | 高频更新 | 合并同行多次操作，减少写入 |

**推荐配置**:
- 多表场景：`parallel_type=table`，`parallel_size=表数量`
- 单大表场景：`parallel_type=rdb_partition`，`parallel_size=8-16`
- 高频更新场景：`parallel_type=rdb_merge`，`parallel_size=8`

#### 13.3.4 网络调优

- 启用 TCP keepalive：`tcp_keepalives_idle=60`
- 增大 TCP 缓冲区：`sysctl -w net.ipv4.tcp_rmem="4096 87380 16777216"`
- 使用专用网络链路，避免与其他流量竞争

#### 13.3.5 GaussDB 端调优

```sql
-- 增加 WAL 发送器和复制槽数量
ALTER SYSTEM SET max_wal_senders = 20;
ALTER SYSTEM SET max_replication_slots = 20;

-- 增大 WAL 缓冲区
ALTER SYSTEM SET wal_buffers = '16MB';

-- 调整检查点频率（减少 WAL 写入压力）
ALTER SYSTEM SET checkpoint_timeout = '15min';
ALTER SYSTEM SET checkpoint_completion_target = 0.9;
```

### 13.4 性能瓶颈分析

| 瓶颈类型 | 症状 | 排查方法 | 解决方案 |
|---------|------|---------|---------|
| **CPU 瓶颈** | CPU 100%，QPS 低 | `top`、`perf` | 增加 parallel_size，优化热点代码 |
| **内存瓶颈** | OOM，频繁 GC | `free -h`、`ps aux` | 减小 buffer_size，降低并行度 |
| **网络瓶颈** | 网络带宽打满 | `iftop`、`nethogs` | 压缩传输，增加带宽 |
| **磁盘 IO 瓶颈** | iowait 高 | `iostat -x 1` | 使用 SSD，优化索引 |
| **锁竞争** | 大量锁等待 | `pg_locks`、`pg_stat_activity` | 禁用外键，使用 replica 角色 |

---

## 十四、部署指南

### 14.1 环境准备

#### 14.1.1 系统要求

| 组件 | 最低要求 | 推荐配置 |
|------|---------|---------|
| **操作系统** | Linux (CentOS 7+, Ubuntu 18.04+) | CentOS 8 / Ubuntu 20.04 |
| **CPU** | 4 核 | 8 核+ |
| **内存** | 8 GB | 16 GB+ |
| **磁盘** | 50 GB | 100 GB+ SSD |
| **网络** | 100 Mbps | 1 Gbps+ |

#### 14.1.2 依赖安装

```bash
# Rust 工具链
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup install 1.85.0
rustup default 1.85.0

# 系统依赖
# CentOS/RHEL
sudo yum install -y gcc cmake clang-devel librdkafka-devel openssl-devel

# Ubuntu/Debian
sudo apt-get install -y build-essential cmake libclang-dev librdkafka-dev libssl-dev
```

### 14.2 编译构建

```bash
# 克隆代码
git clone https://github.com/apecloud/ape-dts.git
cd ape-dts

# 初始化子模块
make init

# 编译 Release 版本
make build

# 编译产物
ls -lh target/release/dt-main
```

### 14.3 Docker 部署

#### 14.3.1 构建镜像

```bash
# 构建 Docker 镜像
make docker-build

# 查看镜像
docker images | grep ape-dts
```

#### 14.3.2 运行容器

```bash
# 准备配置文件
cat > task_config.ini <<EOF
[extractor]
db_type=pg
extract_type=cdc
url=postgres://user:pass@pg_host:5432/mydb

[sinker]
db_type=gaussdb_pg
sink_type=write
url=postgres://user:pass@gaussdb_host:5432/mydb
EOF

# 运行容器
docker run -d \
  --name ape-dts-gaussdb \
  -v $(pwd)/task_config.ini:/app/task_config.ini \
  -v $(pwd)/logs:/app/logs \
  apecloud/ape-dts:2.0.25 \
  /app/task_config.ini
```

### 14.4 生产环境部署

#### 14.4.1 systemd 服务配置

```ini
# /etc/systemd/system/ape-dts-gaussdb.service
[Unit]
Description=ape-dts GaussDB Sync Task
After=network.target

[Service]
Type=simple
User=ape-dts
WorkingDirectory=/opt/ape-dts
ExecStart=/opt/ape-dts/dt-main /opt/ape-dts/config/task_config.ini
Restart=on-failure
RestartSec=10s
StandardOutput=append:/var/log/ape-dts/stdout.log
StandardError=append:/var/log/ape-dts/stderr.log

[Install]
WantedBy=multi-user.target
```

```bash
# 启动服务
sudo systemctl daemon-reload
sudo systemctl enable ape-dts-gaussdb
sudo systemctl start ape-dts-gaussdb

# 查看状态
sudo systemctl status ape-dts-gaussdb
```

#### 14.4.2 日志轮转配置

```bash
# /etc/logrotate.d/ape-dts
/var/log/ape-dts/*.log {
    daily
    rotate 7
    compress
    delaycompress
    missingok
    notifempty
    create 0644 ape-dts ape-dts
    postrotate
        systemctl reload ape-dts-gaussdb > /dev/null 2>&1 || true
    endscript
}
```

### 14.5 高可用部署

#### 14.5.1 主备模式

```
┌─────────────┐         ┌─────────────┐
│  ape-dts    │         │  ape-dts    │
│  (Primary)  │ ◄─────► │  (Standby)  │
└─────────────┘         └─────────────┘
      │                       │
      │                       │
      ▼                       ▼
┌─────────────┐         ┌─────────────┐
│   GaussDB   │         │   GaussDB   │
│  (Primary)  │ ◄─────► │  (Standby)  │
└─────────────┘         └─────────────┘
```

**实现方式**:
- 使用 Keepalived 或 Pacemaker 实现主备切换
- 主节点故障时，备节点自动接管
- 通过共享存储或数据库记录同步位点

#### 14.5.2 监控告警集成

```yaml
# Prometheus 配置
scrape_configs:
  - job_name: 'ape-dts-gaussdb'
    static_configs:
      - targets: ['localhost:9090']
    metrics_path: '/metrics'
```

```yaml
# Alertmanager 告警规则
groups:
  - name: ape-dts-gaussdb
    rules:
      - alert: GaussDBCDCLagHigh
        expr: ape_dts_cdc_lag_seconds > 60
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "GaussDB CDC lag is high"
```

---

## 十五、开发工作流程

### 15.1 开发环境搭建

```bash
# 1. 克隆代码
git clone https://github.com/apecloud/ape-dts.git
cd ape-dts

# 2. 初始化子模块
make init

# 3. 安装开发工具
rustup component add rustfmt clippy

# 4. 编译 Debug 版本
make build-debug

# 5. 运行测试
make test
```

### 15.2 分支管理策略

| 分支类型 | 命名规范 | 说明 |
|---------|---------|------|
| **主分支** | `main` | 稳定版本，受保护 |
| **开发分支** | `develop` | 集成分支 |
| **功能分支** | `feature/gaussdb-cdc-extractor` | 新功能开发 |
| **修复分支** | `fix/gaussdb-json-decode-error` | Bug 修复 |
| **发布分支** | `release/2.1.0` | 版本发布准备 |

### 15.3 开发流程

#### 15.3.1 Phase 1 开发流程（第1周）

```bash
# 1. 创建功能分支
git checkout -b feature/gaussdb-sinker

# 2. 修改文件（参考 5.1 节文件清单）
# - dt-common/src/config/config_enums.rs
# - dt-common/src/config/task_config.rs
# - ...

# 3. 本地测试
cargo test --package dt-common --lib config::config_enums
cargo test --package dt-task --lib sinker_util

# 4. 代码格式化和 Lint
cargo fmt
cargo clippy -- -D warnings

# 5. 提交代码
git add .
git commit -m "feat: add GaussDB sinker support (Phase 1)

- Add GaussDBPg/GaussDBMySQL/GaussDBOracle DbType enums
- Reuse PG sinker for GaussDBPg mode
- Add system DB filtering for GaussDB
- Update precheck builder for GaussDB

Refs: #123"

# 6. 推送并创建 PR
git push origin feature/gaussdb-sinker
gh pr create --title "feat: GaussDB Sinker Support (Phase 1)" \
  --body "Implements Phase 1 of GaussDB support..."
```

#### 15.3.2 Phase 2 开发流程（第2-3周）

```bash
# 1. 创建功能分支
git checkout -b feature/gaussdb-cdc-extractor

# 2. 新建文件（参考 5.2 节文件清单）
mkdir -p dt-connector/src/extractor/gaussdb
touch dt-connector/src/extractor/gaussdb/mod.rs
touch dt-connector/src/extractor/gaussdb/gaussdb_cdc_client.rs
touch dt-connector/src/extractor/gaussdb/gaussdb_cdc_extractor.rs
touch dt-connector/src/extractor/gaussdb/gaussdb_json_decoder.rs

# 3. 实现 GaussDB CDC Extractor
# ...

# 4. 单元测试
cargo test --package dt-connector --lib extractor::gaussdb

# 5. 集成测试
cd dt-tests
cargo test --test integration_test gaussdb_to_pg::cdc::basic_test

# 6. 提交并创建 PR
git add .
git commit -m "feat: add GaussDB CDC extractor (Phase 2)

- Implement GaussDBCdcClient with mppdb_decoding support
- Add GaussDBJsonDecoder for JSON format parsing
- Support LSN-based position tracking
- Add断点续传 support

Refs: #124"
```

### 15.4 代码审查清单

#### 15.4.1 功能审查

- [ ] 是否按照 TiDB 复用模式实现？
- [ ] 是否复用了现有的 PG/MySQL 组件？
- [ ] 是否添加了必要的错误处理？
- [ ] 是否支持断点续传？
- [ ] 是否添加了日志输出？

#### 15.4.2 性能审查

- [ ] 是否有不必要的内存拷贝？
- [ ] 是否有阻塞操作？
- [ ] 批量操作是否合理？
- [ ] 是否有资源泄漏风险？

#### 15.4.3 测试审查

- [ ] 是否添加了单元测试？
- [ ] 是否添加了集成测试？
- [ ] 测试覆盖率是否 > 80%？
- [ ] 是否测试了异常场景？

### 15.5 测试执行

#### 15.5.1 单元测试

```bash
# 运行所有单元测试
cargo test --lib

# 运行特定模块测试
cargo test --package dt-connector --lib extractor::gaussdb

# 查看测试覆盖率
cargo tarpaulin --out Html --output-dir coverage
```

#### 15.5.2 集成测试

```bash
# 准备测试环境
docker-compose -f dt-tests/docker-compose.yml up -d

# 运行 PG → GaussDB 测试
cargo test --test integration_test pg_to_gaussdb

# 运行 GaussDB → PG 测试
cargo test --test integration_test gaussdb_to_pg

# 清理测试环境
docker-compose -f dt-tests/docker-compose.yml down
```

### 15.6 发布流程

#### 15.6.1 版本号规范

遵循语义化版本（Semantic Versioning）：

- **主版本号（Major）**: 不兼容的 API 变更
- **次版本号（Minor）**: 向后兼容的功能新增
- **修订号（Patch）**: 向后兼容的 Bug 修复

GaussDB 支持作为新功能，版本号从 `2.0.25` 升级到 `2.1.0`。

#### 15.6.2 发布步骤

```bash
# 1. 创建发布分支
git checkout -b release/2.1.0

# 2. 更新版本号
# 修改 Cargo.toml 中的 version = "2.1.0"

# 3. 更新 CHANGELOG.md
cat >> CHANGELOG.md <<EOF
## [2.1.0] - 2026-03-30

### Added
- GaussDB PostgreSQL 兼容模式支持（Sinker + Extractor）
- GaussDB CDC 增量同步（基于 mppdb_decoding）
- GaussDB 结构同步（表、索引、约束、视图、序列、函数、权限）
- GaussDB 数据校验
- GaussDB SHA256 认证支持

### Changed
- 优化 PG CDC 性能

### Fixed
- 修复 PG Sinker 的类型转换问题
EOF

# 4. 构建 Release 版本
make build

# 5. 运行完整测试套件
make test

# 6. 创建 Git Tag
git tag -a v2.1.0 -m "Release v2.1.0: GaussDB Support"
git push origin v2.1.0

# 7. 构建 Docker 镜像
make docker-build
docker tag apecloud/ape-dts:latest apecloud/ape-dts:2.1.0
docker push apecloud/ape-dts:2.1.0

# 8. 创建 GitHub Release
gh release create v2.1.0 \
  --title "v2.1.0: GaussDB Support" \
  --notes-file CHANGELOG.md \
  target/release/dt-main
```

---

## 十六、附录

### 16.1 术语表

| 术语 | 全称 | 说明 |
|------|------|------|
| **GaussDB** | Huawei GaussDB | 华为企业级分布式数据库 |
| **openGauss** | openGauss | GaussDB 的开源版本 |
| **CDC** | Change Data Capture | 变更数据捕获 |
| **LSN** | Log Sequence Number | 日志序列号 |
| **WAL** | Write-Ahead Log | 预写式日志 |
| **mppdb_decoding** | - | GaussDB 逻辑复制插件 |
| **pgoutput** | - | PostgreSQL 逻辑复制插件 |
| **Sinker** | - | 数据写入组件 |
| **Extractor** | - | 数据抽取组件 |
| **Parallelizer** | - | 并行分发组件 |
| **OID** | Object Identifier | PostgreSQL 对象标识符 |

### 16.2 参考文档

| 文档 | 链接 |
|------|------|
| GaussDB 产品文档 | https://doc.hcs.huawei.com/db/en-us/gaussdbqlh/25.1.30/ |
| openGauss 官方文档 | https://docs.opengauss.org/ |
| PostgreSQL 逻辑复制 | https://www.postgresql.org/docs/current/logical-replication.html |
| ape-dts GitHub | https://github.com/apecloud/ape-dts |
| Rust 异步编程 | https://rust-lang.github.io/async-book/ |

### 16.3 联系方式

| 角色 | 联系方式 |
|------|---------|
| **项目负责人** | [待填写] |
| **技术负责人** | [待填写] |
| **测试负责人** | [待填写] |
| **技术支持** | [待填写] |

---

**文档结束**

> 本 PRD 共 16 章，涵盖背景、需求、架构、实现、测试、部署、监控、故障排查、性能调优、开发流程等完整内容，为 GaussDB 二次开发提供全面指导。
