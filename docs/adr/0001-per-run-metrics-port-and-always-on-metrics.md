# Per-run metrics port and always-on engine metrics

Status: accepted

To make the console's throughput/progress/lag UI trustworthy under concurrent tasks, we decided that (1) the engine always emits Prometheus metrics — `metrics` stops being an opt-in cargo feature for the binary the orchestrator runs — and (2) each **Run**'s metrics endpoint binds a unique port allocated by the **Orchestrator** and written into that Run's `[metrics]` INI section, instead of the hardcoded `9090`.

## Context

The scraper (`dt-console-server/src/metrics_scraper.rs::scrape_target_from_run`) hardcoded `127.0.0.1:9090` and ignored the task's `metrics_config`, so two concurrent Runs would fight over one port and the UI would show empty/garbled data. The engine also only exposed `/metrics` when built `--features metrics`, and the INI `[metrics]` section was rendered only when the user filled `metrics_config` — so by default there were no metrics at all, and the UI silently fell back to estimated/fake numbers.

## Decision

- Build/run the orchestrator-managed `dt-main` with metrics enabled by default; the `[metrics]` section is always rendered.
- The orchestrator allocates a free port per Run, persists it on the `runs` row, injects it into the rendered INI, and the scraper reads host/port from the Run record (no hardcoded constant).

## Consequences

- Always-on metrics adds a small, bounded overhead to every Run (acceptable; the monitor subsystem already runs to write `monitor.log`/`task.log`).
- The orchestrator now owns port lifecycle (allocate on start, release on stop/reconcile); this is new state but removes the multi-task collision class entirely.
- Reverting means re-introducing the collision, so this is deliberately hard to undo.
