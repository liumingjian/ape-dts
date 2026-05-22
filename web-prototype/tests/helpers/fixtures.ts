export type {
  EndpointFixture,
  TaskFixture,
} from '@/types/domain';

export type { Role, Action } from '@/auth/permissions';

export type { TimeSeriesPoint } from '@/types/domain';

import type { TaskFixture, TimeSeriesPoint } from '@/types/domain';

export const minimalSnapshotTask: TaskFixture = {
  taskId: 'snapshot_task_1',
  kind: 'snapshot',
  extractType: 'snapshot',
  source: { engine: 'mysql', url: '*******************************/src' },
  sink: { engine: 'mysql', url: '*******************************/dst' },
  filter: { doDbs: ['app'] },
  parallelizer: { type: 'snapshot', size: 8 },
  pipeline: { bufferSize: 16000, checkpointIntervalSecs: 10, maxRps: 1000 },
  resumer: { type: 'from_log' },
  metrics: { httpHost: '127.0.0.1', httpPort: 9090 },
};

export const minimalCdcTask: TaskFixture = {
  ...minimalSnapshotTask,
  taskId: 'cdc_task_1',
  kind: 'cdc',
  extractType: 'cdc',
  parallelizer: { type: 'rdb_merge', size: 4 },
};

export const gaussdbOracleCdcTask: TaskFixture = {
  ...minimalCdcTask,
  taskId: 'gaussdb_oracle_cdc_1',
  source: {
    engine: 'gaussdb',
    subMode: 'oracle-mode',
    url: '*****************************/ORCL',
  },
  sink: { engine: 'oracle', url: '****************************/ORCL' },
};

export const minimalStructTask: TaskFixture = {
  ...minimalSnapshotTask,
  taskId: 'struct_task_1',
  kind: 'struct',
  extractType: 'struct',
  parallelizer: { type: 'serial', size: 1 },
};

export function makeSeries(
  start: number,
  count: number,
  stepMs: number,
  fn: (i: number) => number,
): TimeSeriesPoint[] {
  return Array.from({ length: count }, (_, i) => ({
    ts: start + i * stepMs,
    value: fn(i),
  }));
}
