import type { TaskFixture } from '@/types/domain';

function sinkTypeFor(task: TaskFixture): string {
  switch (task.kind) {
    case 'check':
      return 'check';
    case 'struct':
      return 'struct';
    default:
      return 'write';
  }
}

export function renderIni(task: TaskFixture): string {
  const lines: string[] = [];
  lines.push('[global]');
  lines.push(`task_id=${task.taskId}`);
  lines.push('');
  lines.push('[extractor]');
  lines.push(`db_type=${task.source.engine}`);
  lines.push(`extract_type=${task.extractType}`);
  lines.push(`url=${task.source.url}`);
  lines.push('');
  lines.push('[sinker]');
  lines.push(`db_type=${task.sink.engine}`);
  lines.push(`sink_type=${sinkTypeFor(task)}`);
  lines.push(`url=${task.sink.url}`);
  lines.push('');
  lines.push('[filter]');
  if (task.filter.doDbs?.length) lines.push(`do_dbs=${task.filter.doDbs.join(',')}`);
  if (task.filter.doTbs?.length) lines.push(`do_tbs=${task.filter.doTbs.join(',')}`);
  if (task.filter.ignoreDbs?.length) lines.push(`ignore_dbs=${task.filter.ignoreDbs.join(',')}`);
  if (task.filter.ignoreTbs?.length) lines.push(`ignore_tbs=${task.filter.ignoreTbs.join(',')}`);
  lines.push('');
  lines.push('[router]');
  if (task.router?.dbMap) {
    for (const [src, dst] of Object.entries(task.router.dbMap)) {
      lines.push(`db_map=${src}:${dst}`);
    }
  }
  lines.push('');
  lines.push('[parallelizer]');
  lines.push(`parallel_type=${task.parallelizer.type}`);
  lines.push(`parallel_size=${task.parallelizer.size}`);
  lines.push('');
  lines.push('[pipeline]');
  lines.push(`buffer_size=${task.pipeline.bufferSize}`);
  lines.push(`checkpoint_interval_secs=${task.pipeline.checkpointIntervalSecs}`);
  lines.push(`max_rps=${task.pipeline.maxRps}`);
  if (task.resumer) {
    lines.push('');
    lines.push('[resumer]');
    lines.push(`resume_type=${task.resumer.type}`);
  }
  if (task.processor?.luaCode || task.processor?.luaCodeFile) {
    lines.push('');
    lines.push('[processor]');
    if (task.processor.luaCode) lines.push(`lua_code=${task.processor.luaCode}`);
    if (task.processor.luaCodeFile) lines.push(`lua_code_file=${task.processor.luaCodeFile}`);
  }
  if (task.metrics) {
    lines.push('');
    lines.push('[metrics]');
    lines.push(`http_host=${task.metrics.httpHost}`);
    lines.push(`http_port=${task.metrics.httpPort}`);
  }
  return lines.join('\n') + '\n';
}
