# Task Specification

> Scope anchor for the task. Update only when goals or constraints change, and log the reason in PROGRESS.md.

## Task Shape

- **Shape**: `single-full`

## Goals

- 在真实 GaussDB（PG 兼容模式）环境中跑通 `dt-tests` 的 GaussDB 用例，并归档 L4 联调证据。
- 覆盖 4 组证据：
  - `PG -> GaussDB snapshot`
  - `PG -> GaussDB struct/check`
  - `GaussDB -> PG snapshot/check`
  - `GaussDB -> PG cdc`

## Non-Goals

- 不修改/实现 `SHA256` 认证（MVP 主路径为 MD5）。
- 不把专网 GaussDB 联调接入公共 CI。
- 不将任何口令写入可被 git 跟踪的文件（仅允许写入已被 `.gitignore` 忽略的 `dt-tests/tests/.env.local`）。

## Constraints

- 避免在命令行回显密码；优先通过 `dt-tests/tests/.env.local` 注入。
- 仅在需要时对用例做最小调整（例如 slot_name 冲突）。
- GaussDB replication 协议行为以官方驱动 `resources/gsjdbc4.jar` 为准（用于对齐 keepalive/status update 语义）。

## Environment

- **Project root**: `/Users/lmj/projects/ai-project/ape-dts`
- **Language/runtime**: Rust
- **Package manager**: Cargo
- **Test framework**: `cargo test`

## Risk Assessment

- [ ] External dependencies (GaussDB, Docker) — 当前存在真实 GaussDB SASL 协商失败与本地 Docker daemon 不可用的阻塞，详见 `PROGRESS.md`。
- [ ] Long-running tests — CDC 测试可能较慢；必要时增大等待或重试。

## Deliverables

- `dt-tests/tests/.env.local`（gitignored）包含 GaussDB 连接信息。
- `.codex-tasks/20260317-gaussdb-l4-validation/raw/*`：每个测试命令的输出与关键证据归档（脱敏）。
- `.codex-tasks/20260317-gaussdb-l4-validation/PROGRESS.md`：包含最终结论与问题记录。

## Done-When

- `PG <-> GaussDB` 的 struct 验收口径为“逻辑结构等价”，而不是 `pg_catalog` 物理字段逐项一致。
- [ ] `dt-tests` 以下 6 个集成测试均通过：
  - `pg_to_gaussdb::snapshot_tests::test::snapshot_basic_test`
  - `pg_to_gaussdb::struct_tests::test::struct_basic_test`
  - `pg_to_gaussdb::check_tests::test::check_basic_test`
  - `gaussdb_to_pg::snapshot_tests::test::snapshot_basic_test`
  - `gaussdb_to_pg::check_tests::test::check_basic_test`
  - `gaussdb_to_pg::cdc_tests::test::cdc_basic_test`
- [ ] 以上测试输出与关键日志已归档到 `raw/`（不包含密码）。

## Final Validation Command

```bash
cargo test -p dt-tests --test integration_test -- pg_to_gaussdb::snapshot_tests::test::snapshot_basic_test --nocapture
cargo test -p dt-tests --test integration_test -- pg_to_gaussdb::struct_tests::test::struct_basic_test --nocapture
cargo test -p dt-tests --test integration_test -- pg_to_gaussdb::check_tests::test::check_basic_test --nocapture
cargo test -p dt-tests --test integration_test -- gaussdb_to_pg::snapshot_tests::test::snapshot_basic_test --nocapture
cargo test -p dt-tests --test integration_test -- gaussdb_to_pg::check_tests::test::check_basic_test --nocapture
cargo test -p dt-tests --test integration_test -- gaussdb_to_pg::cdc_tests::test::cdc_basic_test --nocapture
```
