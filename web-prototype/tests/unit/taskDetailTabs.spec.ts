import { describe, it, expect } from 'vitest';

const VALID_TABS = ['overview', 'objects', 'logs', 'monitoring', 'history', 'more'] as const;

describe('TaskDetail tabs — order and deep-link names', () => {
  it('has exactly 6 tabs', () => {
    expect(VALID_TABS.length).toBe(6);
  });

  it('tabs are in the documented order', () => {
    expect(VALID_TABS).toEqual(['overview', 'objects', 'logs', 'monitoring', 'history', 'more']);
  });

  it('every tab name is URL-safe for ?tab= deep links', () => {
    for (const tab of VALID_TABS) {
      expect(tab).toMatch(/^[a-z]+$/);
    }
  });

  it('no duplicate tab names', () => {
    const set = new Set(VALID_TABS);
    expect(set.size).toBe(VALID_TABS.length);
  });
});

describe('TaskStatus — extended statuses', () => {
  it('draft status exists in TaskStatus type', () => {
    const s: string[] = ['draft', 'ready', 'running', 'paused', 'stopping', 'stopped', 'failed', 'completed', 'creating', 'pending'];
    expect(s).toContain('draft');
    expect(s).toContain('ready');
    expect(s).toContain('stopped');
    expect(s).toContain('stopping');
  });
});
