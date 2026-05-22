# ape-dts Console — Frontend Context

Domain language for the management plane (web console + orchestrator service) that fronts the ape-dts Rust sync engine. Upstream sources of truth: `docs/agent-summary/architecture.md` (engine architecture), `docs/en/config.md` (canonical INI schema), `dt-common::config::*` (Rust types). This file resolves the terms used by humans (UI labels, API field names, conversations); when a UI term differs from the engine term, the resolution lives here.

## Language

### Core task model

**Snapshot Migration** (全量迁移):
A one-shot bulk copy of source data into the target. Engine: `TaskType::Snapshot`, `ExtractType=snapshot|snapshot_file|scan`, `SinkType=write`.
_Avoid_: sync, full, dump.

**CDC** (增量同步):
Continuous capture and replay of source changes. Engine: `TaskType::CDC`, `ExtractType=cdc`, `SinkType=write|push|merge`.
_Avoid_: replay, incremental sync, replication.

**Snapshot + CDC** (全量+增量):
A composite **Snapshot Migration** followed by **CDC** in one task. Engine: `ExtractType=snapshot_and_cdc`. Modeled as a sub-mode of **Snapshot Migration**, not a separate top-level category.
_Avoid_: full+incremental, hybrid sync.

**Check** (数据校验):
Row-by-row comparison between source and target, producing diff/miss/extra reports. Engine: `TaskType::Check`, `SinkType=check`. Includes the read-back forms **Revise** (订正) and **Review** (复查).
_Avoid_: verify, validation, audit.

**Struct Migration** (结构迁移):
Schema-only migration (DDL, indexes, constraints). Engine: `TaskType::Struct`, `ExtractType=struct`, `SinkType=struct`.
_Avoid_: DDL sync, schema dump.

### Engine identity

**Engine** (引擎):
A supported database/message system: MySQL, PostgreSQL, Oracle, GaussDB, MongoDB, Redis, Kafka, ClickHouse, StarRocks, Doris, Foxlake, TiDB. Engine type maps to `dt_common::config::config_enums::DbType`.
_Avoid_: connector type, database flavor.

**Engine Sub-Mode** (引擎子模式):
A mode flag attached to a polymorphic engine. Today only **GaussDB** has sub-modes: `pg-mode`, `mysql-mode`, `oracle-mode` (the latter is also called `gaussdboracle`). Carried alongside the engine selection in the create-task wizard.
_Avoid_: variant, dialect.

**Endpoint** (端点 / 连接):
The connection profile for one side of a task — engine + url/host/port + auth + extra flags. A task has exactly one source **Endpoint** and one sink **Endpoint**.
_Avoid_: datasource, connection string.

### Task lifecycle

**Task** (任务):
The user-defined unit on the console: identity (`task_id`), source/target endpoints, mode, filter, router, parallelizer, pipeline, resumer, processor, metrics. Persisted in the orchestrator's metadata store. Renders deterministically into one engine **INI Config**.
_Avoid_: job, run, pipeline (in this project, pipeline means something else — see below).

**INI Config** (任务配置 / INI):
The text artifact ape-dts consumes. Sections: `[extractor] [sinker] [filter] [router] [parallelizer] [pipeline] [runtime] [global] [resumer] [processor] [metrics] [data_marker]`. Generated server-side from the **Task**, never edited by hand once a Task exists.
_Avoid_: yaml, manifest, spec.

**Run** (执行):
A single launch of a **Task** by the orchestrator: a process invocation of `ape-dts <ini-path>`. Has a start/stop time, exit status, captured logs, and a position checkpoint. Multiple **Run**s share one **Task**.
_Avoid_: instance, attempt.

**Position** (位点):
Engine-specific checkpoint for resuming CDC: MySQL binlog file+pos / GTID, PostgreSQL & GaussDB LSN + slot + publication, Oracle SCN, MongoDB resume_token, Redis repl_id+offset, Kafka partition+offset.
_Avoid_: offset, lsn (use as engine-specific term, not generic).

**Resumer** (断点续传):
The component that decides where a **Run** picks up from. `resume_type ∈ {from_log, from_target, from_db, dummy}`.

### Engine-internal pipeline

**Extractor** (抽取):
The Rust engine's source-side reader. One per source engine. Implementation lives in `dt-connector::extractor`. Exposed in INI as `[extractor]`.

**Sinker** (写入):
The engine's target-side writer. Implementation in `dt-connector::sinker`. INI section `[sinker]`.

**Parallelizer** (并发算法):
Strategy that fans rows from extractor to sinker(s). One of `serial | snapshot | rdb_partition | rdb_merge | rdb_check | mongo | redis | foxlake | table`.

**Pipeline** (数据管道):
The engine-internal wiring of extractor↔buffer↔parallelizer↔sinker, with `buffer_size`, `checkpoint_interval_secs`, `max_rps`. **Not** a user-visible category; do not surface "pipeline" as a navigation item.
_Avoid_: this is an internal term — the UI uses **Task** as the user-visible aggregate.

**Filter** (过滤):
DB / table / column / event-level inclusion+exclusion: `do_dbs`, `ignore_dbs`, `do_tbs`, `ignore_tbs`, `do_events`, `do_ddls`, `do_structures`, `ignore_cols`, `where_conditions`.

**Router** (路由):
DB / table / column / topic remapping: `db_map`, `tb_map`, `col_map`, `topic_map`.

**Processor** (Lua 处理器):
User-supplied Lua script that mutates `before` / `after` row maps per record. INI: `[processor].lua_code` or `lua_code_file`.

