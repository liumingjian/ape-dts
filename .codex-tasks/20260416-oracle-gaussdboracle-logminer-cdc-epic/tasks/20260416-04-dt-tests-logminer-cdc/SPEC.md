# Spec

## Goal

将 `dt-tests` 的 `Oracle -> GaussDBOracle` `cdc_basic_test` 切换到 `[extractor].cdc_mode=logminer` 并通过集成测试。

## Acceptance

- `cargo test -p dt-tests --test integration_test -- oracle_to_gaussdb_oracle::cdc_tests::test::cdc_basic_test --nocapture` PASS

