# Codex task artifacts

This directory contains Codex/Taskmaster execution artifacts used during long-running work
(plans, progress logs, and selected raw evidence logs).

Notes:

- These files are meant for **context recovery** and to help other agents quickly understand what
  happened, why certain decisions were made, and which evidence logs correspond to which steps.
- **Never commit credentials**. Any local `.env.local` / `pgpass`-style files should remain untracked.
- Large transient files may be removed to keep the repository size reasonable; the authoritative
  summary is always in each task’s `PROGRESS.md`.

## Tasks

- `20260317-gaussdb-l4-validation/`: L4 validation evidence package (PG <-> GaussDBPg snapshot/struct/check + GaussDBPg -> PG CDC)
- `20260316-gaussdb-mvp/`: Earlier MVP planning epic artifacts

