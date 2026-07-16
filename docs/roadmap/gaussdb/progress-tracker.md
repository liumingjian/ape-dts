# GaussDB 全局进度跟踪清单（PRD 真相源）

> 最后更新：**2026-04-16（PG ↔ GaussDBOracle：snapshot/struct/check/precheck/cdc basic PASS；Oracle ↔ GaussDBOracle：snapshot/struct/check/cdc basic（logminer）+ precheck basic PASS；GaussDBOracle → Oracle：snapshot/struct/check/precheck/cdc basic PASS；Oracle XE 本机 docker ready；GaussDBPg→MySQL bootstrap：struct advanced PASS + 一键 e2e 脚本落盘；MySQL→GaussDBMySQL 目标端 failover self-heal PASS）**
>
> 目标：每完成一次 spec 任务后，都能立刻知道“当前已交付什么、证据在哪、下一步做什么”。

## 1. 真相源与更新规则

### 1.1 真相源（优先级）

1. **需求真相源**：`docs/roadmap/gaussdb/prd.md`
2. **实现与验证真相源**：仓库中已跟踪的代码、测试、脚本和运行文档
3. **历史执行背景**：此前的本地任务表、进度记录和原始证据未纳入仓库；本文保留其日期、测试名与 PASS/FAIL/BLOCKED 结论，但不将其作为可访问证据链接

### 1.2 更新规则（强制）

- 每个 spec 完成时：
  - 更新已跟踪的实现、测试与运行文档
  - 将验证命令、结果和必要的诊断写入对应的已跟踪文档
  - 同步更新本文档的对应条目（状态 + 可访问证据）
- 不提交凭据：`.env.local`、`.local/`、带口令的 URL 等严禁进入 git。
- 原始输出默认保留在忽略的本地目录；如确需提交，只允许提交**脱敏片段**（推荐只保留关键日志的 30-80 行，并确保无账号/口令）。

## 2. Dashboard（能力矩阵）

| 方向 | snapshot | cdc | struct | check | precheck | docs/runbook | e2e |
|---|---|---|---|---|---|---|---|
| **PG → GaussDBPg** | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| **GaussDBPg → PG** | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| **GaussDBPg → MySQL（bootstrap）** | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| **MySQL → GaussDBMySQL（首波）** | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| **PG → GaussDBOracle** | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| **GaussDBOracle → PG** | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| **Oracle -> GaussDBOracle（bootstrap）** | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| **GaussDBOracle -> Oracle（snapshot bootstrap）** | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |

说明：

- Struct（PRD MVP）已补齐：在已有 `table/index/constraint/sequence/comment/rbac` 基础上补齐 **view/matview/routine/routine grants**。
- `MySQL → GaussDBMySQL（首波）` 已覆盖 `snapshot/struct/check/cdc(DML)`，并已归档真实环境证据。
- `MySQL → GaussDBMySQL（首波）` 的 `precheck` 已完成独立自动化入口、真实环境证据与无污染校验。
- `GaussDBPg → MySQL（bootstrap）` 已新增 `dt-tests` 入口并完成真实验证：snapshot/check/cdc basic（证据见 历史本地任务/证据（未纳入仓库））。
- `GaussDBPg → MySQL（bootstrap）` 的 `precheck` 已新增 `dt-tests` 入口（证据见 历史本地任务/证据（未纳入仓库））。
- `GaussDBPg → MySQL（bootstrap）` 的 `struct` 已新增 `dt-tests` 入口 `gaussdb_to_mysql::struct_tests::test::struct_basic_test` 并完成真实验证（证据见 历史本地任务/证据（未纳入仓库））。
- `GaussDBPg → MySQL（bootstrap）` 的 `struct advanced` 已覆盖 `default/index(ubtree)` 并完成真实验证（证据见 历史本地任务/证据（未纳入仓库））。
- `GaussDBPg → MySQL（bootstrap）` 已补齐一键回归脚本：`bash scripts/e2e/gaussdb_to_mysql_bootstrap.sh`（quick/full，详见 `docs/roadmap/gaussdb/e2e-test-plan.md`）。
- `PG ↔ GaussDBOracle` 已交付双向同步最小闭环：`snapshot/struct/check/precheck/cdc basic`（证据见 历史本地任务/证据（未纳入仓库））。
  - `PG -> GaussDBOracle`: `pg_to_gaussdb_oracle::cdc_tests::test::cdc_basic_test` PASS
  - `GaussDBOracle -> PG`: `gaussdb_oracle_to_pg::cdc_tests::test::cdc_basic_test` PASS
