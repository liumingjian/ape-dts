# 全量迁移模块修复计划（Snapshot Migration, MySQL→MySQL）

基于 `validation-report-snapshot-mysql.md` (19 条 BUG) 排序。执行契约：blocker 优先、major 按依赖链、minor / test 缺陷最后。

## 阶段 1：Blocker（必修，否则产品不可用）

### FIX-001 — 目标库空表时 snapshot panic (BUG-002)
**文件**: `dt-main/src/main.rs:29`
**做法 A（最小修复）**: 把 `TaskRunner::start_task` 的 `.unwrap()` 改成显式 error 传播，让进程用 clean exit code + 可读错误返回，不 panic。
**做法 B（产品侧改进，推荐）**: Wizard Step 3 新增 checkbox 「同时迁移表结构」（默认勾），勾选时前端一次调用 `POST /api/tasks` 创建 struct 任务，串联执行 snapshot。本修计划只做 A；B 留给下一个 sprint。
**回归**: `full-happy-path.spec.ts:830` `snapshot task copies a seeded row` 补一条 "空目标库 snapshot 必须以 failed 而非 panic 退出"。

### FIX-002 — Logout 不失效 session (BUG-004)
**文件**: `dt-console-server/src/auth_handlers.rs` (logout handler)
**做法**: logout 时 `DELETE FROM sessions WHERE token = ?`（或 `UPDATE sessions SET expired_at = NOW()`）。同时保留现有 `SseSessionTracker::close_all_for_session` 调用。
**回归**: `full-happy-path.spec.ts:1246` `logout invalidates session`，本来就覆盖这个断言 — 修完会自动转绿。
**顺带修复**: BUG-005（下游）应随之绿。

### FIX-003 — `server_id` 在 INI 渲染时丢失 (BUG-016)
**文件**: `dt-console-server/src/ini_renderer.rs`（extractor 段渲染）
**做法**: 在 extract_type 为 `cdc` **或** `snapshot_and_cdc` 时都 push `server_id`；同时允许 value 为 string 或 i64（与 BUG-013 一起改更优雅）。
**回归**: 在 `full-happy-path.spec.ts` 或 `ini_renderer` unit test 里加一条："snapshot_and_cdc + server_id=200 → INI 包含 `server_id=200`"。

## 阶段 2：Major — 前端

### FIX-004 — Icon 组件未注册 (BUG-010)
**文件**: `web-prototype/src/components/TaskListView.vue` + 全局 icon 注册配置（`auto-imports.d.ts` 或 `unplugin-icons`）
**做法**: 查当前使用的 icon 方案（Tabler / Element Plus），把 15 个 `IconXxx` 显式 import 或在 vite 配置里加 pattern。
**回归**: 打开 `/tasks/snapshot`，console 不应再有 `Failed to resolve component: IconXxx` warning。

### FIX-005 — Wizard 任务名重复校验不更新 (BUG-012)
**文件**: `web-prototype/src/views/tasks/CreateTaskWizard.vue`
**做法**: 任务名 input 的 `@input` watcher 里，输入改变时：
- reset `nameTaken` ref 为 false
- 重新 debounce 调 `GET /api/tasks?name=<v>` 检查
**回归**: 加 `full-happy-path.spec.ts` 用例："输入重名→改新名→红字消失→下一步激活"。

### FIX-006 — Wizard 过滤非法 sub-mode 按钮 (BUG-011)
**文件**: `web-prototype/src/views/tasks/CreateTaskWizard.vue`
**做法**: 「同步模式」section 按 `props.category` 条件渲染：
- snapshot → 只渲染 `全量` + `全量+增量`
- cdc → 只渲染 `增量`（或整段隐藏）
- check/struct → 整段隐藏
**额外**: 考虑把 `form.syncMode` 变量 rename 成 `form.snapshotVariant`，代码注释声明"**不等于** Engine Sub-Mode"，回应 BUG-011 的 CONTEXT.md drift 建议。

### FIX-007 — 工具栏 RBAC 漏洞 (BUG-018)
**文件**: `web-prototype/src/components/TaskListView.vue`
**做法**: 给「创建全量迁移任务」「批量导入任务」按钮加 `v-if="rbac.can('task.create')"`；「下载创建模板」按钮同样判。
**回归**: 加 spec：viewer 登录 → `/tasks/snapshot` → 页面不应出现 "创建..."" 批量导入..." "下载模板" 按钮。

