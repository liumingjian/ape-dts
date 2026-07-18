/**
 * Task handlers: list / detail / create / update / delete / lifecycle actions,
 * plus test-connection, precheck, metrics timeseries, logs.
 */
import { http, HttpResponse } from 'msw';
import { pause, ok, notFound, badRequest, parsePage, paginate, q } from './_shared';
import { db, findTask, tasksOf } from '../db';
import type { Task, TaskCategory, TaskStatus, MetricSeries, Run, TaskDetailAggregate } from '@/types/domain';
import { legacyToCategory } from '@/types/domain';
import { maskConnectionStringPw } from '@/utils/localizeError';
import { id, intBetween, isoMinus, pick } from '../fake';

function taskRunId(taskId: string): string {
  return `run_${taskId}`;
}

function taskIdFromRunId(runId: string): string {
  return runId.startsWith('run_') ? runId.slice(4) : runId;
}

function latestRunForTask(task: Task): Run {
  return {
    id: taskRunId(task.id),
    taskId: task.id,
    status: task.status === 'paused' ? 'paused'
      : task.status === 'failed' ? 'failed'
      : task.status === 'stopped' ? 'stopped'
      : task.status === 'running' ? 'running'
      : 'stopped',
    startedAt: task.startedAt ?? task.createdAt,
    stoppedAt: task.completedAt ?? null,
    exitCode: task.status === 'failed' ? 1 : task.status === 'completed' ? 0 : null,
    logDir: `./logs/${task.id}`,
    iniPath: `./tasks/${task.id}.ini`,
    pid: task.status === 'running' ? intBetween(10_000, 60_000) : null,
    position: { kind: 'unknown', raw: task.status === 'running' ? 'live' : 'archived' },
    createdAt: task.createdAt,
  };
}

function buildRunMetricData(task: Task, metric: string, from: number, to: number, stepSeconds: number) {
  const taskSeries = db.metricsByTask[task.id];
  const canonicalMetric = metric === 'extractor_rps_avg' ? 'extractor_pushed_rps_avg' : metric;
  const existing = taskSeries?.[canonicalMetric]?.points
    ?? (metric === 'lag' ? taskSeries?.latency_ms?.points : undefined);
  if (existing?.length) {
    return existing
      .filter((point) => point.t >= from && point.t <= to)
      .map((point) => ({ ts: point.t, value: point.v }))
      .slice(-120);
  }
  const count = Math.max(12, Math.min(120, Math.round((to - from) / (stepSeconds * 1000))));
  const base = metric === 'lag' ? task.metrics.latencyMs : task.metrics.rpsLatest;
  return Array.from({ length: count }, (_, i) => {
    const ratio = count === 1 ? 1 : i / (count - 1);
    const wave = Math.sin(ratio * Math.PI * 2) * 0.16;
    return {
      ts: Math.round(from + ratio * (to - from)),
      value: Math.max(0, Math.round(base * (1 + wave))),
    };
  });
}

function listByCategoryParam(catParam: string, url: URL): Task[] {
  let items: Task[];
  if (catParam === 'sync' || catParam === 'migration') {
    items = [...tasksOf('snapshot'), ...tasksOf('cdc')];
  } else {
    const cat = legacyToCategory(catParam) as TaskCategory;
    items = [...tasksOf(cat)];
  }
  const status = q(url, 'status');
  const engine = q(url, 'engine');
  const rg = q(url, 'resource_group');
  const mode = q(url, 'mode');
  const key = q(url, 'q')?.toLowerCase();
  if (status) items = items.filter((t) => t.status === status);
  if (engine) items = items.filter((t) => t.source.engine === engine || t.target.engine === engine);
  if (rg) {
    const groupName = db.resourceGroups.find((group) => group.id === rg)?.name;
    items = items.filter((t) => t.resourceGroup === (groupName ?? rg));
  }
  if (mode) items = items.filter((t) => t.syncMode === mode);
  if (key) items = items.filter((t) => t.name.toLowerCase().includes(key) || t.id.toLowerCase().includes(key));
  return items;
}

