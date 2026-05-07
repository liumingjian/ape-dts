/**
 * Canonical domain types for the ape-dts Console prototype. All mock data,
 * stores and components derive from these. Fields are aligned with the
 * ape-dts engine concepts (see docs/domain-model.md).
 */

import { maskConnectionStringPw } from '@/utils/localizeError';

export type EngineType =
  | 'mysql'
  | 'postgres'
  | 'mongo'
  | 'redis'
  | 'kafka'
  | 'oracle'
  | 'gaussdb'
  | 'tidb'
  | 'starrocks'
  | 'clickhouse'
  | 'doris'
  | 'foxlake';

export const ENGINE_LABELS: Record<EngineType, string> = {
  mysql: 'MySQL',
  postgres: 'PostgreSQL',
  mongo: 'MongoDB',
  redis: 'Redis',
  kafka: 'Kafka',
  oracle: 'Oracle',
  gaussdb: 'GaussDB',
  tidb: 'TiDB',
  starrocks: 'StarRocks',
  clickhouse: 'ClickHouse',
  doris: 'Doris',
  foxlake: 'Foxlake',
};

export type TaskCategory = 'snapshot' | 'cdc' | 'check' | 'struct';
export type TaskStatus = 'draft' | 'ready' | 'running' | 'paused' | 'stopping' | 'stopped' | 'failed' | 'completed' | 'creating' | 'pending';

export type ExtractType =
  | 'snapshot'
  | 'snapshot_file'
  | 'snapshot_and_cdc'
  | 'cdc'
  | 'struct'
  | 'scan';

export type SyncMode = 'snapshot' | 'cdc' | 'snapshot_cdc';

export const LEGACY_CATEGORY_MAP: Record<'sync' | 'replay' | 'verify', TaskCategory> = {
  sync: 'snapshot',
  replay: 'snapshot',
  verify: 'check',
};

export function legacyToCategory(legacy: string): TaskCategory {
  if (legacy === 'sync') return 'snapshot';
  if (legacy === 'replay') return 'snapshot';
  if (legacy === 'verify') return 'check';
  if (
    legacy === 'snapshot' ||
    legacy === 'cdc' ||
    legacy === 'check' ||
    legacy === 'struct'
  ) {
    return legacy;
  }
  return 'snapshot';
}

export type ParallelType =
  | 'snapshot'
  | 'rdb_merge'
  | 'rdb_partition'
  | 'rdb_check'
  | 'mongo'
  | 'redis'
  | 'serial'
  | 'table';

export type ResumeType = 'from_log' | 'from_target' | 'from_db';

export type GaussdbSubMode = 'pg-mode' | 'mysql-mode' | 'oracle-mode';
export const GAUSSDB_SUB_MODES: readonly GaussdbSubMode[] = ['pg-mode', 'mysql-mode', 'oracle-mode'] as const;

export type AlertLevel = 'critical' | 'major' | 'minor' | 'info';
export type AlertStatus = 'active' | 'cleared';
export type AlertSource = 'rps' | 'latency' | 'error_rate' | 'connection' | 'disk' | 'custom';

export interface Endpoint {
  engine: EngineType;
  subMode?: GaussdbSubMode;
  host: string;
  port: number;
  username: string;
  password: string;
  database?: string;
  ssl?: boolean;
  extra?: Record<string, string>;
}

export interface TaskMetricsSnapshot {
  rpsLatest: number;        // extractor_pushed_rps_avg
  bpsLatest: number;        // extractor_pushed_bps_avg
  sinkerRpsLatest: number;  // sinker_record_count_avg_by_sec
  latencyMs: number;        // replication lag
  queryRtUs: number;        // sinker_rt_per_query_avg (μs)
  bufferSize: number;       // pipeline_buffer_size_avg
  errorCount: number;
  processedRecords: number; // pipeline_sinked_count_latest
}

