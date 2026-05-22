# SPEC — GaussDBOracle 后续推进（non-CDC：snapshot/struct/check + 远端 HA + Oracle XE Docker）

## 目标

在既有 `DbType::GaussDBOracle`（pg-wire + Oracle compatibility mode）bootstrap 基础上，推进到可回归的 non-CDC 主路径：

- `PG -> GaussDBOracle`：补齐 **struct/check** 的 `dt-tests` 自动化入口
- 适配 **Oracle 兼容模式**下不完全支持的 PG DDL（例如 `CREATE SCHEMA IF NOT EXISTS`）
- 让 `gaussdb_pg_candidate_hosts` 在 `GaussDBOracle` 场景同样生效（HA 环境自动选 RW 主）
- 按要求落地 **本机 Oracle 环境**（`wnameless/oracle-xe-11g-r2:latest`）docker compose

## 范围

- 代码适配：
  - `dt-tests`：把 `DbType::GaussDBOracle` 视为 pg-wire GaussDB，纳入 RW 选主与执行 SQL 的逻辑
  - `dt-common`：struct DDL 生成对 `GaussDBOracle` 做兼容（避免 `IF NOT EXISTS` 等不支持语法，并对索引做幂等包装）
  - `dt-connector`：struct check fetcher 对 `GaussDBOracle` 复用 `GaussDBPg` 的“老 PG catalog”降级查询
  - `dt-tests` struct runner：把 `GaussDBOracle` 纳入 cross-engine normalization（ubtree/btree、summary keys 子集等）
- 测试用例：
  - 新增 `pg_to_gaussdb_oracle` 的 `struct basic` 与 `check basic` 夹具与入口
- 环境：
  - 新增 `dt-tests/docker-compose.oracle_xe.yml`（Oracle XE 11gR2）

## 非目标（本任务不做）

- 不实现 Oracle wire-protocol/OCI 级别的连接器（只提供 Oracle XE 容器环境）
- 不在本任务引入 `GaussDBOracle` 的 CDC/resume/failover

## 验收标准

- `dt-tests`：
  - `pg_to_gaussdb_oracle::snapshot_tests::test::smoke_test` PASS
  - `pg_to_gaussdb_oracle::struct_tests::test::struct_basic_test` PASS
  - `pg_to_gaussdb_oracle::check_tests::test::check_basic_test` PASS
- `Oracle XE`：
  - `docker compose -f dt-tests/docker-compose.oracle_xe.yml up -d` 后容器 `oracle-xe-local` 健康检查为 `healthy`

