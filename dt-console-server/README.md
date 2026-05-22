# dt-console-server

Orchestration service for ape-dts. actix-web 4.9 HTTP server backed by SQLite (sqlx). Manages Task/Run/User/License/Alert state, renders engine INI configs, fork-execs `dt-main` as Runs, scrapes Prometheus metrics, tails log files, and evaluates alert rules.

## Quick start

```bash
# Build and run (dev)
cargo run -p dt-console-server

# Build for production (with metrics feature)
cargo build --release --features metrics -p dt-console-server

# Run the release binary
CONSOLE_BIND_ADDR=127.0.0.1:8080 CONSOLE_DB_PATH=./data/console.db \
  target/release/dt-console-server
```

The server listens on `127.0.0.1:8080` by default. SQLite is created at `./data/console.db` on first boot (migrations applied automatically). A default admin account is seeded (`admin` / `admin123`).

## Environment variables

| Variable | Default | Description |
|---|---|---|
| `CONSOLE_BIND_ADDR` | `127.0.0.1:8080` | HTTP listen address |
| `CONSOLE_DB_PATH` | `./data/console.db` | SQLite database file path |
| `CONSOLE_IDLE_TIMEOUT_SECS` | `3600` | Session idle expiry (seconds) |
| `APE_DTS_BINARY_PATH` | `target/release/dt-main` | Path to the engine binary for fork-exec |

## Dev commands

```bash
cargo check -p dt-console-server                        # Type-check only
cargo nextest run -p dt-console-server --lib --bins     # Run tests
cargo clippy -p dt-console-server -- -D warnings        # Lint
cargo fmt --all --check                                 # Format check
cargo build --release --features metrics -p dt-console-server -p dt-main  # Production build
```

## API surface

All endpoints are under `/api`. Error responses use the envelope `{ code, message, details }`. Unsafe methods require an `X-XSRF-TOKEN` header mirrored from the `XSRF-TOKEN` cookie.

### Auth

| Method | Path | Description |
|---|---|---|
| POST | `/api/auth/login` | Login (`{username, password}`) → 200 + session cookie |
| POST | `/api/auth/logout` | Logout → invalidate session |
| GET | `/api/auth/me` | Current user identity |

### Users (admin-only)

| Method | Path | Description |
|---|---|---|
| GET | `/api/users` | List users |
| POST | `/api/users` | Create user |
| GET | `/api/users/:id` | Get user |
| PATCH | `/api/users/:id` | Update user (role, password, disabled) |
| DELETE | `/api/users/:id` | Delete user (last admin protected) |

### Tasks

| Method | Path | Description |
|---|---|---|
| GET | `/api/tasks` | List tasks (`?category=&status=&engine=&q=&resource_group=`) |
| POST | `/api/tasks` | Create task |
| GET | `/api/tasks/:id` | Get task |
| PATCH | `/api/tasks/:id` | Update task (kind immutable) |
| DELETE | `/api/tasks/:id` | Delete task (blocked by active Run) |
| GET | `/api/tasks/:id/preview_ini` | Render INI config as text/plain |
| GET | `/api/tasks/:id/export?format=json\|ini` | Export task |
| POST | `/api/tasks/import` | Import task(s) from JSON or INI |
| POST | `/api/tasks/:id/clone` | Clone task |

### Resource Groups

| Method | Path | Description |
|---|---|---|
| GET | `/api/resource_groups` | List resource groups |
| POST | `/api/resource_groups` | Create resource group |
| GET | `/api/resource_groups/:id` | Get resource group |
| PATCH | `/api/resource_groups/:id` | Update resource group |
| DELETE | `/api/resource_groups/:id` | Delete resource group (default protected) |

### Run Lifecycle

| Method | Path | Description |
|---|---|---|
| POST | `/api/tasks/:id/start` | Start task → fork-exec engine |
| POST | `/api/tasks/:id/stop` | Stop running Run (SIGTERM → SIGKILL) |
| POST | `/api/tasks/:id/pause` | Pause CDC Run (SIGUSR1) |
| POST | `/api/tasks/:id/resume` | Resume paused Run (SIGUSR2) |
| GET | `/api/runs/:id` | Get Run status + metadata |

### Test Connection & Precheck

