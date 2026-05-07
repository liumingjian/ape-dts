/**
 * Canonical domain types for the ape-dts Console prototype. All mock data,
 * stores and components derive from these. Fields are aligned with the
 * ape-dts engine concepts (see docs/domain-model.md).
 */

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
export type TaskStatus = 'running' | 'paused' | 'failed' | 'completed' | 'creating' | 'pending';

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
  size: number;
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

/* ----- time-series point for downsample / query utilities ----- */
export interface TimeSeriesPoint {
  ts: number;
  value: number;
}

/* ----- task creation DTO (WizardForm → API) ----- */
export interface CreateTaskDto {
  name: string;
  description: string;
  category: TaskCategory;
  source: {
    engine: EngineType;
    subMode?: GaussdbSubMode;
    host: string;
    port: number;
    username: string;
    password: string;
    database?: string;
    ssl?: boolean;
  };
  target: {
    engine: EngineType;
    subMode?: GaussdbSubMode;
    host: string;
    port: number;
    username: string;
    password: string;
    database?: string;
    ssl?: boolean;
  };
  syncMode: SyncMode;
  extractType: ExtractType;
  taskType: 'standalone' | 'primary_backup';
  resourceGroup: string;
  instanceIp: string;
  syncObjects: { totalTables: number; selectedTables: number };
  config: {
    parallelizer: ParallelType;
    parallelSize: number;
    bufferSize: number;
    maxRps: number;
    checkpointIntervalSecs: number;
    resumeType: ResumeType;
    metricsEnabled: boolean;
    metricsHttpPort?: number;
  };
  filter?: {
    doDbs?: string[];
    doTbs?: string[];
    doEvents?: string[];
  };
}
