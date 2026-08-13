import { ref, computed, onMounted, onUnmounted } from 'vue';
import { api } from '@/api/client';
import { useDocumentVisibility } from '@/composables/useDocumentVisibility';
import {
  ENGINE_LABELS,
  type DashboardSummary,
  type MetricSeries,
  type MetricPoint,
  type SyncMode,
  type TaskCategory,
  type TaskStatus,
  type AlertLevel,
} from '@/types/domain';

const THIRTY_DAYS_MS = 30 * 24 * 60 * 60 * 1000;
const POLL_INTERVAL_MS = 5_000;

/* ---- API response shapes (camelCase from backend) ---- */
interface TaskRow {
  id: string;
  name: string;
  kind: string;
  status: string;
  dbTypeSource?: string;
  sourceEndpoint?: { url?: string };
  targetEndpoint?: { url?: string };
  createdAt: string;
  updatedAt: string;
}

interface AlertRow {
  id: string;
  taskId?: string | null;
  severity: string;
  status: string;
  firedAt: string;
  recoveredAt?: string | null;
  clearedAt?: string | null;
  metricName?: string | null;
  value?: number;
  threshold?: number;
}

interface LicensePayload {
  sku?: string;
  maxTasks?: number;
  expireAt?: string;
  status?: 'active' | 'expiring_soon' | 'expired' | 'missing';
}

interface TaskListResponse {
  items: TaskRow[];
  total: number;
  page: number;
  size: number;
}

interface AlertListResponse {
  items: AlertRow[];
  total: number;
  page: number;
  size: number;
}

/** Response shape from GET /api/runs/:id/metrics */
interface MetricQueryResponse {
  metric: string;
  data: { ts: number; value: number }[];
  details?: { source?: string[]; hint?: string };
}

interface RunRow {
  id: string;
  taskId: string;
  status: string;
  startedAt: string | null;
  stoppedAt: string | null;
}

/* ---- Helpers ---- */
const STATUS_MAP: Record<string, TaskStatus> = {
  draft: 'pending',
  ready: 'pending',
  defined: 'pending',
  running: 'running',
  // The engine is still alive while it drains, so a pausing task counts as
  // running in the dashboard's coarse buckets (ADR 0004).
  pausing: 'running',
  paused: 'paused',
  stopping: 'running',
  stopped: 'completed',
  finished: 'completed',
  failed: 'failed',
};

function normalizeStatus(s: string): TaskStatus {
  return STATUS_MAP[s] ?? 'pending';
}

const CATEGORY_MAP: Record<string, TaskCategory> = {
  snapshot: 'snapshot',
  cdc: 'cdc',
  check: 'check',
  struct: 'struct',
};

function normalizeCategory(k: string): TaskCategory {
  return CATEGORY_MAP[k] ?? 'snapshot';
}

function syncModeForTask(t: TaskRow): SyncMode {
  return normalizeCategory(t.kind) === 'cdc' ? 'cdc' : 'snapshot';
}

function engineFromUrl(url?: string): string {
  if (!url) return 'mysql';
  try {
    const scheme = new URL(url).protocol.replace(':', '');
    const map: Record<string, string> = {
      mysql: 'mysql', postgres: 'postgres', pg: 'postgres',
      oracle: 'oracle', mongodb: 'mongo', mongo: 'mongo',
      redis: 'redis', kafka: 'kafka',
    };
    return map[scheme] ?? scheme;
  } catch {
    return 'mysql';
  }
}

