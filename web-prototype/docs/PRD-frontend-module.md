# PRD — ape-dts Console: management plane for the ape-dts sync engine

## Problem Statement

Today ape-dts ships only as a single-binary CLI worker. Every Task is launched by hand: an engineer writes an INI file, copies it to a host, starts the binary, then watches `position.log` and `monitor.log` with `tail -f`. There is no way for an operator to see "all my migrations across all hosts on one screen", no way for an on-call to receive an alert when CDC lag spikes, no way for an admin to grant a teammate read-only access without sharing the host login. Customers ask for a web console; today's only answer is a 26-page Vue prototype (`web-prototype/`) that is fully mocked and cannot drive real Tasks.

Operators cannot:
- See running Snapshot / CDC / Check / Struct Migrations in one place
- Create a Task through a guided wizard instead of editing INI by hand
- Start, pause, resume or stop a Task without SSH'ing into the host
- Watch a Task's RPS / latency / buffer charts as it runs
- Tail a Task's logs from the browser
- Receive alerts when a Task fails, stalls, or breaches a threshold
- Validate prerequisites (privileges, replication slots, supplemental logging) before launch
- Audit who started or stopped what
- Restrict destructive operations to specific roles
- Operate without writing custom scripts to wrap the engine

## Solution

We deliver **ape-dts Console**, a web management plane for ape-dts consisting of:

1. A Vue 3 single-page application built on `web-prototype/` (adopted with changes), giving operators the screens listed in `web-prototype/docs/page-map.md` — Dashboard, Task lists for each of the four engine kinds, a 7-step Create-Task Wizard, Task Detail with metrics/logs/history, Alert center, Alert-rule and Alarm-channel management, License, System Monitor, Operate Log, Control Log, and Global Params.

2. A new Rust orchestrator service (`dt-console-server`) in the existing Cargo workspace, which owns Task definition CRUD, INI rendering, process supervision, log/metric ingestion, alert evaluation, RBAC, License enforcement, and audit logging. It exposes a JSON+SSE HTTP API consumed by the SPA. ADRs `0001..0009` under `web-prototype/docs/adr/` capture the architectural decisions; `web-prototype/docs/CONTEXT.md` is the canonical glossary.

3. A clean separation: ape-dts itself stays a stateless worker. The orchestrator does not edit the engine; it generates INI configs, fork-execs the engine, and reads its `/metrics` and `logs/` artifacts. Three executor backends will eventually exist (Local, Docker, Kubernetes); MVP ships with Local only.

The user experience: an operator logs in, picks an engine pair, walks the wizard, runs a precheck, launches the Task, and watches it converge on the Dashboard. An on-call gets paged via Kafka/SNMP when an Alert Rule fires. An admin manages users and License from the same console.

## User Stories

Numbered, organised by capability area. Actors: **Operator** (creates/runs Tasks), **Admin** (manages users/license/global params), **Viewer** (read-only), **Auditor** (reads logs only).

### Authentication & RBAC

1. As an **Admin**, I want to log into the console with a username and password, so that I can manage Tasks without sharing the host shell.
2. As an **Admin**, I want to create, disable, and delete user accounts and assign each one a role (admin / operator / viewer), so that responsibilities are split correctly.
3. As an **Admin**, I want to reset another user's password, so that account recovery does not require database access.
4. As any **User**, I want my session to persist across page reloads and to expire after configurable idle time, so that I am neither inconvenienced nor exposed if I walk away.
5. As an **Operator**, I want destructive buttons (delete, force-stop) to be hidden when I lack permission, so that I never click something I cannot actually do.
6. As a **Viewer**, I want every navigation item I can see to actually work, so that I do not hit 403 errors as a surprise.
7. As an **Admin**, I want every login attempt — successful or failed — recorded in an audit log, so that I can investigate suspicious activity.

### Dashboard

