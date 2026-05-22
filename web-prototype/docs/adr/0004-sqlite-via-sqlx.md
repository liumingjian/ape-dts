# 0004 — Metadata persistence: SQLite via sqlx for MVP

The orchestrator stores Task definitions, Users, Sessions, Resource Groups, Licenses, Alerts, Operate/Control Logs, and Global Params in an embedded SQLite file accessed via sqlx. This keeps "drop a binary, run it" deployments viable for on-prem, gives us free file-copy backups, and—because sqlx is database-agnostic—lets future installations switch to PostgreSQL, MySQL, or GaussDB by changing a DSN without rewriting queries. SQLite's single-writer lock is ample for the console workload (low-thousands of Tasks, low-millions of Alerts).

## Consequences

- Schema migrations live in a `migrations/` directory next to `dt-console-server` and run on startup.
- All queries are written in portable SQL (no SQLite-only types like `JSONB`); `sqlx::query!` macros compile-time check against SQLite during dev.
- ape-dts's existing `[resumer] from_db` metadata table (`apecloud_metadata.apedts_task_position`) remains owned by the engine, separate from the orchestrator's DB. We do not co-tenant.
- Customers needing HA point the orchestrator at an external PG/GaussDB via DSN; that is supported but not the default.
