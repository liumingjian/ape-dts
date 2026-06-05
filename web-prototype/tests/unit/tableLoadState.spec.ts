import { describe, it, expect } from 'vitest';
import type { TableLoadState } from '@/types/domain';

describe('TableLoadState', () => {
  it('accepts the three state literals and rejects others', () => {
    const pending: TableLoadState = { schema: 'public', table: 'orders', state: 'pending' };
    const loading: TableLoadState = { schema: 'public', table: 'users', state: 'loading' };
    const completed: TableLoadState = { schema: 'public', table: 'products', state: 'completed' };

    expect(pending.state).toBe('pending');
    expect(loading.state).toBe('loading');
    expect(completed.state).toBe('completed');

    // @ts-expect-error — 'failed' is not a valid state
    const _invalid: TableLoadState = { schema: 'public', table: 'x', state: 'failed' };
    void _invalid;
  });
});
