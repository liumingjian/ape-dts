import { describe, it, expect } from 'vitest';
import { mapApiTask, type ApiTask } from '@/types/domain';

const BASE: ApiTask = {
  id: 'aaa-bbb-ccc',
  taskId: 'snapshot_mysql_mysql_aaa',
  name: 'test-snapshot',
  kind: 'snapshot',
  dbTypeSource: 'mysql',
  dbTypeTarget: 'postgres',
  sourceEndpoint: { url: 'mysql://root:secret@10.0.0.1:3307/src_db' },
  targetEndpoint: { url: 'postgres://pguser:pwd@10.0.0.2:5434/dst_db' },
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
    lag: 50,
    pipeline_buffer_size_avg: 1024,
    pipeline_sinked_count_latest: 5000,
    progress: 73,
  },
  resourceGroupId: 'rg-1',
  ownerUserId: 'u-1',
  status: 'running',
  createdAt: '2026-05-07T06:00:00.000Z',
  updatedAt: '2026-05-07T06:01:00.000Z',
};

describe('mapApiTask', () => {
  it('maps kind → category', () => {
    const t = mapApiTask(BASE);
    expect(t.category).toBe('snapshot');
  });

  it('maps kind=cdc → category=cdc, syncMode=cdc', () => {
    const t = mapApiTask({ ...BASE, kind: 'cdc' });
    expect(t.category).toBe('cdc');
    expect(t.syncMode).toBe('cdc');
  });

  it('parses sourceEndpoint URL into source.host/port/database', () => {
    const t = mapApiTask(BASE);
    expect(t.source.engine).toBe('mysql');
    expect(t.source.host).toBe('10.0.0.1');
    expect(t.source.port).toBe(3307);
    expect(t.source.database).toBe('src_db');
  });

  it('parses targetEndpoint URL into target fields', () => {
    const t = mapApiTask(BASE);
    expect(t.target.engine).toBe('postgres');
    expect(t.target.host).toBe('10.0.0.2');
    expect(t.target.port).toBe(5434);
  });

  it('normalizes gaussdb_oracle API db type to GaussDB UI engine', () => {
    const t = mapApiTask({
      ...BASE,
      dbTypeSource: 'gaussdb_oracle',
      dbTypeTarget: 'gaussdb_oracle',
    });
    expect(t.source.engine).toBe('gaussdb');
    expect(t.target.engine).toBe('gaussdb');
  });

  it('maps metrics when present', () => {
    const t = mapApiTask(BASE);
    expect(t.metrics.rpsLatest).toBe(120);
    expect(t.metrics.lag).toBe(50);
    expect(t.metrics.bufferSize).toBe(1024);
    expect(t.metrics.processedRecords).toBe(5000);
  });

  it('defaults metrics to 0 when null', () => {
    const t = mapApiTask({ ...BASE, metrics: null });
    expect(t.metrics.rpsLatest).toBe(0);
    expect(t.metrics.lag).toBe(0);
  });

  it('handles empty/invalid endpoint URLs gracefully', () => {
    const t = mapApiTask({ ...BASE, sourceEndpoint: { url: '' }, targetEndpoint: { url: '' } });
    expect(t.source.host).toBe('');
    expect(t.target.host).toBe('');
  });

  it('maps status string directly', () => {
    const t = mapApiTask({ ...BASE, status: 'failed' });
    expect(t.status).toBe('failed');
  });

  it('preserves id, name, createdAt, updatedAt', () => {
    const t = mapApiTask(BASE);
    expect(t.id).toBe('aaa-bbb-ccc');
    expect(t.name).toBe('test-snapshot');
    expect(t.createdAt).toBe('2026-05-07T06:00:00.000Z');
    expect(t.updatedAt).toBe('2026-05-07T06:01:00.000Z');
  });

  it('progress passes through verbatim', () => {
    const t = mapApiTask(BASE);
    expect(t.progressPercent).toBe(73);
  });

  it('progress defaults to 0 when missing', () => {
    const t = mapApiTask({ ...BASE, metrics: null });
    expect(t.progressPercent).toBe(0);
    const t2 = mapApiTask({ ...BASE, metrics: { progress: 0 } });
    expect(t2.progressPercent).toBe(0);
  });

  it('lag passes through verbatim including float 3.5', () => {
    const t = mapApiTask({ ...BASE, metrics: { lag: 12 } });
    expect(t.metrics.lag).toBe(12);
    const t2 = mapApiTask({ ...BASE, metrics: { lag: 3.5 } });
    expect(t2.metrics.lag).toBe(3.5);
  });

  it('lag defaults to 0 when missing', () => {
    const t = mapApiTask({ ...BASE, metrics: null });
    expect(t.metrics.lag).toBe(0);
    const t2 = mapApiTask({ ...BASE, metrics: {} });
    expect(t2.metrics.lag).toBe(0);
  });

  it('pipelineQueueSize passes through verbatim', () => {
    const t = mapApiTask({ ...BASE, metrics: { pipeline_queue_size: 12 } });
    expect(t.metrics.pipelineQueueSize).toBe(12);
  });

  it('pipelineQueueSize defaults to 0 when missing', () => {
    const t = mapApiTask({ ...BASE, metrics: null });
    expect(t.metrics.pipelineQueueSize).toBe(0);
  });

  it('finishedProgressCount and totalProgressCount pass through', () => {
    const t = mapApiTask({ ...BASE, metrics: { finished_progress_count: 3, total_progress_count: 8 } });
    expect(t.metrics.finishedProgressCount).toBe(3);
    expect(t.metrics.totalProgressCount).toBe(8);
  });

  it('finishedProgressCount and totalProgressCount default to 0', () => {
    const t = mapApiTask({ ...BASE, metrics: null });
    expect(t.metrics.finishedProgressCount).toBe(0);
    expect(t.metrics.totalProgressCount).toBe(0);
  });
});
