# Spec: 2026-04-03 GaussDB Gate Run (Batch B + Resilience)

## Summary

Run the unified GaussDB E2E regression matrix for:

- **Batch B (Enhanced regression)**: type-matrix + view/routine + CDC type-matrix + resume.
- **Resilience Gate**: dt-tests failover + e2e script (basic/resume/negatives/failover).

All evidence must be archived under this task directory, and the docs tracker
must be updated to point to this run.

## Constraints

- **No secrets in git**:
  - never commit `.local/` env files
  - never paste passwords into `PROGRESS.md` or raw logs
- **Failover safety**:
  - enforce healthy CM cluster: `GAUSSDB_CM_REQUIRE_HEALTHY=1`
  - run failover **twice**: dt-tests + e2e script
  - require best-effort restore to original primary

## Evidence Contract

- Raw logs:
  - `raw/batch-b/*.log` + `raw/batch-b/summary.tsv`
  - `raw/resilience/*.log` + `raw/resilience/summary.tsv`
- `PROGRESS.md` must record:
  - exact commands run
  - PASS/FAIL outcome
  - where to find the evidence

