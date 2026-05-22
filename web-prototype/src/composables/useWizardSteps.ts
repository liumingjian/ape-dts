import type { TaskCategory, EngineType, GaussdbSubMode } from '@/types/domain';

export type WizardStepKey =
  | 'source'
  | 'test'
  | 'objects'
  | 'processing'
  | 'advanced'
  | 'precheck'
  | 'confirm';

const FULL_STEP_SEQUENCE: readonly WizardStepKey[] = [
  'source',
  'test',
  'objects',
  'processing',
  'advanced',
  'precheck',
  'confirm',
] as const;

const STRUCT_STEP_SEQUENCE: readonly WizardStepKey[] = [
  'source',
  'test',
  'objects',
  'precheck',
  'confirm',
] as const;

export const STEP_KEYS_BY_CATEGORY: Record<TaskCategory, readonly WizardStepKey[]> = {
  snapshot: FULL_STEP_SEQUENCE,
  cdc: FULL_STEP_SEQUENCE,
  check: FULL_STEP_SEQUENCE,
  struct: STRUCT_STEP_SEQUENCE,
};

export interface WizardStep {
  key: WizardStepKey;
  label: string;
}

type Translator = (key: string) => string;

export function buildWizardSteps(category: TaskCategory, t: Translator): WizardStep[] {
  return STEP_KEYS_BY_CATEGORY[category].map((key) => ({
    key,
    label: t(`wizard.step.${key}`),
  }));
}

export function isStepApplicable(category: TaskCategory, step: WizardStepKey): boolean {
  return STEP_KEYS_BY_CATEGORY[category].includes(step);
}

export function defaultSubModeFor(engine: EngineType): GaussdbSubMode | undefined {
  return engine === 'gaussdb' ? 'pg-mode' : undefined;
}

export function requiresSubMode(engine: EngineType): boolean {
  return engine === 'gaussdb';
}
