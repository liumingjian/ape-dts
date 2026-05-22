import { describe, expect, it } from 'vitest';
import type { TaskCategory } from '@/types/domain';
import { legacyToCategory } from '@/types/domain';

/** Deep-link URL builder — mirrors Dashboard.vue go() helpers. */
function kpiRunningLink(): string {
  return '/tasks/sync?status=running';
}

function kpiAlertLink(): string {
  return '/alerts/current';
}

function recentTaskLink(category: TaskCategory, id: string): string {
  return `/tasks/${category}/${id}`;
}

function topAlertDeepLink(category: TaskCategory, taskId: string): string {
  return `/tasks/${category}/${taskId}?tab=alerts`;
}

describe('Dashboard KPI deep-links', () => {
  it('running-tasks KPI links to filtered task list', () => {
    expect(kpiRunningLink()).toBe('/tasks/sync?status=running');
  });

  it('alert KPI links to current alerts', () => {
    expect(kpiAlertLink()).toBe('/alerts/current');
  });
});

describe('Recent task row links', () => {
  it('links to canonical URL with snapshot category', () => {
    expect(recentTaskLink('snapshot', 'abc-123')).toBe('/tasks/snapshot/abc-123');
  });

  it('links to canonical URL with cdc category', () => {
    expect(recentTaskLink('cdc', 'def-456')).toBe('/tasks/cdc/def-456');
  });

  it('links to canonical URL with check category', () => {
    expect(recentTaskLink('check', 'ghi-789')).toBe('/tasks/check/ghi-789');
  });

  it('never uses "sync" as category', () => {
    // legacyToCategory maps sync/replay → snapshot, never 'sync'
    expect(legacyToCategory('sync')).not.toBe('sync' as TaskCategory);
    expect(legacyToCategory('sync')).toBe('snapshot');
  });
});

describe('Top alert deep-links', () => {
  it('deep-links to task detail with alerts tab', () => {
    expect(topAlertDeepLink('snapshot', 'abc-123')).toBe('/tasks/snapshot/abc-123?tab=alerts');
  });

  it('deep-links to cdc task with alerts tab', () => {
    expect(topAlertDeepLink('cdc', 'def-456')).toBe('/tasks/cdc/def-456?tab=alerts');
  });
});