- `GaussDBOracle` 同时保留远端 oracle-mode `testdb` 的 non-CDC basic 证据（见 历史本地任务/证据（未纳入仓库））。
- `GaussDBOracle` 同时保留本机 docker 环境（openGauss `sql_compatibility=A`）作为快速回归替身（证据见 历史本地任务/证据（未纳入仓库））。
- `Oracle <-> GaussDBOracle` 已交付 bootstrap 级 snapshot 双向链路（`sqlplus` CLI via docker exec）：
  - `Oracle -> GaussDBOracle`: `oracle_to_gaussdb_oracle::snapshot_tests::test::smoke_test` PASS
  - `Oracle -> GaussDBOracle`: `oracle_to_gaussdb_oracle::struct_tests::test::struct_basic_test` PASS
  - `Oracle -> GaussDBOracle`: `oracle_to_gaussdb_oracle::cdc_tests::test::cdc_basic_test` PASS（logminer）
  - `Oracle -> GaussDBOracle`: `oracle_to_gaussdb_oracle::check_tests::test::check_basic_test` PASS
  - `Oracle -> GaussDBOracle`: `oracle_to_gaussdb_oracle::precheck_tests::test::struct_supported_basic_test` PASS
  - `GaussDBOracle -> Oracle`: `gaussdb_oracle_to_oracle::snapshot_tests::test::smoke_test` PASS
  - `GaussDBOracle -> Oracle`: `gaussdb_oracle_to_oracle::struct_tests::test::struct_basic_test` PASS
  - `GaussDBOracle -> Oracle`: `gaussdb_oracle_to_oracle::check_tests::test::check_basic_test` PASS
  - `GaussDBOracle -> Oracle`: `gaussdb_oracle_to_oracle::cdc_tests::test::cdc_basic_test` PASS
  - `GaussDBOracle -> Oracle`: `gaussdb_oracle_to_oracle::precheck_tests::test::struct_supported_basic_test` PASS
  - 一键回归脚本：`bash scripts/e2e/oracle_gaussdboracle_bootstrap.sh` PASS
  - 证据：历史本地任务/证据（未纳入仓库） + 历史本地任务/证据（未纳入仓库） + 历史本地任务/证据（未纳入仓库） + 历史本地任务/证据（未纳入仓库） + 历史本地任务/证据（未纳入仓库） + 历史本地任务/证据（未纳入仓库） + 历史本地任务/证据（未纳入仓库）
- SHA256 认证：当前状态为 `BLOCKED`（等待联调环境可用）。
- 当前 active 方向已演进为：`PG <-> GaussDBOracle Sync`、`GaussDBOracle Bootstrap`、`Oracle <-> GaussDBOracle Bootstrap`、`GaussDBMySQL CDC Expansion`、`GaussDBPg Quality Coverage`、`GaussDB -> MySQL Bootstrap`。

## 3. 历史 Epic 概览

此前各 Epic 的本地任务表和进度文件未纳入仓库。本节不再提供失效入口；各方向的交付状态、测试名和历史结论保留在下方 Master Checklist 与变更记录中。

## 4. Master Checklist（面向 PRD 的主清单）

> 注：这里按 PRD MVP 优先级列出关键能力项；证据优先指向已跟踪的测试、脚本、文档或提交记录。仅有历史本地证据的条目会明确标注。

### 4.1 MVP 已交付（已验证）

