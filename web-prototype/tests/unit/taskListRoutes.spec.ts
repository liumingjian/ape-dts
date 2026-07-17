import { describe, it, expect, beforeEach } from 'vitest';
import { routes } from '@/router/index';
import type { RouteRecordRaw } from 'vue-router';

function getLeafRoutes(routes: RouteRecordRaw[]): RouteRecordRaw[] {
  const leaves: RouteRecordRaw[] = [];
  for (const r of routes) {
    if (r.children?.length) {
      leaves.push(...getLeafRoutes(r.children));
    } else if (r.path) {
      leaves.push(r);
    }
  }
  return leaves;
}

function isRenderedLeaf(r: RouteRecordRaw): boolean {
  // A rendered leaf has a component, not just a redirect
  return !('redirect' in r) && !!r.component;
}

describe('Task list routes — canonical task modules', () => {
  let leaves: RouteRecordRaw[];

  beforeEach(() => {
    leaves = getLeafRoutes(routes);
  });

  it('/tasks/migration is the canonical migration route', () => {
    const r = leaves.find((l) => l.path === 'tasks/migration');
    expect(r).toBeDefined();
    expect(r!.name).toBe('MigrationTasks');
    expect(r!.meta?.roles).toContain('viewer');
    expect(isRenderedLeaf(r!)).toBe(true);
  });

  it('/tasks/snapshot|cdc|sync is a legacy redirect, not a rendered leaf', () => {
    const r = leaves.find((l) => l.path === 'tasks/:legacy(snapshot|cdc|sync)');
    expect(r).toBeDefined();
    expect(isRenderedLeaf(r!)).toBe(false);
  });

  it('/tasks/check is a canonical route', () => {
    const r = leaves.find((l) => l.path === 'tasks/check');
    expect(r).toBeDefined();
    expect(r!.name).toBe('CheckTasks');
    expect(r!.meta?.roles).toContain('viewer');
    expect(isRenderedLeaf(r!)).toBe(true);
  });

  it('/tasks/struct is a canonical route', () => {
    const r = leaves.find((l) => l.path === 'tasks/struct');
    expect(r).toBeDefined();
    expect(r!.name).toBe('StructTasks');
    expect(r!.meta?.roles).toContain('viewer');
    expect(isRenderedLeaf(r!)).toBe(true);
  });

  it('/tasks/replay is a redirect, not a rendered leaf', () => {
    const r = leaves.find((l) => l.path === 'tasks/replay');
    expect(r).toBeDefined();
    expect(isRenderedLeaf(r!)).toBe(false);
  });

  it('/tasks/verify is a redirect, not a rendered leaf', () => {
    const r = leaves.find((l) => l.path === 'tasks/verify');
    expect(r).toBeDefined();
    expect(isRenderedLeaf(r!)).toBe(false);
  });
});

describe('Task detail routes', () => {
  let leaves: RouteRecordRaw[];

  beforeEach(() => {
    leaves = getLeafRoutes(routes);
  });

  it('/tasks/:category/:id route exists for migration/check/struct', () => {
    const r = leaves.find((l) => l.path === 'tasks/:category(migration|check|struct)/:id');
    expect(r).toBeDefined();
    expect(r!.name).toBe('TaskDetail');
    expect(r!.meta?.roles).toContain('viewer');
    expect(isRenderedLeaf(r!)).toBe(true);
  });

  it('legacy /tasks/:legacy/:id is a redirect, not a rendered leaf', () => {
    const r = leaves.find((l) => l.path === 'tasks/:legacy(snapshot|cdc|sync|replay|verify)/:id');
    expect(r).toBeDefined();
    expect(isRenderedLeaf(r!)).toBe(false);
  });
});
