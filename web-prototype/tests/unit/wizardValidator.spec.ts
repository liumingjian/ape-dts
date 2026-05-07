import { describe, expect, it } from 'vitest';
import { validateStep, type WizardFormModel } from '@/composables/useWizardValidation';
import { isStepApplicable } from '@/composables/useWizardSteps';

const validSnapshotForm: WizardFormModel = {
  name: 'test-task',
  description: '',
  taskType: 'standalone',
  resourceGroup: 'default',
  instanceIp: '127.0.0.1',
  source: { engine: 'mysql', host: '192.168.1.10', port: 3306, username: 'root', password: 'pw', database: 'app', ssl: false },
  target: { engine: 'mysql', host: '192.168.1.20', port: 3306, username: 'root', password: 'pw', database: 'app', ssl: false },
  syncMode: 'snapshot',
  config: { parallelizer: 'snapshot', parallelSize: 4, bufferSize: 16000, checkpointIntervalSecs: 10, maxRps: 1000, resumeType: 'from_log', metricsEnabled: true, metricsHttpPort: 9090 },
};

const _validStructForm: WizardFormModel = {
  ...validSnapshotForm,
  config: { ...validSnapshotForm.config, parallelizer: 'serial', parallelSize: 1 },
};

describe('WizardValidator (useWizardValidation)', () => {
  describe('source step', () => {
    it('rejects empty state with required-field errors', () => {
      const errors = validateStep('source', {}, 'snapshot');
      expect(errors.name).toBe('task.name.required');
      expect(errors['source.engine']).toBe('engine.required');
      expect(errors['target.engine']).toBe('engine.required');
    });

    it('requires GaussDB sub-mode when source engine is gaussdb', () => {
      const form: Partial<WizardFormModel> = {
        name: 't1',
        source: { engine: 'gaussdb', host: 'host', port: 5432, username: 'u', password: 'p', database: '', ssl: false },
        target: { engine: 'oracle', host: 'host', port: 1521, username: 'u', password: 'p', database: '', ssl: false },
      };
      const errors = validateStep('source', form, 'cdc');
      expect(errors['source.subMode']).toBe('gaussdb.subMode.required');
    });

    it('requires GaussDB sub-mode when target engine is gaussdb', () => {
      const form: Partial<WizardFormModel> = {
        name: 't1',
        source: { engine: 'mysql', host: 'host', port: 3306, username: 'u', password: 'p', database: '', ssl: false },
        target: { engine: 'gaussdb', host: 'host', port: 5432, username: 'u', password: 'p', database: '', ssl: false },
      };
      const errors = validateStep('source', form, 'cdc');
      expect(errors['target.subMode']).toBe('gaussdb.subMode.required');
    });

    it('passes a minimal valid snapshot form', () => {
      const errors = validateStep('source', validSnapshotForm, 'snapshot');
      expect(errors).toEqual({});
    });

    it('requires host on both source and target', () => {
      const form: Partial<WizardFormModel> = {
        name: 'test',
        source: { engine: 'mysql', host: '', port: 3306, username: 'u', password: 'p', database: '', ssl: false },
        target: { engine: 'mysql', host: '', port: 3306, username: 'u', password: 'p', database: '', ssl: false },
      };
      const errors = validateStep('source', form, 'snapshot');
      expect(errors['source.host']).toBe('host.required');
      expect(errors['target.host']).toBe('host.required');
    });
  });

  describe('advanced step', () => {
    it('rejects non-positive parallel size for non-struct', () => {
      const form: Partial<WizardFormModel> = {
        config: { parallelizer: 'serial', parallelSize: 0, bufferSize: 16000, checkpointIntervalSecs: 10, maxRps: 1000, resumeType: 'from_log', metricsEnabled: true, metricsHttpPort: 9090 },
      };
      const errors = validateStep('advanced', form, 'cdc');
      expect(errors['config.parallelSize']).toBe('parallel.size.positive');
    });

    it('skips validation for struct category', () => {
      const form: Partial<WizardFormModel> = {
        config: { parallelizer: 'serial', parallelSize: 0, bufferSize: 16000, checkpointIntervalSecs: 10, maxRps: 1000, resumeType: 'from_log', metricsEnabled: true, metricsHttpPort: 9090 },
      };
      const errors = validateStep('advanced', form, 'struct');
      expect(errors).toEqual({});
    });
  });

  describe('isStepApplicable', () => {
    it('hides processing & advanced steps for struct kind', () => {
      expect(isStepApplicable('struct', 'processing')).toBe(false);
      expect(isStepApplicable('struct', 'advanced')).toBe(false);
      expect(isStepApplicable('struct', 'precheck')).toBe(true);
    });

    it('keeps all steps applicable for snapshot kind', () => {
      const allSteps = ['source', 'test', 'objects', 'processing', 'advanced', 'precheck', 'confirm'] as const;
      for (const step of allSteps) {
        expect(isStepApplicable('snapshot', step)).toBe(true);
      }
    });
  });

  it.todo('snapshot_file replay sub-mode treats source URL as optional');
  it.todo('reports conflicting db_map and tb_map router rules');
  it.todo('rejects Lua processor for engines outside MySQL/PG');
  it.todo('cross-step validation: precheck cannot run with errors in earlier steps');
});
