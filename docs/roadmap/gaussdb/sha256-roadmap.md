# GaussDB SHA256 认证后续路线（非 MVP）

> 目标：在不阻塞 `DbType::GaussDBPg` MVP 的前提下，为后续 **GaussDB 默认 SHA256 认证**提供可执行的升级路径。

## 1. 背景与边界

- 当前 `ape-dts` 通过 `tokio-postgres` / `postgres-protocol` 访问 PG wire protocol。
- GaussDB 默认可能启用“非标准 SHA256”（华为自研，非 SCRAM-SHA-256），导致标准 PG 驱动无法连接。
- MVP 主路径要求 GaussDB 侧启用 MD5（例如 `password_encryption_type=1`），并不交付 SHA256 支持。

本路线的原则：

1. **SHA256 认证不在 ape-dts 主仓分散实现**（避免横切 extractor/sinker/precheck/tests）。
2. 优先在 `apecloud/rust-postgres` 依赖仓完成认证支持，并用独立样例验证握手。
3. 主仓仅在必要处做最小封装/开关（例如保留连接参数透传、错误信息可观测），不以“支持 SHA256”为验收前置条件。

## 2. 实施步骤（建议）

### 2.1 在 `apecloud/rust-postgres` 独立分支实现

- 仓库：`https://github.com/apecloud/rust-postgres`
- 建议分支名：`gaussdb-sha256-auth`
- 目标：完成与 GaussDB 的 SHA256 握手（确保 `tokio-postgres` 与 `sqlx` 的连接链路可用）。

建议先做“观测 + 反推协议”：

1. 在连接失败时抓取 server 的 `Authentication*` 消息（可通过临时日志或抓包）。
2. 明确 GaussDB 侧的 auth method id、challenge/response 结构与加密细节。
3. 只在协议层实现一次（优先放在 `postgres-protocol` / `tokio-postgres` 的认证分支），避免上层重复适配。

### 2.2 增加独立样例程序验证

在 `apecloud/rust-postgres` 仓库内（或单独创建最小 crate）新增样例：

- 输入：`DATABASE_URL`（指向启用 SHA256 的 GaussDB 实例）
- 行为：建立连接并执行 `SELECT 1` / `SELECT version()`，输出结果

目的：

- 在脱离 ape-dts 的情况下验证协议实现正确性
- 将失败范围收敛到认证握手/协议栈本身

### 2.3 合入策略与主仓依赖回切

建议的推进节奏：

1. `apecloud/rust-postgres` 分支完成并验证
2. 通过 PR 合入到 `apecloud/rust-postgres` 主分支（或发布 tag）
3. `ape-dts` 主仓将 workspace 依赖（`tokio-postgres`/`postgres-protocol`/`postgres-types`）切到：
   - 固定 commit rev（更可控），或
   - 固定 tag（便于版本追踪）

当前主仓依赖入口：

- `Cargo.toml`（workspace dependencies）：
  - `tokio-postgres = { git = "https://github.com/apecloud/rust-postgres" }`
  - `postgres-protocol = { git = "https://github.com/apecloud/rust-postgres" }`
  - `postgres-types = { git = "https://github.com/apecloud/rust-postgres" }`

## 3. 风险点与验收建议

- 风险：GaussDB 的“SHA256”并非社区 SCRAM 流程，可能需要完整协议反推。
- 风险：不同 GaussDB 版本/部署形态的 auth 行为不一致。

建议验收最小闭环：

1. 样例程序在 SHA256 环境可稳定 `SELECT 1`
2. `ape-dts` 侧可使用相同 URL 成功建立连接并跑通 `dt-precheck` 的基础查询
3. 不影响原有 PG/MD5 场景（回归 `cargo test`）

