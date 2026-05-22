# ape-dts Console Web Prototype · Page Map & Cross-Page Linkage

Canonical taxonomy (ADR-0006). All pages live under `src/views/<module>/`. Routes are declared in `src/router/index.ts`. Task categories: **snapshot** / **cdc** / **check** / **struct** (中文: 全量迁移 / 增量同步 / 数据校验 / 结构迁移). Legacy `/tasks/sync|replay|verify` redirect to their canonical counterparts.

## Page completeness matrix

| Tier | Path | Component | Status | Highlights |
|---|---|---|---|---|
| P0 | `/login` | `views/auth/Login.vue` | ✅ | Bilingual login, demo creds, branded hero |
| P0 | `/dashboard` | `views/dashboard/Dashboard.vue` | ✅ | License banner · 4 KPI · 24h RPS/latency · status pie · engine bar · 7d alert trend · recent tasks/alerts |
| P0 | `/tasks/snapshot` | `views/tasks/SnapshotTaskList.vue` | ✅ | Sortable table with status / engine / RG filters, 8 s polling, batch actions, `?status=&engine=&q=` honored |
| P0 | `/tasks/cdc` | `views/tasks/CdcTaskList.vue` | ✅ | Same shell as snapshot list, category-specific columns |
| P0 | `/tasks/check` | `views/tasks/CheckTaskList.vue` | ✅ | Same shell as snapshot list |
| P0 | `/tasks/struct` | `views/tasks/StructTaskList.vue` | ✅ | Same shell as snapshot list |
| P0 | `/tasks/create/snapshot` | `views/tasks/CreateTaskWizard.vue` | ✅ | **7-step** wizard (source → test → objects → processing → advanced → precheck → confirm) |
| P0 | `/tasks/create/cdc` | `views/tasks/CreateTaskWizard.vue` | ✅ | **7-step** wizard (same sequence, CDC-specific fields) |
| P0 | `/tasks/create/check` | `views/tasks/CreateTaskWizard.vue` | ✅ | **7-step** wizard (same sequence, check-specific fields) |
| P0 | `/tasks/create/struct` | `views/tasks/CreateTaskWizard.vue` | ✅ | **5-step** wizard (source → test → objects → precheck → confirm); no processing/advanced steps |
| P0 | `/tasks/:category/:id` | `views/tasks/TaskDetail.vue` | ✅ | KPI · 3 charts · config / objects / logs / alerts tabs · edit drawer · `?tab=` honored |
| P1 | `/alerts/current` | `views/alerts/CurrentAlerts.vue` (`AlertTableView` mode=active) | ✅ | Level summary cards · auto-refresh toggle · level/source/engine/IP/taskId/alertId filters · batch clear |
| P1 | `/alerts/history` | `views/alerts/HistoryAlerts.vue` (`AlertTableView` mode=history) | ✅ | Date-range picker · cleared-at column · no batch actions |
| P1 | `/alert-monitor/metrics` | `views/alertMonitor/MetricRules.vue` | ✅ | CRUD drawer, threshold + level + period editor |
| P1 | `/alert-monitor/events` | `views/alertMonitor/EventRules.vue` | ✅ | Toggle + edit drawer for system events |
| P1 | `/alert-monitor/alarm-setting` | `views/alertMonitor/AlarmSetting.vue` | ✅ | Channel cards (Kafka / SNMP), inline test connection |
| P1 | `/alert-monitor/alarm-template` | `views/alertMonitor/AlarmTemplate.vue` | ✅ | Editor with template list, defaults per kind × format |
| P1 | `/system/monitor` | `views/system/SystemMonitor.vue` | ✅ | Host cards summary + sortable table with CPU/Mem/Disk progress bars |
| P1 | `/ops/management` | `views/ops/OpsManagement.vue` | ✅ | Tabbed wrapper around TaskListView (snapshot / cdc / check / struct) |
| P1 | `/license` | `views/license/License.vue` | ✅ | Summary tiles + table + activation dialog |
| P2 | `/alert-monitor/monitor-setting` | `views/alertMonitor/MonitorSetting.vue` | ✅ | Single-form: retention, aggregation window, default channel/template, silence window |
| P2 | `/system/operate-log` | `views/system/OperateLog.vue` | ✅ | Multi-field filter + table |
| P2 | `/ops/control-log` | `views/ops/ControlLog.vue` | ✅ | Aggregated daily log files with preview drawer + download |
| P2 | `/ops/global-params` | `views/ops/GlobalParams.vue` | ✅ | Inline-editable runtime parameters |
| — | `/profile`, `/forbidden`, `/404` | `views/misc/*` | ✅ | Profile + 403 fallback + catch-all |

