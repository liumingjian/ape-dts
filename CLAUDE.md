# Project Instructions

## Core migration red-line

For major changes that can affect extraction, routing, pipeline processing, sinking, checkpoints, or `dt-main` lifecycle, run before declaring the work complete:

```bash
bash scripts/e2e/mysql_to_postgresql_redline.sh
```

The change passes only when Snapshot, CDC readiness, INSERT, UPDATE, DELETE, and final MySQL/PostgreSQL equality all pass with the real `dt-main` binary.

Do not bypass or weaken this red-line. On failure, report the failing stage and artifact path; fix the cause or explicitly leave the work unverified. Detailed scope and diagnostics: [docs/testing/mysql-to-postgresql-redline.md](docs/testing/mysql-to-postgresql-redline.md).

## Agent skills

### Issue tracker

Issues live in GitHub Issues at `liumingjian/ape-dts`, via the `gh` CLI. See `docs/agents/issue-tracker.md`.

### Domain docs

Multi-context layout — read `CONTEXT-MAP.md` first, then the relevant context glossary and ADRs. See `docs/agents/domain.md`.
