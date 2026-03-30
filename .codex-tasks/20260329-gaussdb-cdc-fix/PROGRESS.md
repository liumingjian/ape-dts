# Progress Log

> Taskmaster single-full. Decision log + context recovery anchor.

---

## Context Recovery Block

- **Task**: 修复并跑通 GaussDB -> PG CDC（dt-tests `gaussdb_to_pg::cdc_basic_test`）
- **Shape**: `single-full`
- **Progress**: 4/5
- **Current**: #5 验证 e2e 通过并更新记录
- **Artifact**: `.codex-tasks/20260329-gaussdb-cdc-fix/TODO.csv`
- **Next action**: 先确认 GaussDB SQL 端口可用（psql/pg_isready），再跑 `cargo test -p dt-tests --test integration_test -- gaussdb_to_pg::cdc_tests::test::cdc_basic_test --nocapture` 并归档日志。

---

## Notes

- 相关历史：`.codex-tasks/20260329-gaussdb-prd-e2e` 中 CDC 失败记录与日志。

## 2026-03-30

- 已实现并提交最小修复（见 commit `1b4fd243`）：
  - Extractor 端 endpoint 顺序：优先 base endpoint（sticky），candidate hosts 作为 failover。
  - Test runner 执行 SQL：不再为 GaussDBPg 临时 resolve 新 rw url，避免一次 e2e 内 DML 与 replication 漂移到不同节点。
- 现网连通性异常（需要先恢复环境再做 e2e 验证）：
  - 本地 PG：`select 1` 正常。
  - GaussDB：`10.250.0.30/.51/.52` 的 `8000` 与 `5432` 均出现 `server closed the connection unexpectedly`；
    `pg_isready` 显示 `no response`（TCP 可连通但非 PG 可用状态）。

## Flink Connector Notes

External reference: `flink-connector-gaussdb-cdc` (liumingjian/flink-cdc).

- Replication connection behavior:
  - Replication streaming uses HA port `port+1` and `replication=database`, with long connect timeout (60s).
  - Periodic keepalive/status update is treated as required (`forceUpdateStatus()` semantics), not optional.
- Slot lifecycle:
  - Avoid frequent close/reopen to reduce GaussDB slot contention.
  - Tests prefer unique slot names per run and drop slots afterward.
- Resume semantics:
  - Reconnect resumes from last processed LSN; progress is flushed/committed to the slot regularly.
- Validation strategy:
  - Integration tests wait for slot progress (`confirmed_flush_lsn`) rather than only `active=true`.

Applied to our repo:
- Prefer pinning one primary endpoint within a single test attempt; treat candidate hosts as failover options only.
- Avoid re-resolving/rewriting GaussDB URL mid-attempt for DML execution, otherwise replication source and DML primary can drift.
