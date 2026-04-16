# Epic Specification

## Goal

将 `Oracle -> GaussDBOracle` 的 CDC 从 bootstrap trigger-based 升级为 **LogMiner**（最小闭环）：

- 新增 `OracleCdcExtractor` 的 `logminer` 模式（显式配置启用，不做静默降级）。
- `dt-tests`：`oracle_to_gaussdb_oracle::cdc_basic_test` 切到 logminer 模式并 PASS（insert/update/delete）。
- `dt-precheck`：当启用 logminer 模式时，CDC precheck 校验 LogMiner 必需权限与条件。
- 本机 Oracle XE 11g docker 环境补齐 LogMiner 相关授权（保持 idempotent）。

## Non-Goals

- 不实现 OGG；不实现 DDL-CDC；不实现完整 resume/failover（本 Epic 仅打通 logminer CDC basic）。
- 不实现跨 schema 通配符抓取（仍要求 explicit `filter.do_tbs`）。

## Constraints

- Debug-First：不引入静默 fallback；logminer 模式缺权限/缺配置必须显式失败并给出错误信息。
- 保持既有 trigger-based CDC 作为显式模式（不破坏现有用例与行为）。
- 代码规范遵守 `AGENTS.md`（函数/文件大小限制等）。

## Done-When

- [ ] `SUBTASKS.csv` 全部为 `DONE`
- [ ] `cargo test -p dt-tests --test integration_test -- oracle_to_gaussdb_oracle::cdc_tests::test::cdc_basic_test --nocapture` PASS（logminer）