8. As an **Operator**, I want a single Dashboard that shows running-Task count, today's alert count, total RPS, and average lag with day-over-day deltas, so that I can spot trouble in five seconds.
9. As an **Operator**, I want a 24-hour line chart of cluster RPS and latency, so that I can correlate spikes with business events.
10. As an **Operator**, I want a status pie and an engine bar chart, so that I can see how my fleet is distributed.
11. As an **Operator**, I want a 14-day stacked-bar trend of alerts by severity, so that I can tell whether things are getting better or worse.
12. As an **Operator**, I want a "recent tasks" and "top firing alerts" widget I can click into, so that the Dashboard is a navigation hub.
13. As an **Admin**, I want a banner when the License is approaching expiry, so that I am not surprised by a refusal-to-start.

### Task creation (wizard)

14. As an **Operator**, I want to pick a top-level Task kind from { Snapshot Migration, CDC, Check, Struct Migration }, so that the wizard branches to the right steps.
15. As an **Operator**, when I pick Snapshot Migration, I want to choose a sub-mode { snapshot only, snapshot_file replay, snapshot + cdc }, so that I can express the actual operation.
16. As an **Operator**, I want to pick the source and target Engine from the supported list (MySQL, PostgreSQL, Oracle, GaussDB, MongoDB, Redis, Kafka, ClickHouse, StarRocks, Doris, TiDB, Foxlake), so that I do not have to memorise the engine matrix.
17. As an **Operator**, when I pick GaussDB, I want to pick a sub-mode { pg-mode, mysql-mode, oracle-mode }, so that the engine talks to the right protocol.
18. As an **Operator**, I want to enter source and target connection details and click "Test connection", so that I find out about credential or network problems before launch.
19. As an **Operator**, I want to pick which databases / tables / columns are included or excluded, with wildcard support, so that I can scope the migration.
20. As an **Operator**, I want to define routing rules (db_map / tb_map / col_map / topic_map), so that I can rename objects across heterogeneous engines.
21. As an **Operator**, I want to attach an optional Lua processor script (or upload a `.lua` file), so that I can transform rows in flight without changing the engine.
22. As an **Operator**, I want to pick a Parallelizer strategy and concurrency level, so that I can tune throughput vs ordering guarantees.
23. As an **Operator**, I want to set buffer size, max RPS, checkpoint interval, and resumer mode, so that I can match my pipeline to my SLA.
24. As an **Operator**, I want a Precheck step that runs all relevant checks (privileges, replication slots, supplemental logging, schema compatibility) and reports each item pass / fail / skipped with a remediation hint, so that I do not start a Task that cannot succeed.
25. As an **Operator**, I want to see the rendered INI in a read-only preview before submission, so that I can sanity-check the configuration.
26. As an **Operator**, I want my draft to persist in the browser if I navigate away, so that I do not lose 30 minutes of input on a refresh.
27. As an **Operator**, when I pick the Struct Migration kind, I want the wizard to skip steps that do not apply (Lua, Parallelizer for data, Resumer), so that the form does not waste my time.

### Task lifecycle

28. As an **Operator**, I want to start, pause, resume, and stop a Task with one click, so that I do not need shell access.
29. As an **Operator**, I want to edit a paused Task's filter / router / parallelizer / pipeline parameters and resume it, so that I can iterate without rebuilding.
30. As an **Operator**, I want to delete a finished or failed Task, so that my list does not become a graveyard.
31. As an **Operator**, I want to clone a Task as the starting point for a new one, so that similar migrations are five clicks apart.
32. As an **Operator**, I want to export a Task definition to a downloadable JSON or INI file, and re-import it, so that I can move definitions between environments.
33. As an **Operator**, I want batch operations on a list selection (pause / resume / stop / delete), so that I can act on dozens of Tasks at once.

### Task list & detail

34. As an **Operator**, I want a list page per Task kind with filters (status, engine, resource group, keyword) and 5-second polling, so that I always see fresh state.
35. As an **Operator**, I want each row to show a status badge, engine icons, RPS, lag, and progress, so that I can triage at a glance.
36. As an **Operator**, I want a Task Detail page with a header KPI strip and time-series charts, so that I can drill into one Task without losing context.
37. As an **Operator**, I want tabs on Task Detail for { Config, Objects, Logs, Monitor, Alerts, History }, so that information is layered, not crammed.
38. As an **Operator**, I want the Logs tab to tail a Run's selected log stream (default / position / monitor / commit / check series) live via SSE, so that I can debug without SSH.
39. As an **Operator**, I want to switch between Runs on the History tab and inspect a finished Run's archived logs, so that I can investigate post-mortem.
40. As an **Operator**, I want the Resumer state surfaced read-only on Task Detail (current binlog position / LSN / SCN / resume_token), so that I know where the Task will pick up from.