| Capability | 状态 | 证据 |
|---|---|---|
| `DbType::GaussDBPg` 接入（路由/预检查/测试骨架） | ✅ | 历史本地任务/证据（未纳入仓库） |
| `PG → GaussDBPg` snapshot/struct/check（基础） | ✅ | 历史本地任务/证据（未纳入仓库） |
| `GaussDBPg → PG` snapshot/check/cdc（基础） | ✅ | 历史本地任务/证据（未纳入仓库） |
| `GaussDBPg → PG` CDC：HA 端口 + NoTLS + candidate-first + sticky + 诊断增强 | ✅ | 历史本地任务/证据（未纳入仓库） |
| 无污染 e2e：`scripts/e2e/gaussdb_to_pg_cdc.sh` | ✅ | 历史本地任务/证据（未纳入仓库） |
| `GaussDBPg → PG` CDC P1：resume + failover + 负例套件 | ✅ | 历史本地任务/证据（未纳入仓库） + 历史本地任务/证据（未纳入仓库） |
| `GaussDBMySQL` 首波（`MySQL -> GaussDBMySQL` 目标端优先） | ✅ | 历史本地任务/证据（未纳入仓库） |
| `MySQL → GaussDBMySQL` CDC：目标端 failover 自愈（无 VIP/LB） | ✅ | 历史本地任务/证据（未纳入仓库） |
| `GaussDBPg → MySQL` bootstrap（snapshot/check/cdc basic） | ✅ | 历史本地任务/证据（未纳入仓库） |
| `GaussDBPg → MySQL` precheck basic | ✅ | 历史本地任务/证据（未纳入仓库） |
| `GaussDBPg → MySQL` struct（basic + advanced） | ✅ | 历史本地任务/证据（未纳入仓库） + 历史本地任务/证据（未纳入仓库） |
| 一键回归 e2e：`scripts/e2e/gaussdb_to_mysql_bootstrap.sh` | ✅ | `scripts/e2e/gaussdb_to_mysql_bootstrap.sh` + `docs/roadmap/gaussdb/e2e-test-plan.md` |
| `GaussDBPg` 质量补齐（类型矩阵 / check 细化 / 性能可观测） | ✅ | 历史本地任务/证据（未纳入仓库） |
| `GaussDBOracle` bootstrap（local docker smoke） | ✅ | 历史本地任务/证据（未纳入仓库） + `dt-tests/docker-compose.gaussdb_oracle.yml` |

### 4.2 本 Epic 状态（20260331-gaussdb-prd-align）

| Capability | 状态 | 入口（真表/证据） |
|---|---|---|
| Struct：view + matview(WITH NO DATA)（双向） | ✅ | 历史本地任务/证据（未纳入仓库） |
| Struct：routine（function/proc，仅 plpgsql/sql）（双向） | ✅ | 历史本地任务/证据（未纳入仓库） |
| Struct：routine grants（EXECUTE） | ✅ | 历史本地任务/证据（未纳入仓库） |
| `PG → GaussDBPg` CDC（PRD MVP） | ✅ | 历史本地任务/证据（未纳入仓库） |
| SHA256 认证 | ⛔ BLOCKED | 历史本地任务/证据（未纳入仓库） |

### 4.3 下一阶段 Active Epic

| Capability | 状态 | 入口（真表/证据） |
|---|---|---|
| `GaussDBMySQL` bootstrap：tracker/env contract | ✅ | 历史本地任务/证据（未纳入仓库） |
| `GaussDBMySQL` bootstrap：`DbType + route + smoke`（第一版假设） | FAILED | 历史本地任务/证据（未纳入仓库） |
| `GaussDBMySQL` bootstrap：协议与兼容模式解耦 | ✅ | 历史本地任务/证据（未纳入仓库） |
| `GaussDBMySQL` bootstrap：snapshot basic | ✅ | 历史本地任务/证据（未纳入仓库） |
| `GaussDBMySQL` bootstrap：struct + check basic | ✅ | 历史本地任务/证据（未纳入仓库） |
| `GaussDBMySQL` bootstrap：docs closeout | ✅ | 历史本地任务/证据（未纳入仓库） |
| `GaussDBMySQL` bootstrap：precheck + 真实环境证据 | ✅ | 历史本地任务/证据（未纳入仓库） |
| `GaussDBMySQL` CDC Expansion：cdc basic | ✅ | 历史本地任务/证据（未纳入仓库） |
| `GaussDBMySQL` CDC Expansion：cdc type-matrix | ✅ | 历史本地任务/证据（未纳入仓库） |
| `GaussDBMySQL` CDC Expansion：cdc resume | ✅ | 历史本地任务/证据（未纳入仓库） |
| `GaussDBPg` quality：truth-source normalization | ✅ | 历史本地任务/证据（未纳入仓库） |
| `GaussDBPg` quality：type contract + codecs | ✅ | 历史本地任务/证据（未纳入仓库） |
| `GaussDBPg` quality：non-CDC type matrix e2e | ✅ | 历史本地任务/证据（未纳入仓库） |
| `GaussDBPg` quality：CDC type matrix + fail-fast evidence | ✅ | 历史本地任务/证据（未纳入仓库） |
| `GaussDBPg` quality：统一 e2e 质量门槛规划 | ✅ | 历史本地任务/证据（未纳入仓库） |
| `GaussDB -> MySQL` bootstrap：snapshot/check/cdc basic | ✅ | 历史本地任务/证据（未纳入仓库） |

