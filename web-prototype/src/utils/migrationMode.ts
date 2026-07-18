import type { ExtractType, SyncMode, TaskCategory, TaskViewKind } from '@/types/domain';

export type MigrationMode = SyncMode;

export function isMigrationMode(value: unknown): value is MigrationMode {
  return value === 'snapshot' || value === 'snapshot_cdc' || value === 'cdc';
}

export function categoryForView(viewKind: TaskViewKind): TaskCategory | 'migration' {
  return viewKind === 'migration' ? 'migration' : viewKind;
}

export function createPathForView(viewKind: TaskViewKind): string {
  return viewKind === 'migration' ? '/tasks/create/migration' : `/tasks/create/${viewKind}`;
}

export function detailPathForView(viewKind: TaskViewKind, taskId: string): string {
  return viewKind === 'migration' ? `/tasks/migration/${taskId}` : `/tasks/${viewKind}/${taskId}`;
}

export function listPathForTaskKind(kind: TaskCategory, mode?: MigrationMode): { path: string; query?: Record<string, string> } {
  if (kind === 'check' || kind === 'struct') return { path: `/tasks/${kind}` };
  return { path: '/tasks/migration', query: mode ? { mode } : undefined };
}

export function detailPathForTask(kind: TaskCategory, taskId: string, mode?: MigrationMode): { path: string; query?: Record<string, string> } {
  if (kind === 'check' || kind === 'struct') return { path: `/tasks/${kind}/${taskId}` };
  return { path: `/tasks/migration/${taskId}`, query: mode ? { mode } : undefined };
}

export function wizardTaskKind(viewKind: TaskViewKind, mode: MigrationMode): TaskCategory {
  if (viewKind !== 'migration') return viewKind;
  return mode === 'cdc' ? 'cdc' : 'snapshot';
}

export function extractTypeForMigrationMode(mode: MigrationMode): ExtractType {
  if (mode === 'cdc') return 'cdc';
  if (mode === 'snapshot_cdc') return 'snapshot_and_cdc';
  return 'snapshot';
}
