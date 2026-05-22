# Task Specification

## Task Shape

- **Shape**: `single-full`

## Goals

- 在 “环境已修复” 前提下，先做 GaussDB/PG/slot 的**状态检查**并归档证据。
- 通过**最小代码改动**跑通 `dt-tests` 的 GaussDB -> PG CDC e2e：`gaussdb_to_pg::cdc_basic_test`。
- 对标外部实现（Flink GaussDB CDC connector）梳理关键差异，作为突破口依据。

## Non-Goals

- 不扩大 PRD 范围（不做对象同步扩展、不做 SHA256 认证支持），除非 Boss 明确要求。
- 不将任何口令写入可被 git 跟踪的文件（仅允许使用已被 gitignore 的 `.env.local`）。

## Constraints

- Debug-First：不引入静默降级或 “假成功” 路径；失败必须暴露并可复现。
- 只做必要改动，避免引入与 CDC 无关的重构。

## Acceptance

- `cargo test -p dt-tests --test integration_test -- gaussdb_to_pg::cdc_tests::test::cdc_basic_test --nocapture` 通过。
- `.codex-tasks/20260329-gaussdb-cdc-fix/PROGRESS.md` 记录：
  - 状态检查结果（slot / wal_level / candidate host 关键项）。
  - 失败根因与修复点（含关键日志证据路径）。
  - 外部 Flink connector 的关键实现点对比结论（不需搬运代码，只记录要点）。

## Validation Commands

```bash
# status check
psql -h 127.0.0.1 -p 5432 -d postgres -U lmj --no-password -c "select 1"
PGPASSWORD=*** psql -h 10.250.0.30 -p 8000 -U root -d postgres --no-password -c "select 1"

# e2e
. "$HOME/.cargo/env"
cargo test -p dt-tests --test integration_test -- gaussdb_to_pg::cdc_tests::test::cdc_basic_test --nocapture
```

