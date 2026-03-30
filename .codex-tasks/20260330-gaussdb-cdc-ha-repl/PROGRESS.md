# Progress Log

## 2026-03-30

### Context

用户反馈 dt-main 启动日志包含：

- HA 端口（port+1）replication 失败，并出现 `unexpected character "-"` 的 FATAL
- 回退到 base port 触发 `replication should connect HA port ...`
- 随后扫到 standby 节点，`pg_is_in_recovery=true` 预检失败并重试

本任务聚焦：仅 HA 端口 + replication NoTLS 优先，并清晰日志。

### Reference

flink-cdc gaussdb connector 的 replication 连接：固定 `port+1`，并设置 `sslmode=disable`。

### Diagnosis (from user logs)

见 `raw/20260330_user_startup_snippet.log`：

- 主库 replication 在 HA 端口（`8001`）失败，并出现 `unexpected character "-"`；
- 随后回退到 base port（`8000`）触发 `replication should connect HA port ...`；
- 由于主库 replication 失败，继续扫候选节点导致 standby 的 `pg_is_in_recovery=true` 预检失败噪音。

### Implementation

- `dt-connector/src/extractor/gaussdb/gaussdb_cdc_client.rs`
  - replication 仅尝试 HA 端口（`base_port+1`），不再回退 base port
  - replication 建连默认优先 NoTLS（`sslmode=disable`），仅当错误明确要求 SSL（例如包含 `SSL off`）时回退 TLS
  - warn 日志标注为 `HA replication connect/start failed` 并带上 `sql_port`

### Verification

- `cargo test -p dt-connector -q` ✅
- `cargo test -p dt-tests --test integration_test -- gaussdb_to_pg::cdc_tests::test::cdc_basic_test --nocapture` ✅ (2026-03-30)
