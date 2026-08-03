# ape-dts Console Frontend Executable Specification

Status: Draft for implementation

## 1. Purpose

This specification converts the Console context map and page map into an
executable product and engineering contract. It defines what pages exist, how
users move between them, which data is authoritative, which roles may act, and
which public seams must pass before the Console is considered production-ready.

It corrects stale route and behavior claims in
`web-prototype/docs/page-map.md`. The current code, router tests, backend API,
and engine contracts take precedence where the map disagrees with the merged
implementation.

The migration task-detail requirements in
`docs/specs/migration-task-detail-observability.md` remain the detailed profile
for Run-, Phase-, metric-, object-, log-, and lifecycle-specific behavior. This
document incorporates those requirements by reference and places them in the
whole-Console navigation and acceptance model.

## 2. Scope and sources of truth

### 2.1 Bounded context

Per `CONTEXT-MAP.md`, the Console management plane owns:

- Task and Run management;
- orchestration and process lifecycle;
- migration phase state;
- logs, metrics, alerts, and operational diagnostics;
- authentication, authorization, users, and profiles;
- license and resource-group management;
- the web user interface.

The migration engine remains authoritative for extraction, filtering, routing,
pipeline processing, sinking, checkpoints, resumability, and emitted metrics.
The operations and delivery context owns red-line tests, deployment, CI,
release procedures, and runbooks.

### 2.2 Authority order

When sources disagree, use this order:

1. Persisted backend state and the running engine's observable contracts.
2. Backend API models, validation, authorization, and handlers.
3. Frontend router, permission matrix, production API adapters, and executable
   contract tests.
4. Domain glossary and accepted ADRs.
5. Page maps, quick-start documents, mock fixtures, screenshots, and local
   planning artifacts.

MSW is an opt-in test and demonstration source enabled with
`VITE_USE_MOCK=true`. It is never production truth. Random or synthesized mock
objects and metrics must not be interpreted as engine observations.

### 2.3 State separation

The Console must not conflate these concepts:

- **Task configuration**: persisted identity, endpoints, mode, filters,
  routing, processing, resource group, and runtime configuration.
- **Run observation**: one process execution, its state, PID, start/stop times,
  exit result, checkpoint, logs, metrics, and history.
- **Phase state**: Snapshot, transition to CDC, or CDC state within a Run.
- **View state**: selected tab, filters, pagination, locale, loading state,
  dialog state, and transient form drafts.

View state may select or format authoritative data. It must not manufacture
runtime truth.

## 3. Canonical product taxonomy

### 3.1 User-facing modules

The canonical task navigation contains:

```text
Data Migration | Data Check | Structure Migration
```

Migration modes are:

```text
snapshot | snapshot_cdc | cdc
```

Chinese labels are respectively:

```text
全量 | 全量+增量 | 增量
```

Snapshot, CDC, and Snapshot+CDC are modes of Data Migration. They are not
separate top-level navigation modules. Pipeline is an engine-internal concept
and must not become a navigation category.

### 3.2 Persistence compatibility

The backend continues to persist Task kinds:

```text
snapshot | cdc | check | struct
```

`migration` is a user-facing view kind and list-query alias, not a persisted
Task kind.

Canonical create mapping is:

| User-facing flow | Persisted `kind` | `extractor.extract_type` |
|---|---|---|
| Full migration | `snapshot` | `snapshot` |
| Full + incremental migration | `snapshot` | `snapshot_and_cdc` |
| Incremental migration | `cdc` | `cdc` |
| Data check | `check` | Snapshot check semantics |
| Structure migration | `struct` | `struct` |

Compatibility types such as `SyncMode`, legacy category names, and route maps
may remain at adapters, but must be marked deprecated and must not leak into
new domain APIs.

## 4. Canonical page inventory

The router and menu configuration, not the old page-map route table, define the
current inventory.

### 4.1 Public and shell pages

| Route | Surface | Menu | Roles | Required behavior |
|---|---|---|---|---|
| `/login` | Login | Hidden | Anonymous | Authenticate, preserve a safe redirect, and route an authenticated user to Dashboard. |
| `/forbidden` | Forbidden | Hidden | All | Explain access denial without exposing protected data. |
| `/:pathMatch(.*)*` | Not Found | Hidden | All | Show a 404 surface and recovery navigation. |
| `/profile` | Profile | User menu | Authenticated | Show and update the current user's permitted profile fields. |
| `/dashboard` | Dashboard | Primary | Authenticated | Show license state, operational KPIs, charts, distributions, recent activity, and canonical deep links. |