export const taskHandlers = [
  http.get('/api/tasks', async ({ request }) => {
    await pause();
    const url = new URL(request.url);
    const catParam = q(url, 'category') ?? 'snapshot';
    let items = listByCategoryParam(catParam, url);
    const sort = q(url, 'sort');
    const order = q(url, 'order') === 'asc' ? 1 : -1;
    if (sort) {
      const values: Record<string, (task: Task) => string> = {
        name: (task) => task.name,
        engine: (task) => task.source.engine,
        status: (task) => task.status,
        kind: (task) => task.category,
        created_at: (task) => task.createdAt,
        updated_at: (task) => task.updatedAt,
      };
      const value = values[sort];
      if (value) items = [...items].sort((a, b) => value(a).localeCompare(value(b)) * order);
    }
    const page = Math.max(1, Number(url.searchParams.get('page') ?? 1));
    const pageSize = Math.max(1, Math.min(100, Number(url.searchParams.get('page_size') ?? 20)));
    const total = items.length;
    const start = (page - 1) * pageSize;
    return ok({ items: items.slice(start, start + pageSize), total, page, pageSize });
  }),

  http.get('/api/tasks/:id/detail', async ({ params }) => {
    await pause();
    const task = findTask(String(params.id));
    if (!task) return notFound();
    const run = latestRunForTask(task);
    const currentPhase = task.syncMode === 'cdc' ? 'cdc' : 'snapshot';
    const aggregate: TaskDetailAggregate = {
      task: {
        id: task.id,
        taskId: task.id,
        name: task.name,
        kind: task.category,
        dbTypeSource: task.source.engine,
        dbTypeTarget: task.target.engine,
        sourceEndpoint: { url: task.sourceUrl },
        targetEndpoint: { url: task.targetUrl },
        extractor: { extract_type: task.extractType },
        sinker: {}, filter: {}, router: {}, parallelizer: {}, pipeline: {}, resumer: {}, processor: {}, runtime: {}, metrics: {},
        resourceGroupId: task.resourceGroup,
        ownerUserId: '',
        status: task.status,
        createdAt: task.createdAt,
        updatedAt: task.updatedAt,
        configuredExtractType: task.extractType,
        selectedObjects: [],
      },
      currentRun: {
        id: run.id,
        status: run.status === 'orphaned' ? 'failed' : run.status,
        currentPhase,
        startedAt: run.startedAt,
        stoppedAt: run.stoppedAt,
        exitCode: run.exitCode,
        checkpoint: run.position,
      },
      phases: {
        snapshot: { status: currentPhase === 'snapshot' ? 'running' : 'skipped', startedAt: run.startedAt, completedAt: null },
        transitioning_to_cdc: { status: 'skipped', startedAt: null, completedAt: null },
        cdc: { status: currentPhase === 'cdc' ? 'running' : 'skipped', startedAt: currentPhase === 'cdc' ? run.startedAt : null, completedAt: null },
      },
      metricsSnapshot: {
        runId: run.id,
        phase: currentPhase,
        sampledAt: new Date().toISOString(),
        values: {
          extractor_rps_avg: task.metrics.rpsLatest,
          sinker_rps_avg: task.metrics.sinkerRpsLatest,
          pipeline_queue_size: task.metrics.pipelineQueueSize,
          ...(currentPhase === 'cdc' ? { lag: task.metrics.lag } : {}),
        },
      },
      progress: currentPhase === 'snapshot' ? {
        runId: run.id,
        phase: 'snapshot',
        kind: 'snapshot',
        percent: task.progressPercent,
        copiedRecords: null,
        estimatedTotalRecords: null,
        totalIsEstimate: false,
      } : null,
    };
    return ok(aggregate);
  }),

  http.get('/api/tasks/:id', async ({ params }) => {
    await pause();
    const t = findTask(String(params.id));
    if (!t) return notFound();
    return ok(t);
  }),

  http.get('/api/tasks/:id/runs', async ({ params, request }) => {
    await pause();
    const t = findTask(String(params.id));
    if (!t) return notFound();
    const url = new URL(request.url);
    const { page, size } = parsePage(url);
    return ok(paginate([latestRunForTask(t)], page, size));
  }),

  http.post('/api/tasks', async ({ request }) => {
    await pause(400, 900);
    const body = (await request.json().catch(() => ({}))) as Partial<Task> & { category?: TaskCategory | 'sync' | 'replay' | 'verify' };
    if (!body?.name || !body?.source || !body?.target) {
      return badRequest('missing_required_fields');
    }
    const category: TaskCategory = legacyToCategory(body.category ?? 'snapshot');
    const now = new Date().toISOString();
    const task: Task = {
      id: id(category),
      name: body.name,
      description: body.description ?? '',
      category,
      status: 'creating',
      source: body.source,
      target: body.target,
      sourceUrl: buildMaskedUrl(body.source),
      targetUrl: buildMaskedUrl(body.target),
      syncMode: body.syncMode ?? 'snapshot_cdc',
      extractType: body.extractType ?? (category === 'check' ? 'snapshot' : category === 'struct' ? 'struct' : category === 'cdc' ? 'cdc' : 'snapshot_and_cdc'),
      taskType: body.taskType ?? 'standalone',
      resourceGroup: body.resourceGroup ?? 'default',
      instanceIp: body.instanceIp ?? '127.0.0.1',
      progressPercent: 0,
      syncObjects: body.syncObjects ?? { totalTables: 0, selectedTables: 0 },
      config: body.config ?? {
        parallelizer: 'snapshot', parallelSize: 4, bufferSize: 16_000,
        maxRps: 0, checkpointIntervalSecs: 10, resumeType: 'from_log', metricsEnabled: true,
      },
      createdAt: now,
      updatedAt: now,
      startedAt: undefined,
      completedAt: undefined,
      metrics: {
        rpsLatest: 0, bpsLatest: 0, sinkerRpsLatest: 0, latencyMs: 0, lag: 0,
        queryRtUs: 0, bufferSize: 0, errorCount: 0, processedRecords: 0,
        pipelineQueueSize: 0, finishedProgressCount: 0, totalProgressCount: 0,
      },
      lastHeartbeatAt: now,
    };
    tasksOf(category).unshift(task);
    // Simulate transitions: creating → running in 1.2s
    setTimeout(() => {
      const cur = findTask(task.id);
      if (cur && cur.status === 'creating') {
        cur.status = 'running';
        cur.startedAt = new Date().toISOString();
        cur.metrics.rpsLatest = intBetween(200, 900);
        cur.metrics.sinkerRpsLatest = Math.round(cur.metrics.rpsLatest * 0.95);
        cur.metrics.latencyMs = intBetween(120, 800);
      }
    }, 1200);
    return ok(task);
  }),

  http.patch('/api/tasks/:id', async ({ params, request }) => {
    await pause();
    const t = findTask(String(params.id));
    if (!t) return notFound();
    const patch = (await request.json().catch(() => ({}))) as Partial<Task>;
    Object.assign(t, patch, { updatedAt: new Date().toISOString() });
    return ok(t);
  }),

  http.delete('/api/tasks/:id', async ({ params }) => {
    await pause();
    const target = String(params.id);
    for (const list of [db.snapshotTasks, db.cdcTasks, db.checkTasks, db.structTasks]) {
      const idx = list.findIndex((t) => t.id === target);
      if (idx >= 0) {
        list.splice(idx, 1);
        return ok({ ok: true });
      }
    }
    return notFound();
  }),

  http.post('/api/tasks/:id/action', async ({ params, request }) => {
    await pause();
    const t = findTask(String(params.id));
    if (!t) return notFound();
    const body = (await request.json().catch(() => ({}))) as { action?: string };
    const action = body.action;
    const now = new Date().toISOString();
    const transition: Record<string, TaskStatus> = {
      start: 'running', resume: 'running', pause: 'paused', stop: 'stopped',
      retry: 'running', fail: 'failed', complete: 'completed',
    };
    if (action && transition[action]) {
      t.status = transition[action];
      t.updatedAt = now;
      if (transition[action] === 'running' && !t.startedAt) t.startedAt = now;
      if (transition[action] === 'completed') {
        t.completedAt = now;
        t.progressPercent = 100;
      }
    }
    db.controlLogs.unshift({
      id: id('ctrl'),
      at: now,
      taskId: t.id,
      taskName: t.name,
      action: (action as any) ?? 'edit',
      operator: 'admin',
      result: 'success',
      detail: `action=${action}`,
    });
    return ok(t);
  }),

  /* Test source/target connectivity — ~85% success */
  http.post('/api/tasks/test-connection', async ({ request }) => {
    await pause(500, 1400);
    const body = (await request.json().catch(() => ({}))) as { endpoint?: Record<string, unknown> };
    const endpoint = body.endpoint;
    if (!endpoint?.host || !endpoint?.port) return badRequest('endpoint_incomplete');
    const success = Math.random() > 0.15;
    if (!success) {
      return HttpResponse.json({
        ok: false,
        latencyMs: 0,
        message: pick(['connection refused', 'authentication failed', 'timeout after 5s', 'handshake error']),
      }, { status: 200 });
    }
    return ok({
      ok: true,
      latencyMs: intBetween(12, 240),
      version: pick(['8.0.36', '15.4', '19c', '7.4', 'v6.5']),
      serverId: pick(['1001', '1002', '1003', '2001']),
    });
  }),

  /* Precheck — multi-item checklist; one warning for flavor */
  http.post('/api/tasks/precheck', async () => {
    await pause(800, 1600);
    const allPass = Math.random() > 0.2;
    const items = [
      { key: 'source_connectivity', title: '源端连通性', result: 'pass', detail: '' },
      { key: 'source_privilege', title: '源端权限校验', result: 'pass', detail: '' },
      { key: 'source_binlog', title: 'Binlog/WAL 开启情况', result: 'pass', detail: '' },
      { key: 'target_connectivity', title: '目标端连通性', result: 'pass', detail: '' },
      { key: 'target_privilege', title: '目标端权限校验', result: 'pass', detail: '' },
      { key: 'schema_compat', title: '库表结构兼容性', result: allPass ? 'pass' : 'warn', detail: allPass ? '' : '源端 decimal(38,10) 建议映射为 numeric(38,10)' },
      { key: 'object_dep', title: '视图/触发器/外键引用', result: 'pass', detail: '' },
      { key: 'resource_group', title: '资源组容量', result: 'pass', detail: '' },
      { key: 'license', title: 'License 任务数', result: 'pass', detail: '' },
    ];
    return ok({
      finishedAt: new Date().toISOString(),
      pass: items.every((i) => i.result === 'pass'),
      items,
    });
  }),

  /* Timeseries metrics for detail page charts. */
  http.get('/api/tasks/:id/metrics', async ({ params, request }) => {
    await pause();
    const t = findTask(String(params.id));
    if (!t) return notFound();
    const url = new URL(request.url);
    const group = db.metricsByTask[t.id] ?? {};
    const requested = q(url, 'metrics')?.split(',').filter(Boolean);
    const out: MetricSeries[] = [];
    for (const [metric, s] of Object.entries(group)) {
      if (requested && !requested.includes(metric)) continue;
      out.push(s);
    }
    return ok({ taskId: t.id, series: out, snapshot: t.metrics });
  }),

  http.get('/api/runs/:id/metrics', async ({ params, request }) => {
    await pause();
    const runId = String(params.id);
    const t = findTask(taskIdFromRunId(runId));
    if (!t) return notFound();
    const url = new URL(request.url);
    const metric = q(url, 'metric') ?? 'extractor_rps_avg';
    const now = Date.now();
    const from = Number(q(url, 'from') ?? now - 3600_000);
    const to = Number(q(url, 'to') ?? now);
    const step = Number(q(url, 'step') ?? 60);
    return ok({
      metric,
      data: buildRunMetricData(t, metric, from, to, step),
    });
  }),

  http.get('/api/runs/:id/metrics/latest', async ({ params }) => {
    await pause();
    const t = findTask(taskIdFromRunId(String(params.id)));
    if (!t) return notFound();
    return ok({
      extractor_rps_avg: t.metrics.rpsLatest,
      sinker_rps_avg: t.metrics.sinkerRpsLatest,
      sinker_rt_avg: t.metrics.queryRtUs,
      pipeline_queue_size: t.metrics.pipelineQueueSize,
      sinker_sinked_records: t.metrics.processedRecords,
      extractor_plan_records: t.metrics.totalProgressCount,
      progress: t.progressPercent,
      lag: t.metrics.lag,
    });
  }),

  http.get('/api/runs/:id/objects', async ({ params }) => {
    await pause();
    const t = findTask(taskIdFromRunId(String(params.id)));
    if (!t) return notFound();
    const tableCount = Math.max(1, Math.min(6, t.syncObjects.selectedTables));
    return ok(Array.from({ length: tableCount }, (_, index) => ({
      schema: t.source.database || 'source',
      table: `table_${index + 1}`,
      state: t.status === 'completed' ? 'completed' : index === 0 ? 'loading' : 'pending',
    })));
  }),

  http.get('/api/tasks/:id/logs', async ({ params, request }) => {
    await pause();
    const t = findTask(String(params.id));
    if (!t) return notFound();
    const url = new URL(request.url);
    const level = q(url, 'level');
    const lines: { t: string; level: string; source: string; message: string }[] = [];
    const templates = [
      { level: 'INFO', source: 'ape-dts-engine', message: 'snapshot batch finished, rows=%N% elapsed=%E%ms' },
      { level: 'INFO', source: 'ape-dts-engine', message: 'checkpoint committed at offset=%O%' },
      { level: 'WARN', source: 'ape-dts-extractor', message: 'source table %T% column charset mismatch, falling back to utf8' },
      { level: 'INFO', source: 'ape-dts-sinker', message: 'batched %N% records into target in %E%ms' },
      { level: 'ERROR', source: 'ape-dts-sinker', message: 'retrying failed write for table %T% attempt=%N%' },
      { level: 'DEBUG', source: 'ape-dts-resumer', message: 'resume state updated, position=%O%' },
    ];
    const count = 80;
    for (let i = 0; i < count; i++) {
      const tpl = pick(templates);
      lines.push({
        t: isoMinus(i * 3),
        level: tpl.level,
        source: tpl.source,
        message: tpl.message
          .replace('%N%', String(intBetween(10, 5000)))
          .replace('%E%', String(intBetween(10, 800)))
          .replace('%O%', `0x${intBetween(1_000_000, 10_000_000).toString(16)}`)
          .replace('%T%', pick(['orders', 'users', 'payments', 'logs'])),
      });
    }
    const filtered = level ? lines.filter((l) => l.level === level) : lines;
    return ok({ taskId: t.id, lines: filtered });
  }),

  /* Preview INI config for wizard confirm step. */
  http.post('/api/tasks/preview-ini', async ({ request }) => {
    await pause(300, 700);
    const body = (await request.json().catch(() => ({}))) as Partial<Task>;
    const ini = buildIni(body);
    return ok({ ini });
  }),
];