export interface Task {
  id: string;
  name: string;
  description?: string;
  category: TaskCategory;
  status: TaskStatus;
  source: Endpoint;
  target: Endpoint;
  sourceUrl: string;                  // masked connection string (password hidden)
  targetUrl: string;                  // masked connection string (password hidden)
  syncMode: SyncMode;                 // legacy field kept for back-compat with mock seed data
  extractType: ExtractType;
  taskType: 'standalone' | 'primary_backup';
  resourceGroup: string;
  instanceIp: string;
  progressPercent: number;
  syncObjects: { totalTables: number; selectedTables: number };
  config: {
    parallelizer: ParallelType;
    parallelSize: number;
    bufferSize: number;
    maxRps: number;
    checkpointIntervalSecs: number;
    resumeType: ResumeType;
    metricsEnabled: boolean;
  };
  createdAt: string;
  updatedAt: string;
  startedAt?: string;
  completedAt?: string;
  metrics: TaskMetricsSnapshot;
  lastHeartbeatAt: string;
}

export interface Alert {
  id: string;
  level: AlertLevel;
  status: AlertStatus;
  source: AlertSource;
  message: string;
  taskId: string;
  taskName: string;
  engine: EngineType;
  instanceIp: string;
  service: string;
  firstAt: string;
  lastAt: string;
  clearedAt?: string;
  count: number;
}

export interface SysEvent {
  id: string;
  name: string;
  category: 'task' | 'system' | 'security';
  level: AlertLevel;
  status: 'enabled' | 'disabled';
  source: string;
  periodMin: number;
  triggerCount: number;
  validUntil: string;
  description: string;
}

export interface MetricRule {
  id: string;
  name: string;
  metric: string;          // ape-dts metric name
  operator: '>' | '<' | '>=' | '<=' | '==';
  threshold: number;
  level: AlertLevel;
  status: 'enabled' | 'disabled';
  periodMin: number;
  triggerCount: number;
  recoveryThreshold: number;
  description: string;
}

export interface AlarmChannel {
  id: string;
  name: string;
  kind: 'kafka' | 'snmp';
  enabled: boolean;
  startAt: string;
  endAt: string;
  periodMin: number;
  kafka?: { brokers: string; topic: string; ssl: boolean; distinguishType: boolean };
  snmp?: { agent: string; community: string; version: 'v1' | 'v2c' | 'v3' };
}

export interface AlarmTemplate {
  id: string;
  name: string;
  level: AlertLevel;
  subject: string;
  body: string;
  updatedAt: string;
}

export interface OperateLog {
  id: string;
  at: string;
  user: string;
  ip: string;
  action: string;
  target: string;
  result: 'success' | 'failure';
  detail: string;
}

export interface ControlLog {
  id: string;
  at: string;
  taskId: string;
  taskName: string;
  action: 'start' | 'stop' | 'pause' | 'resume' | 'edit' | 'delete';
  operator: string;
  result: 'success' | 'failure';
  detail: string;
}

export interface License {
  id: string;
  sku: string;
  issuedTo: string;
  maxTasks: number;
  issuedAt: string;
  expireAt: string;
  status: 'active' | 'expiring' | 'expired' | 'perpetual';
}

export interface ResourceGroup {
  id: string;
  name: string;
  description: string;
  taskCount: number;
}

export interface User {
  id: string;
  username: string;
  displayName: string;
  role: 'admin' | 'operator' | 'viewer';
  email: string;
  lastLoginAt: string;
}

export interface SystemHost {
  id: string;
  hostname: string;
  ip: string;
  role: 'master' | 'worker' | 'manager';
  nodeType: 'physical' | 'virtual' | 'container';
  status: 'healthy' | 'warning' | 'error';
  cpuPercent: number;
  memoryPercent: number;
  diskPercent: number;
  uptime: number;         // seconds
}

export interface GlobalParam {
  key: string;
  value: string;
  description: string;
  category: 'runtime' | 'pipeline' | 'security' | 'alarm';
  updatedAt: string;
}

/* ----- time-series points for metrics chart ----- */
export interface MetricPoint { t: number; v: number; }
export interface MetricSeries { taskId: string; metric: string; points: MetricPoint[]; }

