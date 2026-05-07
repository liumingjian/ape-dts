/**
 * Task handlers: list / detail / create / update / delete / lifecycle actions,
 * plus test-connection, precheck, metrics timeseries, logs.
 */
import { http, HttpResponse } from 'msw';
import { pause, ok, notFound, badRequest, parsePage, paginate, q } from './_shared';
import { db, findTask, tasksOf } from '../db';
import type { Task, TaskCategory, TaskStatus, MetricSeries } from '@/types/domain';
import { legacyToCategory } from '@/types/domain';
import { id, intBetween, isoMinus, pick } from '../fake';

function listByCategoryParam(catParam: string, url: URL): Task[] {
  let items: Task[];
  if (catParam === 'sync') {
    items = [...tasksOf('snapshot'), ...tasksOf('cdc')];
  } else {
    const cat = legacyToCategory(catParam) as TaskCategory;
    items = [...tasksOf(cat)];
  }
  const status = q(url, 'status');
  const engine = q(url, 'engine');
  const rg = q(url, 'resourceGroup');
  const mode = q(url, 'mode');
  const key = q(url, 'q')?.toLowerCase();
  if (status) items = items.filter((t) => t.status === status);
  if (engine) items = items.filter((t) => t.source.engine === engine || t.target.engine === engine);
  if (rg) items = items.filter((t) => t.resourceGroup === rg);
  if (mode) items = items.filter((t) => t.syncMode === mode);
  if (key) items = items.filter((t) => t.name.toLowerCase().includes(key) || t.id.toLowerCase().includes(key));
  return items;
}

export const taskHandlers = [
  http.get('/api/tasks', async ({ request }) => {
    await pause();
    const url = new URL(request.url);
    const catParam = q(url, 'category') ?? 'snapshot';
    const items = listByCategoryParam(catParam, url);
    const { page, size } = parsePage(url);
    return ok(paginate(items, page, size));
  }),

  http.get('/api/tasks/:id', async ({ params }) => {
    await pause();
    const t = findTask(String(params.id));
    if (!t) return notFound();
    return ok(t);
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
        rpsLatest: 0, bpsLatest: 0, sinkerRpsLatest: 0, latencyMs: 0,
        queryRtUs: 0, bufferSize: 0, errorCount: 0, processedRecords: 0,
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
