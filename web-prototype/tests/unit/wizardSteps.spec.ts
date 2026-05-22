import { describe, expect, it } from 'vitest';
import {
  STEP_KEYS_BY_CATEGORY,
  buildWizardSteps,
  defaultSubModeFor,
  isStepApplicable,
  requiresSubMode,
  type WizardStepKey,
} from '@/composables/useWizardSteps';
import type { TaskCategory } from '@/types/domain';

const fakeT = (key: string) => `t:${key}`;

const FULL_SEQUENCE: WizardStepKey[] = [
  'source',
  'test',
  'objects',
  'processing',
  'advanced',
  'precheck',
  'confirm',
];

const STRUCT_SEQUENCE: WizardStepKey[] = [
  'source',
  'test',
  'objects',
  'precheck',
  'confirm',
];

describe('useWizardSteps · STEP_KEYS_BY_CATEGORY', () => {
  it('exposes the full 7-step sequence for snapshot, cdc and check', () => {
    for (const cat of ['snapshot', 'cdc', 'check'] as TaskCategory[]) {
      expect(STEP_KEYS_BY_CATEGORY[cat]).toEqual(FULL_SEQUENCE);
    }
  });

  it('skips processing and advanced for struct', () => {
    expect(STEP_KEYS_BY_CATEGORY.struct).toEqual(STRUCT_SEQUENCE);
    expect(STEP_KEYS_BY_CATEGORY.struct).not.toContain('processing');
    expect(STEP_KEYS_BY_CATEGORY.struct).not.toContain('advanced');
  });
});

describe('useWizardSteps · buildWizardSteps', () => {
  it('returns label resolved through the translator for each step', () => {
    const steps = buildWizardSteps('cdc', fakeT);
    expect(steps).toHaveLength(7);
    expect(steps[0]).toEqual({ key: 'source', label: 't:wizard.step.source' });
    expect(steps[6]).toEqual({ key: 'confirm', label: 't:wizard.step.confirm' });
  });

  it('returns 5 entries for struct', () => {
    const steps = buildWizardSteps('struct', fakeT);
    expect(steps.map((s) => s.key)).toEqual(STRUCT_SEQUENCE);
  });
});

describe('useWizardSteps · isStepApplicable', () => {
  it('hides processing and advanced for struct', () => {
    expect(isStepApplicable('struct', 'processing')).toBe(false);
    expect(isStepApplicable('struct', 'advanced')).toBe(false);
    expect(isStepApplicable('struct', 'precheck')).toBe(true);
  });

  it('keeps every canonical step applicable for non-struct kinds', () => {
    for (const cat of ['snapshot', 'cdc', 'check'] as TaskCategory[]) {
      for (const step of FULL_SEQUENCE) {
        expect(isStepApplicable(cat, step)).toBe(true);
      }
    }
  });
});

describe('useWizardSteps · GaussDB sub-mode helpers', () => {
  it('returns pg-mode as the default sub-mode for gaussdb', () => {
    expect(defaultSubModeFor('gaussdb')).toBe('pg-mode');
  });

  it('returns undefined for non-gaussdb engines', () => {
    expect(defaultSubModeFor('mysql')).toBeUndefined();
    expect(defaultSubModeFor('oracle')).toBeUndefined();
    expect(defaultSubModeFor('postgres')).toBeUndefined();
  });

  it('flags gaussdb as requiring a sub-mode and others as not', () => {
    expect(requiresSubMode('gaussdb')).toBe(true);
    expect(requiresSubMode('mysql')).toBe(false);
    expect(requiresSubMode('oracle')).toBe(false);
  });
});