/* ----- dashboard summary ----- */
export interface DashboardKpiSpark {
  running: number[];
  todayAlerts: number[];
  totalRps: number[];
  avgLatencyMs: number[];
}

export interface DashboardTopTask {
  id: string;
  name: string;
  category: TaskCategory;
  status: TaskStatus;
  sourceEngine: EngineType;
  targetEngine: EngineType;
  rps: number;
  latencyMs: number;
  spark: number[];
}

export type ActivityEventType =
  | 'task.started'
  | 'task.completed'
  | 'task.failed'
  | 'task.paused'
  | 'task.resumed'
  | 'alert.triggered'
  | 'alert.cleared'
  | 'license.expiring'
  | 'system.deploy';

export type ActivityEventCategory = 'task' | 'alert' | 'system';
export type ActivityEventTone = 'success' | 'warning' | 'danger' | 'info' | 'neutral';

export interface ActivityEvent {
  id: string;
  type: ActivityEventType;
  category: ActivityEventCategory;
  tone: ActivityEventTone;
  title: string;
  description?: string;
  taskId?: string;
  taskName?: string;
  taskCategory?: TaskCategory;
  sourceEngine?: EngineType;
  targetEngine?: EngineType;
  alertLevel?: AlertLevel;
  occurredAt: string;
}

export interface DashboardSummary {
  kpi: {
    running: { total: number; delta: number };
    todayAlerts: { total: number; delta: number };
    totalRps: { value: number; delta: number };
    avgLatencyMs: { value: number; delta: number };
  };
  kpiSparks: DashboardKpiSpark;
  rpsSeries: MetricSeries[];
  latencySeries: MetricSeries[];
  statusDist: { status: TaskStatus; count: number }[];
  engineDist: { engine: EngineType; count: number }[];
  alertTrend: { date: string; critical: number; major: number; minor: number; info: number }[];
  recentTasks: Task[];
  topRunningTasks: DashboardTopTask[];
  topAlerts: Alert[];
  recentEvents: ActivityEvent[];
  licenseWarnCount: number;
}

/* ----- pagination wrapper ----- */
export interface Paginated<T> {
  items: T[];
  total: number;
  page: number;
  pageSize: number;
}

/* ----- INI-rendering fixture types (consolidated from taskFixture) ----- */
export interface EndpointFixture {
  engine: string;
  subMode?: 'pg-mode' | 'mysql-mode' | 'oracle-mode';
  url: string;
}

export interface TaskFixture {
  taskId: string;
  kind: 'snapshot' | 'cdc' | 'check' | 'struct';
  extractType:
    | 'snapshot'
    | 'snapshot_file'
    | 'snapshot_and_cdc'
    | 'cdc'
    | 'struct'
    | 'scan';
  source: EndpointFixture;
  sink: EndpointFixture;
  filter: {
    doDbs?: string[];
    doTbs?: string[];
    ignoreDbs?: string[];
    ignoreTbs?: string[];
    doEvents?: string[];
  };
  router?: {
    dbMap?: Record<string, string>;
    tbMap?: Record<string, string>;
  };
  parallelizer: { type: string; size: number };
  pipeline: { bufferSize: number; checkpointIntervalSecs: number; maxRps: number };
  resumer?: { type: 'from_log' | 'from_target' | 'from_db' | 'dummy' };
  processor?: { luaCode?: string; luaCodeFile?: string };
  metrics?: { httpHost: string; httpPort: number };
}

/* ----- Run (execution) type ----- */
export type RunStatus = 'pending' | 'running' | 'paused' | 'stopping' | 'stopped' | 'failed' | 'orphaned';

export interface Run {
  id: string;
  taskId: string;
  status: RunStatus;
  startedAt: string | null;
  stoppedAt: string | null;
  exitStatus: number | null;
  logDir: string | null;
  iniPath: string | null;
  pid: number | null;
  position: RunPosition | null;
  createdAt: string;
}