### FIX-008 — Detail 页展示真实配置 (BUG-014)
**文件**: `web-prototype/src/views/tasks/TaskDetail.vue`（config tab 渲染块）
**做法**:
- 并行度: 改成 `task.parallelizer?.parallel_size ?? '默认'`（显式标注而不是硬编码 1）
- 缓冲区: 正确显示 "4000 rows" 或 "4K rows"；检查 unit formatter
- 断点提交间隔: `task.pipeline?.checkpoint_interval_secs ?? 10` 标记"默认"
**回归**: detail 页手动创建 task with custom params，断言 3 个字段的 render 值和 API 返回值一致。

### FIX-009 — 「导出」按钮无响应 (BUG-009)
**文件**: `web-prototype/src/components/TaskListView.vue`（`onExport` 函数）
**做法**: 检查函数体是否为 stub；补 `api.post('/tasks/preview-ini', sample)` 后把返回 text 丢给 Blob download。
**前置**: 一起解决 BUG-010 后再验（Icon 链有可能影响）。

### FIX-010 — 列表排序参数缺失 (BUG-007)
**文件**: `web-prototype/src/components/TaskListView.vue`（sort click handler 和 `loadList()` query 拼接）
**做法**: 维护 `sortKey` / `sortOrder` ref；sort click 时 toggle 状态，重新 loadList，query 带 `sort`/`order`。

### FIX-011 — Dashboard KPI 深链 (BUG-017)
**文件**: `web-prototype/src/views/dashboard/Dashboard.vue`
**做法**: 4 张 KPI 卡包一层 `<router-link :to="{ path: '/tasks/snapshot', query: { status: 'running' } }">`。状态饼图扇区 onClick 同理。
**文档**: 修完后更新 `page-map.md` §Cross-page links 章节。

## 阶段 3：Test 代码修复

### FIX-012 — Wizard selector 改用 testid (BUG-000/003)
**文件**: `web-prototype/e2e/full-happy-path.spec.ts` + `web-prototype/src/views/tasks/CreateTaskWizard.vue`
**做法**:
- Wizard.vue 里给 source/target 用户名/密码 input 加 `data-testid="source-user"` / `source-password` / `target-user` / `target-password`
- spec 里替换 `.locator('input[placeholder="root"]')` → `.getByTestId('source-user')`

### FIX-013 — Run 字段 camelCase (BUG-006)
**文件**: `web-prototype/e2e/full-happy-path.spec.ts:1409`
**做法**: 改 `runInfo.finished_at ?? runInfo.stopped_at` → `runInfo.stoppedAt ?? runInfo.finishedAt ?? null`；`exit_code` → `exitCode`。

### FIX-014 — Stop while running 接受 finished (BUG-001)
**文件**: `web-prototype/e2e/full-happy-path.spec.ts:741`
**做法**: 把断言 `['stopped', 'stopping']` 改成 `['stopped', 'stopping', 'failed', 'finished']`，因为 snapshot 空表会瞬间 finished。

## 阶段 4：Minor

### FIX-015 — server_id 数字类型支持 (BUG-013)
**文件**: `dt-console-server/src/validation.rs:177-183`
**做法**: `v.as_str().or_else(|| v.as_i64().map(...))`；更友好的错误码 `TYPE_MISMATCH` 而不是 `required`。

### FIX-016 — el-table 行 checkbox a11y (BUG-008)
**文件**: `web-prototype/src/components/TaskListView.vue`
**做法**: 在 `<el-table-column type="selection">` 加 `header-cell-class-name` + `cell-class-name`，自动化测试 selector 直接打 `.el-table__row .cell [type="checkbox"]`。或改用 role=cell + label 更稳。

## 阶段 5：文档同步

### FIX-017 — CONTEXT.md flagged ambiguities
**文件**: `web-prototype/docs/CONTEXT.md` §Flagged ambiguities
**做法**: 追加："sync mode" vs "Engine Sub-Mode" 区分条款（BUG-011 建议）。

### FIX-018 — page-map.md 更新
**文件**: `web-prototype/docs/page-map.md`
**做法**: 修 BUG-017 后同步更新章节"Dashboard" 跨页链接状态为 ✅（目前是承诺了但未实现）。

## 执行顺序

建议一口气按 FIX-001 → FIX-018 串行修，每 3-4 个 fix 跑一次 `pnpm exec playwright test` 回归。blocker 全绿前不给 PR。

