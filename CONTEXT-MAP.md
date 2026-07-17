# Context Map

## Migration engine

Scope: extraction, filtering, routing, parallelization, pipeline processing,
sinking, checkpoints, resumability, and engine metrics.

Code:
- `dt-common/`
- `dt-precheck/`
- `dt-connector/`
- `dt-parallelizer/`
- `dt-pipeline/`
- `dt-task/`
- `dt-main/`
- `dt-tests/`

Glossary: created lazily when engine-specific terms are resolved.
ADRs: `docs/adr/` for system-wide decisions.

## Console management plane

Scope: Task and Run management, orchestration, phase state, logs, metrics,
alerts, authentication, licensing, and the web user interface.

Code:
- `dt-console-server/`
- `web-prototype/`

Glossary: `web-prototype/docs/CONTEXT.md`
ADRs: `web-prototype/docs/adr/`

## Operations and delivery

Scope: end-to-end red lines, deployment, CI, release procedures, and
operational runbooks.

Docs and automation:
- `docs/`
- `scripts/`
- `.github/`

Glossary: created lazily when operations-specific terms are resolved.
ADRs: `docs/adr/`