### 4.2 Task pages

| Route | Component/surface | Menu | Roles | Required behavior |
|---|---|---|---|---|
| `/tasks/migration` | `SyncTaskList` → shared `TaskListView` | Data Migration | Authenticated | Server-paginated migration aggregate with mode, status, engine, resource-group, and text filters. |
| `/tasks/check` | Check task list → shared `TaskListView` | Data Check | Authenticated | Server-paginated Check tasks with truthful capability-specific columns and actions. |
| `/tasks/struct` | Struct task list → shared `TaskListView` | Structure Migration | Authenticated | Server-paginated Struct tasks with truthful capability-specific columns and actions. |
| `/tasks/create/migration` | `CreateTaskWizard` | Hidden action | Admin, operator | Seven-step migration wizard; mode comes from the canonical query contract. |
| `/tasks/create/check` | `CreateTaskWizard` | Hidden action | Admin, operator | Seven-step Check wizard with a backend-valid sinker contract. |
| `/tasks/create/struct` | `CreateTaskWizard` | Hidden action | Admin, operator | Five-step Struct wizard with backend-valid array filters. |
| `/tasks/:category(migration\|check\|struct)/:id` | `TaskDetail` | Hidden | Authenticated | Render Task identity and current/latest Run state; preserve mode and tab context. |

The shared list implementation lives under `src/components/TaskListView.vue`;
not every page implementation lives under `src/views/<module>`.

### 4.3 Alert and operations pages

| Route | Surface | Roles | Required behavior |
|---|---|---|---|
| `/alerts/current` | Current alerts | Authenticated | Active-alert summary, filters, task links, and authorized clear actions. |
| `/alerts/history` | Alert history | Authenticated | Date-filtered immutable cleared-alert history. |
| `/ops/management` | Operations management | Admin, operator | Operational task views without inventing separate canonical routes. |
| `/ops/control-log` | Control logs | Admin, operator | Immutable lifecycle intent/result history with diagnostics. |
| `/ops/global-params` | Global parameters | Admin | Validated global runtime configuration. |

### 4.4 System, license, and alert-configuration pages

| Route | Surface | Roles |
|---|---|---|
| `/license` | License summary and activation | Authenticated read; authorized mutation only |
| `/system/users` | User management | Admin |
| `/system/monitor` | System monitor | Admin |
| `/system/operate-log` | Operate log | Admin |
| `/alert-monitor/metrics` | Metric alert rules | Admin |
| `/alert-monitor/events` | Event rules | Admin |
| `/alert-monitor/monitor-setting` | Monitor settings | Admin |
| `/alert-monitor/alarm-setting` | Alarm channels | Admin |
| `/alert-monitor/alarm-template` | Alarm templates | Admin |

Each route must have a route-inventory test proving its component, auth
requirement, role metadata, canonical path, and menu module or hidden status.

## 5. Route and deep-link contract

### 5.1 Canonical task routes

```text
/tasks/migration
/tasks/check
/tasks/struct
/tasks/create/migration?mode=snapshot|snapshot_cdc|cdc
/tasks/create/check
/tasks/create/struct
/tasks/migration/:id?mode=snapshot|snapshot_cdc|cdc
/tasks/check/:id
/tasks/struct/:id
```

Canonical routes must render directly and survive reload. Router and server
hosting must support browser-history fallback.

### 5.2 Compatibility redirects

The following are redirect-only compatibility surfaces, not rendered pages:

| Legacy route | Canonical destination |
|---|---|
| `/tasks/snapshot` | `/tasks/migration?mode=snapshot` |
| `/tasks/cdc` | `/tasks/migration?mode=cdc` |
| `/tasks/sync` | `/tasks/migration`, preserving or normalizing mode |
| `/tasks/replay` | `/tasks/migration` |
| `/tasks/verify` | `/tasks/check` |
| Legacy create variants | Corresponding canonical `/tasks/create/...` route |
| Legacy detail variants | Corresponding canonical detail route |

Redirects must preserve unrelated query parameters and hash fragments. A saved
CDC URL must never silently default to Snapshot+CDC.

