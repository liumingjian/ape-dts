# ape-dts Console Web Prototype · Page Map & Cross-Page Linkage

Locked at the end of build sprint (2026-04-22). All pages live under `src/views/<module>/`. Routes are declared in `src/router/index.ts`.

## Page completeness matrix

| Tier | Path | Component | Status | Highlights |
|---|---|---|---|---|
| P0 | `/login` | `views/auth/Login.vue` | ✅ | Bilingual login, demo creds, branded hero |
| P0 | `/dashboard` | `views/dashboard/Dashboard.vue` | ✅ | License banner · 4 KPI · 24h RPS/latency · status pie · engine bar · 7d alert trend · recent tasks/alerts |
| P0 | `/tasks/sync` | `views/tasks/SyncTaskList.vue` | ✅ | Sortable table with status / engine / RG filters, 8 s polling, batch actions, `?status=&engine=&q=` query honored |
| P0 | `/tasks/create/sync` | `views/tasks/CreateTaskWizard.vue` | ✅ | 7-step wizard, real validation, INI preview, draft persistence |
| P0 | `/tasks/sync/:id` | `views/tasks/TaskDetail.vue` | ✅ | KPI · 3 charts · config / objects / logs / alerts tabs · edit drawer · `?tab=` honored |
| P1 | `/tasks/replay`, `/tasks/verify` | wrap `TaskListView` | ✅ | Same shell as sync list |
| P1 | `/alerts/current` | `views/alerts/CurrentAlerts.vue` (`AlertTableView` mode=active) | ✅ | Level summary cards · auto-refresh toggle · level/source/engine/IP/taskId/alertId filters · batch clear |
| P1 | `/alerts/history` | `views/alerts/HistoryAlerts.vue` (`AlertTableView` mode=history) | ✅ | Date-range picker · cleared-at column · no batch actions |
| P1 | `/alert-monitor/metrics` | `views/alertMonitor/MetricRules.vue` | ✅ | CRUD drawer, threshold + level + period editor |
| P1 | `/alert-monitor/events` | `views/alertMonitor/EventRules.vue` | ✅ | Toggle + edit drawer for system events |
| P1 | `/alert-monitor/alarm-setting` | `views/alertMonitor/AlarmSetting.vue` | ✅ | Channel cards (Kafka / SNMP), inline test connection |
| P1 | `/alert-monitor/alarm-template` | `views/alertMonitor/AlarmTemplate.vue` | ✅ | Editor with template list, defaults per kind × format |
| P1 | `/system/monitor` | `views/system/SystemMonitor.vue` | ✅ | Host cards summary + sortable table with CPU/Mem/Disk progress bars |
| P1 | `/ops/management` | `views/ops/OpsManagement.vue` | ✅ | Tabbed wrapper around `TaskListView` (sync / replay / verify) |
| P1 | `/license` | `views/license/License.vue` | ✅ | Summary tiles + table + activation dialog |
| P2 | `/alert-monitor/monitor-setting` | `views/alertMonitor/MonitorSetting.vue` | ✅ | Single-form: retention, aggregation window, default channel/template, silence window |
| P2 | `/system/operate-log` | `views/system/OperateLog.vue` | ✅ | Multi-field filter + table |
| P2 | `/ops/control-log` | `views/ops/ControlLog.vue` | ✅ | Aggregated daily log files with preview drawer + download |
| P2 | `/ops/global-params` | `views/ops/GlobalParams.vue` | ✅ | Inline-editable runtime parameters |
| — | `/profile`, `/login`, `/404` | `views/misc/*` | ✅ | scaffolds |

## Cross-page links (audited)

```
LicenseBanner (global, except /dashboard)
   └─ click "前往处理" → /license

Dashboard
   ├─ KPI 运行中任务 → /tasks/sync?status=running
   ├─ KPI 今日告警 → /alerts/current
   ├─ 状态饼图扇区 → /tasks/sync?status=<status>
   ├─ 最近任务 row → /tasks/{category}/{id}
   ├─ 高优先级告警 row → /tasks/{category}/{id}?tab=alerts
   ├─ "查看全部任务" → /tasks/sync
   ├─ "查看全部告警" → /alerts/current
   └─ License banner (built-in) → /license

TaskListView (sync/replay/verify)
   ├─ name link / "查看任务" → /tasks/{category}/{id}
   ├─ "编辑" → /tasks/{category}/{id}?tab=config&edit=1
   ├─ create button → CreateTaskModeDialog → /tasks/create/{category}?mode=<mode>
   └─ honors ?status=, ?engine=, ?q= from URL

AlertTableView (current / history)
   ├─ task name link → /tasks/{cat}/{taskId}?tab=alerts (cat inferred from id prefix)
   ├─ "查看任务" → same as above
   ├─ honors ?level=, ?taskId= from URL
   └─ summary cards toggle the level filter (active mode only)

TaskDetail
   ├─ honors ?tab= and ?edit=1 from URL
   ├─ back button → /tasks/{category}
   └─ delete confirm → /tasks/sync (fallback)

OpsManagement
   └─ Embeds 3 TaskListView instances; all child links propagate normally.

CreateTaskWizard
   └─ on submit → /tasks/{category}/{newId} (so the user lands on the live task)

LicenseBanner / License
   └─ activate dialog → calls /api/licenses/activate; on success refreshes the page summary
```

All listed links were verified against `src/router/index.ts` route definitions and the `:category(sync|replay|verify)/:id` constraint. Two issues were found and fixed during this audit:

1. `Dashboard.goToTask` was emitting `/tasks/{id}` (missing the category segment) → would 404. Now uses `row.category`.
2. `Dashboard.goToTaskAlert` had the same omission. Now infers the category from the `taskId` prefix the seeder produces.

## Mock data notes

- `src/mock/db.ts` seeds 24 sync, 8 replay, 6 verify tasks; 12 active + 200 history alerts; 18 metric rules; 300 events; 4 licenses (1 perpetual / 2 expiring / 1 expired); 6 hosts; 8 global params.
- A 5 s ticker (`tickRunningMetrics`) jitters running task metrics and pushes a fresh point to each metric series so dashboard charts feel alive without backend.
- All endpoints inject 300–900 ms latency; tests use `pause(lo, hi)`. No artificial failure on alert pages (matches their high refresh rate).

## How to extend

1. Add a new route under `src/router/index.ts`. Use the `module` meta key so `Sidebar.vue` can highlight the active group.
2. Add page i18n keys to `src/locales/zh-CN.json` (full) and `src/locales/en-US.json` (scaffold).
3. New mock endpoint? Create / amend a handler in `src/mock/handlers/<module>.ts`; register it in `src/mock/handlers/index.ts`.
4. Cross-page deep link? Read `useRoute().query` in `onMounted` and seed the local filters from it (mirror the pattern in `TaskListView.vue` and `AlertTableView.vue`).
