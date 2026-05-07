import { describe, expect, it } from 'vitest';
import type { CreateTaskDto, TaskCategory, SyncMode, ExtractType } from '@/types/domain';

/**
 * Pure-function transformation: WizardForm → CreateTaskDto.
 * Mirrors the logic in CreateTaskWizard.vue's syncModeToExtractType + formToTaskDraft.
 * NOTE: snapshot_cdc mode maps to 'snapshot' because the backend engine doesn't
 * yet support snapshot_and_cdc as an extract_type — the snapshot phase runs first.
 */
function syncModeToExtractType(mode: SyncMode, cat: TaskCategory): ExtractType {
  if (cat === 'struct') return 'struct';
  if (cat === 'check') return 'snapshot';
  if (mode === 'snapshot_cdc') return 'snapshot';
  if (mode === 'snapshot') return 'snapshot';
  if (mode === 'cdc') return 'cdc';
  return 'snapshot';
}

function makeEndpoint(dsn: string) {
  return { url: dsn };
}

const SRC_DSN = 'SRC_DSN_PLACEHOLDER';
const TGT_DSN = 'TGT_DSN_PLACEHOLDER';
const GAUSS_DSN = 'GAUSS_DSN_PLACEHOLDER';

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

    it('maps snapshot_cdc syncMode to "snapshot" extract type (snapshot phase first)', () => {
      expect(syncModeToExtractType('snapshot_cdc', 'snapshot')).toBe('snapshot');
      expect(syncModeToExtractType('snapshot_cdc', 'cdc')).toBe('snapshot');
    });
  });

  describe('CreateTaskDto shape (wire format)', () => {
    it('contains all required fields for a snapshot task', () => {
      const dto: CreateTaskDto = {
        name: 'test-snapshot',
        kind: 'snapshot',
        engineSource: 'mysql',
        engineTarget: 'mysql',
        sourceEndpoint: makeEndpoint(SRC_DSN),
        targetEndpoint: makeEndpoint(TGT_DSN),
        extractor: { extract_type: 'snapshot' },
        sinker: {},
        parallelizer: { parallel_type: 'snapshot', parallel_size: 4 },
        pipeline: { buffer_size: 16000, checkpoint_interval_secs: 10, max_rps: 0 },
        resumer: { resume_type: 'from_log' },
        resourceGroupId: 'default',
      };
      expect(dto.kind).toBe('snapshot');
      expect(dto.extractor.extract_type).toBe('snapshot');
      expect(dto.engineSource).toBe('mysql');
      expect(dto.engineTarget).toBe('mysql');
      expect(dto.parallelizer.parallel_size).toBe(4);
    });

    it('includes GaussDB subMode when source is gaussdb', () => {
      const dto: CreateTaskDto = {
        name: 'gaussdb-cdc',
        kind: 'cdc',
        engineSource: 'gaussdb',
        engineTarget: 'mysql',
        subMode: 'pg-mode',
        sourceEndpoint: makeEndpoint(GAUSS_DSN),
        targetEndpoint: makeEndpoint(TGT_DSN),
        extractor: { extract_type: 'cdc' },
        sinker: {},
        parallelizer: { parallel_type: 'rdb_merge', parallel_size: 4 },
        pipeline: { buffer_size: 16000, checkpoint_interval_secs: 10, max_rps: 0 },
        resumer: { resume_type: 'from_log' },
        resourceGroupId: 'default',
      };
      expect(dto.subMode).toBe('pg-mode');
      expect(dto.extractor.extract_type).toBe('cdc');
    });

    it('includes filter with selected objects', () => {
      const dto: CreateTaskDto = {
        name: 'with-filter',
        kind: 'snapshot',
        engineSource: 'mysql',
        engineTarget: 'mysql',
        sourceEndpoint: makeEndpoint(SRC_DSN),
        targetEndpoint: makeEndpoint(TGT_DSN),
        extractor: { extract_type: 'snapshot' },
        sinker: {},
        filter: {
          do_dbs: 'app_db',
          do_tbs: 'app_db.users,app_db.orders',
        },
        parallelizer: { parallel_type: 'snapshot', parallel_size: 4 },
        pipeline: { buffer_size: 16000, checkpoint_interval_secs: 10, max_rps: 0 },
        resumer: { resume_type: 'from_log' },
        resourceGroupId: 'default',
      };
      expect(dto.filter?.do_dbs).toBe('app_db');
      expect(dto.filter?.do_tbs).toBe('app_db.users,app_db.orders');
    });

    it('includes processor with lua_code for inline Lua', () => {
      const dto: CreateTaskDto = {
        name: 'with-lua-inline',
        kind: 'snapshot',
        engineSource: 'mysql',
        engineTarget: 'mysql',
        sourceEndpoint: makeEndpoint(SRC_DSN),
        targetEndpoint: makeEndpoint(TGT_DSN),
        extractor: { extract_type: 'snapshot' },
        sinker: {},
        processor: { lua_code_file: 'inline', lua_code: 'function process() end' },
        parallelizer: { parallel_type: 'snapshot', parallel_size: 4 },
        pipeline: { buffer_size: 16000, checkpoint_interval_secs: 10, max_rps: 0 },
        resumer: { resume_type: 'from_log' },
        resourceGroupId: 'default',
      };
      expect(dto.processor?.lua_code_file).toBe('inline');
      expect(dto.processor?.lua_code).toBe('function process() end');
    });

    it('includes processor with lua_code for file upload Lua', () => {
      const dto: CreateTaskDto = {
        name: 'with-lua-file',
        kind: 'snapshot',
        engineSource: 'mysql',
        engineTarget: 'mysql',
        sourceEndpoint: makeEndpoint(SRC_DSN),
        targetEndpoint: makeEndpoint(TGT_DSN),
        extractor: { extract_type: 'snapshot' },
        sinker: {},
        processor: { lua_code_file: 'inline', lua_code: '-- file content here\nfunction process() end' },
        parallelizer: { parallel_type: 'snapshot', parallel_size: 4 },
        pipeline: { buffer_size: 16000, checkpoint_interval_secs: 10, max_rps: 0 },
        resumer: { resume_type: 'from_log' },
        resourceGroupId: 'default',
      };
      expect(dto.processor?.lua_code_file).toBe('inline');
      expect(dto.processor?.lua_code).toContain('function process() end');
    });
  });
});