### 5.3 List state

The canonical list URL supports shareable state for:

```text
status | engine | mode | q | page
```

Resource group is also a backend list filter. The target contract requires it
to be URL-backed as `resourceGroup` or to be explicitly documented as
non-shareable local state; it must not appear shareable in documentation while
remaining local only.

The frontend and backend must use one pagination vocabulary. The target request
contract is:

```http
GET /api/tasks?category=migration&mode=cdc&status=running&engine=mysql&resource_group=<id>&q=<text>&page=1&page_size=20&sort=<field>&order=<asc|desc>
```

The existing frontend `size` versus backend `page_size` mismatch is a defect.
Acceptance requires the selected page size to change the backend query and the
number of returned rows.

The list refresh interval is currently five seconds. Documentation must not
claim eight seconds. Polling pauses or slows when the document is hidden and
must not reset filters, sorting, selection, or pagination.

### 5.4 Task detail state

Current compatibility tab keys are:

```text
config | objects | logs | monitor | alerts | history
```

`?edit=1` opens the permitted configuration edit surface. The target primary
navigation defined by the migration observability profile is:

```text
Overview | Sync objects | Logs | Monitoring | Run history | More
```

Implementation may migrate from current keys without breaking existing deep
links. Old keys must redirect or map deterministically during the compatibility
window.

Back and delete navigation use `listPathForTaskKind` semantics. Migration tasks
return to `/tasks/migration` with mode context preserved; they do not fall back
to `/tasks/snapshot`.

### 5.5 Cross-page links

At minimum, these links are contractual:

- Dashboard running-task KPI → `/tasks/migration?status=running`.
- Dashboard alert KPI → `/alerts/current`.
- Dashboard task/activity item → canonical task detail with mode context.
- Alert task link → canonical task detail with `tab=alerts` or its target
  Monitoring mapping.
- Task list name/view → canonical task detail.
- Task list edit → canonical detail with `tab=config&edit=1` during the
  compatibility window.
- Task list create → canonical create route and selected mode.
- Wizard submit → newly created canonical task detail.
- License banner → `/license`.

Every link must have a router-level test and at least one browser deep-link
scenario proving reload and query/hash preservation.

## 6. Wizard contract

### 6.1 Steps

Migration and Check currently use seven steps:

```text
Source | Connection test | Objects | Processing | Advanced | Precheck | Confirm
```

Structure Migration uses five steps:

```text
Source | Connection test | Objects | Precheck | Confirm
```

The step model must be derived from the canonical task capability, not copied
independently into page labels, tests, and submit logic.

### 6.2 Draft and preview behavior

- Draft state is isolated by task kind and migration mode.
- Credentials remain masked outside active credential inputs.
- Connection test and precheck use draft preview endpoints and do not persist a
  Task.
- Preview responses must identify validation warnings versus blocking errors.
- Confirm renders the exact sanitized payload/INI semantics that submit will
  use.

### 6.3 Submit behavior

- Submit calls the canonical create endpoint once.
- Optional immediate start happens only after successful creation and uses the
  persisted Task ID.
- Partial outcomes are explicit; the UI never reports a started Task when
  create or start failed.
- On success, the user lands on the live task detail page.

Before Check is accepted, its payload must supply the backend-required
`sinker.check_log_dir`. Before Struct is accepted, `filter.do_dbs` and
`filter.do_tbs` must use the non-empty array shape required by backend
validation. Page existence alone is not acceptance.

## 7. Authentication and authorization

### 7.1 Role capabilities

| Capability | Admin | Operator | Viewer |
|---|---:|---:|---:|
| Read dashboard/tasks/runs/alerts/license | Yes | Yes | Yes |
| Create tasks | Yes | Yes | No |
| Start/stop tasks | Yes | Yes | No |
| Pause/resume | Only when engine capability is acknowledged | Only when engine capability is acknowledged | No |
| Clear alerts | Yes | Yes | No |
| Read control logs | Yes | Yes | No |
| Manage users/system/alert configuration/global parameters | Yes | No | No |
| Activate or mutate license | Authorized admin policy | No | No |

### 7.2 Enforcement

Frontend route guards and hidden/disabled controls are usability controls, not
security boundaries. Every protected action and data endpoint must enforce the
same policy on the server.

Acceptance covers:

