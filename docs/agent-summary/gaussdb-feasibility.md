# GaussDB 扩展可行性评估报告

## Context

Boss 需要评估在 ape-dts 中扩展 GaussDB 支持的可行性。GaussDB 是华为基于 openGauss（源自 PostgreSQL 9.2/Postgres-XC）的企业级数据库。ape-dts 当前支持 10 种数据源，采用 Trait 多态 + 工厂模式架构，新增数据库类型需修改 15+ 文件。

参考地址: https://support.huaweicloud.com/intl/en-us/productdesc-gaussdb/gaussdb_01_003.html

---

## 一、多维度可行性评估

### 1. 协议与驱动兼容性 — 风险：中等

| 评估项 | 结论 |
|--------|------|
| Wire Protocol (libpq v3) | **兼容** — GaussDB 支持 PG 线协议，tokio-postgres/sqlx 可连接 |
| JDBC/ODBC 驱动 | **兼容** — GaussDB 官方确认支持开源 PG 驱动 |
| 认证协议 | **有条件兼容** — GaussDB 默认 SHA256（华为自研，非 SCRAM-SHA-256），标准 PG 驱动仅支持 MD5。需配置 `password_encryption_type=1` 使用 MD5 |
| TLS | 当前 CDC 客户端使用 NoTls（`pg_cdc_client.rs:47`），内网场景可行 |
| 连接池（sqlx::PgPool） | **兼容** — 标准 PG 连接池可用 |

**结论**: 驱动层可行，但需要求 GaussDB 部署时配置 MD5 认证。或者修改 apecloud/rust-postgres fork 支持 GaussDB SHA256。

### 2. CDC/逻辑复制 — 风险：高（最大挑战）

| 特性 | PostgreSQL (当前实现) | GaussDB (openGauss) | 兼容性 |
|------|----------------------|---------------------|--------|
| 复制槽创建 | `CREATE_REPLICATION_SLOT ... LOGICAL "pgoutput"` | `pg_create_logical_replication_slot(name, 'mppdb_decoding')` | **不兼容** |
| 输出插件 | `pgoutput` | `mppdb_decoding` | **不兼容** |
| Publication 模型 | `CREATE PUBLICATION ... FOR ALL TABLES` | **不支持** Publication/Subscription | **不兼容** |
| 消息解码格式 | pgoutput 二进制（Begin/Commit/Insert/Update/Delete/Relation） | mppdb_decoding 自有格式（JSON 或自定义） | **不兼容** |
| LSN 位点格式 | X/Y 格式 | 兼容的 LSN 格式 | **兼容** |
| StandbyStatusUpdate | 标准 PG 协议 | 兼容 | **兼容** |
| wal_level=logical | 需配置 | 需配置 | **兼容** |

**核心问题**: `pgoutput` 解码器和 `CREATE PUBLICATION` 在 GaussDB 中**不可用**。当前 `PgCdcClient` (`pg_cdc_client.rs`) 和 `PgCdcExtractor` (`pg_cdc_extractor.rs`) 的 CDC 实现**无法直接复用**。

**需要 POC 验证**:
- mppdb_decoding 输出格式的具体结构
- 逻辑复制槽的创建和管理 API
- WAL 流的消息格式是否可用 postgres_protocol 库解析

### 3. SQL 语法兼容性（Sinker）— 风险：低

| SQL 特性 | 使用位置 | GaussDB 支持 |
|----------|---------|-------------|
| `ON CONFLICT (pk) DO UPDATE SET` | `rdb_query_builder.rs:225-258` | **兼容**（openGauss 3.0+） |
| `SET search_path` | `pg_sinker.rs` | **兼容** |
| `SET extra_float_digits=3` | `pg_cdc_client.rs:168` | **兼容** |
| `SET session_replication_role = 'replica'` | `task_util.rs:140` | **兼容** |
| `$N::type` 参数化查询 | `rdb_query_builder.rs:602-621` | **兼容** |
| `"schema"."table"` 双引号标识符 | 全局 | **兼容** |
| `current_database()` | `task_util.rs:516` | **兼容** |
| `DELETE/INSERT/UPDATE` 标准 DML | sinker 全局 | **兼容** |

