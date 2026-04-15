# SPEC — GaussDB ↔ MySQL 双向链路进度复盘与是否可跳转下一模块

## 目标

回顾当前仓库内与 `GaussDB`、`MySQL` 相关的双向链路交付情况，给出可落地的结论：

- 双向链路（本轮定义：`MySQL → GaussDBMySQL` + `GaussDBPg → MySQL`）是否已完成到“可用/可回归”的阶段
- 是否可以进入下一模块（以及推荐的下一模块/下一步）
- 同步修正文档真相源（tracker / plan）使其与实际交付一致

## 范围（本轮）

- `MySQL → GaussDBMySQL`
  - bootstrap：snapshot / struct / check / precheck / docs
  - cdc expansion：cdc basic / type-matrix / resume + negatives / target failover self-heal / docs
- `GaussDBPg → MySQL`（bootstrap + 后续补齐）
  - snapshot / check / cdc basic（Epic 已闭环）
  - precheck basic（dt-tests 入口 + 真环境 PASS）
  - struct basic + struct advanced（default/index 覆盖 + 真环境 PASS）
  - e2e 入口：`scripts/e2e/gaussdb_to_mysql_bootstrap.sh`（quick/full）

## 非目标

- 不在本任务中引入新的功能面（如：`GaussDB → MySQL` 的 cdc resume/failover、DDL 同步、更多对象类型等）
- 不解锁 `SHA256` 与 `GaussDBOracle`（如需推进，另开 spec/epic）

## 交付物

- `.codex-tasks/20260415-gaussdb-mysql-bidir-review/PROGRESS.md`：评估结论 + 证据索引 + 下一步建议
- 文档修正：
  - `docs/agent-summary/gaussdb-progress-tracker.md`（dashboard / 更新时间 / 说明）
  - `docs/agent-summary/plan.md`（Epic C 现状与范围叙述对齐）

## 验收标准

- 能明确回答“是否完成/是否可跳下一模块”，并给出不含歧义的推荐下一步
- tracker/plan 的表述与实际代码/测试/证据一致，可通过 `rg` 检索到对应证据入口
- 不提交任何凭据或未脱敏日志（`.env.local`、`.local/`、`raw/` 敏感内容等）

