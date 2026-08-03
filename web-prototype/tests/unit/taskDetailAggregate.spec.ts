import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';

const source = readFileSync(resolve(process.cwd(), 'src/views/tasks/TaskDetail.vue'), 'utf8');

describe('TaskDetail aggregate contract', () => {
  it('polls one authoritative task detail aggregate', () => {
    expect(source).toContain('`/tasks/${taskId.value}/detail`');
    expect(source).not.toContain('`/runs/${currentRunId.value}/metrics/latest`');
    expect(source).not.toContain('loadCurrentRunId');
  });

  it('does not use Task compatibility metrics or progress as runtime truth', () => {
    expect(source).not.toMatch(/task\.value\?\.progressPercent/);
    expect(source).not.toMatch(/task\.metrics\.(rpsLatest|lag|pipelineQueueSize)/);
    expect(source).toContain("progress.value?.phase === 'snapshot'");
    expect(source).toContain("currentPhase === 'cdc'");
  });

  it('renders the production diagnostic contract for aggregate failure', () => {
    expect(source).toContain('detailError.code');
    expect(source).toContain('detailError.message');
    expect(source).toContain('detailError.status');
    expect(source).toContain('detailError.requestId');
    expect(source).toContain('lastDetailRefresh');
    expect(source).toContain('copyDetailDiagnostics');
  });

  it('uses canonical metric names', () => {
    expect(source).toContain("'extractor_rps_avg'");
    expect(source).toContain("'sinker_rps_avg'");
    expect(source).toContain("'pipeline_queue_size'");
    expect(source).not.toContain('sinker_record_count_avg_by_sec');
    expect(source).not.toContain('pipeline_buffer_size_avg');
    expect(source).not.toContain('sinker_rt_per_query_avg');
  });
});
