import { describe, expect, it } from 'vitest';
import { canPerform, visibleNavItems } from '@/auth/permissions';
import type { Action, Role } from '../helpers/fixtures';

const ALL_ROLES: Role[] = ['admin', 'operator', 'viewer'];

describe('AuthGuard (frontend projection)', () => {
  describe('canPerform matrix', () => {
    it('admin can perform every action', () => {
      const actions: Action[] = [
        'task.create',
        'task.delete',
        'task.start',
        'task.stop',
        'task.read',
        'user.manage',
        'license.activate',
        'alert.clear',
      ];
      for (const a of actions) expect(canPerform('admin', a)).toBe(true);
    });

    it('operator cannot manage users or activate license', () => {
      expect(canPerform('operator', 'user.manage')).toBe(false);
      expect(canPerform('operator', 'license.activate')).toBe(false);
      expect(canPerform('operator', 'task.delete')).toBe(false);
    });

    it('viewer can only read tasks', () => {
      expect(canPerform('viewer', 'task.read')).toBe(true);
      expect(canPerform('viewer', 'task.start')).toBe(false);
      expect(canPerform('viewer', 'alert.clear')).toBe(false);
    });
  });

  describe('visibleNavItems', () => {
    it('admin sees all top-level groups', () => {
      const items = visibleNavItems('admin');
      expect(items).toEqual(
        expect.arrayContaining(['dashboard', 'tasks', 'alerts', 'system', 'license']),
      );
    });

    it('viewer never sees admin-only groups', () => {
      const items = visibleNavItems('viewer');
      expect(items).not.toContain('system');
      expect(items).not.toContain('license');
      expect(items).not.toContain('alert-monitor');
    });

    it('every role sees a non-empty navigation', () => {
      for (const r of ALL_ROLES) expect(visibleNavItems(r).length).toBeGreaterThan(0);
    });
  });

  it.todo('login attempt records to operate_log regardless of outcome');
  it.todo('session idle timeout invalidates the session cookie');
  it.todo('navigating to admin-only route as operator returns 403 not 404');
});
