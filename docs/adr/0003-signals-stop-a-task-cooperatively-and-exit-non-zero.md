# Signals stop a task cooperatively and exit non-zero

Status: accepted

`dt-main` handles SIGINT **and** SIGTERM by cancelling the task-wide `CancellationToken` (see [ADR 0002](0002-cancellation-token-as-the-task-shutdown-primitive.md)) and waiting a bounded window (`SHUTDOWN_TIMEOUT_SECS`, default 8s) for the task to drain, record its final position and close its connections. A run stopped by a signal exits with `128 + signal` (130 / 143); a run that would not converge inside the window exits with `4`.

## Context

The old handler listened only for ctrl-c, slept `SHUTDOWN_TIMEOUT_SECS` and called `exit(0)`. Three things followed from that:

- Nothing was actually stopped: the sleep was not a drain, so whatever the pipeline had buffered when the timer fired was lost, and the last checkpoint was whichever periodic one happened to have landed. The task resumed from there and replayed.
- SIGTERM was not handled at all. k8s and systemd send SIGTERM, and so does the console executor's stop path, so every rolling update or "stop task" click was a hard kill — a replayed segment on every restart, and a 10s grace window (`CONSOLE_STOP_GRACE_SECS`) that had nothing to wait for.
- The exit code was always 0. An orchestrator could not tell a task that finished from a task that was killed half way through.

## Decision

- SIGINT and SIGTERM are both handled, and identically: the first one cancels the root token; a second one exits immediately, because an operator who asks twice means it.
- The graceful window bounds the wait, it is not the mechanism: the process leaves as soon as the task converges. `SHUTDOWN_TIMEOUT_SECS` defaults to 8s so the graceful path finishes inside the console's 10s SIGTERM grace window instead of racing its SIGKILL.
- Exit codes carry the outcome: `0` finished, `2` init failed, `3` task failed, `128 + signal` stopped by a signal after a clean drain, `4` the graceful window expired and the task was abandoned (its final position may be missing).
- A precheck has nothing to drain, so a signal ends it at once, still with `128 + signal`.

## Consequences

- "Stopped by SIGTERM" is never mistaken for success. The console's stop path already writes its own `stopped` status before reaping, so the non-zero code does not turn an intentional stop into a `failed` run; a signal from anywhere else does surface as non-zero, which is the point.
- The console's grace window becomes real: the engine now uses it to drain rather than ignoring it until SIGKILL.
- Restarts stop replaying the tail of the last window, because the shutdown path forces a final `record_checkpoint` through the pipeline's cancellation drain.
- Any new engine wait that cannot observe the token reintroduces the timeout branch: exit code `4` is the visible symptom, and it names the signal and the window in stderr.