export function useDashboardData() {
  const { isVisible } = useDocumentVisibility();

  const tasks = ref<TaskRow[]>([]);
  const alerts = ref<AlertRow[]>([]);
  const license = ref<LicensePayload | null>(null);
  const loading = ref(false);
  const prevRunningCount = ref(0);
  const prevAlertCount = ref(0);

  // Metric series for RPS and latency charts
  const rpsSeries = ref<MetricSeries[]>([]);
  const latencySeries = ref<MetricSeries[]>([]);
  const totalRpsValue = ref(0);
  const avgLatencyValue = ref(0);

  async function loadTasks() {
    try {
      const data = await api.get<TaskListResponse>('/tasks?size=50');
      tasks.value = data.items ?? [];
    } catch {
      /* keep stale data */
    }
  }

  async function loadAlerts() {
    try {
      const data = await api.get<AlertListResponse>('/alerts?status=firing&size=50');
      alerts.value = data.items ?? [];
    } catch {
      /* keep stale data */
    }
  }

  async function loadLicense() {
    try {
      license.value = await api.get<LicensePayload>('/license');
    } catch {
      license.value = null;
    }
  }

  /** Fetch metric series for all running tasks from /api/runs/:id/metrics */
  async function loadMetrics() {
    const runningTasks = tasks.value.filter(
      (t) => normalizeStatus(t.status) === 'running',
    );
    if (runningTasks.length === 0) {
      rpsSeries.value = [];
      latencySeries.value = [];
      totalRpsValue.value = 0;
      avgLatencyValue.value = 0;
      return;
    }

    const now = Date.now();
    const from = now - 3600_000; // last 1h
    const step = 60;
    const newRps: MetricSeries[] = [];
    const newLat: MetricSeries[] = [];
    let rpsSum = 0;
    let latSum = 0;
    let latN = 0;

    for (const t of runningTasks) {
      // Get the latest run for this task
      let runId = '';
      try {
        const runs = await api.get<{ items: RunRow[] }>(`/tasks/${t.id}/runs?page=1&size=1`);
        const items = runs.items ?? [];
        const active = items.find(
          (r) => r.status === 'running' || r.status === 'pausing' || r.status === 'paused',
        );
        if (!active) continue;
        runId = active.id;
      } catch { continue; }

      // Fetch extractor_rps_avg
      try {
        const res = await api.get<MetricQueryResponse>(
          `/runs/${runId}/metrics?metric=extractor_rps_avg&from=${from}&to=${now}&step=${step}`,
        );
        if (res.data.length > 0) {
          const points: MetricPoint[] = res.data.map((p) => ({ t: p.ts, v: p.value }));
          newRps.push({ taskId: t.id, metric: 'extractor_rps_avg', points });
          rpsSum += points[points.length - 1]?.v ?? 0;
        }
      } catch { /* ignore individual metric errors */ }

      // Fetch lag — only for CDC tasks (snapshot tasks never emit lag)
      if (t.kind === 'cdc') {
        try {
          const res = await api.get<MetricQueryResponse>(
            `/runs/${runId}/metrics?metric=lag&from=${from}&to=${now}&step=${step}`,
          );
          if (res.data.length > 0) {
            const points: MetricPoint[] = res.data.map((p) => ({ t: p.ts, v: p.value }));
            newLat.push({ taskId: t.id, metric: 'lag', points });
            const latest = points[points.length - 1]?.v ?? 0;
            latSum += latest;
            latN += 1;
          }
        } catch { /* ignore individual metric errors */ }
      }
    }

    rpsSeries.value = newRps;
    latencySeries.value = newLat;
    totalRpsValue.value = rpsSum;
    avgLatencyValue.value = latN > 0 ? latSum / latN : 0;
  }

  async function load() {
    loading.value = true;
    try {
      await Promise.all([loadTasks(), loadAlerts(), loadLicense()]);
      // Load metrics after tasks are available (needs running task list)
      await loadMetrics();
    } finally {
      loading.value = false;
    }
  }

  /* ---- Computed summary ---- */
  const summary = computed<DashboardSummary>(() => {
    const allTasks = tasks.value;
    const allAlerts = alerts.value;
    const taskById = new Map(allTasks.map((t) => [t.id, t]));

    // KPIs
    const runningTasks = allTasks.filter((t) => normalizeStatus(t.status) === 'running');
    const runningCount = runningTasks.length;
    const runningDelta = runningCount - prevRunningCount.value;

    const todayStart = new Date();
    todayStart.setHours(0, 0, 0, 0);
    const todayAlertCount = allAlerts.filter(
      (a) => new Date(a.firedAt).getTime() >= todayStart.getTime(),
    ).length;
    const alertDelta = todayAlertCount - prevAlertCount.value;

    // Status distribution
    const statusBuckets: Record<string, number> = {};
    for (const t of allTasks) {
      const s = normalizeStatus(t.status);
      statusBuckets[s] = (statusBuckets[s] ?? 0) + 1;
    }
    const statusDist = Object.entries(statusBuckets).map(([status, count]) => ({
      status: status as TaskStatus,
      count,
    }));

    // Engine distribution
    const engineBuckets: Record<string, number> = {};
    for (const t of allTasks) {
      const eng = engineFromUrl(t.sourceEndpoint?.url);
      engineBuckets[eng] = (engineBuckets[eng] ?? 0) + 1;
    }
    const engineDist = Object.entries(engineBuckets).map(([engine, count]) => ({
      engine: engine as keyof typeof ENGINE_LABELS,
      count,
    }));

    // 14-day alert trend (synthesised from available data)
    const alertTrend: {
      date: string;
      critical: number;
      major: number;
      minor: number;
      info: number;
    }[] = [];
    for (let i = 13; i >= 0; i--) {
      const d = new Date();
      d.setDate(d.getDate() - i);
      const dateStr = d.toISOString().slice(0, 10);
      const dayAlerts = allAlerts.filter(
        (a) => a.firedAt.slice(0, 10) === dateStr,
      );
      alertTrend.push({
        date: dateStr,
        critical: dayAlerts.filter((a) => a.severity === 'critical').length,
        major: dayAlerts.filter((a) => a.severity === 'major').length,
        minor: dayAlerts.filter((a) => a.severity === 'minor').length,
        info: dayAlerts.filter((a) => a.severity === 'info').length,
      });
    }

    // Recent events (derived from tasks + alerts)
    const recentEvents = [
      ...allTasks.slice(0, 5).map((t) => ({
        id: `task-${t.id}`,
        type: 'task.started' as const,
        category: 'task' as const,
        tone: 'success' as const,
        title: t.name || t.id,
        taskId: t.id,
        taskCategory: normalizeCategory(t.kind),
        taskSyncMode: syncModeForTask(t),
        occurredAt: t.updatedAt,
      })),
      ...allAlerts.slice(0, 5).map((a) => {
        const task = a.taskId ? taskById.get(a.taskId) : undefined;
        return {
          id: `alert-${a.id}`,
          type: 'alert.triggered' as const,
          category: 'alert' as const,
          tone: 'danger' as const,
          title: `Alert ${a.severity}`,
          taskId: a.taskId ?? '',
          taskCategory: task ? normalizeCategory(task.kind) : undefined,
          taskSyncMode: task ? syncModeForTask(task) : undefined,
          alertLevel: a.severity as AlertLevel,
          occurredAt: a.firedAt,
        };
      }),
    ].sort(
      (a, b) =>
        new Date(b.occurredAt).getTime() - new Date(a.occurredAt).getTime(),
    );

    // Top running tasks
    const topRunningTasks = runningTasks.slice(0, 5).map((t) => ({
      id: t.id,
      name: t.name || t.id,
      category: normalizeCategory(t.kind),
      status: normalizeStatus(t.status) as 'running',
      sourceEngine: engineFromUrl(t.sourceEndpoint?.url) as keyof typeof ENGINE_LABELS,
      targetEngine: engineFromUrl(t.targetEndpoint?.url) as keyof typeof ENGINE_LABELS,
      rps: 0,
      latencyMs: 0,
      spark: [],
    }));

    // License warning
    const shouldWarn =
      license.value?.status === 'expired' ||
      license.value?.status === 'expiring_soon' ||
      (license.value?.expireAt
        ? (() => {
            const diff = new Date(license.value!.expireAt!).getTime() - Date.now();
            return diff > 0 && diff <= THIRTY_DAYS_MS;
          })()
        : false);

    return {
      kpi: {
        running: { total: runningCount, delta: runningDelta },
        todayAlerts: { total: todayAlertCount, delta: alertDelta },
        totalRps: { value: totalRpsValue.value, delta: 0 },
        avgLatencyMs: { value: avgLatencyValue.value, delta: 0 },
      },
      kpiSparks: { running: [], todayAlerts: [], totalRps: [], avgLatencyMs: [] },
      rpsSeries: rpsSeries.value,
      latencySeries: latencySeries.value,
      statusDist,
      engineDist,
      alertTrend,
      recentTasks: [],
      topRunningTasks,
      topAlerts: [],
      recentEvents,
      licenseWarnCount: shouldWarn ? 1 : 0,
    };
  });

  /* ---- Polling with visibility gating ---- */
  let pollHandle: ReturnType<typeof setInterval> | null = null;
  onMounted(() => {
    load();
    pollHandle = setInterval(() => {
      if (isVisible.value) {
        // Capture previous counts for delta computation
        const runningCount = tasks.value.filter(
          (t) => normalizeStatus(t.status) === 'running',
        ).length;
        prevRunningCount.value = runningCount;

        const todayStart = new Date();
        todayStart.setHours(0, 0, 0, 0);
        prevAlertCount.value = alerts.value.filter(
          (a) => new Date(a.firedAt).getTime() >= todayStart.getTime(),
        ).length;

        load();
      }
    }, POLL_INTERVAL_MS);
  });
  onUnmounted(() => {
    if (pollHandle !== null) clearInterval(pollHandle);
  });

  return { summary, loading, load, license };
}
