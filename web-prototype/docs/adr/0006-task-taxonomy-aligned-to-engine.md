# 0006 — Task taxonomy aligned to engine `TaskType`

The console exposes exactly four top-level task kinds, matching `dt_common::config::config_enums::TaskType`: **Snapshot Migration / CDC / Check / Struct Migration** (中文: 全量迁移 / 增量同步 / 数据校验 / 结构迁移). The prototype's `sync | replay | verify` taxonomy is removed because (a) `sync` was an umbrella that conflated three engine modes, (b) `replay` had no engine counterpart and was actually `ExtractType=snapshot_file` — now a *source option* on Snapshot Migration, (c) `struct` was missing entirely. **Snapshot + CDC** (`ExtractType=snapshot_and_cdc`) is a sub-mode of Snapshot Migration, never a fifth top-level kind.

## Consequences

- `web-prototype/src/types/domain.ts` `TaskCategory` becomes `'snapshot' | 'cdc' | 'check' | 'struct'`; `SyncMode` is removed from the top-level Task and folded into Snapshot Migration's `extractType: 'snapshot' | 'snapshot_file' | 'snapshot_and_cdc'`.
- Routes change: `/tasks/sync|replay|verify` → `/tasks/snapshot|cdc|check|struct`. Permanent redirects keep old bookmarks working during transition.
- The create-task wizard branches on the chosen kind; some steps (e.g. Lua processor) are unavailable for Struct.
- All i18n keys under `tasks.category.*` are renamed; `zh-CN.json` uses the README's canonical Chinese terms.
- The prototype's `TaskListView` shell stays — only its filters and category prop change.