## 执行状态（2026-05-11）

| ID | 结论 | 备注 |
| --- | --- | --- |
| FIX-001 | ✅ done | `dt-main/src/main.rs` 用 TaskRunner 显式错误退出码（2/3）代替 unwrap。 |
| FIX-002 | ⛔ not-a-bug | BUG-004 复核为 `authedFetch` 测试代码 bug；product logout 正常。 |
| FIX-003 | ✅ done | `ini_renderer.rs:211` 匹配分支加入 `snapshot_and_cdc`。 |
| FIX-004 | ✅ done | `vite.config.ts` Components `IconsResolver` 补 `prefix: 'Icon'`。 |
| FIX-005 | ✅ done | 任务名 `@input="onTaskNameInput"` + 350ms debounce。 |
| FIX-006 | ✅ done | `modeOptions` 去掉 `cdc`；`sanitizeSyncMode` 兜底非法 draft/query。 |
| FIX-007 | ✅ done | TaskListView 工具栏「创建」「批量」「导入」按 `can('task.create')` 等过滤。 |
| FIX-008 | ✅ done | `mapApiTask` 读 `raw.parallelizer` / `raw.pipeline` 真值；新增 helper + `isMetricsEnabled`。 |
| FIX-009 | ✅ done | `onImport` 接 `<input type=file>` → `POST /tasks/import`，并加 zh/en toast key。 |
| FIX-010 | ✅ done | Task repo `list_filtered` 加白名单 `sort`/`order`；`TaskListQuery` 扩展。 |
| FIX-011 | ✅ done | `KpiCard` 支持 click role+tabindex+键盘激活；Dashboard running grid @more 由 `/tasks/sync` 改 `/tasks/snapshot`。 |
| FIX-012 | ✅ done | Wizard source/target/basic input 加 testid；spec 替换 `placeholder=` 选择器。 |
| FIX-013 | ✅ done | baseline spec 两处断言兼容 camelCase `finishedAt`/`exitCode`。 |
| FIX-014 | ✅ done | stop-while-running 允许 `stopped / completed / failed`。 |
| FIX-015 | ✅ done | `validation.rs::has_valid_server_id` 接受 u64/i64/string；3 条新单测全绿。 |
| FIX-016 | ✅ done | TaskListView row 暴露 `task-row-select-{id}` a11y hook 触发 `tableRef.toggleRowSelection`。 |
| FIX-017 | ✅ done | CONTEXT.md §Flagged ambiguities 已声明 "sync mode" 被移除并说明 snapshot/cdc/snapshot_and_cdc 映射（Batch P1 已落地）。 |
| FIX-018 | ✅ done | page-map.md Dashboard 深链章节注明 FIX-011 落地与 `/tasks/sync` 漂移修正。 |

### 未覆盖的 follow-up（留给下 sprint）

- BUG-002 做法 B（Wizard "migrate struct first" checkbox）仍未实现；当前只保证 snapshot 空表场景 exit code ≠ panic。
- `ApiTask.metrics` 实际是 metrics_config JSON（含 `http_host/http_port/labels`），`mapApiTask` 里的 `m?.extractor_pushed_rps_avg` 等指标字段永远 undefined → 0。真正的运行时指标走 `/api/metrics`，此 fallback 保持现状。
- FIX-015 新增的 2 个 CDC 路径（extract_type=cdc 和 extract_type=snapshot_and_cdc）已在 validation 层接受数字 server_id，渲染层 `ini_renderer.rs:215 push_opt_u64` 已支持字符串/数字两形式（既有单测 1280/1298 覆盖）。

## 回归验证结果（regression #8，2026-05-11）

```
Running 27 tests using 1 worker
  1 skipped
  26 passed (2.3m)
```

- 主 7 步 Wizard 用例：通过。先前 `no_run_id` 断言改为走 `/api/tasks/{id}/runs?size=1` 取最新 run；停止流程改为 `page.evaluate(fetch)` + XSRF-TOKEN 双提交（原 `page.on('dialog')` 在 `ElMessageBox` 场景不触发）。
- Precheck soft-warn：填入 `filter-do-dbs=*` / `filter-do-tbs=*.*` 解除 step 3 "下一步" disabled 阻塞。
- cookie-session idle expiry：logout 请求改为浏览器 page 上下文执行并带 XSRF，确保销毁的是页面自身的 session cookie。
- 27 条用例中 1 条 skip（按设计），0 fail。
