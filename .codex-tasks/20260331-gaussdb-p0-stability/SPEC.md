# GaussDBPg P0 稳定性演进：候选选主优先 + CDC 失败更可诊断

## 背景

当前 GaussDBPg CDC（`mppdb_decoding` + JSON）在真实主备/集群环境下，容易因为连接到 standby 或 replication 端口策略/TLS 策略不一致而产生反复重试噪音；同时当 CDC 流出现非 DML 事件或 JSON 兼容问题时，错误信息不够聚焦，定位成本高。

本任务以 [`docs/agent-summary/plan.md`](/Users/lmj/projects/ai-project/ape-dts/docs/agent-summary/plan.md) 为唯一真相源，收敛在 `DbType::GaussDBPg` 的 P0 稳定性，不扩大到 `gaussdb_mysql/oracle` 或 SHA256。

## 真实环境约束（脱敏/示例）

- `gaussdb_pg_candidate_hosts` 提供 SQL 端口列表：例如 `10.250.0.52:8000,10.250.0.30:8000,10.250.0.51:8000`
- replication 使用 HA 端口 `sql_port + 1`：例如 `8001`
- replication 默认 NoTLS（`sslmode=disable`），仅当服务端明确要求 SSL 才回退 TLS
- 稳定性策略：设置候选列表后“候选优先选主”；CDC 解析失败 “fail fast”

## 目标与成功标准

1. 当设置 `gaussdb_pg_candidate_hosts` 时：按候选顺序探测并选择 `pg_is_in_recovery=false` 的 RW 主库，base URL 仅作为兜底。
2. 增加 “上次成功端点 sticky”：重连优先尝试上次成功的 `(host, sql_port)`，减少反复探测与 standby 噪音。
3. CDC 解码失败路径更可诊断：
   - unsupported `op_type` 明确提示可能是 DDL/对象事件（MVP 不支持），并给出建议动作
   - JSON/字段缺失/unsupported 等错误都打印 `LSN + category + raw_snippet(<=200)`，不打印整行
   - 保持 fail fast，不静默跳过
4. 回归：`dt-connector` 相关单测通过；`dt-tests` gaussdb->pg CDC 用例至少保证编译通过（真实环境回归另行在 PROGRESS 记录）。

