/**
 * Dashboard summary — one aggregation endpoint that the Dashboard page polls.
 */
import { http } from 'msw';
import { pause, ok } from './_shared';
import { db, allTasks } from '../db';
import type {
  DashboardSummary, MetricSeries, TaskStatus, EngineType, DashboardTopTask,
  ActivityEvent, ActivityEventType, ActivityEventTone,
} from '@/types/domain';

const SPARK_LEN = 32;

function tail(values: number[], n: number): number[] {
  return values.slice(-n);
}

function buildSpark(seed: number, base: number, amp: number): number[] {
  const out: number[] = [];
  for (let i = 0; i < SPARK_LEN; i++) {
    const phase = ((i + seed) / SPARK_LEN) * Math.PI * 3;
    const sinu = Math.sin(phase) * amp;
    const noise = (Math.random() - 0.5) * amp * 0.5;
    out.push(Math.max(0, Math.round(base + sinu + noise)));
  }
  return out;
}

export const dashboardHandlers = [
  http.get('/api/dashboard/summary', async () => {
    await pause();

    const all = allTasks();

    const statusCounts: Record<TaskStatus, number> = {
      draft: 0, ready: 0, running: 0, paused: 0, stopping: 0, stopped: 0, failed: 0, completed: 0, creating: 0, pending: 0,
    };
    const engineCounts: Partial<Record<EngineType, number>> = {};
    for (const t of all) {
      statusCounts[t.status] = (statusCounts[t.status] ?? 0) + 1;
      engineCounts[t.source.engine] = (engineCounts[t.source.engine] ?? 0) + 1;
    }

    const totalRps = all
      .filter((t) => t.status === 'running')
      .reduce((s, t) => s + (t.metrics.rpsLatest ?? 0), 0);

    const runningTasks = all.filter((t) => t.status === 'running');
    const avgLatency = runningTasks.length
      ? Math.round(runningTasks.reduce((s, t) => s + (t.metrics.latencyMs ?? 0), 0) / runningTasks.length)
      : 0;

    /* Build last-hour aggregates (60 points) from per-task series for dashboard charts. */
    const topRunning = runningTasks
      .slice()
      .sort((a, b) => (b.metrics.rpsLatest ?? 0) - (a.metrics.rpsLatest ?? 0))
      .slice(0, 4);

    const take60 = (s?: MetricSeries): MetricSeries | null => {
      if (!s) return null;
      const pts = s.points.slice(-60);
      return { taskId: s.taskId, metric: s.metric, points: pts };
    };

    const rpsSeries = topRunning
      .map((t) => take60(db.metricsByTask[t.id]?.extractor_pushed_rps_avg))
      .filter(Boolean) as MetricSeries[];

    const latencySeries = topRunning
      .map((t) => take60(db.metricsByTask[t.id]?.latency_ms))
      .filter(Boolean) as MetricSeries[];

    /* Top-5 running tasks list (sparkline + headline RPS / latency) for the
     * dashboard right rail. Falls back to synthesized series for tasks that
     * weren't pre-seeded with metrics so the panel never collapses. */
    const top5Running = runningTasks
      .slice()
      .sort((a, b) => (b.metrics.rpsLatest ?? 0) - (a.metrics.rpsLatest ?? 0))
      .slice(0, 5);
    const topRunningTasks: DashboardTopTask[] = top5Running.map((t, idx) => {
      const rpsPts = db.metricsByTask[t.id]?.extractor_pushed_rps_avg?.points ?? [];
      const spark = rpsPts.length
        ? tail(rpsPts.map((p) => p.v), SPARK_LEN)
        : buildSpark(idx * 7, t.metrics.rpsLatest || 200, (t.metrics.rpsLatest || 200) * 0.3);
      return {
        id: t.id,
        name: t.name,
        category: t.category,
        status: t.status,
        sourceEngine: t.source.engine,
        targetEngine: t.target.engine,
        rps: t.metrics.rpsLatest,
        latencyMs: t.metrics.latencyMs,
        spark,
      };
    });

    const alertTrend: DashboardSummary['alertTrend'] = [];
    const now = new Date();
    for (let d = 13; d >= 0; d--) {
      const date = new Date(now.getTime() - d * 86400_000).toISOString().slice(0, 10);
      const critical = Math.max(0, Math.round(2 + (Math.random() - 0.5) * 4));
      const major = Math.max(0, Math.round(4 + (Math.random() - 0.5) * 6));
      const minor = Math.max(0, Math.round(7 + (Math.random() - 0.5) * 8));
      const info = Math.max(0, Math.round(3 + (Math.random() - 0.5) * 4));
      alertTrend.push({ date, critical, major, minor, info });
    }

    const todayAlerts = db.activeAlerts.length
      + db.historyAlerts.filter((a) => {
        const aged = Date.now() - new Date(a.firstAt).getTime();
        return aged < 86400_000;
      }).length;

    /* Recent events — unified activity timeline merging task lifecycle,
     * alert trips, and license/system signals. Sorted newest first. */
    const recentEvents = buildRecentEvents();

    /* KPI sparklines — drive the inline chart inside each top KPI card. We
     * synthesize stable shapes so first paint is always populated. */
    const kpiSparks: DashboardSummary['kpiSparks'] = {
      running: buildSpark(11, statusCounts.running || 8, Math.max(2, (statusCounts.running || 8) * 0.2)),
      todayAlerts: buildSpark(23, todayAlerts || 6, Math.max(2, (todayAlerts || 6) * 0.4)),
      totalRps: buildSpark(37, totalRps || 1500, (totalRps || 1500) * 0.25),
      avgLatencyMs: buildSpark(53, avgLatency || 800, (avgLatency || 800) * 0.3),
    };

    const payload: DashboardSummary = {
      kpi: {
        running: {
          total: statusCounts.running,
          delta: Math.round(statusCounts.running * 0.1 * (Math.random() - 0.3)),
        },
        todayAlerts: {
          total: todayAlerts,
          delta: Math.round(todayAlerts * 0.15 * (Math.random() - 0.5)),
        },
        totalRps: {
          value: totalRps,
          delta: Math.round(totalRps * 0.08 * (Math.random() - 0.3)),
        },
        avgLatencyMs: {
          value: avgLatency,
          delta: Math.round(avgLatency * 0.1 * (Math.random() - 0.5)),
        },
      },
      kpiSparks,
      rpsSeries,
      latencySeries,
      statusDist: (Object.keys(statusCounts) as TaskStatus[])
        .filter((s) => statusCounts[s] > 0)
        .map((s) => ({ status: s, count: statusCounts[s] })),
      engineDist: Object.entries(engineCounts)
        .map(([engine, count]) => ({ engine: engine as EngineType, count: count! }))
        .sort((a, b) => b.count - a.count),
      alertTrend,
      recentTasks: [...all]
        .sort((a, b) => b.updatedAt.localeCompare(a.updatedAt))
        .slice(0, 5),
      topRunningTasks,
      recentEvents,
      topAlerts: db.activeAlerts
        .slice()
        .sort((a, b) => {
          const order: Record<string, number> = { critical: 0, major: 1, minor: 2, info: 3 };
          return order[a.level] - order[b.level];
        })
        .slice(0, 5),
      licenseWarnCount: db.licenses.filter((l) => l.status === 'expiring' || l.status === 'expired').length,
    };

    return ok(payload);
  }),

  http.get('/api/ping', () => ok({ ok: true, ts: Date.now() })),
];

