import { ref, computed, onMounted, onUnmounted } from 'vue';
import { api } from '@/api/client';
import { useDocumentVisibility } from '@/composables/useDocumentVisibility';
import {
  ENGINE_LABELS,
  type DashboardSummary,
  type TaskCategory,
  type TaskStatus,
  type AlertLevel,
} from '@/types/domain';

const THIRTY_DAYS_MS = 30 * 24 * 60 * 60 * 1000;
const POLL_INTERVAL_MS = 5_000;

/* ---- API response shapes (snake_case from backend) ---- */
interface TaskRow {
  id: string;
  name: string;
  kind: string;
  status: string;
  db_type?: string;
  source_endpoint?: { url?: string };
  target_endpoint?: { url?: string };
  created_at: string;
  updated_at: string;
}

interface AlertRow {
  id: string;
  task_id: string;
  severity: string;
  status: string;
  fired_at: string;
  recovered_at?: string;
  cleared_at?: string;
  metric?: string;
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

/* ---- Helpers ---- */
const STATUS_MAP: Record<string, TaskStatus> = {
  draft: 'pending',
  ready: 'pending',
  defined: 'pending',
  running: 'running',
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

  async function load() {
    loading.value = true;
    try {
      await Promise.all([loadTasks(), loadAlerts(), loadLicense()]);
    } finally {
      loading.value = false;
    }
  }

  /* ---- Computed summary ---- */
  const summary = computed<DashboardSummary>(() => {
    const allTasks = tasks.value;
    const allAlerts = alerts.value;

    // KPIs
    const runningTasks = allTasks.filter((t) => normalizeStatus(t.status) === 'running');
    const runningCount = runningTasks.length;
    const runningDelta = runningCount - prevRunningCount.value;

    const todayStart = new Date();
    todayStart.setHours(0, 0, 0, 0);
    const todayAlertCount = allAlerts.filter(
      (a) => new Date(a.fired_at).getTime() >= todayStart.getTime(),
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
      const eng = engineFromUrl(t.source_endpoint?.url);
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
        (a) => a.fired_at.slice(0, 10) === dateStr,
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
        occurredAt: t.updated_at,
      })),
      ...allAlerts.slice(0, 5).map((a) => ({
        id: `alert-${a.id}`,
        type: 'alert.triggered' as const,
        category: 'alert' as const,
        tone: 'danger' as const,
        title: `Alert ${a.severity}`,
        taskId: a.task_id,
        alertLevel: a.severity as AlertLevel,
        occurredAt: a.fired_at,
      })),
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
      sourceEngine: engineFromUrl(t.source_endpoint?.url) as keyof typeof ENGINE_LABELS,
      targetEngine: engineFromUrl(t.target_endpoint?.url) as keyof typeof ENGINE_LABELS,
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
        totalRps: { value: 0, delta: 0 },
        avgLatencyMs: { value: 0, delta: 0 },
      },
      kpiSparks: { running: [], todayAlerts: [], totalRps: [], avgLatencyMs: [] },
      rpsSeries: [],
      latencySeries: [],
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
          (a) => new Date(a.fired_at).getTime() >= todayStart.getTime(),
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