## 5. 决策记录（Decision Log）

- 2026-03-31：
  - PRD 作为需求真相源；`plan.md` 将对齐 PRD 并演进为迭代计划。
  - Struct 扩展：view+matview(WITH NO DATA)+routine(plpgsql/sql)+routine grants；双向同时。
  - router：不改写定义体内部引用，只路由对象 header。
  - matview 已存在默认跳过（不自动重建）。
  - PG→GaussDB CDC 已补齐并有 dt-tests 覆盖；并修复 parallelizer 对 sinker I/O 的 panic 以提升稳定性。
  - SHA256 先纳入 epic，但等待联调环境后再启动（BLOCKED）。
- 2026-04-02：
  - `GaussDBPg -> PG` CDC P1 resilience 已完成真实环境闭环验证：basic/resume/negative/failover e2e 与 `dt-tests cdc_resume_test/cdc_failover_test` 均有证据归档。
  - failover 测试链路补强为“切主期间允许短暂抖动，但任务需自动重连并继续同步”，并将 `cm_ctl switchover` 流程固化到集成测试中。
  - 下一阶段按双 Epic 推进：`GaussDBMySQL Bootstrap` 负责新模式首波落地，`GaussDBPg Quality Coverage` 负责类型矩阵 / check 细化 / 性能可观测。
  - `GaussDB` 统一 e2e 回归矩阵已落盘（Quick/Full/Resilience 三层），并完成 Batch A 主路径回归（PASS，证据见 历史本地任务/证据（未纳入仓库））。
  - `GaussDBMySQL` 本轮锁定为 **目标端优先**：`MySQL -> GaussDBMySQL`，首个 spec 只做到 **骨架 + smoke**。
  - 新环境事实已确认：GaussDB 兼容模式属于**数据库级属性**。已验证 `postgres://root@10.250.0.51:8000/jyp_test_m?sslmode=require` 对应库 `jyp_test_m` 的 `sql_compatibility=M`，说明 “MySQL-compatible” 不应被简单建模为 “一定走 mysql:// 协议”。
  - `GaussDBPg` non-CDC type matrix e2e 已闭环：修复了当前环境中 `'' -> NULL` 的夹具歧义，并确认 GaussDB `tinyint` 的底层 `pg_type.typname=int1`，现已统一映射进 `Int16` 路径。
  - `GaussDBPg` CDC type matrix 也已闭环：真实运行先暴露出 `blob` 在 CDC decoder 中被当成十六进制文本透传，修复后 `blob/tinyint/smalldatetime/nvarchar2/clob` 已在 `GaussDBPg -> PG` CDC 路径通过。
  - `GaussDBMySQL` bootstrap 已完成两步关键纠偏：
    - 协议与兼容模式已拆分建模
    - `MySQL -> GaussDBMySQL snapshot basic` 已在 “本机 MySQL 8 -> `postgres://.../jyp_test_m`” 真实环境通过
  - 本轮 bootstrap 已进一步收口到 `struct + check + docs`：
    - `MysqlStruct` / `MysqlCheck` 已支持 `DbType::GaussDBMySQL + postgres://`
    - 目标端 `SHOW CREATE DATABASE/TABLE` 与数据对账均可通过 pg-wire `simple_query` 完成
    - `docs/templates/mysql_to_gaussdb_mysql.md` 已更新为真实的 pg-wire MySQL-mode 环境契约与验证命令
  - snapshot / struct / check 首波的关键适配为：
    - 写路径走 `PgSinker`，但 SQL 生成使用 pg-wire + MySQL-mode 的兼容分支
    - 目标端对账读取改为 `tokio-postgres simple_query`
    - 候选主库重写扩展到 `GaussDBMySQL + postgres://`，避免写入/清理命中只读 standby
  - 已记录的后续质量项：
    - `DATETIME` 在目标端 simple-query 对账路径里当前按文本比较，后续类型矩阵 / check 细化时再收紧
  - `GaussDBPg Quality Coverage` 已完成前两步：
    - truth-source normalization
    - PRD 首批特有类型 alias/codec 契约（`smalldatetime/tinyint/nvarchar2/clob/blob`）
  - `GaussDBPg Quality Coverage` 的 child 3 已开工：
    - 已补 `pg_to_gaussdb snapshot type_matrix_test` 与 `gaussdb_to_pg check type_matrix_test` 入口和首批夹具
    - `--no-run` 编译已通过
    - 首次真运行被当前沙箱网络限制阻断，错误为 `Operation not permitted (os error 1)`
  - `GaussDBOracle` 与 `SHA256` 本轮均不进入 active implementation，只保留 roadmap / blocked 条目。