| Method | Path | Description |
|---|---|---|
| POST | `/api/tasks/:id/test_connection` | Validate source/target connectivity |
| POST | `/api/tasks/:id/precheck` | Run engine precheck checks |
| POST | `/api/tasks/preview/test_connection` | Draft-mode test connection (no persistence) |
| POST | `/api/tasks/preview/precheck` | Draft-mode precheck (no persistence) |

### Metrics

| Method | Path | Description |
|---|---|---|
| GET | `/api/runs/:id/metrics?metric=&from=&to=&step=` | Query time-series metric data |

### Logs

| Method | Path | Description |
|---|---|---|
| GET | `/api/runs/:id/logs/stream?file=&level=` | SSE log stream |
| GET | `/api/runs/:id/logs/:file` | Read log file |

### Alerts

| Method | Path | Description |
|---|---|---|
| GET | `/api/alerts?status=&level=&engine=&taskId=` | List alerts |
| POST | `/api/alerts/:id/clear` | Clear alert |
| POST | `/api/alerts/clear_batch` | Batch clear alerts |
| GET | `/api/alerts/stream` | SSE alert event stream |
| POST | `/api/alarm_channels/:id/test` | Test alarm channel dispatch |

### Alert Rules (admin-only)

| Method | Path | Description |
|---|---|---|
| GET | `/api/alert_rules` | List alert rules |
| POST | `/api/alert_rules` | Create alert rule |
| GET | `/api/alert_rules/:id` | Get alert rule |
| PATCH | `/api/alert_rules/:id` | Update alert rule |
| DELETE | `/api/alert_rules/:id` | Delete alert rule |
| POST | `/api/alert_rules/:id/evaluate_now` | Debug: evaluate rule against fixed series |

### Alarm Channels (admin-only)

| Method | Path | Description |
|---|---|---|
| GET | `/api/alarm_channels` | List alarm channels |
| POST | `/api/alarm_channels` | Create alarm channel (Kafka / SNMP) |
| GET | `/api/alarm_channels/:id` | Get alarm channel |
| PATCH | `/api/alarm_channels/:id` | Update alarm channel |
| DELETE | `/api/alarm_channels/:id` | Delete alarm channel |

### Alarm Templates (admin-only)

| Method | Path | Description |
|---|---|---|
| GET | `/api/alarm_templates` | List alarm templates |
| POST | `/api/alarm_templates` | Create alarm template |
| GET | `/api/alarm_templates/:id` | Get alarm template |
| PATCH | `/api/alarm_templates/:id` | Update alarm template |
| DELETE | `/api/alarm_templates/:id` | Delete alarm template |
| POST | `/api/alarm_templates/:id/preview` | Preview mustache interpolation |

### Audit & System

| Method | Path | Description |
|---|---|---|
| GET | `/api/operate_logs?from=&to=&actor=&action=&result=` | Operate log (admin-only) |
| GET | `/api/control_logs?task_id=&action=&from=&to=` | Control log |
| GET | `/api/license` | Current license state |
| POST | `/api/license/activate` | Activate license (admin-only) |
| GET | `/api/system/hosts` | System host list |
| GET | `/api/global_params` | Global runtime parameters |
| PATCH | `/api/global_params` | Update global parameters |
| GET | `/api/healthz` | Liveness probe |
| GET | `/api/readyz` | Readiness probe (DB + scraper) |

## RBAC

| Role | Capabilities |
|---|---|
| **admin** | All operations: user/license management, task CRUD + lifecycle, alert rules/channels/templates, operate logs, global params |
| **operator** | Task CRUD + lifecycle, alert clear, control log, ops management |
| **viewer** | Read-only: task list, task detail, alerts, dashboard |

## Architecture

- **IniRenderer**: pure `Task → String` function built on `dt-common::config::*` types; byte-exact golden tests for every (kind × engine) matrix.
- **LocalExecutor**: fork-execs `dt-main` with per-Run cwd containing the rendered INI; SIGTERM/SIGKILL lifecycle; PID-based supervision.
- **MetricsScraper**: polls each running Run's `:9090/metrics` every 10s; parses Prometheus text via `prometheus-parse`; writes to `metric_points` table.
- **LogTailer**: tails per-Run log files; ships via SSE.
- **AlertEngine**: evaluates alert rules on tick against fresh metric points; dwell time prevents flapping; dispatches via Kafka/SNMP channels.
