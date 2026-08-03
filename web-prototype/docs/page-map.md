# ape-dts Console Web Prototype · Page Map & Cross-Page Linkage

The canonical task taxonomy is **Data Migration**, **Data Check**, and **Structure Migration**. Snapshot, Snapshot+CDC, and CDC are Data Migration modes, not top-level modules. Routes are declared in `src/router/index.ts`; shared task-list behavior lives in `src/components/TaskListView.vue`.

## Canonical page inventory

| Menu status | Path | Surface | Roles |
|---|---|---|---|
| Hidden | `/login` | Login | Anonymous |
| Hidden | `/forbidden` | Forbidden | All |
| Hidden | `/:pathMatch(.*)*` | Not Found | All |
| User menu | `/profile` | Profile | Authenticated |
| Primary | `/dashboard` | Dashboard | Authenticated |
| Data Migration | `/tasks/migration` | Migration task list | Authenticated |
| Data Check | `/tasks/check` | Check task list | Authenticated |
| Structure Migration | `/tasks/struct` | Structure task list | Authenticated |
| Hidden action | `/tasks/create/migration?mode=snapshot\|snapshot_cdc\|cdc` | Migration wizard | Admin, operator |
| Hidden action | `/tasks/create/check` | Check wizard | Admin, operator |
| Hidden action | `/tasks/create/struct` | Structure wizard | Admin, operator |
| Hidden | `/tasks/:category(migration\|check\|struct)/:id` | Task detail | Authenticated |
| Alerts | `/alerts/current` | Current alerts | Authenticated |
| Alerts | `/alerts/history` | Alert history | Authenticated |
| Operations | `/ops/management` | Operations management | Admin, operator |
| Operations | `/ops/control-log` | Control logs | Admin, operator |
| Operations | `/ops/global-params` | Global parameters | Admin |
| License | `/license` | License | Authenticated read |
| System | `/system/users` | User management | Admin |
| System | `/system/monitor` | System monitor | Admin |
| System | `/system/operate-log` | Operate log | Admin |
| Alert Config | `/alert-monitor/metrics` | Metric rules | Admin |
| Alert Config | `/alert-monitor/events` | Event rules | Admin |
| Alert Config | `/alert-monitor/monitor-setting` | Monitor settings | Admin |
| Alert Config | `/alert-monitor/alarm-setting` | Alarm channels | Admin |
| Alert Config | `/alert-monitor/alarm-template` | Alarm templates | Admin |

The executable inventory in `tests/unit/routeInventory.spec.ts` verifies the component, authentication status, role metadata, canonical path, and menu or hidden status for every row.

## Migration modes and wizard branches

| Mode | Query value | Persisted kind | Extract type | Wizard steps |
|---|---|---|---|---|
| Snapshot | `snapshot` | `snapshot` | `snapshot` | 7 |
| Snapshot + CDC | `snapshot_cdc` | `snapshot` | `snapshot_and_cdc` | 7 |
| CDC | `cdc` | `cdc` | `cdc` | 7 |
| Data Check | — | `check` | Check semantics | 7 |
| Structure Migration | — | `struct` | `struct` | 5 |

Migration and Check use Source, Connection test, Objects, Processing, Advanced, Precheck, and Confirm. Structure Migration omits Processing and Advanced.

## Compatibility redirects

Legacy paths are redirect-only. Unrelated query parameters and hash fragments are preserved.

| Legacy path | Canonical destination |
|---|---|
| `/tasks/snapshot` | `/tasks/migration?mode=snapshot` |
| `/tasks/cdc` | `/tasks/migration?mode=cdc` |
| `/tasks/sync` | `/tasks/migration`, preserving a valid mode and otherwise defaulting to `snapshot_cdc` |
| `/tasks/replay` | `/tasks/migration` |
| `/tasks/verify` | `/tasks/check` |
| `/tasks/create/snapshot` | `/tasks/create/migration?mode=snapshot` |
| `/tasks/create/cdc` | `/tasks/create/migration?mode=cdc` |
| `/tasks/create/sync` | `/tasks/create/migration`, preserving a valid mode and otherwise defaulting to `snapshot_cdc` |
| `/tasks/create/replay` | `/tasks/create/migration?mode=snapshot` |
| `/tasks/create/verify` | `/tasks/create/check` |
| `/tasks/{snapshot\|cdc\|sync\|replay}/:id` | `/tasks/migration/:id` with the corresponding mode |
| `/tasks/verify/:id` | `/tasks/check/:id` |

## Contractual cross-page links

- License banner → `/license`.
- Dashboard running-task KPI and “more” action → `/tasks/migration?status=running`.
- Dashboard alert KPI → `/alerts/current`.
- Dashboard task and activity rows → canonical task detail with migration mode context.
- Dashboard alert activity → canonical task detail with `tab=alerts`.
- Task-list name/view → canonical task detail.
- Task-list edit → canonical detail with `tab=config&edit=1`.
- Task-list create → canonical create route with the selected migration mode.
- Alert task link → canonical task detail with `tab=alerts`.
- Wizard submit → the newly created canonical task detail.
- Task-detail back and delete → canonical task list; migration mode context is retained.

The public router and rendered-browser tests cover direct navigation, compatibility redirects, authentication return targets, active navigation, breadcrumbs, and reload behavior.

## URL-backed view state

Task lists honor `status`, `engine`, `mode`, `q`, and `page`. Resource-group filtering is represented as `resourceGroup`. Task detail honors compatibility tab keys and `edit=1`. Current task-list polling is five seconds and pauses when the document is hidden.

## Mock data

MSW is enabled only with `VITE_USE_MOCK=true`. Production wiring uses the real backend; mock objects and metrics are not runtime truth.

## Extending the map

1. Add the route to `src/router/index.ts` with explicit `module`, `hideInMenu`, `public`, and `roles` metadata.
2. Add a matching row to `tests/unit/routeInventory.spec.ts`.
3. Add i18n keys to both locale files.
4. Cover cross-page behavior through the rendered public route or link seam, including direct navigation and reload where applicable.
