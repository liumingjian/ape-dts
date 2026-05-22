# GaussDBPg P0 稳定性：全面 E2E 验证（基于 .local/e2e/.env）

## 目标

在真实环境（配置位于 `.local/e2e/.env`）对以下能力做一次端到端验证，并保留可复现证据：

- 设置 `gaussdb_pg_candidate_hosts` 后，CDC 连接策略为“候选优先选主，base URL 兜底”
- replication 走 HA 端口 `sql_port + 1` 且默认 NoTLS
- 发生复制连接中断时，重连优先尝试“上次成功端点（sticky）”
- CDC 解码失败仍为 fail-fast，且日志包含 `LSN + category + raw_snippet(<=200)`（此项以单测覆盖为主，E2E 做最小观察）

## 环境输入

- `.local/e2e/.env`（本地文件，可能包含口令，不应写入 git 记录）
- 通过脚本 `scripts/e2e/gaussdb_to_pg_cdc.sh` 执行真实 CDC E2E

## 成功标准

1. E2E 脚本执行成功（源端 DML 最终一致落到目标 PG），并自动清理源/目标测试对象与 replication slot。
2. E2E 日志（默认 `default.log`）出现以下关键证据：
   - `prefer_candidates=true` 且 probe order 以候选列表开头
   - `replication connection starts ... port=<sql_port+1>` 且 `ssl=off`
3. Sticky 重连验证（可选但优先尝试）：
   - 通过终止 `application_name=gaussdb-replication` 的 backend 强制断链
   - 任务自动重连，日志出现 `last_success=<host:port>` 且 probe order 以该端点开头

## 验证命令（摘要）

从仓库根目录执行：

```bash
source .local/e2e/.env
bash scripts/e2e/gaussdb_to_pg_cdc.sh
```

若启用 sticky 重连测试：

```bash
source .local/e2e/.env
export TEST_STICKY_RECONNECT=1
bash scripts/e2e/gaussdb_to_pg_cdc.sh
```