**结论**: Sinker 完全可复用 PG 实现，openGauss 3.0+ 的 ON CONFLICT 支持是关键。

### 4. 元数据与类型系统 — 风险：低-中

| 系统表/函数 | 使用位置 | GaussDB 支持 |
|-------------|---------|-------------|
| `pg_catalog.pg_type` | `type_registry.rs` | **兼容** |
| `pg_catalog.pg_namespace` | `pg_meta_manager.rs` | **兼容** |
| `pg_catalog.pg_enum` | `type_registry.rs` | **兼容** |
| `pg_class`, `pg_attribute` | metadata 查询 | **兼容** |
| `information_schema.columns/tables` | struct extractor, precheck | **兼容** |
| `pg_get_indexdef()` | `pg_struct_fetcher.rs` | **兼容** |
| `pg_get_constraintdef()` | `pg_struct_fetcher.rs` | **兼容** |
| `current_setting('server_version_num')` | `pg_prechecker.rs` | **需调整** — GaussDB 返回自己的版本号格式 |
| `format_type(atttypid, atttypmod)` | metadata 查询 | **兼容** |

**GaussDB 特有数据类型**（需新增映射）:
- `smalldatetime` — 需添加 OID 映射和 ColValue 转换
- `tinyint` — 需添加 OID 映射
- `nvarchar2` — Oracle 兼容类型
- `clob`, `blob` — Oracle 兼容类型

**结论**: pg_catalog 系统表高度兼容，OID 体系一致。版本检查需调整，GaussDB 特有类型需增量添加。

### 5. Precheck 兼容性 — 风险：中

| 检查项 | 当前实现 | GaussDB 调整 |
|--------|---------|-------------|
| 版本检查 | `PG_SUPPORT_DB_VERSION_NUM_MIN = 120000` | 需改为 GaussDB 版本号格式 |
| `wal_level = 'logical'` | `pg_prechecker.rs:91` | **兼容** |
| `max_replication_slots > 0` | `pg_prechecker.rs:160` | **兼容** |
| `max_wal_senders > 0` | `pg_prechecker.rs:173` | **兼容** |
| schema/table 发现 | `information_schema` | **兼容** |
| 系统库过滤 | `pg_catalog, information_schema` | 需增加 GaussDB 特有系统 schema |

### 6. DDL 解析兼容性 — 风险：中

`ddl_parser.rs` 中有 15+ 处 `if self.db_type == DbType::Pg` 的 PG 特有 DDL 解析逻辑。GaussDB 的 DDL 语法与 PG 基本兼容，但可能有扩展语法（如分布式表、列存表）需要处理。

---

## 二、实现策略评估

### 策略 A：完全复用 PG 实现（TiDB 模式）

**TiDB 先例**: `DbType::Mysql | DbType::Tidb` 共享 MySQL sinker（`task_config.rs:487`），TiDB 仅作为 sinker 目标。

**对 GaussDB 的适用性**:
- Sinker: **完全适用** — `DbType::Pg | DbType::GaussDB`
- Snapshot Extractor: **完全适用** — 使用标准 SQL
- Struct Extractor: **基本适用** — 使用 pg_catalog
- CDC Extractor: **不适用** — pgoutput/Publication 不兼容

### 策略 B：完全独立实现

**评估**: 不推荐。GaussDB 与 PG 的 SQL/Catalog 兼容度极高，独立实现会产生大量重复代码。

### 策略 C：混合策略（推荐）