export type RunPosition =
  | { kind: 'binlog'; file: string; pos: number; gtid?: string }
  | { kind: 'lsn'; lsn: string; slot?: string }
  | { kind: 'scn'; scn: string }
  | { kind: 'resume_token'; token: string }
  | { kind: 'repl'; replId: string; offset: number }
  | { kind: 'offset'; partition: number; offset: number }
  | { kind: 'unknown'; raw: string };

/* ----- Metrics query response from /api/runs/:id/metrics ----- */
export interface MetricQueryResponse {
  metric: string;
  data: { ts: number; value: number }[];
  details?: { source?: string[]; hint?: string };
}

/* ----- time-series point for downsample / query utilities ----- */
export interface TimeSeriesPoint {
  ts: number;
  value: number;
}

/* ----- task creation DTO (WizardForm → API) — snake_case wire format ----- */
export interface CreateTaskDto {
  name: string;
  kind: TaskCategory;
  engineSource: EngineType;
  engineTarget: EngineType;
  subMode?: GaussdbSubMode;
  sourceEndpoint: { url: string };
  targetEndpoint: { url: string };
  extractor: { extract_type: ExtractType };
  sinker: Record<string, unknown>;
  filter?: {
    do_dbs?: string;
    do_tbs?: string;
    ignore_dbs?: string;
    ignore_tbs?: string;
    do_events?: string;
  };
  router?: Record<string, string>;
  parallelizer: {
    parallel_type: ParallelType;
    parallel_size: number;
  };
  pipeline: {
    buffer_size: number;
    checkpoint_interval_secs: number;
    max_rps: number;
  };
  resumer: {
    resume_type: ResumeType;
  };
  processor?: {
    lua_code_file?: string;
    lua_code?: string;
  };
  runtime?: Record<string, unknown>;
  metrics?: {
    http_host?: string;
    http_port?: number;
    labels?: string;
  };
  resourceGroupId: string;
}

/* ----- API response types (backend snake_case → frontend camelCase) ----- */

/** Raw shape returned by GET /api/tasks from the Rust backend. */
export interface ApiTask {
  id: string;
  taskId: string;
  name: string;
  kind: string;                         // snapshot | cdc | check | struct
  dbTypeSource: string;                 // mysql | postgres | ...
  dbTypeTarget: string;
  sourceEndpoint: { url?: string };
  targetEndpoint: { url?: string };
  extractor: Record<string, unknown> | null;
  sinker: Record<string, unknown> | null;
  filter: Record<string, unknown> | null;
  router: Record<string, unknown> | null;
  parallelizer: Record<string, unknown> | null;
  pipeline: Record<string, unknown> | null;
  resumer: Record<string, unknown> | null;
  processor: Record<string, unknown> | null;
  runtime: Record<string, unknown> | null;
  metrics: {
    extractor_pushed_rps_avg?: number;
    extractor_pushed_bps_avg?: number;
    sinker_record_count_avg_by_sec?: number;
    replication_lag?: number;
    sinker_rt_per_query_avg?: number;
    pipeline_buffer_size_avg?: number;
    pipeline_sinked_count_latest?: number;
    error_count?: number;
  } | null;
  resourceGroupId: string;
  ownerUserId: string;
  status: string;
  createdAt: string;
  updatedAt: string;
  startedAt?: string;
  completedAt?: string;
}

/** Parse a database connection URL string into its host/port/username/database parts. */
function parseEndpointUrl(url: string): { host: string; port: number; username: string; database: string } {
  try {
    const u = new URL(url);
    return {
      host: u.hostname,
      port: Number(u.port) || 3306,
      username: decodeURIComponent(u.username || ''),
      database: decodeURIComponent(u.pathname.slice(1) || ''),
    };
  } catch {
    return { host: '', port: 0, username: '', database: '' };
  }
}

/** Map a backend ApiTask to the frontend Task type used by the SPA. */