### Alerts

41. As an **Operator**, I want a Current Alerts page showing all unacknowledged firings with severity / source / engine / IP / task_id filters, so that I can address what matters first.
42. As an **Operator**, I want to clear one or many alerts in batch, so that the list reflects reality.
43. As an **Auditor**, I want a History Alerts page with a date-range picker, so that I can reconstruct an incident.
44. As an **Admin**, I want to define metric-threshold Alert Rules (metric, operator, threshold, severity, dwell time), so that the system pages me for the right things.
45. As an **Admin**, I want to enable or disable system Event rules (task failed, license expiring, host down), so that I do not fight the defaults.
46. As an **Admin**, I want to configure Alarm Channels (Kafka, SNMP) and test them with a synthetic alert, so that I can verify delivery before an incident.
47. As an **Admin**, I want to edit Alarm Templates per severity / format, so that downstream pagers receive readable messages.
48. As an **Admin**, I want a global silence window (e.g. weekly maintenance), so that we do not page during planned downtime.
49. As an **Operator**, I want a live Alert stream on the page so I see new firings without polling, so that incident response is real-time.

### License

50. As an **Admin**, I want to see the current License (sku, max-tasks, expiry, status), so that I know what I am entitled to.
51. As an **Admin**, I want to activate or renew a License by entering an activation code, so that renewals do not require redeploys.
52. As an **Admin**, I want the system to refuse to create a Task once max-tasks is reached, with a clear error, so that overruns are caught early.
53. As an **Admin**, I want a banner across the app when the License is within 30 days of expiry, so that nobody is caught off guard.

### System Monitor & Operations

54. As an **Admin**, I want a System Monitor page listing host CPU / memory / disk usage of orchestrator+executor hosts, so that I can capacity-plan.
55. As an **Admin**, I want an Operate Log page showing every console action (login, edit settings, role change), filterable by user/time/result, so that I can answer audit questions.
56. As an **Operator**, I want a Control Log page showing every Task lifecycle action (start/stop/pause/resume/edit/delete) with operator and result, so that I can reconstruct what happened to a stuck Task.
57. As an **Admin**, I want to view and edit Global Params (runtime, pipeline, security, alarm) in-line, so that operational tuning does not require a redeploy.
58. As an **Operator**, I want to organise Tasks into Resource Groups by team or environment, so that the list page is filterable by group.

### Internationalisation & accessibility

59. As a **User**, I want to switch the UI language between Simplified Chinese and English, so that I can use the console in my preferred language.
60. As a **User**, I want every page string to be localised in both languages on day one, so that I do not see English fragments in a Chinese deployment.
61. As a **User**, I want my locale preference to persist across sessions, so that I do not re-pick it every login.

### Browser & deployment

62. As a **User**, I want shareable URLs for any page or filter state (e.g. "tasks running on MySQL filtered by group"), so that I can paste a link in chat.
63. As a **User**, I want history-mode routing (no `#/`) so that links look professional and integrate with corporate web infrastructure.
64. As a **User**, I want sub-100 ms page transitions for cached routes, so that the console feels native.

### Engineering quality (cross-cutting)

65. As a **Developer**, I want CI to run lint + unit tests + e2e + build on every PR, so that broken changes do not reach a release.
66. As a **Developer**, I want a single command (`pnpm dev` against `cargo run -p dt-console-server`) to run the full stack locally, so that onboarding is one paragraph in the README.
67. As a **Developer**, I want Mock Service Worker available in dev mode behind a flag, so that I can iterate on UI without standing up the orchestrator.
68. As a **Developer**, I want the orchestrator to share `dt-common::config` types with the engine, so that an INI field name change cannot drift between the two.

## Implementation Decisions

### Architectural decisions (recorded as ADRs in `web-prototype/docs/adr/`)

