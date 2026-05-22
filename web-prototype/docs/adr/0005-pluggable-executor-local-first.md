# 0005 — Pluggable Executor abstraction, Local fork-exec ships first

Different on-prem customers run different infrastructure: bare metal, single-host Docker, K8s clusters. The orchestrator therefore defines an `Executor` trait (`spawn(task) -> RunHandle`, `kill`, `status`, `tail_logs`) and ships a `LocalExecutor` first that fork-execs the `ape-dts` binary on the orchestrator host. `DockerExecutor` and `KubernetesExecutor` are explicitly second-phase implementations of the same trait.

## Consequences

- The MVP requires the `ape-dts` binary be reachable on the orchestrator host's `PATH` (or a configured absolute path).
- Resource isolation in MVP is process-level only; CPU/memory limits via cgroups or container quotas are deferred.
- INI files are written under a per-Run working directory the orchestrator manages; `logs/` per Run is captured by tailing files (Local) — Docker/K8s implementations will switch to stdout streaming.
- Anything the executor abstraction cannot uniformly express must not become a Task-level field.
