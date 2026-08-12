# Pause is a graceful stop with a resumable position

Status: accepted

"Pause" in the console is a **graceful stop carrying intent**, and "resume" starts a **new Run** from the paused Run's position log. The engine is unchanged: pause sends the same SIGTERM a stop does ([ADR 0003](0003-signals-stop-a-task-cooperatively-and-exit-non-zero.md)), the process drains, records its position and exits `143`, and nothing survives in memory. A Run therefore still corresponds to exactly one engine process, and `paused` is a terminal-but-resumable resting state rather than a suspended process.

## Context

The console shipped a pause/resume pair that sent SIGUSR1 and SIGUSR2. `dt-main` never registered handlers for either, so their default action applied: **Terminate**. Pressing "pause" killed the engine outright — no drain, no final checkpoint — and the console then wrote `paused`, claiming a state that did not exist. "Resume" sent SIGUSR2 to a pid that was already gone and wrote `running` on top of it.

Two ways out:

1. **A real soft pause** in the engine: a new state threaded through the extractors, the pipeline, the position writer, the heartbeats and the monitor, holding connections and replication slots open for as long as the operator left it paused.
2. **Pause as a stop with intent**, reusing the graceful shutdown path already built for signals.

## Decision

Pause is a graceful stop; resume is a new Run.

- **`pausing` is a real state**, symmetric with `stopping`. The console writes it *before* signalling, because it is the record of intent the supervisor reads afterwards.
- **The supervisor dispatches on the exit code and the Run's current status.** A cooperative exit (`143`, and `130` for a SIGINT from elsewhere) means `paused` under `pausing`, `stopped` under `stopping`, and — under `running`, where nobody in the console asked — `stopped` with `stop_method="external"` plus an `external_stop` control-log entry. Exit `4` (the shutdown window expired) is `failed` even under `pausing`: an unfinished position log makes "resumable" a lie.
- **A non-console SIGTERM is not a failure.** A rolling update sends SIGTERM to every pod; marking those Runs `failed` would make alerting useless. The control log keeps it auditable instead.
- **Resume renders the INI with three forced overrides**, each of which is a silent data error if omitted rather than a loud failure: `[resumer]` pinned to the paused Run's `log_dir`; `recreate_slot_if_exists=false` for PG/GaussDB (the console's golden default is `true`, and recreating the slot throws away the position being resumed from); and every explicit start marker cleared (`start_lsn`, `start_time_utc`, `start_scn`, binlog file/position), which would otherwise outrank the resumer. The overrides are written to the audit log.
- **The paused predecessor is closed out** as `stopped` with `stop_method="resumed"` the moment the successor's engine is up, and the successor records `resumed_from_run_id`. Two Runs of one task in an active status would make `find_active_by_task` pick one arbitrarily.
- **Pause is offered only for `snapshot` and `cdc` tasks**, and not for managed `snapshot_and_cdc` ones: `check` and `struct` have no position to resume from, and the two-phase handover owns its own start marker.
- **Stopping a `paused` Run discards it** (`stop_method="discarded"`) without signalling anything — its pid may since have been recycled.
- **The SIGUSR channel is deleted.** `EngineSignal` carries only `Term` and `Kill`, the two signals the engine actually handles.

## Consequences

- Pausing releases everything: the metrics port, the concurrency slot, the source connection and the replication slot's *reader* (the slot itself stays, which is why it must not be recreated on resume).
- A paused task is not free to leave paused forever. The source must still hold the position: binlogs expire and replication slots get dropped. Validating that a resume position is still reachable belongs in precheck, per db_type — it is not implemented here.
- A paused Run has no process, so orchestrator restart reconciliation deliberately skips `paused`; reconciling it would find a dead pid and mark it `failed`, destroying the very position the operator paused to keep.
- One task becomes a *chain* of Runs. `resumed_from_run_id` makes the chain reconstructable; presenting it as a single timeline is a UI concern and out of scope here.
- Resume goes through the whole start path — licence check, precheck, port allocation, supervision — so a resumed Run cannot drift away from a started one.
- Soft pause is not ruled out forever; it is ruled out as a *fix*. If holding the source position without a process is ever needed, it is a feature with its own design.