| 组件 | 策略 | 理由 |
|------|------|------|
| Sinker (DML/DDL/Struct) | 复用 PG | SQL 语法完全兼容，ON CONFLICT 可用 |
| Snapshot Extractor | 复用 PG | 标准 SQL + sqlx 连接池 |
| Struct Extractor | 复用 PG（微调） | pg_catalog 兼容，版本检查需调整 |
| CDC Extractor | **新建** | pgoutput/Publication 不兼容，需实现 mppdb_decoding 解析 |
| Precheck | 扩展 PG（参数化） | 大部分检查兼容，版本检查需调整 |
| MetaManager | 复用 PG（扩展） | OID 体系一致，增加 GaussDB 特有类型 |

---

## 三、实现路线图

### Phase 1: GaussDB 作为 Sinker 目标（最小可行，约 2-3 天）

支持 MySQL/PG → GaussDB 迁移场景。

**需修改文件**:
1. `dt-common/src/config/config_enums.rs` — 添加 `GaussDB` DbType 变体
2. `dt-common/src/config/task_config.rs` — sinker 配置 `DbType::Pg | DbType::GaussDB`
3. `dt-common/src/system_dbs.rs` — 添加 GaussDB 系统库过滤（同 PG）
4. `dt-common/src/utils/sql_util.rs` — 添加 GaussDB 转义规则（同 PG）
5. `dt-task/src/task_util.rs` — ConnClient 处理 GaussDB（复用 PG 连接池）
6. `dt-task/src/sinker_util.rs` — GaussDB sinker 创建（映射到 PG sinker）
7. `dt-precheck/src/builder/prechecker_builder.rs` — GaussDB 预检查
8. `dt-connector/src/data_marker.rs` — 数据标记支持 GaussDB
9. `dt-common/src/meta/ddl_meta/ddl_parser.rs` — DDL 解析支持 GaussDB（`DbType::Pg | DbType::GaussDB`）
10. `dt-common/src/meta/struct_meta/structure/constraint.rs` — 约束类型映射
11. `dt-connector/src/rdb_query_builder.rs` — 查询构建（复用 PG 逻辑）

### Phase 2: GaussDB Snapshot + Struct Extractor（约 1-2 天）

支持 GaussDB → X 全量迁移场景。

**需修改文件**:
1. `dt-common/src/config/task_config.rs` — extractor 配置添加 GaussDB 分支
2. `dt-common/src/config/extractor_config.rs` — 添加 GaussDB Snapshot/Struct/Check 变体（或复用 PG 变体）
3. `dt-task/src/extractor_util.rs` — GaussDB snapshot/struct extractor 创建
4. `dt-precheck/src/prechecker/pg_prechecker.rs` — 调整版本检查逻辑
5. `dt-connector/src/extractor/resumer/utils.rs` — 断点续传支持 GaussDB

### Phase 3: GaussDB CDC Extractor（约 5-10 天，最大工作量）

支持 GaussDB → X 增量同步场景。**需先完成 POC 验证 mppdb_decoding 格式**。

**新建文件**:
1. `dt-connector/src/extractor/gaussdb/mod.rs`
2. `dt-connector/src/extractor/gaussdb/gaussdb_cdc_client.rs` — mppdb_decoding 槽管理
3. `dt-connector/src/extractor/gaussdb/gaussdb_cdc_extractor.rs` — 消息解析与转换

**需修改文件**:
1. `dt-common/src/config/extractor_config.rs` — 添加 `GaussDBCdc` 变体
2. `dt-common/src/config/task_config.rs` — GaussDB CDC extractor 配置
3. `dt-task/src/extractor_util.rs` — 注册 GaussDB CDC extractor
4. `dt-common/src/meta/position.rs` — 添加 GaussDB CDC 位点类型（如 LSN 格式兼容可复用 PgCdc）

### Phase 4: GaussDB 特有类型支持（约 1-2 天，可增量）

**需修改文件**:
1. `dt-common/src/meta/pg/type_registry.rs` — 添加 GaussDB 特有 OID
2. `dt-common/src/meta/adaptor/pg_col_value_convertor.rs` — 添加 smalldatetime/tinyint 等转换

---

## 四、风险矩阵

