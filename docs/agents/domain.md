# Domain Docs

This repository uses a multi-context domain-documentation layout.

## Context map

Read `CONTEXT-MAP.md` first. It identifies the domain context relevant to the
work and points to its glossary and context-scoped ADRs.

## Current contexts

- Migration engine: Rust workspace crates `dt-common`, `dt-connector`,
  `dt-parallelizer`, `dt-pipeline`, `dt-task`, `dt-main`, `dt-precheck`,
  and `dt-tests`.
- Console management plane: `dt-console-server` and `web-prototype`.
- Operations and delivery: repository-level testing, deployment, CI, and
  operational documentation.

The Console glossary currently lives at
`web-prototype/docs/CONTEXT.md`, with context-specific ADRs under
`web-prototype/docs/adr/`.

Engine and operations glossaries should be created lazily by
`/domain-modeling` when their first terms are resolved. Do not invent empty
glossaries merely to satisfy the layout.

System-wide decisions belong under `docs/adr/`. Console-only decisions remain
under `web-prototype/docs/adr/`.

## Consumer rules

Before exploring or changing an area:

1. Read `CONTEXT-MAP.md`.
2. Read the relevant context glossary if it exists.
3. Read relevant system-wide and context-specific ADRs.
4. Use canonical glossary terms in issue titles, specifications, test names,
   UI labels, API fields, and implementation discussions.
5. Surface conflicts with an existing ADR instead of silently overriding it.

If a referenced glossary or ADR directory does not exist, proceed silently.
The domain-modeling workflow creates those files only when real terminology
or decisions have been resolved.
