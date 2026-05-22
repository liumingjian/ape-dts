import { describe, it, expect } from 'vitest';
import { mapApiTask, type ApiTask, type SyncMode, type ResumeType } from '@/types/domain';

const BASE: ApiTask = {
  id: 'aaa-bbb-ccc',
  taskId: 'snapshot_mysql_mysql_aaa',
  name: 'test-snapshot',
  kind: 'snapshot',
  dbTypeSource: 'mysql',
  dbTypeTarget: 'postgres',
  sourceEndpoint: { url: 'mysql://root:pw@10.0.0.1:3307/src_db' },
  targetEndpoint: { url: 'postgres://root:pw@10.0.0.2:5434/dst_db' },
  extractor: null,
  sinker: null,
  filter: null,
  router: null,
  parallelizer: null,
  pipeline: null,
  resumer: null,
  processor: null,
  runtime: null,
  metrics: {
    extractor_pushed_rps_avg: 120,
    replication_lag: 50,
    pipeline_buffer_size_avg: 1024,
    pipeline_sinked_count_latest: 5000,
  },
  resourceGroupId: 'rg-1',
  ownerUserId: 'u-1',
  status: 'running',
  createdAt: '2026-05-07T06:00:00.000Z',
  updatedAt: '2026-05-07T06:01:00.000Z',
};

describe('mapApiTask — Bug 4: SyncMode "full" must map to "snapshot"', () => {
  it('maps kind=snapshot → syncMode="snapshot" (not "full")', () => {
    const t = mapApiTask(BASE);
    expect(t.syncMode).toBe('snapshot' as SyncMode);
    expect(t.syncMode).not.toBe('full');
  });

  it('maps kind=check → syncMode="snapshot"', () => {
    const t = mapApiTask({ ...BASE, kind: 'check' });
    expect(t.syncMode).toBe('snapshot');
  });

  it('maps kind=struct → syncMode="snapshot"', () => {
    const t = mapApiTask({ ...BASE, kind: 'struct' });
    expect(t.syncMode).toBe('snapshot');
  });

  it('maps kind=cdc → syncMode="cdc"', () => {
    const t = mapApiTask({ ...BASE, kind: 'cdc' });
    expect(t.syncMode).toBe('cdc');
  });
});

describe('mapApiTask — Bug 5: ResumeType "auto" must map to "from_log"', () => {
  it('maps default resumeType to "from_log" (not "auto")', () => {
    const t = mapApiTask(BASE);
    expect(t.config.resumeType).toBe('from_log' as ResumeType);
    expect(t.config.resumeType).not.toBe('auto');
  });

  it('maps resumeType from resumer field when present', () => {
    const t = mapApiTask({
      ...BASE,
      resumer: { resume_type: 'from_target' },
    });
    expect(t.config.resumeType).toBe('from_target');
  });
});

describe('mapApiTask — Bug 3: progressPercent must be 0-100 percentage, not raw count', () => {
  it('calculates progressPercent as percentage when total is available', () => {
    const t = mapApiTask({
      ...BASE,
      metrics: {
        ...BASE.metrics!,
        pipeline_sinked_count_latest: 5000,
      },
      filter: { do_tbs: 'db1.t1,db1.t2,db1.t3,db1.t4,db1.t5', do_dbs: '' },
    });
    // With 5 tables and 5000 sinked count, progressPercent should be a
    // percentage (0-100), not the raw count 5000
    expect(t.progressPercent).toBeLessThanOrEqual(100);
    expect(t.progressPercent).toBeGreaterThanOrEqual(0);
    expect(t.progressPercent).not.toBe(5000);
  });

  it('returns 0 when no metrics are available', () => {
    const t = mapApiTask({ ...BASE, metrics: null });
    expect(t.progressPercent).toBe(0);
  });

  it('returns 0 when pipeline_sinked_count_latest is 0', () => {
    const t = mapApiTask({
      ...BASE,
      metrics: { pipeline_sinked_count_latest: 0 },
    });
    expect(t.progressPercent).toBe(0);
  });

  it('progressPercent is capped at 100', () => {
    const t = mapApiTask({
      ...BASE,
      metrics: {
        ...BASE.metrics!,
        pipeline_sinked_count_latest: 999999,
      },
      filter: { do_tbs: 'db1.t1', do_dbs: '' },
    });
    expect(t.progressPercent).toBeLessThanOrEqual(100);
  });
});
