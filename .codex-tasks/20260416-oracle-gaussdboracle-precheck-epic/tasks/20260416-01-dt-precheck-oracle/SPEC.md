# Spec

## Goal

在 `dt-precheck` 中新增 `DbType::Oracle` 的 precheck 支持，使 `PrecheckerBuilder` 在 source/sink 任一端为 Oracle 时可正常构建并执行检查。

## Scope

- 新增 `OracleFetcher`（基于 `dt_connector::oracle::OracleSqlPlusClient`）
- 新增 `OraclePrechecker`
- `PrecheckerBuilder::build_checker` 增加 `DbType::Oracle` 分支

## Acceptance

- `cargo test -p dt-precheck --no-run` 通过

