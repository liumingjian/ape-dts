# Epic Specification

## Goal

- 补齐能力矩阵缺口：交付 `PG ↔ GaussDBOracle` 的**双向同步**最小闭环，并提供 `dt-tests` 的自动化回归入口与证据索引。
  - 第一阶段（non-CDC basic）：`snapshot/struct/check` 在 **PG → GaussDBOracle** 与 **GaussDBOracle → PG** 两个方向均可跑通。
  - 第二阶段（sync）：补齐 `CDC` 主链路（至少 basic），形成 “全量 + 增量” 的同步闭环。

## Non-Goals

- 不承诺 Oracle wire-protocol/OCI/JDBC（Oracle 侧 connector 已在独立 Epic 以 `sqlplus` bootstrap 交付）。
- 不承诺 `GaussDBOracle` 的 failover/resume/DDL-CDC 等高阶能力（后续按需要再开 Epic 扩展）。
- 不做类型矩阵（type-matrix）全覆盖；先跑通最小 DML（insert/update/delete）主路径。

## Constraints

- GaussDBOracle 环境：复用远端 oracle-mode `testdb`（通过本地 `dt-tests/tests/.env.local` 注入，禁止提交凭据）。
- 连到 GaussDBOracle 的 RW 主：必须复用 `gaussdb_pg_candidate_hosts` 自动选主逻辑，避免命中只读节点。
- 所有验证必须给出可复现命令，并写入对应 `PROGRESS.md`；`SUBTASKS.csv` 未验证不得置 `DONE`。

## Risk Assessment

- **GaussDBOracle CDC 可用性不确定**：oracle-mode DB 是否支持/启用 `mppdb_decoding` 逻辑复制插件需要真实验证；若不支持，将以证据标记为 `FAILED/BLOCKED` 并给出替代方案（例如先完成 PG→GaussDBOracle CDC）。
- **兼容模式差异**：oracle-mode 下部分 DDL/系统表字段与 PG 不同，struct/check 需沿用既有降级/normalize 逻辑。

## Child Deliverables

- `dt-tests`：新增 `gaussdb_oracle_to_pg` 的 snapshot/struct/check basic（non-CDC 双向补齐）。
- `dt-tests`：新增 `pg_to_gaussdb_oracle` 的 cdc basic（PG→GaussDBOracle 增量同步）。
- `dt-common/dt-task`：启用 `DbType::GaussDBOracle` 的 `extract_type=cdc`（复用 `GaussDBCdcExtractor`）并新增 `gaussdb_oracle_to_pg` 的 cdc basic（若环境支持）。
- 文档：更新 `docs/agent-summary/*` 能力矩阵与 e2e 用例矩阵，使入口可被 `rg` 直接检索。

## Dependency Notes

- 子任务 `#3` 依赖 `#1`（先有 non-CDC 基础用例与 fixtures，再扩展到 CDC）。

## Done-When

- [ ] `SUBTASKS.csv` 全部为 `DONE`
- [ ] non-CDC 双向：`gaussdb_oracle_to_pg` 的 snapshot/struct/check basic PASS
- [ ] 增量同步：`pg_to_gaussdb_oracle` 的 cdc basic PASS
- [ ] 若环境支持：`gaussdb_oracle_to_pg` 的 cdc basic PASS（否则以证据标记并写明后续方案）