/** Build a masked connection URL from endpoint fields. */
function buildMaskedUrl(ep: Task['source'] | undefined): string {
  if (!ep) return '';
  const schemeMap: Record<string, string> = {
    mysql: 'mysql', postgres: 'postgres', gaussdb: 'postgres', oracle: 'oracle',
    mongo: 'mongodb', redis: 'redis', kafka: 'kafka', tidb: 'mysql',
    starrocks: 'mysql', clickhouse: 'clickhouse', doris: 'mysql', foxlake: 'postgres',
  };
  const scheme = schemeMap[ep.engine] ?? 'mysql';
  const raw = `${scheme}://${ep.username}:${ep.password}@${ep.host}:${ep.port}${ep.database ? `/${ep.database}` : ''}`;
  return maskConnectionStringPw(raw);
}

/** Render task into ape-dts task.ini flavor. */
function buildIni(t: Partial<Task>): string {
  const src = t.source ?? ({} as any);
  const tgt = t.target ?? ({} as any);
  const cfg = t.config ?? ({} as any);
  const L = (lines: string[]) => lines.filter(Boolean).join('\n');
  return L([
    '[extractor]',
    `db_type=${src.engine ?? 'mysql'}`,
    `extract_type=${t.syncMode ?? 'snapshot_cdc'}`,
    `url=${src.engine ?? 'mysql'}://${src.username ?? 'root'}:***@${src.host ?? 'localhost'}:${src.port ?? 3306}/${src.database ?? ''}`,
    '',
    '[sinker]',
    `db_type=${tgt.engine ?? 'postgres'}`,
    `sink_type=write`,
    `url=${tgt.engine ?? 'postgres'}://${tgt.username ?? 'root'}:***@${tgt.host ?? 'localhost'}:${tgt.port ?? 5432}/${tgt.database ?? ''}`,
    `batch_size=200`,
    '',
    '[filter]',
    `do_dbs=${src.database ?? '*'}`,
    `do_tbs=*.*`,
    `do_events=insert,update,delete`,
    '',
    '[router]',
    `db_map=`,
    `tb_map=`,
    `col_map=`,
    '',
    '[parallelizer]',
    `parallel_type=${cfg.parallelizer ?? 'snapshot'}`,
    `parallel_size=${cfg.parallelSize ?? 4}`,
    '',
    '[pipeline]',
    `buffer_size=${cfg.bufferSize ?? 16000}`,
    `max_rps_per_sinker=${cfg.maxRps ?? 0}`,
    `checkpoint_interval_secs=${cfg.checkpointIntervalSecs ?? 10}`,
    '',
    '[resumer]',
    `resume_from=${cfg.resumeType ?? 'from_log'}`,
    '',
    '[runtime]',
    `log_level=info`,
    `log_dir=./logs`,
    '',
    '[metrics]',
    `enabled=${cfg.metricsEnabled ?? true}`,
    `http_port=9090`,
  ]);
}
