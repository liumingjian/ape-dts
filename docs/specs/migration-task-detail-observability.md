# Migration Task Detail: Truthful Observability and Operations

Status: Draft for implementation

## 1. Problem

The Console migration task detail page does not truthfully represent a real
`Snapshot + CDC` run. The current page mixes Task, Run, and migration phase
state; reads runtime metrics from configuration fields; requests metric names
that the engine does not emit; synthesizes sync-object data in the frontend;
and reports the log SSE connection as healthy while rendering no logs.

For the verified MySQL to PostgreSQL run, the page simultaneously showed:

- Task status: running
- Progress: 100%
- Completed/total tables: 0/0
- Throughput: 0 rows/s
- Empty logs with SSE shown as connected

The actual run had completed Snapshot, entered CDC, persisted metrics and
checkpoints, and written non-empty log files. The Console therefore cannot be
used as a trustworthy operational surface.

## 2. Goals

1. Represent Task, Run, and Phase as distinct concepts.
2. Show finite Snapshot progress without assigning a completion percentage to
   continuous CDC.
3. Display runtime metrics from the current Run using canonical engine metric
   names.
4. Display real selected objects and phase-specific object state.
5. Make live and archived Run logs visible and diagnosable.
6. Ensure lifecycle actions never report success without engine confirmation.
7. Present source-to-target topology and operational state with production
   software visual quality.

## 3. Non-goals

- Changing extraction, routing, sinking, checkpoint, or CDC algorithms.
- Adding structure migration, Check-task, or DDL CDC functionality.
- Making Snapshot row totals exact with a live `COUNT(*)`.
- Introducing Kubernetes or remote executors.
- Redesigning unrelated Console pages.
- Adding new metric names to the engine when an existing canonical metric
  already expresses the required value.

## 4. Canonical domain model

### 4.1 Task

A persisted migration definition containing identity, endpoints, configured
extract type, filters, routing, pipeline settings, and runtime configuration.
A Task can produce multiple Runs.

Task state describes whether the definition has an active Run and which
operations are available. It does not describe Snapshot completion.

### 4.2 Run

One execution of a Task. A Run owns its process lifecycle, start and stop
times, exit result, logs, checkpoint, metrics, and phase history.

Canonical Run states:

```text
pending | running | pausing | paused | stopping | stopped | failed
```

A Run must not enter `paused` until the engine confirms that processing
stopped safely — in practice, until it exits `143` after a completed drain
(see [ADR 0004](../adr/0004-pause-is-a-graceful-stop-with-a-resumable-position.md)).

### 4.3 Phase

A bounded stage within a Run.

Canonical phase names:

```text
snapshot | transitioning_to_cdc | cdc
```

Canonical phase states:

```text
pending | running | completed | failed | skipped
```

A `snapshot_and_cdc` Run has a Snapshot phase followed by a CDC phase. The Run
remains `running` while CDC is active even though Snapshot is `completed`.

### 4.4 Sync Object

A selected database or table participating in a Task. Runtime object state is
phase-specific:

Snapshot object state:

```text
pending | loading | completed | failed
```

CDC object state:

```text
pending | subscribed | active | idle | lagging | failed | stopped
```

A CDC-subscribed object must never be labelled simply `completed` while the
Run is active.

### 4.5 Progress

Progress is finite-work completion and therefore applies to Snapshot, not
continuous CDC.

Snapshot progress has two complementary views:

- Row progress: copied records / estimated source records.
- Object progress: completed tables / total selected tables.

`extractor_plan_records` is an estimate. UI text must preserve that fact, for
example `10 / ~100 records` or `10 / estimated 100 records`.

If a reliable denominator is unavailable, the UI shows the copied count and
`Estimating total` rather than inventing a percentage.

### 4.6 CDC health

CDC replaces progress with:

- apply throughput;
- replication lag;
- queue backlog;
- current checkpoint;
- last event time;
- cumulative applied changes.

