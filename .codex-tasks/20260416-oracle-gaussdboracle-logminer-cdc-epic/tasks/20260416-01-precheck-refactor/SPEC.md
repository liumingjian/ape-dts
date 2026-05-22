# Spec

## Goal

重构 `dt-precheck` 的 Oracle prechecker 文件拆分，满足 `AGENTS.md` 的文件行数限制（<=300 行），并为后续 logminer 模式 precheck 扩展留出清晰边界。

## Acceptance

- `cargo test -p dt-precheck --no-run` PASS
- Oracle prechecker 相关单文件 <= 300 行