## Wizard step branches

The create-task wizard branches on `TaskCategory` via `STEP_KEYS_BY_CATEGORY` in `src/composables/useWizardSteps.ts`:

| Step | Snapshot | CDC | Check | Struct |
|---|---|---|---|---|
| 1. source | ✅ | ✅ | ✅ | ✅ |
| 2. test | ✅ | ✅ | ✅ | ✅ |
| 3. objects | ✅ | ✅ | ✅ | ✅ |
| 4. processing | ✅ | ✅ | ✅ | — |
| 5. advanced | ✅ | ✅ | ✅ | — |
| 6. precheck | ✅ | ✅ | ✅ | ✅ |
| 7. confirm | ✅ | ✅ | ✅ | ✅ |

Struct Migration skips processing (Lua processor) and advanced (parallelizer/pipeline) steps per ADR-0006, yielding a 5-step flow.

## Legacy route redirects

| Old path | Redirects to | Notes |
|---|---|---|
| `/tasks/sync` | `/tasks/snapshot` (or `/tasks/cdc` if `?mode=cdc`) | Query preserved |
| `/tasks/replay` | `/tasks/snapshot` | |
| `/tasks/verify` | `/tasks/check` | |
| `/tasks/create/sync` | `/tasks/create/snapshot` | |
| `/tasks/create/replay` | `/tasks/create/snapshot` | |
| `/tasks/create/verify` | `/tasks/create/check` | |
| `/tasks/sync/:id` | `/tasks/snapshot/:id` | |
| `/tasks/replay/:id` | `/tasks/snapshot/:id` | |
| `/tasks/verify/:id` | `/tasks/check/:id` | |

## Cross-page links (audited)

```
LicenseBanner (global, except /dashboard)
   └─ click "前往处理" → /license

Dashboard
   ├─ KPI 运行中任务 → /tasks/snapshot?status=running  (KpiCard click — FIX-011 2026-05-11)
   ├─ KPI 今日告警 → /alerts/current  (KpiCard click)
   ├─ 状态饼图扇区 → /tasks/snapshot?status=<status>  (onStatusClick)
   ├─ 最近任务 row → /tasks/{category}/{id}
   ├─ 高优先级告警 row → /tasks/{category}/{id}?tab=alerts
   ├─ "查看全部任务" → /tasks/snapshot  (RunningTaskGrid @more, was /tasks/sync prior to FIX-011 2026-05-11)
   ├─ "查看全部告警" → /alerts/current
   └─ License banner (built-in) → /license

TaskListView (snapshot/cdc/check/struct)
   ├─ name link / "查看任务" → /tasks/{category}/{id}
   ├─ "编辑" → /tasks/{category}/{id}?tab=config&edit=1
   ├─ create button → CreateTaskModeDialog → /tasks/create/{category}
   └─ honors ?status=, ?engine=, ?q= from URL

AlertTableView (current / history)
   ├─ task name link → /tasks/{category}/{taskId}?tab=alerts
   ├─ "查看任务" → same as above
   ├─ honors ?level=, ?taskId= from URL
   └─ summary cards toggle the level filter (active mode only)

TaskDetail
   ├─ honors ?tab= and ?edit=1 from URL
   ├─ back button → /tasks/{category}
   └─ delete confirm → /tasks/snapshot (fallback)

OpsManagement
   └─ Embeds 4 TaskListView instances (snapshot/cdc/check/struct); all child links propagate normally.

CreateTaskWizard
   ├─ 7-step branch for snapshot / cdc / check
   ├─ 5-step branch for struct (no processing/advanced steps)
   └─ on submit → /tasks/{category}/{newId} (so the user lands on the live task)

LicenseBanner / License
   └─ activate dialog → calls /api/license/activate; on success refreshes the page summary
```

All listed links were verified against `src/router/index.ts` route definitions and the `:category(snapshot|cdc|check|struct)/:id` constraint.

## Mock data notes

- MSW is opt-in only when `VITE_USE_MOCK=true`; production wiring uses the real backend.
- Backend provides live data — no MSW ticker needed for dashboard metrics.

## How to extend

1. Add a new route under `src/router/index.ts`. Use the `module` meta key so `Sidebar.vue` can highlight the active group.
2. Add page i18n keys to BOTH `src/locales/zh-CN.json` and `src/locales/en-US.json` (parity test enforced).
3. Cross-page deep link? Read `useRoute().query` in `onMounted` and seed the local filters from it (mirror the pattern in `TaskListView.vue` and `AlertTableView.vue`).
