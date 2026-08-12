# CancellationToken as the task shutdown primitive

Status: accepted

Task shutdown is carried by a `tokio_util::sync::CancellationToken` threaded from `TaskContext` down through the queue, extractor, pipeline and monitor loops, replacing the `Arc<AtomicBool> shut_down` flag. Every blocking wait in the engine must observe it.

## Context

`shut_down` was a flag that only *polling* code could see. Nothing awaiting a `Notify`, a socket, or a timer could be woken by it, so a task that failed on one side left the other side parked forever: a pipeline that died on a sinker error left the extractor blocked in `DtQueue::push` on a full queue, `tokio::join!` in `start_single_task` never returned, and the process neither exited nor reported an error. Code that *could* observe the flag only did so by spinning — `BaseExtractor::wait_task_finish` and `BasePipeline::start` burned a full core through the CDC idle window (measured at ~100% of one core on an idle CDC task before this change, ~0.5% after).

## Decision

- One token per task, created in `TaskContext`; each sub task takes a `child_token()`, so a sub task failure stays local while a task-wide cancel reaches every side.
- Cancellation is the shutdown signal for *both* causes — normal completion (`wait_task_finish`) and failure (either side of `start_single_task`, `AbortGuard` drop).
- Every wait that can block indefinitely takes a cancellation arm: `DtQueue::push` / `wait_until_drained` / `wait_for_data`, the pipeline idle wait, the actix server in `HttpServerPipeline`, the monitor flush loop, the heartbeat sleeps.
- A wait released by cancellation reports `Error::Cancelled`, which `report_task_results` demotes so a downstream cancellation never masks the root-cause error.
- Waits keep a short timeout as a backstop, so a lost wake-up degrades into a bounded delay instead of a stall.

## Consequences

- Shutdown is cooperative and convergent: `start_multi_task` cancels and joins its siblings with a bounded timeout instead of dropping the `JoinSet` and aborting in-flight writes that have not recorded their positions. Aborting is the last resort, and it is logged.
- Idle CPU drops from a pegged core to near zero, and shutdown latency drops from "up to one heartbeat interval" to "immediately".
- New blocking code in the engine is now obliged to take the token; a wait without a cancellation arm reintroduces the deadlock class.
- Graceful shutdown on a signal (rather than the current hard `exit(0)` timer in `dt-main`) becomes a matter of cancelling the root token — that is the follow-up ticket, not part of this decision.