- **Independent orchestrator service** (ADR-0001): a new Rust crate in the existing Cargo workspace owns the management plane; the engine stays unchanged.
- **On-prem single-tenant posture** (ADR-0002): no organisations, no SaaS multi-tenancy, License is enforced.
- **Built-in users + cookie sessions + three roles** (ADR-0003): admin / operator / viewer, OIDC deferred.
- **SQLite via sqlx for metadata** (ADR-0004): drop-a-binary deploy, swappable to Postgres / MySQL / GaussDB later.
- **Pluggable Executor abstraction, Local first** (ADR-0005): Docker / Kubernetes are subsequent implementations of the same trait.
- **Task taxonomy aligned to engine** (ADR-0006): four kinds — Snapshot Migration / CDC / Check / Struct Migration; the prototype's `sync / replay / verify` taxonomy is removed.
- **Brand: "ape-dts Console"** (ADR-0007): "DRS" is dropped.
- **Polling + SSE for realtime** (ADR-0008): WebSocket explicitly rejected.
- **Built-in Prometheus scraper + SQLite TS** (ADR-0009): no external Prometheus required to use the console.

### Modules to be built or modified

**`dt-console-server` (new crate)**

- **Executor** — trait with `spawn`, `kill`, `status`, `tail_logs`. `LocalExecutor` (MVP) fork-execs `ape-dts` with a per-Run working directory; future `DockerExecutor` and `KubernetesExecutor` implement the same trait.
- **IniRenderer** — pure function that takes a Task definition and returns the engine INI text, built on `dt-common::config::*` so the schema cannot drift.
- **TaskRepository / RunRepository / UserRepository / AlertRepository / etc.** — sqlx-backed repos; one per aggregate root.
- **TimeSeriesStore** — encapsulates ingest, retention, and downsampling for metric points; simple `record` + `query` interface, complex internals.
- **MetricsScraper** — background loop that polls each running Run's `/metrics:9090`, parses the Prometheus text format, and writes to TimeSeriesStore.
- **AlertEngine** — evaluates Alert Rules on a tick against fresh metric points, emits firing Alerts to the SSE publisher and the alarm-channel dispatcher.
- **AlarmDispatcher** — fan-out to Kafka / SNMP channels; pluggable for future channel kinds.
- **LogTailer** — abstracts log access per executor (file tail for Local; stream for Docker/K8s); produces `Stream<LogChunk>`.
- **SsePublisher** — in-memory broadcaster with topic subscriptions (`runs/{id}/logs`, `alerts/firing`); applies backpressure and per-stream rate limiting.
- **Authenticator** — login, password hashing (bcrypt), session creation/validation, role enforcement; surfaces a `UserContext` extractor for handlers.
- **LicenseValidator** — parses activation codes, returns License records, gates Task creation on the cap.
- **HTTP API layer** — JSON routes for auth, tasks, runs, alerts, alert rules, alarm channels, alarm templates, license, system, operate-log, control-log, global params; SSE routes for live log and alert streams.

**Web SPA (evolving `web-prototype/`)**

- **WizardValidator** — pure step-level validators returning typed error maps.
- **API client** — typed fetch with 401 → redirect-to-login, automatic CSRF token, `VITE_USE_MOCK` flag toggles MSW for local dev only.
- **useLogStream / useAlertStream composables** — wrap `EventSource` with reconnect, backpressure, and topic teardown.
- **useRbac composable** — derives button visibility from auth role + route `meta.roles`.
- **TaskCategory rename + routes** — `'sync' | 'replay' | 'verify'` is replaced by `'snapshot' | 'cdc' | 'check' | 'struct'`; old routes 301-redirect.
- **Wizard branching for Struct & GaussDB sub-mode** — Struct skips Lua / data parallelizer / resumer steps; GaussDB shows a sub-mode picker that drives later step content.
- **Branding** — `BrandMark`, login hero, footer, `index.html` `<title>`, all `app.brand.*` i18n keys updated.
- **i18n parity** — `en-US.json` brought to 100% of `zh-CN.json`.
- **Routing** — switch from `createWebHashHistory` to `createWebHistory`; nginx fallback config provided in deployment docs.
- **MSW gating** — only loaded when `VITE_USE_MOCK=true`; production builds never include the mock layer.

### Schema changes (new SQLite database in `dt-console-server`)