- anonymous direct-route behavior;
- authenticated route denial;
- hidden or disabled unauthorized controls;
- direct API denial using the same role;
- no protected response data before redirect;
- last-admin, self-role-escalation, and immutable-audit constraints.

## 8. Data and API truth contract

### 8.1 General response behavior

Production API errors render:

- backend error code and message;
- HTTP status where available;
- request ID;
- last refresh time;
- retry control;
- copy-diagnostics control.

Errors must not become an empty table, chart, log panel, or zero-valued KPI.
Credentials, activation codes, and endpoint passwords are redacted in API
responses, logs, diagnostics, and copied values.

### 8.2 Required task seams

The frontend requires typed contracts for:

- list Tasks with server filtering, sorting, and pagination;
- get Task configuration;
- create, update, delete, import, and export Tasks;
- preview INI, connection test, and precheck;
- start, stop, and future acknowledged lifecycle actions;
- list Runs and select current/latest Run;
- fetch Run-aware detail aggregate;
- latest and time-series Run metrics;
- selected objects and phase-specific runtime object state;
- persisted and live logs;
- alerts, resource groups, license, users, and audit/control logs.

### 8.3 Run-aware task detail

The target aggregate is:

```http
GET /api/tasks/{taskId}/detail
```

It separates:

```json
{
  "task": {},
  "currentRun": {},
  "phases": {},
  "metricsSnapshot": {},
  "progress": {}
}
```

The complete field semantics are defined in
`docs/specs/migration-task-detail-observability.md`.

Metrics and object queries identify the Run. Phase is explicit or is attributed
from persisted phase transition timestamps. Per-Task compatibility fields such
as `progressPercent` must not override current Run/Phase truth.

### 8.4 Lifecycle API

The canonical lifecycle surface is one endpoint per explicit action:

```http
POST /api/tasks/{taskId}/start
POST /api/tasks/{taskId}/stop
POST /api/tasks/{taskId}/pause
POST /api/tasks/{taskId}/resume
```

Each mutation uses the repository's idempotency contract and returns accepted
intent separately from confirmed Run state. Mock and production contracts must
not diverge by using `/action` plus a request-body action only in tests.

Pause/resume remain unavailable until the engine exposes an acknowledged,
checkpoint-safe protocol. Sending a signal successfully is not confirmation.
Stop moves through `stopping` and its confirmation is phase- and
checkpoint-aware.

## 9. Runtime observability profile

The following is mandatory for migration detail and any Dashboard aggregation
that derives from it.

### 9.1 Run and Phase states

Run states:

```text
pending | running | pausing | paused | stopping | stopped | failed
```

Phase names:

```text
snapshot | transitioning_to_cdc | cdc
```

Phase states:

```text
pending | running | completed | failed | skipped
```

A Snapshot+CDC Run remains running while CDC is active even after Snapshot is
completed.

### 9.2 Canonical metrics

```text
extractor_rps_avg
extractor_pushed_rps_avg
sinker_rps_avg
sinker_rt_avg
pipeline_queue_size
pipeline_queue_bytes
sinker_sinked_records
sinker_sinked_bytes
progress
extractor_plan_records
timestamp
```

Replication lag may be exposed through the agreed backend/engine lag contract.
The frontend must not request or present prototype aliases including:

```text
sinker_record_count_avg_by_sec
pipeline_buffer_size_avg
sinker_rt_per_query_avg
pipeline_sinked_count_latest
```

Unknown metrics produce a diagnostic state rather than silent no-data.

### 9.3 Snapshot versus CDC

Snapshot displays estimated copied-record and completed-object progress. It
marks estimated totals and does not display CDC lag.

CDC displays:

- apply throughput;
- replication lag;
- queue backlog;
- checkpoint;
- last event time;
- cumulative applied changes.

CDC never displays whole-task `100%`. A real zero is shown with context. Healthy
idle CDC reads equivalently to `0 rows/s · No new changes`; missing, stale, or
failed observations render distinct states.

### 9.4 Sync objects

Selected objects come from persisted Task filters or a backend object endpoint.
Runtime state comes from engine positions, completion markers, checkpoints, and
Run observations.

Snapshot object states:

```text
pending | loading | completed | failed
```

CDC object states:

```text
pending | subscribed | active | idle | lagging | failed | stopped
```

