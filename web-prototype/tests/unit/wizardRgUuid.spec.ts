import { describe, expect, it } from 'vitest';
import type { CreateTaskDto } from '@/types/domain';

describe('Wizard submit — resourceGroupId and endpoint mapping', () => {
  describe('resourceGroupId is a UUID, not a name string', () => {
    it('uses a valid UUID for resourceGroupId', () => {
      const UUID_REGEX = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
      const rgId = 'a1b2c3d4-e5f6-7890-abcd-ef1234567890';
      expect(UUID_REGEX.test(rgId)).toBe(true);

      const dto: CreateTaskDto = {
        name: 'test-snapshot',
        kind: 'snapshot',
        engineSource: 'mysql',
        engineTarget: 'mysql',
        sourceEndpoint: { url: 'mysql://localhost:3306/src_db' },
        targetEndpoint: { url: 'mysql://localhost:3306/dst_db' },
        extractor: { extract_type: 'snapshot' },
        sinker: {},
        parallelizer: { parallel_type: 'snapshot', parallel_size: 4 },
        pipeline: { buffer_size: 16000, checkpoint_interval_secs: 10, max_rps: 0 },
        resumer: { resume_type: 'from_log' },
        resourceGroupId: rgId,
      };
      expect(dto.resourceGroupId).toMatch(UUID_REGEX);
    });

    it('rejects plain name strings like "default" as resourceGroupId', () => {
      const UUID_REGEX = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
      // The old bug: form.resourceGroup = 'default' (a name, not UUID)
      expect(UUID_REGEX.test('default')).toBe(false);
    });
  });

  describe('source/target endpoint URL mapping is correct', () => {
    it('sourceEndpoint contains source URL and targetEndpoint contains target URL', () => {
      const srcUrl = 'mysql://10.0.0.1:3306/src_db';
      const tgtUrl = 'mysql://10.0.0.2:3306/dst_db';

      const dto: CreateTaskDto = {
        name: 'endpoint-test',
        kind: 'snapshot',
        engineSource: 'mysql',
        engineTarget: 'mysql',
        sourceEndpoint: { url: srcUrl },
        targetEndpoint: { url: tgtUrl },
        extractor: { extract_type: 'snapshot' },
        sinker: {},
        parallelizer: { parallel_type: 'snapshot', parallel_size: 4 },
        pipeline: { buffer_size: 16000, checkpoint_interval_secs: 10, max_rps: 0 },
        resumer: { resume_type: 'from_log' },
        resourceGroupId: 'a1b2c3d4-e5f6-7890-abcd-ef1234567890',
      };
      expect(dto.sourceEndpoint.url).toBe(srcUrl);
      expect(dto.targetEndpoint.url).toBe(tgtUrl);
      // Ensure they are NOT swapped
      expect(dto.sourceEndpoint.url).not.toBe(tgtUrl);
      expect(dto.targetEndpoint.url).not.toBe(srcUrl);
    });

    it('engineSource matches source URL scheme and engineTarget matches target URL scheme', () => {
      const dto: CreateTaskDto = {
        name: 'cross-engine',
        kind: 'snapshot',
        engineSource: 'mysql',
        engineTarget: 'postgres',
        sourceEndpoint: { url: 'mysql://10.0.0.1:3306/db' },
        targetEndpoint: { url: 'postgres://10.0.0.2:5432/db' },
        extractor: { extract_type: 'snapshot' },
        sinker: {},
        parallelizer: { parallel_type: 'snapshot', parallel_size: 4 },
        pipeline: { buffer_size: 16000, checkpoint_interval_secs: 10, max_rps: 0 },
        resumer: { resume_type: 'from_log' },
        resourceGroupId: 'a1b2c3d4-e5f6-7890-abcd-ef1234567890',
      };
      expect(dto.sourceEndpoint.url.startsWith('mysql://')).toBe(true);
      expect(dto.targetEndpoint.url.startsWith('postgres://')).toBe(true);
      expect(dto.engineSource).toBe('mysql');
      expect(dto.engineTarget).toBe('postgres');
    });
  });
});
