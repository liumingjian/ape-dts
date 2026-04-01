# GaussDB 全局进度跟踪清单（PRD 真相源）

> 最后更新：**2026-04-01**
>
> 目标：每完成一次 spec 任务后，都能立刻知道“当前已交付什么、证据在哪、下一步做什么”。

## 1. 真相源与更新规则

### 1.1 真相源（优先级）

1. **需求真相源**：`docs/agent-summary/gaussdb-prd.md`
2. **执行真表**：`.codex-tasks/20260331-gaussdb-prd-align/SUBTASKS.csv`
3. **证据归档**：`.codex-tasks/*/*/PROGRESS.md`（含验证命令与结果）与必要的脱敏 `raw/` 片段

### 1.2 更新规则（强制）

- 每个 `single-full` spec 任务完成时：
  - 更新 `.codex-tasks/20260331-gaussdb-prd-align/SUBTASKS.csv` 的状态与完成时间
  - 将验证命令与结果写入该任务的 `PROGRESS.md`
  - 同步更新本文档的对应条目（状态 + 证据链接）
- 不提交凭据：`.env.local`、`.local/`、带口令的 URL 等严禁进入 git。
- `raw/` 仅允许提交**脱敏片段**（推荐只保留关键日志的 30-80 行，并确保无账号/口令）。

## 2. Dashboard（能力矩阵）

| 方向 | snapshot | cdc | struct | check | precheck | docs/runbook | e2e |
|---|---|---|---|---|---|---|---|
| **PG → GaussDBPg** | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| **GaussDBPg → PG** | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |

说明：

- Struct（PRD MVP）已补齐：在已有 `table/index/constraint/sequence/comment/rbac` 基础上补齐 **view/matview/routine/routine grants**。
- SHA256 认证：当前以 `BLOCKED` 方式纳入执行真表（等待联调环境可用）。

## 3. 执行真表（Epic）

> 本节只做“入口索引”，以 `SUBTASKS.csv` 为准。

- Epic：`.codex-tasks/20260331-gaussdb-prd-align/`
  - `EPIC.md`
  - `SUBTASKS.csv`（执行真表）
  - `PROGRESS.md`
- Epic：`.codex-tasks/20260331-gaussdb-cdc-resilience/`
  - `EPIC.md`
  - `SUBTASKS.csv`
  - `PROGRESS.md`

## 4. Master Checklist（面向 PRD 的主清单）

> 注：这里按 PRD MVP 优先级列出关键能力项；每项必须能指向至少一个证据入口（`PROGRESS.md` 或提交记录）。

### 4.1 MVP 已交付（已验证）

| Capability | 状态 | 证据 |
|---|---|---|
| `DbType::GaussDBPg` 接入（路由/预检查/测试骨架） | ✅ | `.codex-tasks/20260316-gaussdb-mvp/SUBTASKS.csv` |
| `PG → GaussDBPg` snapshot/struct/check（基础） | ✅ | `.codex-tasks/20260329-gaussdb-prd-e2e/PROGRESS.md` |
| `GaussDBPg → PG` snapshot/check/cdc（基础） | ✅ | `.codex-tasks/20260329-gaussdb-prd-e2e/PROGRESS.md` |
| `GaussDBPg → PG` CDC：HA 端口 + NoTLS + candidate-first + sticky + 诊断增强 | ✅ | `.codex-tasks/20260331-gaussdb-p0-stability/PROGRESS.md` |
| 无污染 e2e：`scripts/e2e/gaussdb_to_pg_cdc.sh` | ✅ | `.codex-tasks/20260331-gaussdb-p0-stability-e2e/PROGRESS.md` |
| `GaussDBPg → PG` CDC P1：resume + failover + 负例套件 | IN_PROGRESS | `.codex-tasks/20260331-gaussdb-cdc-resilience/PROGRESS.md` |

### 4.2 本 Epic 状态（20260331-gaussdb-prd-align）

| Capability | 状态 | 入口（真表/证据） |
|---|---|---|
| Struct：view + matview(WITH NO DATA)（双向） | ✅ | `.codex-tasks/20260331-gaussdb-prd-align/tasks/20260331-02-struct-view-matview/PROGRESS.md` |
| Struct：routine（function/proc，仅 plpgsql/sql）（双向） | ✅ | `.codex-tasks/20260331-gaussdb-prd-align/tasks/20260331-03-struct-routine/PROGRESS.md` |
| Struct：routine grants（EXECUTE） | ✅ | `.codex-tasks/20260331-gaussdb-prd-align/tasks/20260331-04-struct-routine-grants/PROGRESS.md` |
| `PG → GaussDBPg` CDC（PRD MVP） | ✅ | `.codex-tasks/20260331-gaussdb-prd-align/tasks/20260331-05-pg-to-gaussdb-cdc/PROGRESS.md` |
| SHA256 认证 | ⛔ BLOCKED | `.codex-tasks/20260331-gaussdb-prd-align/SUBTASKS.csv#6` |

## 5. 决策记录（Decision Log）

- 2026-03-31：
  - PRD 作为需求真相源；`plan.md` 将对齐 PRD 并演进为迭代计划。
  - Struct 扩展：view+matview(WITH NO DATA)+routine(plpgsql/sql)+routine grants；双向同时。
  - router：不改写定义体内部引用，只路由对象 header。
  - matview 已存在默认跳过（不自动重建）。
  - PG→GaussDB CDC 已补齐并有 dt-tests 覆盖；并修复 parallelizer 对 sinker I/O 的 panic 以提升稳定性。
  - SHA256 先纳入 epic，但等待联调环境后再启动（BLOCKED）。

## 6. 更新流程（每完成一个 spec 后怎么做）

1. 子任务目录下确认 `PROGRESS.md` 已写入“验证命令 + 结果（PASS/FAIL）”。
2. 更新 epic `.codex-tasks/20260331-gaussdb-prd-align/SUBTASKS.csv` 对应行状态与 `completed_at`。
3. 更新本文档：
   - Dashboard（必要时）
   - Master Checklist 的对应条目状态与证据链接
4. 提交到 git（不包含凭据与未脱敏 raw）。
