import { describe, expect, it } from 'vitest';
import {
  validateStep,
  parseConnectionUrl,
  engineFromUrlScheme,
  TASK_ID_REGEX,
  type WizardFormModel,
} from '@/composables/useWizardValidation';

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

describe('TASK_ID_REGEX', () => {
  it('accepts valid task IDs', () => {
    expect(TASK_ID_REGEX.test('abc')).toBe(true);
    expect(TASK_ID_REGEX.test('My_Task-01')).toBe(true);
    expect(TASK_ID_REGEX.test('A')).toBe(false); // too short (1 char, min 2)
    expect(TASK_ID_REGEX.test('Ab')).toBe(true);
    expect(TASK_ID_REGEX.test('a' .repeat(64))).toBe(true);
  });

  it('rejects invalid task IDs', () => {
    expect(TASK_ID_REGEX.test('1abc')).toBe(false); // must start with letter
    expect(TASK_ID_REGEX.test('My Task!')).toBe(false); // spaces and !
    expect(TASK_ID_REGEX.test('a' .repeat(65))).toBe(false); // too long
    expect(TASK_ID_REGEX.test('')).toBe(false);
  });
});

describe('validateStep · source step', () => {
  it('rejects empty state with required-field errors', () => {
    const errors = validateStep('source', {}, 'snapshot');
    expect(errors.name).toBe('task.name.required');
    expect(errors['source.engine']).toBe('engine.required');
    expect(errors['target.engine']).toBe('engine.required');
  });

  it('rejects invalid task name format', () => {
    const form: Partial<WizardFormModel> = {
      name: '1bad name!',
      source: { engine: 'mysql', host: 'h', port: 3306, username: 'u', password: 'p', database: '', ssl: false },
      target: { engine: 'mysql', host: 'h', port: 3306, username: 'u', password: 'p', database: '', ssl: false },
    };
    const errors = validateStep('source', form, 'snapshot');
    expect(errors.name).toBe('task.name.invalidFormat');
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

describe('validateStep · advanced step', () => {
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

describe('parseConnectionUrl', () => {
  it('parses a full MySQL URL with credentials', () => {
    const result = parseConnectionUrl('mysql://root:changeme@db.example.com:3307/orders');
    expect(result).toEqual({
      host: 'db.example.com',
      port: 3307,
      username: 'root',
      password: 'changeme',
      database: 'orders',
    });
  });

  it('parses a URL without database', () => {
    const result = parseConnectionUrl('postgres://admin:changeme@10.0.0.1:5432');
    expect(result).toEqual({
      host: '10.0.0.1',
      port: 5432,
      username: 'admin',
      password: 'changeme',
      database: '',
    });
  });

  it('parses a URL without password', () => {
    const result = parseConnectionUrl('mysql://root@db.host:3306/db');
    expect(result).toEqual({
      host: 'db.host',
      port: 3306,
      username: 'root',
      password: '',
      database: 'db',
    });
  });

  it('returns null for non-URL strings', () => {
    expect(parseConnectionUrl('not-a-url')).toBeNull();
    expect(parseConnectionUrl('')).toBeNull();
    expect(parseConnectionUrl('just-a-host')).toBeNull();
  });
});

describe('engineFromUrlScheme', () => {
  it('detects mysql from mysql://', () => {
    expect(engineFromUrlScheme('mysql://host/db')).toBe('mysql');
  });

  it('detects postgres from postgres://', () => {
    expect(engineFromUrlScheme('postgres://host/db')).toBe('postgres');
  });

  it('detects oracle from oracle://', () => {
    expect(engineFromUrlScheme('oracle://host/db')).toBe('oracle');
  });

  it('detects kafka from kafka://', () => {
    expect(engineFromUrlScheme('kafka://host:9092')).toBe('kafka');
  });

  it('returns null for no scheme', () => {
    expect(engineFromUrlScheme('host:3306')).toBeNull();
  });
});
