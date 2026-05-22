import { describe, expect, it, beforeEach } from 'vitest';
import { createPinia, setActivePinia } from 'pinia';
import { useWizardDraftStore, type WizardDraftForm } from '@/stores/wizardDraft';

// Mock localStorage
const localStorageMock = (() => {
  let store: Record<string, string> = {};
  return {
    getItem: (key: string) => store[key] ?? null,
    setItem: (key: string, value: string) => { store[key] = value; },
    removeItem: (key: string) => { delete store[key]; },
    clear: () => { store = {}; },
  };
})();
Object.defineProperty(globalThis, 'localStorage', { value: localStorageMock });

function sampleForm(): WizardDraftForm {
  return {
    taskName: 'test-task',
    description: 'desc',
    taskType: 'standalone',
    resourceGroup: 'default',
    instanceIp: '127.0.0.1',
    source: { engine: 'mysql', subMode: undefined, host: 'h1', port: 3306, username: 'root', password: 'pw', database: 'db', ssl: false },
    target: { engine: 'mysql', subMode: undefined, host: 'h2', port: 3306, username: 'root', password: 'pw', database: 'db', ssl: false },
    targetHasPdb: false,
    syncMode: 'snapshot',
    rate: { mode: 'unlimited', maxRps: 10000 },
    fullType: { schema: true, data: true, index: false },
    conflict: 'insert',
    filter: { doDbs: '*', doTbs: '', ignoreDbs: '', ignoreTbs: '', doEvents: ['insert', 'update', 'delete'] },
    config: {
      parallelizer: 'snapshot', parallelSize: 4, bufferSize: 16000,
      checkpointIntervalSecs: 10, maxRps: 0, resumeType: 'from_log',
      metricsEnabled: true, metricsHttpPort: 9090, metricsHttpHost: '127.0.0.1', metricsLabels: '',
    },
    router: { dbMap: '', tbMap: '', colMap: '', topicMap: '' },
    processor: { luaInline: '', luaFile: null, luaFileName: '' },
    procRules: [],
    startMode: 'now',
    delayAlertEnabled: false,
    delayAlertSecs: 60,
    currentStep: 0,
  };
}

describe('useWizardDraftStore', () => {
  beforeEach(() => {
    localStorageMock.clear();
    setActivePinia(createPinia());
  });

  it('saves and loads a draft for a category', () => {
    const store = useWizardDraftStore();
    const form = sampleForm();
    store.save('snapshot', form);
    const loaded = store.load('snapshot');
    expect(loaded).toBeTruthy();
    expect(loaded!.taskName).toBe('test-task');
    expect(loaded!.source.engine).toBe('mysql');
  });

  it('returns null when no draft exists', () => {
    const store = useWizardDraftStore();
    expect(store.load('cdc')).toBeNull();
  });

  it('discards a draft and clears localStorage', () => {
    const store = useWizardDraftStore();
    const form = sampleForm();
    store.save('snapshot', form);
    store.discard('snapshot');
    expect(store.load('snapshot')).toBeNull();
    expect(localStorageMock.getItem('console.wizard.snapshot')).toBeNull();
  });

  it('detects dirty state after modification', () => {
    const store = useWizardDraftStore();
    const form = sampleForm();
    store.save('snapshot', form);
    store.snapshotOriginal('snapshot');
    // Modify the form
    form.taskName = 'changed-name';
    store.save('snapshot', form);
    expect(store.isDirty('snapshot')).toBe(true);
  });

  it('is not dirty immediately after snapshot', () => {
    const store = useWizardDraftStore();
    const form = sampleForm();
    store.save('snapshot', form);
    store.snapshotOriginal('snapshot');
    expect(store.isDirty('snapshot')).toBe(false);
  });

  it('persists across simulated reload', () => {
    const store = useWizardDraftStore();
    const form = sampleForm();
    store.save('snapshot', form);
    // Simulate page reload by creating a new Pinia + store
    setActivePinia(createPinia());
    const store2 = useWizardDraftStore();
    const loaded = store2.load('snapshot');
    expect(loaded).toBeTruthy();
    expect(loaded!.taskName).toBe('test-task');
  });
});