The production frontend must not generate table names, counts, or states.
Synthetic `table_1` fixtures are permitted only in explicitly marked mock
contract tests and must not serve as production acceptance evidence.

## 10. Log and diagnostic contract

### 10.1 SSE protocol

```text
event: log
data: {"timestamp":"...","level":"INFO","source":"dt-main","file":"default.log","message":"..."}
```

The frontend subscribes with `addEventListener('log', handler)`. A stream is not
healthy merely because an `EventSource` object was constructed. The UI reports
connected only after open plus protocol delivery or explicit backend
confirmation.

### 10.2 Persisted fallback

Opening Logs shows existing persisted lines and then follows live lines. When
SSE fails, times out, or yields no events while persisted logs exist, the UI
loads REST logs and announces:

```text
Live stream unavailable; showing persisted logs.
```

The log surface exposes Run ID, phase, file, connection state, last event,
search, level filters, follow/pause, download, and diagnostics. If both live and
persisted paths fail, the full API error contract is visible.

## 11. Visual, responsive, and accessibility contract

### 11.1 Product presentation

Touched surfaces use a restrained operational hierarchy: compact but readable,
stable controls, clear status and mode labels, aligned endpoint fields, and no
decorative elements that compete with operational data.

Database topology uses accessible icons plus engine text:

```text
[MySQL] source -- Snapshot + CDC --> target [PostgreSQL]
```

Host, port, and database align predictably. Credentials never render.

### 11.2 Responsive invariants

Acceptance viewports are at minimum:

```text
Desktop: 1440 × 900
Tablet: 768 × 1024
Mobile: 390 × 844
```

At each viewport:

- identity, status, primary action, and diagnostics remain visible;
- narrow headers use two rows rather than uncontrolled wrapping;
- primary tabs scroll or move low-frequency items under More;
- charts have stable non-zero height and resize after becoming visible;
- tables provide readable responsive scrolling or reflow;
- core information does not depend on clipped text;
- long Task names truncate with an accessible tooltip.

### 11.3 Accessibility invariants

- Interactive KPI cards, table actions, tabs, and mode controls are keyboard
  operable and have visible focus.
- Buttons, icons, and tabs have accessible names.
- Status never depends on color alone.
- Log connection/fallback/error changes are announced through an appropriate
  live region without flooding assistive technology.
- Charts provide textual titles, units, summaries, and differentiated loading,
  idle, no-sample, and error states.
- Both supported locales fit controls without obscuring essential actions.

## 12. Known current-to-target gaps

These are defects or incomplete contracts, not accepted product behavior:

1. `web-prototype/docs/page-map.md` still documents Snapshot and CDC as separate
   canonical routes instead of Migration modes.
2. Frontend list requests use `size` while the backend reads `page_size`.
3. Resource-group filtering is sent to the API but is not consistently
   represented in shareable URL state.
4. Check wizard output may omit required `sinker.check_log_dir`.
5. Struct wizard filter fields may serialize strings where backend validation
   requires non-empty arrays.
6. Task detail composes Task, Run, and metric calls instead of consuming a
   complete Run/Phase-aware aggregate.
7. Some detail charts and tests still use legacy metric names.
8. Log streaming uses default message handling and optimistic connection state
   instead of the named-event and confirmation contract.
9. Persisted-log fallback errors may collapse to empty content.
10. Pause/resume are exposed without an acknowledged engine protocol.
11. Mock objects synthesize names and states that are unsuitable as production
    acceptance truth.
12. Current detail tab keys differ from the target operations-first navigation.

A page or test marked present does not close these gaps. Each gap requires the
public-seam acceptance evidence below.

## 13. Delivery milestones

### Milestone 1: map and contract alignment

- Update `web-prototype/docs/page-map.md` to canonical Migration routes and the
  current inventory.
- Add executable route/menu/link inventory tests.
- Unify pagination and URL-backed filter names.
- Correct Check and Struct wizard payload contracts.
- Mark compatibility terminology and routes as deprecated.

### Milestone 2: truthful task operations

- Implement the Run/Phase-aware detail aggregate.
- Replace legacy metric names.
- remove production-synthesized objects;
- implement named log events, persisted fallback, and visible diagnostics;
- disable unacknowledged pause/resume;
- make stop and restart messaging phase-aware.

### Milestone 3: production presentation and accessibility

