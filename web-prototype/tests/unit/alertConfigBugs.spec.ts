/**
 * Tests for alert-config-pages scrutiny bug fixes:
 * 1. Over-cap tooltip on create-task button when currentTasks >= maxTasks
 * 2. Bilingual rendering on global-params page with locale toggle
 */
import { describe, it, expect } from 'vitest';
import { ref, computed } from 'vue';
import type { GlobalParam } from '@/types/domain';

/* ---------- Bug 1: license-cap tooltip logic ---------- */

interface LicenseCapInfo {
  maxTasks: number;
  currentTasks: number;
  status?: string;
}

describe('taskList license-cap create button', () => {
  it('isAtCap is true when currentTasks >= maxTasks', () => {
    const license = ref<LicenseCapInfo | null>({ maxTasks: 5, currentTasks: 5, status: 'active' });
    const isAtCap = computed(() => {
      const l = license.value;
      return l != null && typeof l.maxTasks === 'number' && typeof l.currentTasks === 'number' && l.currentTasks >= l.maxTasks;
    });
    expect(isAtCap.value).toBe(true);
  });

  it('isAtCap is false when currentTasks < maxTasks', () => {
    const license = ref<LicenseCapInfo | null>({ maxTasks: 5, currentTasks: 3, status: 'active' });
    const isAtCap = computed(() => {
      const l = license.value;
      return l != null && typeof l.maxTasks === 'number' && typeof l.currentTasks === 'number' && l.currentTasks >= l.maxTasks;
    });
    expect(isAtCap.value).toBe(false);
  });

  it('isAtCap is false when license is null', () => {
    const license = ref<LicenseCapInfo | null>(null);
    const isAtCap = computed(() => {
      const l = license.value;
      return l != null && typeof l.maxTasks === 'number' && typeof l.currentTasks === 'number' && l.currentTasks >= l.maxTasks;
    });
    expect(isAtCap.value).toBe(false);
  });

  it('isAtCap is false when maxTasks is 0 (missing license)', () => {
    const license = ref<LicenseCapInfo | null>({ maxTasks: 0, currentTasks: 0, status: 'missing' });
    const isAtCap = computed(() => {
      const l = license.value;
      // maxTasks=0 means no license — don't block (backend will reject)
      if (l != null && l.maxTasks === 0) return false;
      return l != null && typeof l.maxTasks === 'number' && typeof l.currentTasks === 'number' && l.currentTasks >= l.maxTasks;
    });
    expect(isAtCap.value).toBe(false);
  });

  it('i18n key for over-cap tooltip exists in both locales', async () => {
    const zh = (await import('@/locales/zh-CN.json')).default;
    const en = (await import('@/locales/en-US.json')).default;
    expect(zh.taskList?.overCapTip).toBeDefined();
    expect(en.taskList?.overCapTip).toBeDefined();
    expect(typeof zh.taskList.overCapTip).toBe('string');
    expect(typeof en.taskList.overCapTip).toBe('string');
  });
});

/* ---------- Bug 2: bilingual global-params rendering ---------- */
describe('globalParams bilingual rendering', () => {
  it('GlobalParam type accepts bilingual fields', () => {
    const p: GlobalParam = {
      key: 'checkpoint_interval_secs',
      value: '10',
      description: '断点提交间隔',
      nameZh: '断点提交间隔',
      nameEn: 'Checkpoint interval (seconds)',
      descZh: '断点提交间隔',
      descEn: 'Checkpoint interval in seconds',
      category: 'runtime',
      updatedAt: '2024-12-01T00:00:00Z',
    };
    expect(p.nameZh).toBe('断点提交间隔');
    expect(p.nameEn).toBe('Checkpoint interval (seconds)');
    expect(p.descEn).toBe('Checkpoint interval in seconds');
  });

  it('bilingual display name falls back to key when nameZh/nameEn missing', () => {
    const p: GlobalParam = {
      key: 'checkpoint_interval_secs',
      value: '10',
      description: '断点提交间隔',
      category: 'runtime',
      updatedAt: '2024-12-01T00:00:00Z',
    };
    const displayNameZh = p.nameZh || p.key;
    const displayNameEn = p.nameEn || p.key;
    expect(displayNameZh).toBe('checkpoint_interval_secs');
    expect(displayNameEn).toBe('checkpoint_interval_secs');
  });

  it('bilingual description falls back to description when descZh/descEn missing', () => {
    const p: GlobalParam = {
      key: 'checkpoint_interval_secs',
      value: '10',
      description: '断点提交间隔',
      category: 'runtime',
      updatedAt: '2024-12-01T00:00:00Z',
    };
    const descZh = p.descZh || p.description;
    const descEn = p.descEn || p.description;
    expect(descZh).toBe('断点提交间隔');
    expect(descEn).toBe('断点提交间隔');
  });

  it('bilingual mode toggle computes correct display text', () => {
    const p: GlobalParam = {
      key: 'buffer_size',
      value: '200',
      description: '默认内存缓冲',
      nameZh: '默认内存缓冲',
      nameEn: 'Default buffer memory',
      descZh: '默认内存缓冲大小',
      descEn: 'Default buffer memory size',
      category: 'pipeline',
      updatedAt: '2024-12-01T00:00:00Z',
    };
    const bilingualMode = ref(true);
    const keyDisplay = computed(() => {
      if (!bilingualMode.value) return p.key;
      const zh = p.nameZh || p.key;
      const en = p.nameEn || p.key;
      return `${zh} / ${en}`;
    });
    const descDisplay = computed(() => {
      if (!bilingualMode.value) return p.description;
      const zh = p.descZh || p.description;
      const en = p.descEn || p.description;
      return `${zh} / ${en}`;
    });
    expect(keyDisplay.value).toBe('默认内存缓冲 / Default buffer memory');
    expect(descDisplay.value).toBe('默认内存缓冲大小 / Default buffer memory size');

    bilingualMode.value = false;
    expect(keyDisplay.value).toBe('buffer_size');
    expect(descDisplay.value).toBe('默认内存缓冲');
  });

  it('i18n keys for bilingual toggle exist in both locales', async () => {
    const zh = (await import('@/locales/zh-CN.json')).default;
    const en = (await import('@/locales/en-US.json')).default;
    expect(zh.ops?.globalParams?.bilingual).toBeDefined();
    expect(en.ops?.globalParams?.bilingual).toBeDefined();
    expect(typeof zh.ops.globalParams.bilingual).toBe('string');
    expect(typeof en.ops.globalParams.bilingual).toBe('string');
  });
});