/* Compose a unified activity feed (≈ 20 events, newest first) by merging
 * task lifecycle transitions, alert trips, and license / system signals.
 * Mirrors what an orchestrator audit log would surface. */
function buildRecentEvents(): ActivityEvent[] {
  const all = allTasks();
  const events: ActivityEvent[] = [];
  const now = Date.now();

  const TASK_RECIPES: Array<{
    type: ActivityEventType;
    tone: ActivityEventTone;
    pickStatus?: TaskStatus;
    titleZh: (name: string) => string;
    descZh?: (rps: number) => string;
  }> = [
    {
      type: 'task.failed',
      tone: 'danger',
      pickStatus: 'failed',
      titleZh: (n) => `任务失败：${n}`,
      descZh: () => '同步链路异常，已切换为告警状态',
    },
    {
      type: 'task.started',
      tone: 'info',
      pickStatus: 'running',
      titleZh: (n) => `任务启动：${n}`,
      descZh: (rps) => `首批数据已开始下发，初始 RPS ≈ ${formatShortLocal(rps)}`,
    },
    {
      type: 'task.completed',
      tone: 'success',
      pickStatus: 'completed',
      titleZh: (n) => `任务完成：${n}`,
      descZh: () => '全量同步阶段已结束',
    },
    {
      type: 'task.paused',
      tone: 'warning',
      pickStatus: 'paused',
      titleZh: (n) => `任务暂停：${n}`,
      descZh: () => '运维手动暂停以保障目标库压力',
    },
  ];

  let cursor = 30_000; // start 30s ago
  for (const recipe of TASK_RECIPES) {
    const candidates = recipe.pickStatus
      ? all.filter((t) => t.status === recipe.pickStatus)
      : all;
    if (!candidates.length) continue;
    const picks = candidates.slice(0, 3);
    for (const t of picks) {
      cursor += Math.round(60_000 + Math.random() * 600_000);
      events.push({
        id: `evt-${recipe.type}-${t.id}`,
        type: recipe.type,
        category: 'task',
        tone: recipe.tone,
        title: recipe.titleZh(t.name),
        description: recipe.descZh?.(t.metrics.rpsLatest),
        taskId: t.id,
        taskName: t.name,
        taskCategory: t.category,
        sourceEngine: t.source.engine,
        targetEngine: t.target.engine,
        occurredAt: new Date(now - cursor).toISOString(),
      });
    }
  }

  for (const a of db.activeAlerts.slice(0, 4)) {
    cursor += Math.round(60_000 + Math.random() * 300_000);
    const tone: ActivityEventTone = a.level === 'critical' || a.level === 'major' ? 'danger' : 'warning';
    events.push({
      id: `evt-alert-${a.id}`,
      type: 'alert.triggered',
      category: 'alert',
      tone,
      title: `告警触发：${a.message}`,
      description: `${a.taskName} · ${a.service} · ×${a.count}`,
      taskId: a.taskId,
      taskName: a.taskName,
      alertLevel: a.level,
      occurredAt: a.lastAt,
    });
  }

  const expiring = db.licenses.filter((l) => l.status === 'expiring' || l.status === 'expired').slice(0, 2);
  for (const lic of expiring) {
    cursor += Math.round(120_000 + Math.random() * 600_000);
    events.push({
      id: `evt-license-${lic.id}`,
      type: 'license.expiring',
      category: 'system',
      tone: 'warning',
      title: 'License 即将过期',
      description: `${lic.issuedTo} 将于 ${lic.expireAt?.slice(0, 10) ?? '近期'} 过期`,
      occurredAt: new Date(now - cursor).toISOString(),
    });
  }

  events.push({
    id: 'evt-system-config-1',
    type: 'system.deploy',
    category: 'system',
    tone: 'neutral',
    title: '全局参数已更新',
    description: 'max_parallel_tasks: 16 → 20，由超级管理员变更',
    occurredAt: new Date(now - 1_800_000).toISOString(),
  });

  return events
    .sort((a, b) => new Date(b.occurredAt).getTime() - new Date(a.occurredAt).getTime())
    .slice(0, 20)
    .map((e, idx) => ({ ...e, id: e.id || `evt-${idx}` })) as ActivityEvent[];
}

function formatShortLocal(v: number): string {
  if (v >= 1000) return `${(v / 1000).toFixed(1)}K`;
  return `${Math.round(v)}`;
}
