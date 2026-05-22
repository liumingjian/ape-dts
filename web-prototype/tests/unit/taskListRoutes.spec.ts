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

describe('Task list routes — per-category canonical paths', () => {
  let leaves: RouteRecordRaw[];

  beforeEach(() => {
    leaves = getLeafRoutes(routes);
  });

  it('/tasks/snapshot is a canonical route', () => {
    const r = leaves.find((l) => l.path === 'tasks/snapshot');
    expect(r).toBeDefined();
    expect(r!.name).toBe('SnapshotTasks');
    expect(r!.meta?.roles).toContain('viewer');
    expect(isRenderedLeaf(r!)).toBe(true);
  });

  it('/tasks/cdc is a canonical route', () => {
    const r = leaves.find((l) => l.path === 'tasks/cdc');
    expect(r).toBeDefined();
    expect(r!.name).toBe('CdcTasks');
    expect(r!.meta?.roles).toContain('viewer');
    expect(isRenderedLeaf(r!)).toBe(true);
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

  it('/tasks/sync is a redirect, not a rendered leaf', () => {
    const r = leaves.find((l) => l.path === 'tasks/sync');
    expect(r).toBeDefined();
    expect(isRenderedLeaf(r!)).toBe(false);
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

  it('/tasks/:category/:id route exists', () => {
    const r = leaves.find((l) => l.path === 'tasks/:category(snapshot|cdc|check|struct)/:id');
    expect(r).toBeDefined();
    expect(r!.name).toBe('TaskDetail');
    expect(r!.meta?.roles).toContain('viewer');
    expect(isRenderedLeaf(r!)).toBe(true);
  });

  it('legacy /tasks/:legacy/:id is a redirect, not a rendered leaf', () => {
    const r = leaves.find((l) => l.path === 'tasks/:legacy(sync|replay|verify)/:id');
    expect(r).toBeDefined();
    expect(isRenderedLeaf(r!)).toBe(false);
  });
});