### Runtime observability

**Metric** (指标):
A Prometheus gauge emitted by ape-dts when built with the `metrics` cargo feature. Names: `extractor_rps_*`, `extractor_bps_*`, `pipeline_buffer_size_*`, `sinker_rps_*`, `sinker_bps_*`, `sinker_rt_per_query_*`, `progress`, `timestamp`. The console scrapes `:9090/metrics` from each running task.
_Avoid_: stat, counter (those are the engine's internal monitor counters, not what the console exposes).

**Log Stream** (任务日志):
Per-**Run** rolling files written to `logs/`: `default.log`, `commit.log`, `position.log`, `monitor.log`, `finished.log`, `statistic.log`, `task.log`, `http.log`, plus `miss/diff/sql/extra/summary.log` for Check tasks. The orchestrator tails and ships these to the console.

**Heartbeat** (源库心跳):
Periodic write the engine performs against a source-side `heartbeat_tb` / `heartbeat_key` so CDC liveness can be observed even on idle sources.

### Console-only concepts (no engine counterpart)

**Orchestrator** (编排服务):
The control-plane HTTP service (Rust crate, axum/actix-web) that owns Task CRUD, INI rendering, **Executor** dispatch, log/metric ingestion, auth, License, and operate-log. Single binary deploy, on-prem.

**Executor** (执行器):
A pluggable backend that knows how to spawn / kill / status / tail-logs of an ape-dts process. MVP implementation: `Local` (fork-exec on the orchestrator host). Future: `Docker`, `Kubernetes`.

**Resource Group** (资源组):
A logical grouping for Tasks (e.g. by environment / team / customer). Tasks belong to exactly one Resource Group. Console-only; not visible to the engine.

**License** (许可证):
On-prem activation record (sku, max_tasks, expire_at). Enforced by orchestrator at Task-creation time.

**User** (用户) / **Role** (角色):
Console accounts, roles ∈ `{admin, operator, viewer}`. Server-side cookie sessions. Not exposed to the engine.

**Operate Log** (操作日志) / **Control Log** (控制日志):
**Operate Log** records human actions in the console (login, edit settings). **Control Log** records lifecycle events on Tasks (start/stop/pause/resume/delete). Two separate audit streams; do not merge.

**Alert** (告警) / **Alert Rule** (告警规则) / **Alert Channel** (告警通道):
**Alert Rule** = threshold definition over a **Metric** or system **Event**; firing produces an **Alert** routed via an **Alert Channel** (MVP supports Kafka, SNMP only).

## Relationships

- A **Task** has exactly one source **Endpoint** and one target **Endpoint**.
- A **Task** is exactly one of {**Snapshot Migration**, **CDC**, **Check**, **Struct Migration**}; it never spans categories.
- A **Task** belongs to exactly one **Resource Group** and is owned by exactly one **User**.
- A **Task** produces 0..N **Run**s. A **Run** writes 1 INI file, 1 process, N **Log Stream**s, and emits **Metric**s.
- An **Executor** dispatches **Run**s; one **Executor** kind is configured per orchestrator instance.
- An **Alert Rule** observes **Metric**s emitted by **Run**s; firing produces an **Alert** addressed by `task_id` + `instance_ip`.
- The **License** caps the count of concurrently-defined **Task**s.

## Example dialogue

> **PM:** "Operations want to re-run a finished migration but skip already-loaded tables."
> **Dev:** "That's not a new **Task** — same source/target/filter/router/parallelizer. We start a new **Run** of the existing **Task** with `resume_type=from_target`."
> **PM:** "And if they want to compare the result afterwards?"
> **Dev:** "That's a separate **Task**, kind = **Check**. Same endpoints, but its `SinkType=check`. The console shows it as `数据校验` in a different list."
> **PM:** "We say 'sync' a lot — is that a **Task** kind?"
> **Dev:** "No. The prototype's `sync` was an umbrella for several engine modes. We dropped it. The user picks **Snapshot Migration**, **CDC**, or **Snapshot + CDC** (sub-mode of Snapshot Migration). 'Sync' is fine in casual speech but never in a UI label or API field."

## Flagged ambiguities

- "sync" / "replay" / "verify" (prototype `TaskCategory`) were used as top-level categories that did not map cleanly to engine `TaskType`. **Resolved**: replaced by `Snapshot / CDC / Check / Struct` (中文 `全量迁移 / 增量同步 / 数据校验 / 结构迁移`). `replay` (which mapped to `ExtractType=snapshot_file`) is now a *source option* on **Snapshot Migration**, not a category.
- "sync mode" (prototype `SyncMode = 'snapshot' | 'cdc' | 'snapshot_cdc'`) collided with the new top-level kinds. **Resolved**: removed. The top-level **Snapshot Migration** kind has an `extract_type` field with values `snapshot | snapshot_file | snapshot_and_cdc`; `cdc` is its own top-level kind.
- "DRS / Data Replication Service" (prototype branding) collides with Huawei Cloud's DRS product and is absent from the Rust core. **Resolved**: brand is **ape-dts Console**.
- "pipeline" was overloaded — the engine uses it for the internal extractor↔sinker wiring; product talk used it for "a sync flow". **Resolved**: in this project **pipeline** strictly means the engine-internal `[pipeline]` section; the user-visible aggregate is **Task**.
- "task / job / run / instance" were used interchangeably. **Resolved**: only **Task** (definition) and **Run** (one execution) exist. Never use job or instance.
- "user / account" — the prototype had no clear distinction. **Resolved**: only **User** exists; "account" is not used.
