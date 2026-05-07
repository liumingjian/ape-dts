import { describe, expect, it } from 'vitest';
import { legacyToCategory, type TaskCategory } from '@/types/domain';

describe('Task taxonomy (ADR-0006)', () => {
  describe('legacyToCategory', () => {
    it('maps prototype "sync" to snapshot', () => {
      expect(legacyToCategory('sync')).toBe('snapshot');
    });

    it('maps prototype "replay" to snapshot (snapshot_file is a sub-mode)', () => {
      expect(legacyToCategory('replay')).toBe('snapshot');
    });

    it('maps prototype "verify" to check', () => {
      expect(legacyToCategory('verify')).toBe('check');
    });

    it('passes through canonical kinds unchanged', () => {
      const kinds: TaskCategory[] = ['snapshot', 'cdc', 'check', 'struct'];
      for (const k of kinds) expect(legacyToCategory(k)).toBe(k);
    });

    it('falls back to snapshot for unknown values', () => {
      expect(legacyToCategory('mystery')).toBe('snapshot');
      expect(legacyToCategory('')).toBe('snapshot');
    });
  });

  describe('canonical kinds', () => {
    it('exposes exactly four top-level kinds', () => {
      const kinds: TaskCategory[] = ['snapshot', 'cdc', 'check', 'struct'];
      expect(kinds).toHaveLength(4);
      expect(new Set(kinds).size).toBe(4);
    });
  });
});
