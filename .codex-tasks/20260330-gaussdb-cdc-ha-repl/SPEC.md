# GaussDB -> PG CDC: HA replication port + NoTLS 优先策略修复

## 背景

在 GaussDB (PG compatible) CDC 启动阶段，日志出现以下现象（示例）：

- `candidate replication connect/start failed: <host>:8001, error: ... unexpected character "-"`（HA 端口）
- `candidate replication connect/start failed: <host>:8000, error: replication should connect HA port ...`（非 HA 端口回退）
- 当主库 replication 启动失败后，会继续尝试候选节点，从而产生 standby 节点的 `pg_is_in_recovery=true` 预检失败噪音。

参考实现（flink-cdc / gaussdb connector）行为：

- replication 连接固定使用 `port + 1`（HA 端口）
- replication 连接 `sslmode=disable`（禁用 SSL）

## 目标

将 ape-dts 的 GaussDB CDC replication 连接策略对齐为：

1. replication 仅尝试 HA 端口（`base_port + 1`），不再回退 `base_port`
2. replication 默认优先 NoTLS（`sslmode=disable`），仅在错误明确提示需要 SSL 时回退到 TLS
3. 日志更清晰：区分 “HA replication connect/start failed” 与 standby 预检失败

## 成功标准

- 启动 CDC 时，不再出现对 `base_port` 的 replication 尝试与 “should connect HA port” 噪音
- 在主库可用情况下，不应再遍历到 standby 节点并反复打印 `pg_is_in_recovery=true`
- `cargo test -p dt-tests --test integration_test -- gaussdb_to_pg::cdc_tests::test::cdc_basic_test --nocapture` 通过

