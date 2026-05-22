# tracker+epic+plan.md 对齐（PRD 真相源）

## 背景

当前 GaussDB 相关事实分散在：

- 需求：`docs/agent-summary/gaussdb-prd.md`
- 历史 MVP 计划：`docs/agent-summary/plan.md`（需对齐 PRD，避免长期冲突）
- 工程证据：`.codex-tasks/*`

需要建立可持续迭代的“全局进度跟踪清单”，并确保每次完成一个 spec（single-full）后都能清晰知道下一步安排与证据位置。

## 目标

1. 新增可读 dashboard：`docs/agent-summary/gaussdb-progress-tracker.md`。
2. 建立本 epic 的执行真表：`.codex-tasks/20260331-gaussdb-prd-align/SUBTASKS.csv`（已创建）。
3. 更新 `docs/agent-summary/plan.md`：从“历史 MVP 收敛计划”演进为“PRD 驱动的迭代计划”，并明确下一阶段优先级与依赖（SHA256 标注 BLOCKED）。

## 成功标准

- tracker 文档存在且包含：
  - dashboard 矩阵
  - master checklist（映射到 epic 子任务与既有 `.codex-tasks` 证据）
  - 证据索引 + 决策记录 + 开放问题 + 更新流程
- `plan.md` 不再与 PRD 在关键范围/里程碑上自相矛盾（能解释“已完成、接下来做什么、哪些 BLOCKED”）。

## 验收

```bash
test -f docs/agent-summary/gaussdb-progress-tracker.md
rg -n \"PRD\" docs/agent-summary/plan.md > /dev/null
```