Tables: `tasks`, `runs`, `users`, `sessions`, `resource_groups`, `licenses`, `alerts`, `alert_rules`, `alarm_channels`, `alarm_templates`, `operate_logs`, `control_logs`, `global_params`, `metric_points`, `system_hosts`. Schema migrations run on orchestrator startup. All queries are written in portable SQL so the same migrations apply to PostgreSQL / MySQL / GaussDB DSNs.

### API contracts (HTTP + SSE)

- Auth: `POST /api/auth/login`, `POST /api/auth/logout`, `GET /api/auth/me`.
- Users: `GET / POST / PATCH / DELETE /api/users[/:id]` (admin only).
- Tasks: `GET / POST / PATCH / DELETE /api/tasks[/:id]`, `POST /api/tasks/:id/(start|stop|pause|resume)`, `POST /api/tasks/:id/test_connection`, `POST /api/tasks/:id/precheck`, `GET /api/tasks/:id/preview_ini`.
- Runs: `GET /api/runs/:id`, `GET /api/runs/:id/metrics?metric=&from=&to=&step=`, `GET /api/runs/:id/logs/stream` (SSE).
- Alerts: `GET /api/alerts`, `POST /api/alerts/:id/clear`, `POST /api/alerts/clear_batch`, `GET /api/alerts/stream` (SSE).
- Alert config: `GET / POST / PATCH / DELETE /api/alert_rules`, `/api/alarm_channels`, `/api/alarm_templates`; `POST /api/alarm_channels/:id/test`.
- License: `GET /api/license`, `POST /api/license/activate`.
- System: `GET /api/system/hosts`, `GET / PATCH /api/global_params`.
- Audit: `GET /api/operate_logs`, `GET /api/control_logs`.

All non-SSE responses are JSON. Errors use a single `{ code, message, details }` envelope. Field names mirror `dt-common::config` exactly where applicable; metric names mirror the engine's Prometheus gauge names verbatim.

### Specific interactions

- The wizard's Precheck step calls `POST /api/tasks/:id/precheck` (or a draft equivalent for unsaved Tasks); the orchestrator runs the engine's `dt-precheck` crate per-item and returns per-item pass/fail/skip + hints.
- Lifecycle endpoints write a `control_logs` entry before and after the executor call.
- All session-bearing API calls are recorded in `operate_logs` if they mutate state.
- `MetricsScraper` keys time-series rows by `(task_id, run_id, metric_name, ts)` so historical Runs remain queryable.
- The `metrics` cargo feature is required for orchestrator-driven metric collection; `LocalExecutor` builds the ape-dts binary with that feature enabled by default.

## Testing Decisions

A good test in this codebase is one that:
- Asserts only externally observable behaviour (returned values, emitted events, persisted rows, rendered output) — never private fields, never call counts on internal helpers.
- Uses real or in-memory backing stores where realistic (sqlx against `:memory:` SQLite) rather than mocks of the repository layer.
- Drives the unit under test through its public interface only; if a test needs to reach inside, the interface is wrong.
- Is deterministic: synthetic time, synthetic metric points, no `tokio::time::sleep` in pure-logic tests.
- Names the behaviour, not the method: "starts a Task only when the License cap is below max" rather than "test_start_task_1".

The MVP test investment goes to the five highest-leverage modules, identified during grilling as places where one failure produces a class of bugs that corrupts user trust:

- **IniRenderer** — golden-file tests: feed Task structs covering every kind × every engine × representative options, assert byte-exact INI output. Catches field-drift bugs that the prototype already has a history of (the audit caught two missing-`category`-segment regressions in nav code by similar discipline).
- **WizardValidator** — table-driven tests over (step, state) → expected errors, including boundary conditions (empty filter list, 0 parallel size, conflicting router rules). Pure functions, instant feedback.
- **Authenticator** — role-enforcement matrix (role × action → allowed/denied) tested against the actual middleware behind axum's test-helper, plus session lifecycle (login/refresh/idle expiry/logout) against an in-memory SQLite.
- **AlertEngine** — fed deterministic time-series points, assert exactly-which Alert events are emitted for each rule shape; cover edge cases (recovery threshold, dwell time, recovery while still firing).
- **TimeSeriesStore** — ingestion + query correctness across retention boundaries, downsampling correctness, monotonic-timestamp invariants under concurrent writers.

