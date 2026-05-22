# 0001 — Independent orchestrator service in front of ape-dts

ape-dts is a stateless single-binary CLI worker with no control-plane API; only `/metrics` (Prometheus) and the data-plane `pipeline_type=http_server` subscription endpoint exist. Rather than bolt task CRUD, persistence, RBAC, and process supervision onto the engine, we are introducing a separate Rust crate `dt-console-server` in the same Cargo workspace that owns the management plane. The web console talks only to the orchestrator; the orchestrator talks to ape-dts processes via a pluggable **Executor** trait. This keeps the engine cleanly a worker, lets us reuse `dt-common::config::*` types for INI rendering and validation (eliminating field drift between front-end and engine), and avoids language fragmentation in the workspace.

## Considered options

- **A. Direct frontend → ape-dts** — rejected: would require inventing dozens of HTTP endpoints inside the data-plane binary and a parallel state store; pollutes the engine.
- **B. Independent orchestrator (chosen)** — Rust crate, axum or actix-web, talks to engine via an Executor abstraction.
- **C. K8s Operator/CRD only** — rejected: forces every customer to run K8s; the on-prem target cannot assume that.
- **D. Frontend + mocks only, defer backend shape** — rejected: would lock UX assumptions before validating they are implementable.

## Consequences

- A new crate (`dt-console-server`) and a new metadata store (SQLite via sqlx, see ADR-0004) become deployment artifacts.
- INI generation logic must live in the orchestrator using `dt-common::config::*` so the engine and the orchestrator share one schema; do not re-implement parsing in TypeScript.
- The engine remains free to evolve its INI without breaking the UI as long as `dt-common::config` evolves with it.
