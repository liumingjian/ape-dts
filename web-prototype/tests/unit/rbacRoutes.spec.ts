import { describe, expect, it } from 'vitest';
import { routes } from '@/router/index';
import type { RouteRecordRaw } from 'vue-router';
import type { Role } from '@/auth/permissions';

const ALL_ROLES: Role[] = ['admin', 'operator', 'viewer'];

/**
 * Collect all leaf routes (routes with a `component`) from the nested route tree.
 * Skip redirect-only routes. Parent routes with both redirect and children
 * are recursed (only the parent itself is skipped, not its children).
 */
function leafRoutes(tree: RouteRecordRaw[]): RouteRecordRaw[] {
  const out: RouteRecordRaw[] = [];
  for (const r of tree) {
    if (r.children?.length) {
      out.push(...leafRoutes(r.children));
    } else if (r.component && !r.redirect) {
      out.push(r);
    }
  }
  return out;
}

describe('Route-level RBAC — meta.roles', () => {
  const leaves = leafRoutes(routes);

  it('every protected route declares a non-empty meta.roles array', () => {
    const protectedRoutes = leaves.filter((r) => !r.meta?.public);
    for (const r of protectedRoutes) {
      const roles = r.meta?.roles as Role[] | undefined;
      expect(roles, `${r.path} should have meta.roles`).toBeDefined();
      expect(roles!.length, `${r.path} meta.roles should not be empty`).toBeGreaterThan(0);
      for (const role of roles!) {
        expect(ALL_ROLES).toContain(role);
      }
    }
  });

  it('public routes are exempt from meta.roles', () => {
    const publicRoutes = leaves.filter((r) => r.meta?.public);
    expect(publicRoutes.length).toBeGreaterThan(0);
  });

  it('admin can access every protected route', () => {
    const protectedRoutes = leaves.filter((r) => !r.meta?.public);
    for (const r of protectedRoutes) {
      const roles = r.meta?.roles as Role[];
      expect(roles, `${r.path}`).toContain('admin');
    }
  });

  it('viewer cannot access admin-only routes', () => {
    const adminOnlyRoutes = leaves.filter(
      (r) => !r.meta?.public && (r.meta?.roles as Role[])?.length === 1 && (r.meta?.roles as Role[])[0] === 'admin',
    );
    expect(adminOnlyRoutes.length).toBeGreaterThan(0);
    for (const r of adminOnlyRoutes) {
      expect((r.meta?.roles as Role[])).not.toContain('viewer');
    }
  });

  it('operator cannot access alert-monitor routes', () => {
    const alertMonitorRoutes = leaves.filter(
      (r) => r.path.startsWith('alert-monitor/') && !r.meta?.public,
    );
    expect(alertMonitorRoutes.length).toBeGreaterThan(0);
    for (const r of alertMonitorRoutes) {
      expect((r.meta?.roles as Role[])).not.toContain('operator');
      expect((r.meta?.roles as Role[])).not.toContain('viewer');
    }
  });

  it('operator cannot access license route', () => {
    const licenseRoute = leaves.find((r) => r.path === 'license');
    expect(licenseRoute).toBeDefined();
    expect((licenseRoute!.meta?.roles as Role[])).not.toContain('operator');
  });

  it('operator cannot access global-params route', () => {
    const gpRoute = leaves.find((r) => r.path === 'ops/global-params');
    expect(gpRoute).toBeDefined();
    expect((gpRoute!.meta?.roles as Role[])).not.toContain('operator');
  });

  it('operator can access ops/management and ops/control-log', () => {
    const mgmt = leaves.find((r) => r.path === 'ops/management');
    const ctrl = leaves.find((r) => r.path === 'ops/control-log');
    expect(mgmt).toBeDefined();
    expect(ctrl).toBeDefined();
    expect((mgmt!.meta?.roles as Role[])).toContain('operator');
    expect((ctrl!.meta?.roles as Role[])).toContain('operator');
  });

  it('viewer can access dashboard, tasks, and alerts routes', () => {
    const viewerRoutes = ['dashboard', 'tasks/sync', 'tasks/check', 'tasks/struct', 'alerts/current', 'alerts/history'];
    for (const p of viewerRoutes) {
      const r = leaves.find((l) => l.path === p);
      expect(r, `${p} should exist`).toBeDefined();
      expect((r!.meta?.roles as Role[])).toContain('viewer');
    }
  });
});