An idle source with zero throughput is healthy when lag, heartbeat, process,
and checkpoint signals are healthy. The UI must distinguish `Idle: no new
changes` from missing metrics or a stalled Run.

## 5. Required backend contract

The task detail surface requires a Run-aware aggregate. It may be implemented
as a new endpoint or a backward-compatible extension, but Task configuration
and runtime observations must remain separate.

Recommended endpoint:

```http
GET /api/tasks/{taskId}/detail
```

Recommended response shape:

```json
{
  "task": {
    "id": "task-id",
    "name": "mysql-to-postgresql",
    "status": "running",
    "configuredExtractType": "snapshot_and_cdc",
    "sourceEndpoint": {},
    "targetEndpoint": {},
    "selectedObjects": []
  },
  "currentRun": {
    "id": "run-id",
    "status": "running",
    "currentPhase": "cdc",
    "startedAt": "2026-07-16T07:58:21.823Z",
    "stoppedAt": null,
    "exitCode": null,
    "checkpoint": {
      "kind": "mysql_binlog",
      "file": "mysql-bin.000003",
      "position": 2671,
      "eventTimestamp": "2026-07-16T07:59:28Z"
    }
  },
  "phases": {
    "snapshot": {
      "status": "completed",
      "startedAt": "...",
      "completedAt": "..."
    },
    "cdc": {
      "status": "running",
      "startedAt": "...",
      "completedAt": null
    }
  },
  "metricsSnapshot": {
    "sampledAt": "...",
    "extractorRps": 0,
    "sinkerRps": 0,
    "pipelineQueueSize": 0,
    "sinkedRecords": 4,
    "replicationLagSeconds": 0.3,
    "lastEventAt": "..."
  },
  "progress": {
    "kind": "cdc",
    "percent": null,
    "copiedRecords": 3,
    "estimatedTotalRecords": 3,
    "totalIsEstimate": true,
    "completedTables": 1,
    "totalTables": 1
  }
}
```

### 5.1 Canonical metric names

The Console uses engine-emitted names as its storage and query contract:

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

The frontend must not request legacy or prototype names such as:

```text
sinker_record_count_avg_by_sec
pipeline_buffer_size_avg
sinker_rt_per_query_avg
pipeline_sinked_count_latest
```

Unknown metric names produce a visible diagnostic state and must not be
silently converted to an empty chart.

### 5.2 Phase attribution

Every metrics snapshot and time-series query must identify the Run. Phase must
be returned explicitly or deterministically associated using persisted phase
transition timestamps. A Snapshot value retained in the same Run must not be
presented as current CDC progress.

### 5.3 Selected objects

Selected objects come from the persisted Task filters or a backend object
endpoint. Runtime states come from engine positions, finished markers, and
Run observations. The frontend must not generate object names, row counts, or
states.

## 6. Log contract

### 6.1 SSE protocol

The backend and frontend use one named event and one payload schema:

```text
event: log
data: {"timestamp":"...","level":"INFO","source":"dt-main","file":"default.log","message":"..."}
```

The frontend subscribes with `addEventListener('log', handler)`. It must not
parse raw text as the structured payload above.

### 6.2 Historical fallback

When SSE disconnects, times out, or receives no events while the selected log
file contains data, the UI loads the REST log endpoint and displays:

```text
Live stream unavailable; showing persisted logs.
```

The page exposes:

- Run ID;
- phase;
- selected log file;
- connection state;
- last event timestamp;
- search and level filtering;
- automatic scrolling control;
- log download;
- API error code and request ID.

### 6.3 Error behavior

Log, metric, alert, and history failures must render an error state containing:

- backend error code and message;
- HTTP status when available;
- request ID;
- last refresh time;
- retry control;
- copy-diagnostics control.

Errors must not be reduced to an empty panel.

## 7. Lifecycle action requirements

### 7.1 Pause and resume

