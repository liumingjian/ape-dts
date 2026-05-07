# 0009 — Built-in Prometheus scraper, time-series stored in SQLite

The orchestrator runs an in-process Prometheus scraper that polls each running Run's `/metrics:9090` endpoint on a 10-second interval and writes time-series rows to a SQLite table (`task_id, metric_name, ts, value`). Recent points (≤24h) are kept at native resolution; older points are downsampled per a retention policy. We deliberately do **not** require an external Prometheus deployment for the console to function; this matches the on-prem "drop a binary" posture from ADR-0002 and ADR-0004. Customers with an existing Prometheus stack can opt-in to remote-write export so the console becomes one of several consumers of the same data.

## Considered options

- **A. Built-in scraper + SQLite TS (chosen)** — zero-extra-dependency on-prem default.
- **B. Mandatory external Prometheus + Grafana** — rejected: forces every customer to install, configure, and maintain a separate stack just to view the console's own charts.
- **C. Engine pushes to Pushgateway** — rejected: requires changes to ape-dts itself; outside the management plane's remit.
- **D. Frontend reads Prometheus via PromQL directly** — rejected: couples UI to PromQL syntax and forces the previous Prometheus dependency.

## Consequences

- Metric names referenced by Alert Rules and dashboard charts are exactly the gauge names emitted by ape-dts (`extractor_rps_avg`, `pipeline_buffer_size_avg`, etc.) — never a renamed alias.
- Retention/downsample knobs live in `[metrics]` global params and are tunable per deployment.
- A Run that disables the `metrics` cargo feature appears with empty charts; the console explicitly surfaces this and points at the Run's `[metrics]` config rather than failing silently.
- If a customer wants long-term retention beyond what SQLite is comfortable with, they enable remote-write to their own TSDB; the orchestrator's tables remain the short-term cache.