Beyond unit tests we run **one** end-to-end happy path with Playwright: login → create a Snapshot Migration via the wizard → start Task → see metrics on Detail → stop Task. This pins the integration of frontend, API, SSE, executor, scraper, and time-series store. Additional e2e coverage is deferred.

Prior art in the repo: `dt-tests/` is the engine's integration-test suite (database-backed scenario tests), which establishes the convention of "use a real backing store, not mocks" — orchestrator tests follow the same approach. The frontend has no prior tests; the conventions adopted here become the precedent.

CI runs lint (ESLint+Prettier on the SPA, `cargo clippy` on the workspace), `cargo test --workspace`, `pnpm test` (Vitest), `pnpm test:e2e` (Playwright with a synthetic backend fixture), and both builds. PRs cannot merge red.

## Out of Scope

- **Multi-tenant SaaS, billing, organisations, workspaces.** ADR-0002 is explicit.
- **OIDC / SAML / LDAP single sign-on.** Built-in users only; SSO is a v2 plug-in adapter.
- **Email / webhook / DingTalk / Feishu / Slack / Teams alarm channels.** MVP keeps the prototype's Kafka and SNMP only.
- **Docker and Kubernetes Executors.** Trait is shipped, only `LocalExecutor` is implemented in MVP.
- **External Prometheus / Grafana mandatory dependency.** Built-in scraper covers MVP; opt-in remote-write is a v2 ergonomics pass.
- **Engine-side changes.** The engine binary is not modified by this PRD; we only consume its CLI, `/metrics`, and log files.
- **Bidirectional WebSocket transport.** Polling + SSE only.
- **Mobile-responsive layouts.** Console is desktop-first; tablet/mobile is best-effort.
- **High availability of the orchestrator itself.** Single-instance MVP; HA via external state store and load balancer is a v2 deployment topology.
- **Per-Run resource quotas (CPU / memory limits).** Process-level isolation only; cgroup or container limits arrive with `DockerExecutor`.
- **Bundle / route-level code-splitting.** Listed as a v2 performance pass.
- **Storybook, accessibility audit, screen-reader certification.** Not in MVP.
- **Pre-existing prototype dead code we did not introduce.** ADR-0006 only renames what we rename; old prototype files we do not touch are left as-is.

## Further Notes

**Source of truth for terminology**: `web-prototype/docs/CONTEXT.md`. Whenever a UI label or API field is debated, the glossary settles it; if the glossary is silent, the engine's `dt-common::config` is the next-up source.

**Source of truth for architecture**: ADRs `0001..0009` under `web-prototype/docs/adr/`. New decisions during implementation that meet the ADR criteria (hard-to-reverse, surprising-without-context, real-trade-off) get a new numbered ADR; small tactical choices do not.

**Phasing recommendation** (not a hard contract):
1. Orchestrator skeleton + Executor + IniRenderer + auth + Task CRUD (3–4 weeks)
2. Run lifecycle + scraper + log tail + SSE + remaining APIs (2–3 weeks)
3. Frontend hardening: rebrand, taxonomy, struct & GaussDB UX, real-API wiring, RBAC enforcement, history routing, i18n parity, MSW gating (2–3 weeks)
4. Engineering bar: lint, tests, CI, e2e (1 week, runs in parallel with phase 3)

Estimated total 6–8 engineer-weeks, consistent with the audit estimate ("4–8 weeks for hardening, vs 8–16 weeks for redesign").

**Relationship to the existing prototype**: this PRD is "adopt with changes". `web-prototype/` stays the canonical home of the SPA — we do not move or rename the directory in MVP. The 26 pages, the 1653-line wizard, the typed domain model, and the design tokens are kept; the changes are the named modules above.

**Open items that do not block MVP** (will be settled during implementation, recorded as ADRs if they meet the criteria):
- Whether License activation is online or offline.
- Operate-log retention policy and archive format.
- Alarm-template variable interpolation syntax (proposed: mustache `{{var}}`).
- Whether Resource Group membership is mutable after Task creation (proposed: yes, admin-only).