- Adopt Overview/Objects/Logs/Monitoring/History task-detail navigation while
  preserving old deep links.
- Add database topology and accessible engine identity.
- stabilize KPI/chart states and responsive layouts;
- complete keyboard, focus, semantic, and bilingual acceptance.

### Milestone 4: whole-Console executable coverage

- Cover the route, menu, RBAC, API, i18n, responsive, and diagnostic matrices.
- Run real Console-created MySQL→PostgreSQL Snapshot+CDC acceptance.
- Keep the core engine red-line mandatory for engine-affecting changes.

## 14. Verification seams

Tests exercise public boundaries, not private implementation details:

1. Router/menu definitions → rendered route and active navigation.
2. Auth session/role → route visibility and backend authorization result.
3. List view state → exact backend query and returned pagination.
4. Wizard inputs → preview contract → persisted Task → canonical detail URL.
5. Engine metrics → Console ingestion → Run metrics API → rendered KPI/chart.
6. Run/Phase persistence → detail aggregate → phase-specific UI.
7. Persisted Task filters and engine observations → object API → rendered
   object states.
8. Run log files → REST/SSE APIs → live and fallback log UI.
9. Lifecycle intent → engine acknowledgement → confirmed Run state.
10. Alerts/Dashboard rows → canonical task deep links.
11. Locale resources → all rendered routes in Chinese and English.
12. CSS/layout → desktop, tablet, and mobile browser acceptance.
13. Real MySQL→PostgreSQL source mutations → Console-created Run → target
   equality.

Targeted unit and contract suites must be augmented with Playwright for route,
interaction, responsive, and accessibility behavior. Mock fixtures verify API
adapters; they do not prove real migration behavior.

## 15. Acceptance scenarios

### 15.1 Navigation and authentication

1. An anonymous visit to a protected deep link redirects to `/login` with an
   exact safe return target; successful login returns to that target.
2. An authenticated visit to `/login` redirects to `/dashboard`.
3. Every canonical route renders after direct navigation and reload.
4. Every legacy task route reaches the expected canonical route without losing
   query or hash state.
5. Sidebar selection and breadcrumb labels reflect Migration, Check, or Struct,
   never a stale standalone CDC module.

### 15.2 Authorization

1. Viewer can read permitted pages but cannot discover or invoke task mutation,
   alert clear, user management, or configuration actions.
2. Operator can create and start/stop Tasks and clear alerts but cannot access
   admin-only pages or APIs.
3. Admin can access all defined management pages and authorized actions.
4. Direct API calls receive the same denial as the route/action policy.

### 15.3 Lists and deep links

1. Migration All includes Snapshot, Snapshot+CDC, and CDC Tasks with correct
   server totals.
2. Selecting each mode changes the URL and sends the corresponding backend
   query.
3. Page-size selection sends `page_size` and returns that page size.
4. Status, engine, mode, text, and the decided resource-group state survive
   reload and browser navigation.
5. Dashboard and alert links open the correct canonical detail, mode, and tab.
6. Delete returns to the correct canonical list and preserves relevant mode.

### 15.4 Wizards

1. Migration wizard has seven steps and defaults to `snapshot_cdc` only on the
   canonical route without an explicit mode.
2. Legacy CDC create opens canonical Migration creation in `cdc` mode.
3. Full, Full+Incremental, and Incremental submit the exact mapping in section
   3.2.
4. Check submit includes a backend-valid check-log sinker configuration.
5. Struct submit uses backend-valid object filter arrays and five steps.
6. Preview and precheck do not persist a Task.
7. Successful submit redirects to the live canonical detail page.

### 15.5 Migration observability

1. Snapshot running shows estimated row and object progress and no CDC lag.
2. Snapshot completion transitions the same Run through
   `transitioning_to_cdc` into CDC without marking the Task complete.
3. Healthy idle CDC shows zero throughput with `No new changes`, plus healthy
   checkpoint/heartbeat context and no completion percentage.
4. INSERT, UPDATE, and DELETE update applied totals, checkpoint, last-event time,
   and target data while the object remains subscribed/active.
5. Missing or unknown metrics produce diagnostics with metric name, backend
   error, and request ID.
6. Live Logs show persisted history followed by named-event streamed lines.
7. Stream failure falls back to persisted logs with an announcement; dual
   failure shows diagnostics.