Pause is a graceful stop carrying intent; resume starts a **new Run** from the
paused Run's position log. See
[ADR 0004](../adr/0004-pause-is-a-graceful-stop-with-a-resumable-position.md).

1. Pause moves the Run to `pausing` and sends SIGTERM — the same cooperative
   signal a stop sends.
2. The engine drains, forces a final checkpoint and exits `143`.
3. The supervisor reads the exit code: `143` under `pausing` becomes `paused`;
   exit `4` (the drain never converged) becomes `failed`, because a position
   that was never finished is not resumable.
4. Resume renders a fresh INI pinned to the paused Run's position log, starts
   a new Run, and closes the predecessor out as `stopped`/`resumed`.

The acknowledgement is the exit code, not the signal: sending a signal
successfully is still not pause confirmation, and nothing but the supervisor
may write `paused`.

Pause is offered for `snapshot` and `cdc` tasks only — `check` and `struct`
have no position to resume from, and managed `snapshot_and_cdc` tasks own
their own snapshot→cdc start marker.

### 7.2 Stop

The confirmation dialog is phase-aware. It states:

- current phase;
- latest confirmed checkpoint;
- whether a subsequent Run restarts Snapshot or resumes CDC;
- duplicate or replay implications;
- whether the action is reversible.

The Run passes through `stopping` before reaching `stopped`.

## 8. User interface requirements

### 8.1 Header

The header contains Task identity, current Run status, current phase, and the
primary safe action. Secondary actions are grouped under `More`; destructive
actions are visually separated.

Long Task names truncate on one line with a tooltip. Narrow viewports use a
two-row layout rather than uncontrolled flex wrapping.

### 8.2 Migration topology

The overview displays a source-to-target topology:

```text
[MySQL icon] source  -- Snapshot + CDC -->  target [PostgreSQL icon]
```

Each endpoint shows engine, host, port, and database in aligned fields.
Credentials are never displayed. Full URLs are masked by default and support
safe copying.

Database identity uses an accessible SVG icon plus text; color alone is not
sufficient.

### 8.3 Snapshot + CDC summary

During CDC, the primary summary reads equivalently to:

```text
Running · CDC phase
Snapshot: completed, 3 / estimated 3 records, 1 / 1 tables
CDC: continuously replicating, 0 rows/s, lag 0.3 s
Checkpoint: mysql-bin.000003:2671
Last event: 2026-07-16 15:59:28
```

The page does not show overall `100%` during active CDC.

### 8.4 KPI behavior

The primary CDC KPIs are:

- apply throughput;
- replication lag;
- pipeline backlog;
- cumulative applied changes or last checkpoint.

A status card must not display a hard-coded numeric zero. Missing values render
as `—` with a reason. Real zero values render as zero with contextual health
text.

### 8.5 Charts

Product labels replace metric keys. Each chart has a title, unit, explanation,
and differentiated loading, no-sample, idle, error, and populated states.
Chart containers have a stable explicit height and resize when their tab or
container becomes visible.

### 8.6 Navigation

Primary detail navigation is:

```text
Overview | Sync objects | Logs | Monitoring | Run history | More
```

Configuration may be shown in a drawer or secondary surface. Alerts may be
integrated with Monitoring. Narrow viewports use scrollable tabs or a More
menu.

## 9. Milestones

### Milestone 1: truthful and safe operations

- Fix the log SSE event and payload contract.
- Add persisted-log fallback and visible diagnostics.
- Remove or disable unacknowledged pause/resume.
- Replace legacy metric names with canonical names.
- Separate Task configuration from current Run metrics.
- Remove generated sync-object data.
- Expose current phase.

### Milestone 2: phase-aware domain model

- Add Run and Phase summaries to the task detail API.
- Implement Snapshot-specific progress and CDC-specific health.
- Expose selected objects and phase-specific object states.
- Promote checkpoint and last-event information to primary CDC state.
- Make stop and restart messaging phase-aware.

### Milestone 3: production UI quality

