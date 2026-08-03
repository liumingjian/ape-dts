import { defineStore } from 'pinia';
import { ref, computed } from 'vue';
import type { EngineType, GaussdbSubMode, SyncMode, ParallelType, ResumeType, TaskViewKind } from '@/types/domain';

/* ---------- shape of the persisted draft ---------- */

export interface DraftEndpoint {
  engine: EngineType;
  subMode?: GaussdbSubMode;
  host: string;
  candidateHosts?: string[];
  port: number;
  username: string;
  password: string;
  database: string;
  ssl: boolean;
}

export interface DraftConfig {
  parallelizer: ParallelType;
  parallelSize: number;
  bufferSize: number;
  checkpointIntervalSecs: number;
  maxRps: number;
  resumeType: ResumeType;
  metricsEnabled: boolean;
  metricsHttpPort: number;
  metricsHttpHost: string;
  metricsLabels: string;
}

export interface DraftRouter {
  dbMap: string;   // "src:dst" per line
  tbMap: string;   // "src_db.t1:dst_db.t1" per line
  colMap: string;
  topicMap: string; // "*.*:default_topic" per line
}

export interface DraftFilter {
  doDbs: string;
  doTbs: string;
  ignoreDbs: string;
  ignoreTbs: string;
  doEvents: string[];    // insert,update,delete
}

export interface DraftProcessor {
  luaInline: string;
  luaFile: string | null;
  luaFileName: string;
}

export interface DraftProcRule {
  target: string;
  doEvents: string[];
  where: string;
  ignoreCols: string;
}

export interface WizardDraftForm {
  taskName: string;
  description: string;
  taskType: 'standalone' | 'primary_backup';
  resourceGroup: string;
  instanceIp: string;
  source: DraftEndpoint;
  target: DraftEndpoint;
  targetHasPdb: boolean;
  syncMode: SyncMode;
  rate: { mode: 'limited' | 'unlimited'; maxRps: number };
  fullType: { schema: boolean; data: boolean; index: boolean };
  conflict: 'insert' | 'replace' | 'ignore';
  filter: DraftFilter;
  config: DraftConfig;
  router: DraftRouter;
  processor: DraftProcessor;
  procRules: DraftProcRule[];
  startMode: 'now' | 'later';
  delayAlertEnabled: boolean;
  delayAlertSecs: number;
  currentStep: number;
}

export type WizardDraftKey = `console.wizard.${TaskViewKind}`;

function draftKey(category: TaskViewKind): WizardDraftKey {
  return `console.wizard.${category}`;
}

function persistedDraft(form: WizardDraftForm): WizardDraftForm {
  return {
    ...form,
    source: { ...form.source, password: '' },
    target: { ...form.target, password: '' },
  };
}

export const useWizardDraftStore = defineStore('wizardDraft', () => {
  /* ---- per-category draft ---- */
  const drafts = ref<Record<string, WizardDraftForm | null>>({});
  const originalHash = ref<Record<string, string | null>>({});

  function load(category: TaskViewKind): WizardDraftForm | null {
    const key = draftKey(category);
    if (drafts.value[key]) return drafts.value[key];
    try {
      const raw = localStorage.getItem(key);
      if (!raw) return null;
      const parsed = JSON.parse(raw) as WizardDraftForm;
      const safeDraft = persistedDraft(parsed);
      drafts.value[key] = safeDraft;
      localStorage.setItem(key, JSON.stringify(safeDraft));
      // snapshot original hash for dirty tracking
      if (!originalHash.value[key]) {
        originalHash.value[key] = JSON.stringify(safeDraft);
      }
      return safeDraft;
    } catch {
      return null;
    }
  }

  function save(category: TaskViewKind, form: WizardDraftForm): void {
    const key = draftKey(category);
    drafts.value[key] = form;
    localStorage.setItem(key, JSON.stringify(persistedDraft(form)));
  }

  function discard(category: TaskViewKind): void {
    const key = draftKey(category);
    drafts.value[key] = null;
    originalHash.value[key] = null;
    localStorage.removeItem(key);
  }

  function isDirty(category: TaskViewKind): boolean {
    const key = draftKey(category);
    const current = drafts.value[key];
    if (!current && !originalHash.value[key]) return false;
    const currentHash = current ? JSON.stringify(persistedDraft(current)) : '';
    return currentHash !== (originalHash.value[key] ?? '');
  }

  /** Snapshot the current draft as the "original" for dirty tracking. */
  function snapshotOriginal(category: TaskViewKind): void {
    const key = draftKey(category);
    const current = drafts.value[key];
    originalHash.value[key] = current ? JSON.stringify(persistedDraft(current)) : null;
  }

  const hasAnyDraft = computed(() =>
    Object.values(drafts.value).some((d) => d !== null),
  );

  return {
    drafts,
    load,
    save,
    discard,
    isDirty,
    snapshotOriginal,
    hasAnyDraft,
    draftKey,
  };
});