/** Count the number of table references in a filter's do_tbs field. */
function countFilterTables(filter: Record<string, unknown> | null): number {
  if (!filter) return 0;
  const doTbs = filter.do_tbs ?? filter.doTbs;
  if (typeof doTbs === 'string' && doTbs.length > 0) {
    return doTbs.split(',').filter((s: string) => s.trim().length > 0).length;
  }
  if (Array.isArray(doTbs)) return doTbs.length;
  return 0;
}

/** Compute progress as percentage (0–100) from sinked count and filter tables. */
function computeProgress(sinkedCount: number, filter: Record<string, unknown> | null): number {
  const tables = countFilterTables(filter);
  if (tables === 0 || sinkedCount === 0) return 0;
  // Assume ~1000 rows per table as a rough baseline for percentage.
  // Without a total-row-count metric from the backend, this is the best estimate.
  const estimatedTotal = tables * 1000;
  const pct = (sinkedCount / estimatedTotal) * 100;
  return Math.min(Math.round(pct), 100);
}

/** Resolve ResumeType from the resumer field, defaulting to 'from_log'. */
function resolveResumeType(resumer: Record<string, unknown> | null): ResumeType {
  if (!resumer) return 'from_log';
  const raw = resumer.resume_type ?? resumer.resumeType;
  if (raw === 'from_target') return 'from_target';
  if (raw === 'from_db') return 'from_db';
  // 'auto', undefined, or unknown → default to 'from_log'
  return 'from_log';
}

export function mapApiTask(raw: ApiTask): Task {
  const srcRaw = raw.sourceEndpoint?.url ?? '';
  const tgtRaw = raw.targetEndpoint?.url ?? '';
  const src = parseEndpointUrl(srcRaw);
  const tgt = parseEndpointUrl(tgtRaw);
  const m = raw.metrics;
  return {
    id: raw.id,
    name: raw.name,
    category: (raw.kind || 'snapshot') as TaskCategory,
    status: (raw.status || 'draft') as TaskStatus,
    source: {
      engine: (raw.dbTypeSource || 'mysql') as EngineType,
      host: src.host,
      port: src.port,
      username: src.username,
      password: '',
      database: src.database,
    },
    target: {
      engine: (raw.dbTypeTarget || 'mysql') as EngineType,
      host: tgt.host,
      port: tgt.port,
      username: tgt.username,
      password: '',
      database: tgt.database,
    },
    sourceUrl: maskConnectionStringPw(srcRaw),
    targetUrl: maskConnectionStringPw(tgtRaw),
    syncMode: (raw.kind === 'cdc' ? 'cdc' : 'snapshot') as SyncMode,
    extractType: (raw.kind === 'cdc' ? 'cdc' : 'snapshot') as ExtractType,
    taskType: 'standalone',
    resourceGroup: raw.resourceGroupId ?? '',
    instanceIp: src.host,
    progressPercent: computeProgress(m?.pipeline_sinked_count_latest ?? 0, raw.filter),
    syncObjects: { totalTables: 0, selectedTables: 0 },
    config: {
      parallelizer: 'snapshot' as ParallelType,
      parallelSize: 1,
      bufferSize: 4,
      maxRps: 0,
      checkpointIntervalSecs: 10,
      resumeType: resolveResumeType(raw.resumer),
      metricsEnabled: true,
    },
    createdAt: raw.createdAt,
    updatedAt: raw.updatedAt,
    startedAt: raw.startedAt,
    completedAt: raw.completedAt,
    metrics: {
      rpsLatest: m?.extractor_pushed_rps_avg ?? 0,
      bpsLatest: m?.extractor_pushed_bps_avg ?? 0,
      sinkerRpsLatest: m?.sinker_record_count_avg_by_sec ?? 0,
      latencyMs: m?.replication_lag ?? 0,
      queryRtUs: m?.sinker_rt_per_query_avg ?? 0,
      bufferSize: m?.pipeline_buffer_size_avg ?? 0,
      errorCount: m?.error_count ?? 0,
      processedRecords: m?.pipeline_sinked_count_latest ?? 0,
    },
    lastHeartbeatAt: raw.updatedAt,
  };
}