| 风险项 | 严重度 | 概率 | 影响 | 缓解方案 |
|--------|--------|------|------|---------|
| SHA256 认证不兼容 | 高 | 中 | 驱动无法连接 | 配置 MD5 认证；或 fork 驱动添加 SHA256 支持 |
| pgoutput 不可用 | **致命** | 确定 | CDC 无法工作 | 必须实现 mppdb_decoding 解析器 |
| mppdb_decoding 格式未知 | 高 | 中 | CDC 开发无法推进 | 需 POC 抓取并文档化输出格式 |
| ON CONFLICT 版本限制 | 中 | 低 | Sinker upsert 不工作 | 限定 openGauss 3.0+ 为最低版本 |
| GaussDB 版本碎片化 | 中 | 中 | 不同版本行为不一致 | 明确目标版本（建议 openGauss 5.0+） |
| pg_catalog 差异 | 低 | 低 | 元数据查询失败 | 增量修复 |

---

## 五、POC 验证清单（实施前必做）

1. **POC-1**: tokio-postgres 连接 GaussDB（MD5 认证）— 验证基础连通性
2. **POC-2**: sqlx::PgPool 连接 GaussDB — 验证连接池和查询执行
3. **POC-3**: 查询 `pg_catalog.pg_type` — 验证 OID 兼容性
4. **POC-4**: 创建 mppdb_decoding 逻辑复制槽 — 抓取输出格式
5. **POC-5**: 执行 `ON CONFLICT DO UPDATE` — 验证 upsert 兼容性
6. **POC-6**: `current_setting('server_version_num')` — 检查版本号格式

---

## 六、总结与建议

### 可行性结论：**可行，但 CDC 是关键挑战**

| 功能 | 可行性 | 复用度 | 工作量 |
|------|--------|--------|--------|
| GaussDB 作为 Sinker | **高** | ~95% 复用 PG | 2-3 天 |
| GaussDB Snapshot Extractor | **高** | ~90% 复用 PG | 1-2 天 |
| GaussDB Struct Extractor | **高** | ~85% 复用 PG | 含在 Phase 2 |
| GaussDB CDC Extractor | **中** | ~30% 复用（仅连接层） | 5-10 天 |
| GaussDB 特有类型 | **高** | 增量扩展 | 1-2 天 |

### 推荐路径

1. **先做 Phase 1**（Sinker）— 风险最低、价值最高，立即支持 "任意数据库 → GaussDB" 迁移
2. **并行执行 POC** — 验证驱动连通性和 CDC 格式
3. **再做 Phase 2**（Snapshot）— 支持 "GaussDB → 任意数据库" 全量迁移
4. **POC 通过后做 Phase 3**（CDC）— 支持 "GaussDB → 任意数据库" 增量同步
5. **Phase 4 按需** — 遇到 GaussDB 特有类型时增量添加

**总工期估算**: Phase 1-2 约 3-5 天，Phase 3 约 5-10 天（依赖 POC 结果），总计 ~10-18 天

### 关键文件参考

| 文件 | 角色 |
|------|------|
| `dt-common/src/config/config_enums.rs` | DbType 枚举定义 |
| `dt-common/src/config/task_config.rs:487` | TiDB 复用模式参考（`DbType::Mysql \| DbType::Tidb`） |
| `dt-connector/src/extractor/pg/pg_cdc_client.rs` | PG CDC 客户端（GaussDB 不兼容部分） |
| `dt-connector/src/sinker/pg/pg_sinker.rs` | PG Sinker（GaussDB 可复用） |
| `dt-connector/src/rdb_query_builder.rs:225` | ON CONFLICT 查询构建 |
| `dt-common/src/meta/pg/type_registry.rs` | 类型 OID 注册表 |
| `dt-task/src/task_util.rs` | 连接池创建和数据库操作 |
| `dt-task/src/extractor_util.rs` | Extractor 工厂方法 |
| `dt-task/src/sinker_util.rs` | Sinker 工厂方法 |
