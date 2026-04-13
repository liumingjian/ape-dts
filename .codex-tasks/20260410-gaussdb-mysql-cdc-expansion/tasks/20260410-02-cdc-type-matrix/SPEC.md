# Child Spec

## Title

MySQL -> GaussDBMySQL cdc type-matrix

## Parent Epic

- `.codex-tasks/20260410-gaussdb-mysql-cdc-expansion/EPIC.md`

## Goal

在 `cdc basic` 已跑通的前提下，把 `MySQL -> GaussDBMySQL` 在 CDC 路径下的
类型兼容性做成可回归的最小矩阵：

1. 新增 `mysql_to_gaussdb_mysql::cdc_tests::test::cdc_type_matrix_test` 自动化入口
2. 覆盖常见业务类型（数值/字符串/时间/JSON），并包含 insert+update+delete
3. 在真实环境中跑通并归档证据（脱敏，且前后无污染）

## Constraints

- 只做 `MySQL -> GaussDBMySQL`（目标端 `sql_compatibility=M` 且通过 pg-wire 连接）
- 继续保持 CDC 语义：仅 DML（不加入 DDL CDC）
- 如遇到类型不支持或不兼容：保持 fail-fast，并在 PROGRESS 里记录事实与下一步

## Acceptance

- `cargo test -p dt-tests --test integration_test -- mysql_to_gaussdb_mysql::cdc_tests::test::cdc_type_matrix_test --nocapture`
- 真实环境证据归档到 child `raw/`

