import { describe, expect, it } from 'vitest';
import { canPerform, visibleNavItems, type Action, type Role } from '@/auth/permissions';

const ALL_ACTIONS: Action[] = [
  'task.create',
  'task.delete',
  'task.start',
  'task.stop',
  'task.read',
  'user.manage',
  'license.activate',
  'alert.clear',
];

describe('permissions · canPerform', () => {
  it('admin can perform every action', () => {
    for (const a of ALL_ACTIONS) expect(canPerform('admin', a)).toBe(true);
  });

  it('operator cannot delete tasks, manage users, or activate licenses', () => {
    expect(canPerform('operator', 'task.create')).toBe(true);
    expect(canPerform('operator', 'task.start')).toBe(true);
    expect(canPerform('operator', 'task.stop')).toBe(true);
    expect(canPerform('operator', 'alert.clear')).toBe(true);
    expect(canPerform('operator', 'task.delete')).toBe(false);
    expect(canPerform('operator', 'user.manage')).toBe(false);
    expect(canPerform('operator', 'license.activate')).toBe(false);
  });

  it('viewer is read-only (can read tasks, alerts, license)', () => {
    expect(canPerform('viewer', 'task.read')).toBe(true);
    expect(canPerform('viewer', 'alert.read')).toBe(true);
    expect(canPerform('viewer', 'license.read')).toBe(true);
    for (const a of ALL_ACTIONS.filter((x) => x !== 'task.read')) {
      expect(canPerform('viewer', a)).toBe(false);
    }
  });

  it('null / undefined role denies every action', () => {
    expect(canPerform(null, 'task.read')).toBe(false);
    expect(canPerform(undefined, 'task.read')).toBe(false);
  });
});

describe('permissions · visibleNavItems', () => {
  it('admin sees every module', () => {
    expect(visibleNavItems('admin')).toEqual([
      'dashboard',
      'tasks',
      'alerts',
      'license',
      'alertMonitor',
      'system',
      'ops',
    ]);
  });

  it('operator sees base + license + ops, not alert-monitor / system', () => {
    expect(visibleNavItems('operator')).toEqual(['dashboard', 'tasks', 'alerts', 'license', 'ops']);
  });

  it('viewer sees base modules + license (read-only)', () => {
    expect(visibleNavItems('viewer')).toEqual(['dashboard', 'tasks', 'alerts', 'license']);
  });

  it('null role sees nothing', () => {
    expect(visibleNavItems(null)).toEqual([]);
  });
});

describe('permissions · matrix is exhaustive over Role', () => {
  it('every role yields a defined nav set', () => {
    for (const r of ['admin', 'operator', 'viewer'] satisfies Role[]) {
      expect(visibleNavItems(r).length).toBeGreaterThan(0);
    }
  });
});