- Add the source-to-target topology and database icons.
- Rework header hierarchy and responsive actions.
- Replace internal metric names with product labels.
- Stabilize chart layouts and empty states.
- Reorganize tabs around operational frequency.
- Validate desktop, tablet, and mobile layouts.

## 10. Acceptance scenarios

### 10.1 Snapshot running

Given a Snapshot phase with copied records and an estimated total, the page
shows row and table progress and explicitly marks the total as estimated. It
does not show CDC lag.

### 10.2 Transition to CDC

When Snapshot completes, the same Run records Snapshot as completed, enters
`transitioning_to_cdc`, and then enters CDC. The page never reports the whole
Task as completed during this transition.

### 10.3 CDC idle

Given a healthy CDC process with no new source changes, throughput displays
`0 rows/s` with `No new changes`; checkpoint, heartbeat or last-event health
remain visible. The page does not show a completion percentage.

### 10.4 CDC change

When the source receives an insert, update, or delete, applied-change totals,
throughput, checkpoint, and last-event time update from Run metrics. The
selected object remains subscribed or active, not completed.

### 10.5 Missing metric

When a requested metric is unavailable, the chart renders a diagnostic state
with the metric name, backend error, and request ID. It does not silently show
`No data`.

### 10.6 Live logs

Given an active Run writing `default.log`, opening Logs displays existing
persisted lines followed by newly streamed lines. The connection indicator is
healthy only after the protocol has delivered or explicitly confirmed the
stream.

### 10.7 Stream failure

When SSE fails but persisted logs are readable, the UI announces fallback and
continues to show logs. When both fail, it shows the backend error and request
ID.

### 10.8 Pause unavailable

Pause is not offered for task kinds with no resumable position (`check`,
`struct`) or for managed `snapshot_and_cdc` tasks; the API answers those with
409 `UNSUPPORTED_FOR_KIND`. The Console still cannot enter a paused state by
merely sending a signal: it shows `pausing` until the engine's exit code says
the position landed.

### 10.9 Stop during CDC

The confirmation dialog shows current phase and checkpoint and explains the
next-run resume behavior before allowing Stop.

### 10.10 Responsive layout

At desktop, tablet, and mobile widths, Task identity, topology, phase status,
primary actions, charts, and tabs do not overlap, collapse to zero height, or
rely on horizontal clipping for core information.

## 11. Verification seams

Implementation tests must exercise these public seams:

1. Engine metrics endpoint to Console ingestion and metrics API.
2. Run and phase persistence to task-detail API.
3. Run log files to REST and SSE log APIs.
4. Task-detail API to rendered migration detail page.
5. Lifecycle action API to confirmed engine process behavior.
6. Real MySQL to PostgreSQL Snapshot + CDC flow through the Console.

For changes affecting the migration engine, the repository core red-line
remains mandatory:

```bash
bash scripts/e2e/mysql_to_postgresql_redline.sh
```

Console-specific acceptance additionally requires a real Console-created Run
that demonstrates Snapshot completion, CDC INSERT/UPDATE/DELETE, visible logs,
correct phase state, and source/target equality.

## 12. Likely implementation areas

Backend:

- `dt-console-server/src/task_handlers.rs`
- `dt-console-server/src/run_handlers.rs`
- `dt-console-server/src/two_phase.rs`
- `dt-console-server/src/metrics_handlers.rs`
- `dt-console-server/src/log_sse_handlers.rs`
- `dt-console-server/src/models/mod.rs`

Frontend:

- `web-prototype/src/views/tasks/TaskDetail.vue`
- `web-prototype/src/types/domain.ts`
- `web-prototype/src/composables/useLogStream.ts`
- `web-prototype/src/components/ChartCard.vue`
- `web-prototype/src/components/EngineTag.vue`
- task-detail API client modules and tests

Domain documentation:

- `web-prototype/docs/CONTEXT.md`
- `web-prototype/docs/adr/` only if implementation resolves a hard-to-reverse,
  surprising trade-off
