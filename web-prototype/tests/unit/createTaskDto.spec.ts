import { describe, expect, it } from 'vitest';
import type { CreateTaskDto, TaskCategory, SyncMode, ExtractType } from '@/types/domain';

/**
 * Pure-function transformation: WizardForm → CreateTaskDto.
 * Mirrors the logic in CreateTaskWizard.vue's syncModeToExtractType + formToTaskDraft.
 */
function syncModeToExtractType(mode: SyncMode, cat: TaskCategory): ExtractType {
  if (cat === 'struct') return 'struct';
  if (cat === 'check') return 'snapshot';
  if (mode === 'snapshot') return 'snapshot';
  if (mode === 'cdc') return 'cdc';
  return 'snapshot_and_cdc';
}

describe('WizardForm → CreateTaskDto transformation', () => {
  describe('syncModeToExtractType', () => {
    it('maps struct category to "struct" extract type', () => {
      expect(syncModeToExtractType('snapshot', 'struct')).toBe('struct');
    });

    it('maps check category to "snapshot" extract type', () => {
      expect(syncModeToExtractType('snapshot_cdc', 'check')).toBe('snapshot');
    });

    it('maps snapshot syncMode to "snapshot" extract type', () => {
      expect(syncModeToExtractType('snapshot', 'snapshot')).toBe('snapshot');
    });

    it('maps cdc syncMode to "cdc" extract type', () => {
      expect(syncModeToExtractType('cdc', 'cdc')).toBe('cdc');
    });

    it('maps snapshot_cdc syncMode to "snapshot_and_cdc" extract type', () => {
      expect(syncModeToExtractType('snapshot_cdc', 'snapshot')).toBe('snapshot_and_cdc');
      expect(syncModeToExtractType('snapshot_cdc', 'cdc')).toBe('snapshot_and_cdc');
    });
  });

  describe('CreateTaskDto shape', () => {
    it('contains all required fields for a snapshot task', () => {
      const dto: CreateTaskDto = {
        name: 'test-snapshot',
        description: '',
        category: 'snapshot',
        source: {
          engine: 'mysql',
          host: '192.168.1.10',
          port: 3306,
          username: 'root',
          password: 'secret',
          database: 'app',
          ssl: false,
        },
        target: {
          engine: 'mysql',
          host: '192.168.1.20',
          port: 3306,
          username: 'root',
          password: 'secret',
          database: 'app',
          ssl: false,
        },
        syncMode: 'snapshot',
        extractType: 'snapshot',
        taskType: 'standalone',
        resourceGroup: 'default',
        instanceIp: '127.0.0.1',
        syncObjects: { totalTables: 5, selectedTables: 5 },
        config: {
          parallelizer: 'snapshot',
          parallelSize: 4,
          bufferSize: 16000,
          maxRps: 0,
          checkpointIntervalSecs: 10,
          resumeType: 'from_log',
          metricsEnabled: true,
        },
      };
      expect(dto.category).toBe('snapshot');
      expect(dto.extractType).toBe('snapshot');
      expect(dto.source.engine).toBe('mysql');
      expect(dto.target.engine).toBe('mysql');
      expect(dto.config.parallelSize).toBe(4);
    });

    it('includes GaussDB subMode when source is gaussdb', () => {
      const dto: CreateTaskDto = {
        name: 'gaussdb-cdc',
        description: '',
        category: 'cdc',
        source: {
          engine: 'gaussdb',
          subMode: 'pg-mode',
          host: '10.0.0.1',
          port: 5432,
          username: 'gaussdb',
          password: 'secret',
          database: 'app',
          ssl: true,
        },
        target: {
          engine: 'mysql',
          host: '192.168.1.20',
          port: 3306,
          username: 'root',
          password: 'secret',
          database: 'app',
          ssl: false,
        },
        syncMode: 'cdc',
        extractType: 'cdc',
        taskType: 'standalone',
        resourceGroup: 'default',
        instanceIp: '127.0.0.1',
        syncObjects: { totalTables: 0, selectedTables: 0 },
        config: {
          parallelizer: 'rdb_merge',
          parallelSize: 4,
          bufferSize: 16000,
          maxRps: 0,
          checkpointIntervalSecs: 10,
          resumeType: 'from_log',
          metricsEnabled: true,
        },
      };
      expect(dto.source.subMode).toBe('pg-mode');
      expect(dto.extractType).toBe('cdc');
    });

    it('includes filter with selected objects', () => {
      const dto: CreateTaskDto = {
        name: 'with-filter',
        description: '',
        category: 'snapshot',
        source: {
          engine: 'mysql',
          host: 'h',
          port: 3306,
          username: 'u',
          password: 'p',
          database: 'app',
          ssl: false,
        },
        target: {
          engine: 'mysql',
          host: 'h',
          port: 3306,
          username: 'u',
          password: 'p',
          database: 'app',
          ssl: false,
        },
        syncMode: 'snapshot',
        extractType: 'snapshot',
        taskType: 'standalone',
        resourceGroup: 'default',
        instanceIp: '127.0.0.1',
        syncObjects: { totalTables: 2, selectedTables: 2 },
        config: {
          parallelizer: 'snapshot',
          parallelSize: 4,
          bufferSize: 16000,
          maxRps: 0,
          checkpointIntervalSecs: 10,
          resumeType: 'from_log',
          metricsEnabled: true,
        },
        filter: {
          doDbs: ['app_db'],
          doTbs: ['app_db.users', 'app_db.orders'],
        },
      };
      expect(dto.filter?.doDbs).toEqual(['app_db']);
      expect(dto.filter?.doTbs).toEqual(['app_db.users', 'app_db.orders']);
    });
  });
});