- 2026-04-03：
  - 已完成 历史本地任务/证据（未纳入仓库） gate run：
    - Batch B（6 条增强回归）`6/6 PASS`
    - script resilience（`basic/resume/slot-active/no-repl-user/failover`）`5/5 PASS`
    - `dt-tests cdc_failover_test` `FAIL`
  - 新增的关键证据表明：
    - 真实 failover 期间，CDC 已能从 `10.250.0.30:8001` 重连到 `10.250.0.51:8001` 并继续同步。
    - 脚本路径的 best-effort restore 已能把主库恢复回 `node 2 / 10.250.0.30`，并完成 slot/schema/temp-role 无污染清理。
    - 当前剩余红点集中在 `dt-tests` failover restore 校验，具体表现为 `cm_ctl busy / convergence timeout` 导致最终 `orig_primary_node=2, final_primary_node=1`。
  - 因此，`GaussDBPg → PG CDC P1：resume + failover + 负例套件` 状态从“历史交付完成”收敛为 **PARTIAL**：
    - e2e 运行链路已闭环
    - `dt-tests` failover 自动回原主仍需进一步稳定化
- 2026-04-09：
  - 已完成 历史本地任务/证据（未纳入仓库） 的后续收口：
    - `dt-tests cdc_failover_test` 真实环境 PASS
    - failover restore 红点关闭
  - 本轮稳定化的关键收敛点：
    - dt-tests 改为使用每次运行唯一的 GaussDB CDC slot，避免共享环境重复运行造成 slot 污染
    - restore 阶段允许在短暂无法解析 RW 主时回退到上一次成功的 CM host
    - final safety check 增加 CM datanode health convergence wait，避免 `Standby Building(0%)` 的瞬时态误杀
  - 因此当前项目阶段调整为：
    - `GaussDBPg` 主线能力已基本闭环
    - `GaussDBMySQL` 首波已全部闭环，后续主要是扩展能力决策
    - `SHA256` 与 `GaussDBOracle` 仍维持 `BLOCKED / roadmap`
  - 已完成 历史本地任务/证据（未纳入仓库）：
    - `GaussDBMySQL` precheck 已支持 pg-wire 目标（`postgres://.../jyp_test_m`）
    - `mysql_to_gaussdb_mysql` precheck 正负例均已通过
    - 源端/目标端测试 schema 均已清理，`precheck` 不再是 `PARTIAL`
- 2026-04-10：
  - 已新建 历史本地任务/证据（未纳入仓库） 作为 `GaussDBMySQL` 第二阶段 Epic。
  - 当前 active child 为 `MySQL→GaussDBMySQL cdc basic`：
    - 优先填补 dashboard 中 `MySQL -> GaussDBMySQL` 唯一缺失的核心能力项 `cdc`
    - 首轮范围锁为 DML 主路径，不提前混入 DDL / resume / failover
  - 当前阶段判断：
    - `GaussDBPg` 主线已基本闭环
    - `GaussDBMySQL` 已从 bootstrap 进入 CDC Expansion
    - `SHA256` 与 `GaussDBOracle` 仍维持 `BLOCKED / roadmap`
- 2026-04-15：
  - `GaussDBOracle` 已解锁 **本机 Docker** 环境（openGauss `sql_compatibility=A`）并形成最小 smoke 闭环：
    - compose：`dt-tests/docker-compose.gaussdb_oracle.yml`
    - dt-tests：`pg_to_gaussdb_oracle::snapshot_tests::test::smoke_test` PASS
    - 证据：历史本地任务/证据（未纳入仓库）

## 6. 更新流程（每完成一个 spec 后怎么做）

1. 在已跟踪的测试、脚本或运行文档中记录验证命令与结果（PASS/FAIL）。
2. 更新本文档：
   - Dashboard（必要时）
   - Master Checklist 的对应条目状态与可访问证据
3. 提交到 git（不包含凭据与未脱敏原始输出）。
