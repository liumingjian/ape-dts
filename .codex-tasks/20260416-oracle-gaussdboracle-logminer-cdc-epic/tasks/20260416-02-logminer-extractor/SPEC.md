# Spec

## Goal

实现 `Oracle -> GaussDBOracle` 的 LogMiner CDC extractor（显式模式），并保持现有 trigger-based Oracle CDC 不受影响。

## Acceptance

- `cargo test -p dt-connector -p dt-task --no-run` PASS
- `task_config.ini` 可通过 `[extractor].cdc_mode=logminer` 显式启用

