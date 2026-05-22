import { describe, it, expect } from 'vitest';
import { canPerform, type Action } from '@/auth/permissions';

describe('Task lifecycle RBAC — pause/resume permissions', () => {
  it('admin can pause and resume tasks', () => {
    expect(canPerform('admin', 'task.pause')).toBe(true);
    expect(canPerform('admin', 'task.resume')).toBe(true);
  });

  it('operator can pause and resume tasks', () => {
    expect(canPerform('operator', 'task.pause')).toBe(true);
    expect(canPerform('operator', 'task.resume')).toBe(true);
  });

  it('viewer cannot pause or resume tasks', () => {
    expect(canPerform('viewer', 'task.pause')).toBe(false);
    expect(canPerform('viewer', 'task.resume')).toBe(false);
  });

  it('viewer cannot start or stop tasks', () => {
    expect(canPerform('viewer', 'task.start')).toBe(false);
    expect(canPerform('viewer', 'task.stop')).toBe(false);
  });

  it('viewer cannot delete tasks', () => {
    expect(canPerform('viewer', 'task.delete')).toBe(false);
  });

  it('operator can start and stop tasks', () => {
    expect(canPerform('operator', 'task.start')).toBe(true);
    expect(canPerform('operator', 'task.stop')).toBe(true);
  });

  it('operator cannot delete tasks', () => {
    expect(canPerform('operator', 'task.delete')).toBe(false);
  });

  it('admin can delete tasks', () => {
    expect(canPerform('admin', 'task.delete')).toBe(true);
  });

  it('null role cannot perform any action', () => {
    const actions: Action[] = ['task.start', 'task.stop', 'task.pause', 'task.resume', 'task.delete', 'task.create'];
    for (const a of actions) {
      expect(canPerform(null, a)).toBe(false);
    }
  });
});