8. Pause is unavailable unless engine acknowledgement capability exists.
9. Stop during CDC explains phase, checkpoint, resume/replay implications, and
   passes through `stopping`.

### 15.6 Responsive, accessibility, and localization

1. Desktop, tablet, and mobile viewports meet every invariant in section 11.
2. All primary operations are keyboard accessible with visible focus.
3. Status and database identity remain understandable without color.
4. Chinese and English expose the same routes, fields, actions, errors, and
   validation semantics.
5. Truncated names, chart summaries, and log state are available to assistive
   technology.

### 15.7 Real migration gate

For engine-affecting work, run:

```bash
bash scripts/e2e/mysql_to_postgresql_redline.sh
```

It passes only when Snapshot, CDC readiness, INSERT, UPDATE, DELETE, and final
MySQL/PostgreSQL equality pass using the real `dt-main` binary.

Whole-Console migration acceptance additionally requires a Console-created
Snapshot+CDC Run that demonstrates:

- Snapshot completion;
- transition into CDC;
- CDC INSERT, UPDATE, and DELETE propagation;
- truthful Run/Phase, metric, object, and log rendering;
- final source/target equality;
- visible diagnostic evidence if any API or stream fails.

The opt-in real-backend Playwright configuration must discover and execute the
Console scenario only when its backend and Docker prerequisites are present.
Default MSW CI must skip it explicitly rather than accidentally connecting to a
local backend.

## 16. Non-goals

- No extraction, routing, sinking, checkpoint, or CDC algorithm redesign in the
  frontend remediation slices.
- No new Structure, Check, or DDL-CDC engine functionality solely to satisfy
  this specification.
- No live exact `COUNT(*)` requirement for Snapshot totals.
- No Kubernetes or remote executor introduction.
- No unrelated Dashboard, License, System, Operations, or Alert redesign.
- No new engine metric name where a canonical emitted metric already exists.
- No re-expansion of Snapshot/CDC into deprecated top-level navigation.
- No persisted `kind=migration` database migration.
- No client-side final aggregation that breaks server pagination or sorting.
- No frontend-generated production object names, counts, progress, phases, or
  health states.
- No mock-only evidence presented as proof of a real migration chain.

## 17. Likely implementation and test areas

Backend:

- `dt-console-server/src/task_handlers.rs`
- `dt-console-server/src/run_handlers.rs`
- `dt-console-server/src/precheck_handlers.rs`
- `dt-console-server/src/metrics_handlers.rs`
- `dt-console-server/src/log_sse_handlers.rs`
- `dt-console-server/src/repositories/task_repository.rs`
- `dt-console-server/src/models/mod.rs`
- `dt-console-server/src/validation.rs`

Frontend:

- `web-prototype/src/router/index.ts`
- `web-prototype/src/config/menu.ts`
- `web-prototype/src/auth/permissions.ts`
- `web-prototype/src/components/TaskListView.vue`
- `web-prototype/src/views/tasks/CreateTaskWizard.vue`
- `web-prototype/src/views/tasks/TaskDetail.vue`
- `web-prototype/src/types/domain.ts`
- `web-prototype/src/utils/migrationMode.ts`
- `web-prototype/src/composables/useLogStream.ts`
- both locale files and shared layout/chart components

Executable coverage:

- route, route-config, menu, permission, and deep-link unit tests;
- task-list API/query and pagination contract tests;
- wizard DTO, validation, draft, and step tests;
- Run/Phase, KPI, object, metric-name, and log-stream contract tests;
- bilingual parity tests;
- `web-prototype/e2e/full-happy-path.spec.ts`;
- `web-prototype/e2e/mysql-to-postgres.spec.ts`;
- `scripts/e2e/mysql_to_postgresql_redline.sh`.

## 18. Completion definition

This specification is complete only when:

1. Every canonical page and compatibility redirect has executable inventory
   coverage.
2. Every role is verified at both UI and API boundaries.
3. Task, Run, Phase, metrics, objects, and logs render authoritative data with
   visible failure diagnostics.
4. All current-to-target gaps in section 12 are closed or explicitly deferred
   with an owner and compatibility behavior.
5. Chinese and English pass functional and responsive acceptance.
6. The real Console-created MySQL→PostgreSQL Snapshot+CDC scenario passes.
7. Engine-affecting work passes the repository core red-line without bypass or
   weakening.
