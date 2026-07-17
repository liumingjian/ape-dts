/**
 * Mutable in-memory mock database. Seeded once on first access. MSW handlers
 * mutate this object; UI state reads through HTTP so changes propagate.
 */
import type {
  Task, Alert, SysEvent, MetricRule, AlarmChannel, AlarmTemplate,
  OperateLog, ControlLog, License, ResourceGroup, User, SystemHost,
  GlobalParam, EngineType, TaskCategory, TaskStatus, SyncMode, ExtractType,
  MetricSeries, MetricPoint, AlertLevel,
} from '@/types/domain';
import { maskConnectionStringPw } from '@/utils/localizeError';
import { pick, intBetween, floatBetween, id, isoMinus, jitter } from './fake';

const ENGINES: EngineType[] = [
  'mysql', 'postgres', 'mongo', 'redis', 'oracle', 'gaussdb', 'tidb',
  'starrocks', 'clickhouse', 'doris', 'kafka', 'foxlake',
];

const RESOURCE_GROUPS = ['default', 'production', 'staging', 'dev'];

const STATUS_POOL: TaskStatus[] = [
  'running', 'running', 'running', 'running', 'running',
  'running', 'running', 'running', 'running', 'running',
  'running', 'running',
  'paused', 'paused', 'paused',
  'failed', 'failed',
  'completed', 'completed', 'completed', 'completed', 'completed',
  'creating', 'creating',
];

const SYNC_MODES: SyncMode[] = ['snapshot', 'cdc', 'snapshot_cdc'];

const ALERT_MESSAGES: Record<string, string[]> = {
  rps: ['同步吞吐率低于阈值', '源端抽取速率异常', '数据积压风险'],
  latency: ['同步延迟超过 5 秒', 'CDC 位点滞后', '复制延迟上升'],
  error_rate: ['写入失败率超过 1%', '批量写入异常', '目标端响应异常'],
  connection: ['源端连接中断', '目标端连接超时', '心跳丢失'],
  disk: ['本地日志盘使用率超过 85%', '断点续传文件增长过快'],
  custom: ['自定义指标触发', '业务规则告警'],
};

function buildTaskName(src: EngineType, tgt: EngineType, idx: number): string {
  const verbs = ['orders', 'users', 'payments', 'inventory', 'events', 'logs', 'metrics', 'profiles'];
  const v = verbs[idx % verbs.length];
  return `${v}_${src}_to_${tgt}_${pick(['prd', 'staging', 'dev', 'uat'])}`;
}

function buildEndpoint(engine: EngineType) {
  const portMap: Record<EngineType, number> = {
    mysql: 3306, tidb: 4000, postgres: 5432, gaussdb: 5432, oracle: 1521,
    mongo: 27017, redis: 6379, kafka: 9092, starrocks: 9030, clickhouse: 9000,
    doris: 9030, foxlake: 443,
  };
  return {
    engine,
    host: `10.${intBetween(10, 250)}.${intBetween(0, 250)}.${intBetween(1, 254)}`,
    port: portMap[engine],
    username: pick(['root', 'admin', 'app_user', 'ape_dts_repl']),
    password: '********',
    database: ['mysql', 'postgres', 'gaussdb', 'oracle'].includes(engine) ? pick(['app_db', 'orders', 'crm', 'core']) : undefined,
    ssl: Math.random() > 0.6,
  };
}

function buildMaskedUrl(endpoint: ReturnType<typeof buildEndpoint>): string {
  const schemeMap: Record<EngineType, string> = {
    mysql: 'mysql', postgres: 'postgres', gaussdb: 'postgres', oracle: 'oracle',
    mongo: 'mongodb', redis: 'redis', kafka: 'kafka', tidb: 'mysql',
    starrocks: 'mysql', clickhouse: 'clickhouse', doris: 'mysql', foxlake: 'postgres',
  };
  const scheme = schemeMap[endpoint.engine];
  const raw = `${scheme}://${endpoint.username}:${endpoint.password}@${endpoint.host}:${endpoint.port}${endpoint.database ? `/${endpoint.database}` : ''}`;
  return maskConnectionStringPw(raw);
}

function setTaskRoute(task: Task, sourceEngine: EngineType, targetEngine: EngineType, name: string): void {
  const source = buildEndpoint(sourceEngine);
  const target = buildEndpoint(targetEngine);
  task.source = source;
  task.target = target;
  task.sourceUrl = buildMaskedUrl(source);
  task.targetUrl = buildMaskedUrl(target);
  task.name = name;
}

function buildMetricsSnapshot(status: TaskStatus) {
  if (status === 'running') {
    const rps = intBetween(50, 2500);
    return {
      rpsLatest: rps,
      bpsLatest: rps * intBetween(80, 420),
      sinkerRpsLatest: Math.round(rps * floatBetween(0.9, 1.0, 3)),
      latencyMs: intBetween(80, 3200),
      lag: 0,
      queryRtUs: intBetween(800, 6000),
      bufferSize: intBetween(0, 80),
      errorCount: 0,
      processedRecords: intBetween(100_000, 100_000_000),
      pipelineQueueSize: 0,
      finishedProgressCount: 0,
      totalProgressCount: 0,
    };
  }
  if (status === 'failed') {
    return {
      rpsLatest: 0, bpsLatest: 0, sinkerRpsLatest: 0,
      latencyMs: intBetween(10_000, 60_000), lag: 0, queryRtUs: 0,
      bufferSize: intBetween(200, 2000),
      errorCount: intBetween(10, 400),
      processedRecords: intBetween(100, 10_000_000),
      pipelineQueueSize: 0,
      finishedProgressCount: 0,
      totalProgressCount: 0,
    };
  }
  return {
    rpsLatest: 0, bpsLatest: 0, sinkerRpsLatest: 0,
    latencyMs: 0, lag: 0, queryRtUs: 0, bufferSize: 0, errorCount: 0,
    processedRecords: intBetween(0, 1_000_000),
    pipelineQueueSize: 0,
    finishedProgressCount: 0,
    totalProgressCount: 0,
  };
}

function defaultExtractType(category: TaskCategory, syncMode: SyncMode): ExtractType {
  if (category === 'check') return 'snapshot';
  if (category === 'struct') return 'struct';
  if (category === 'cdc') return 'cdc';
  if (syncMode === 'snapshot') return 'snapshot';
  if (syncMode === 'cdc') return 'cdc';
  return 'snapshot_and_cdc';
}

function makeTask(idx: number, category: TaskCategory, forcedStatus?: TaskStatus): Task {
  const source = buildEndpoint(pick(ENGINES));
  // Avoid identical source/target engine for diversity
  let tgt: EngineType = pick(ENGINES);
  while (tgt === source.engine && Math.random() > 0.5) tgt = pick(ENGINES);
  const target = buildEndpoint(tgt);
  const status = forcedStatus ?? pick(STATUS_POOL);
  const createdMin = intBetween(5, 60 * 24 * 7);
  const syncMode: SyncMode =
    category === 'cdc' ? 'cdc' : category === 'snapshot' ? pick(SYNC_MODES) : 'snapshot';
  const startedMin = createdMin - intBetween(0, 60);
  return {
    id: id(category),
    name: buildTaskName(source.engine, target.engine, idx),
    description: Math.random() > 0.3 ? pick([
      '将订单表从 MySQL 实时同步到 GaussDB 分析侧',
      '金融对账数据全量 + 增量迁移',
      '合规归档：将热数据复制到 ClickHouse',
      '多活场景下主备数据校验',
      '用于回放昨日 CDC 以验证下游',
      '业务搬迁：旧集群向新集群灰度同步',
      '为数据湖提供每日 Foxlake 投递',
    ]) : '',
    category,
    status,
    source,
    target,
    sourceUrl: buildMaskedUrl(source),
    targetUrl: buildMaskedUrl(target),
    syncMode,
    extractType: defaultExtractType(category, syncMode),
    taskType: Math.random() > 0.3 ? 'standalone' : 'primary_backup',
    resourceGroup: pick(RESOURCE_GROUPS),
    instanceIp: `127.0.0.${intBetween(1, 20)}`,
    progressPercent: status === 'completed' ? 100
      : status === 'running' ? floatBetween(10, 98, 1)
      : status === 'paused' ? floatBetween(5, 90, 1)
      : status === 'failed' ? floatBetween(10, 80, 1)
      : 0,
    syncObjects: { totalTables: intBetween(1, 200), selectedTables: intBetween(1, 120) },
    config: {
      parallelizer: pick(['snapshot', 'rdb_merge', 'rdb_partition', 'serial', 'table']),
      parallelSize: pick([1, 2, 4, 8, 16]),
      bufferSize: 16_000,
      maxRps: pick([0, 1000, 5000, 10_000]),
      checkpointIntervalSecs: 10,
      resumeType: pick(['from_log', 'from_target', 'from_db']),
      metricsEnabled: Math.random() > 0.2,
    },
    createdAt: isoMinus(createdMin),
    updatedAt: isoMinus(intBetween(0, 60)),
    startedAt: status !== 'creating' && status !== 'pending' ? isoMinus(Math.max(0, startedMin)) : undefined,
    completedAt: status === 'completed' ? isoMinus(intBetween(0, createdMin)) : undefined,
    metrics: buildMetricsSnapshot(status),
    lastHeartbeatAt: new Date().toISOString(),
  };
}

/** generate a 24h × 1min series fluctuating around `base` with sinusoidal+noise */
function buildMetricSeries(taskId: string, metric: string, base: number, amp: number): MetricSeries {
  const points: MetricPoint[] = [];
  const now = Math.floor(Date.now() / 60_000) * 60_000;
  for (let i = 24 * 60 - 1; i >= 0; i--) {
    const t = now - i * 60_000;
    const phase = (i / (24 * 60)) * Math.PI * 4;
    const sinu = Math.sin(phase) * amp;
    const noise = (Math.random() - 0.5) * amp * 0.6;
    points.push({ t, v: Math.max(0, Math.round((base + sinu + noise) * 100) / 100) });
  }
  return { taskId, metric, points };
}

function buildAlert(idx: number, tasks: Task[], active: boolean): Alert {
  const level: AlertLevel = pick(active
    ? ['critical', 'major', 'major', 'minor', 'minor', 'minor', 'info']
    : ['critical', 'major', 'minor', 'info']);
  const source = pick(['rps', 'latency', 'error_rate', 'connection', 'disk', 'custom'] as const);
  const task = pick(tasks);
  const firstMin = intBetween(active ? 1 : 60, active ? 60 * 24 : 60 * 24 * 30);
  const lastMin = active ? intBetween(0, firstMin) : intBetween(0, Math.min(firstMin, 120));
  return {
    id: id('alrt'),
    level,
    status: active ? 'active' : 'cleared',
    source,
    message: pick(ALERT_MESSAGES[source]),
    taskId: task.id,
    taskName: task.name,
    engine: task.source.engine,
    instanceIp: task.instanceIp,
    service: pick(['ape-dts-engine', 'ape-dts-metrics', 'ape-dts-resumer', 'ape-dts-checker']),
    firstAt: isoMinus(firstMin),
    lastAt: isoMinus(lastMin),
    clearedAt: active ? undefined : isoMinus(intBetween(0, 30)),
    count: intBetween(1, 40),
  };
}

function buildEvent(_idx: number): SysEvent {
  const name = pick(['任务启动', '任务停止', '任务失败', '连接中断', '位点重置', '预检查失败', '断点续传', '告警通道不可达']);
  return {
    id: id('evt'),
    name,
    category: pick(['task', 'system', 'security']),
    level: pick(['critical', 'major', 'minor', 'info']),
    status: Math.random() > 0.1 ? 'enabled' : 'disabled',
    source: pick(['ape-dts-engine', 'ape-dts-metrics', 'ape-dts-resumer', 'ape-dts-controller']),
    periodMin: pick([1, 5, 10, 30]),
    triggerCount: intBetween(1, 10),
    validUntil: new Date(Date.now() + intBetween(1, 90) * 86400_000).toISOString(),
    description: `当发生「${name}」达到阈值时触发告警`,
  };
}

function buildMetricRule(): MetricRule {
  const opts = [
    { name: '源端抽取速率过低', metric: 'extractor_pushed_rps_avg', operator: '<' as const, threshold: 100, level: 'major' as const, unit: 'rows/s' },
    { name: '同步延迟过高', metric: 'sinker_rt_per_query_avg', operator: '>' as const, threshold: 5000, level: 'critical' as const, unit: 'μs' },
    { name: '写入批量过小', metric: 'sinker_records_per_query_avg', operator: '<' as const, threshold: 50, level: 'minor' as const, unit: 'rows' },
    { name: '缓冲队列堆积', metric: 'pipeline_buffer_size_avg', operator: '>' as const, threshold: 500, level: 'major' as const, unit: 'records' },
    { name: '写入带宽骤降', metric: 'sinker_bps_avg_by_sec', operator: '<' as const, threshold: 1024 * 1024, level: 'minor' as const, unit: 'B/s' },
    { name: '记录大小异常', metric: 'pipeline_record_size_avg', operator: '>' as const, threshold: 1024 * 50, level: 'info' as const, unit: 'B' },
    { name: '目标端查询延迟', metric: 'sinker_rt_per_query_max', operator: '>' as const, threshold: 10000, level: 'critical' as const, unit: 'μs' },
  ];
  const o = pick(opts);
  return {
    id: id('rule'),
    name: o.name,
    metric: o.metric,
    operator: o.operator,
    threshold: o.threshold,
    level: o.level,
    status: Math.random() > 0.2 ? 'enabled' : 'disabled',
    periodMin: pick([1, 5, 10]),
    triggerCount: pick([1, 2, 3]),
    recoveryThreshold: Math.round(o.threshold * 0.8),
    description: `${o.name}（单位：${o.unit}）`,
  };
}

function buildLog(kind: 'op' | 'ctrl', idx: number, tasks: Task[]): OperateLog | ControlLog {
  if (kind === 'op') {
    return {
      id: id('op'),
      at: isoMinus(intBetween(0, 60 * 24 * 30)),
      user: pick(['admin', 'alice', 'bob', 'carol']),
      ip: `10.${intBetween(10, 250)}.${intBetween(0, 250)}.${intBetween(1, 254)}`,
      action: pick(['登录', '创建任务', '编辑任务', '删除任务', '修改告警规则', '修改全局参数', '导出日志']),
      target: pick(['User', 'Task', 'Alert', 'MetricRule', 'GlobalParam']),
      result: Math.random() > 0.05 ? 'success' : 'failure',
      detail: '操作已记录',
    };
  }
  const t = pick(tasks);
  return {
    id: id('ctrl'),
    at: isoMinus(intBetween(0, 60 * 24 * 30)),
    taskId: t.id,
    taskName: t.name,
    action: pick(['start', 'stop', 'pause', 'resume', 'edit', 'delete']),
    operator: pick(['admin', 'alice', 'bob', 'carol']),
    result: Math.random() > 0.05 ? 'success' : 'failure',
    detail: '',
  };
}

function buildLicense(status: License['status']): License {
  const issued = isoMinus(intBetween(90, 720) * 24 * 60);
  const expire =
    status === 'perpetual' ? '2099-12-31T23:59:59.000Z'
    : status === 'expired' ? isoMinus(intBetween(1, 60) * 24 * 60)
    : status === 'expiring' ? new Date(Date.now() + intBetween(1, 14) * 86400_000).toISOString()
    : new Date(Date.now() + intBetween(90, 365) * 86400_000).toISOString();
  return {
    id: id('lic'),
    sku: pick(['APE-ENT-PROD', 'APE-STD', 'APE-LITE']),
    issuedTo: 'ape-dts 演示环境',
    maxTasks: pick([50, 100, 200, 500]),
    issuedAt: issued,
    expireAt: expire,
    status,
  };
}

function buildHost(idx: number): SystemHost {
  return {
    id: id('host'),
    hostname: `ape-dts-${pick(['node', 'work', 'ctrl'])}-${String(idx + 1).padStart(2, '0')}`,
    ip: `10.250.0.${intBetween(10, 250)}`,
    role: pick(['master', 'worker', 'worker', 'worker', 'manager']),
    nodeType: pick(['physical', 'virtual', 'container']),
    status: Math.random() > 0.85 ? 'warning' : Math.random() > 0.97 ? 'error' : 'healthy',
    cpuPercent: floatBetween(5, 85, 1),
    memoryPercent: floatBetween(20, 90, 1),
    diskPercent: floatBetween(30, 85, 1),
    uptime: intBetween(3600, 86400 * 120),
  };
}

/* ========== seed ========== */

interface Db {
  snapshotTasks: Task[];
  cdcTasks: Task[];
  checkTasks: Task[];
  structTasks: Task[];
  activeAlerts: Alert[];
  historyAlerts: Alert[];
  events: SysEvent[];
  metricRules: MetricRule[];
  alarmChannels: AlarmChannel[];
  alarmTemplates: AlarmTemplate[];
  operateLogs: OperateLog[];
  controlLogs: ControlLog[];
  licenses: License[];
  license: License | null;
  resourceGroups: ResourceGroup[];
  users: User[];
  hosts: SystemHost[];
  globalParams: GlobalParam[];
  metricsByTask: Record<string, Record<string, MetricSeries>>;
}

function seed(): Db {
  const snapshotTasks: Task[] = Array.from({ length: 18 }, (_, i) => makeTask(i, 'snapshot'));
  const cdcTasks: Task[] = Array.from({ length: 14 }, (_, i) => makeTask(i, 'cdc'));
  const checkTasks: Task[] = Array.from({ length: 6 }, (_, i) => makeTask(i, 'check'));
  const structTasks: Task[] = Array.from({ length: 4 }, (_, i) => makeTask(i, 'struct'));
  // Force at least a few predictable statuses per category for demo richness.
  if (snapshotTasks.length >= 4) {
    snapshotTasks[0].status = 'running'; snapshotTasks[0].metrics = buildMetricsSnapshot('running');
    snapshotTasks[1].status = 'paused'; snapshotTasks[1].metrics = buildMetricsSnapshot('paused');
    snapshotTasks[2].status = 'failed'; snapshotTasks[2].metrics = buildMetricsSnapshot('failed');
    snapshotTasks[3].status = 'completed'; snapshotTasks[3].metrics = buildMetricsSnapshot('completed');
    setTaskRoute(snapshotTasks[0], 'mysql', 'gaussdb', 'orders_mysql_to_gaussdb_prd');
    setTaskRoute(snapshotTasks[1], 'oracle', 'gaussdb', 'billing_oracle_to_gaussdb_uat');
    setTaskRoute(snapshotTasks[2], 'postgres', 'clickhouse', 'events_postgres_to_clickhouse_prd');
    setTaskRoute(snapshotTasks[3], 'mongo', 'postgres', 'profiles_mongo_to_postgres_archive');
  }
  if (cdcTasks.length >= 2) {
    cdcTasks[0].status = 'running'; cdcTasks[0].metrics = buildMetricsSnapshot('running');
    cdcTasks[1].status = 'paused'; cdcTasks[1].metrics = buildMetricsSnapshot('paused');
    setTaskRoute(cdcTasks[0], 'mysql', 'kafka', 'orders_mysql_to_kafka_cdc_prd');
    setTaskRoute(cdcTasks[1], 'gaussdb', 'postgres', 'ledger_gaussdb_to_postgres_cdc');
  }

  const allTasks = [...snapshotTasks, ...cdcTasks, ...checkTasks, ...structTasks];

  const activeAlerts = Array.from({ length: 12 }, (_, i) => buildAlert(i, allTasks, true));
  const historyAlerts = Array.from({ length: 200 }, (_, i) => buildAlert(i, allTasks, false));

  const events = Array.from({ length: 300 }, (_, i) => buildEvent(i));
  const metricRules = Array.from({ length: 18 }, () => buildMetricRule());

  const alarmChannels: AlarmChannel[] = [
    {
      id: id('chan'), name: '主 Kafka 通道', kind: 'kafka', enabled: true,
      startAt: new Date().toISOString(), endAt: '2099-12-31T23:59:59.000Z', periodMin: 1,
      kafka: { brokers: '10.250.0.11:9092,10.250.0.12:9092', topic: 'ape-dts-alarm', ssl: true, distinguishType: true },
    },
    {
      id: id('chan'), name: '备 SNMP 通道', kind: 'snmp', enabled: false,
      startAt: new Date().toISOString(), endAt: '2099-12-31T23:59:59.000Z', periodMin: 5,
      snmp: { agent: '10.250.0.50:162', community: 'public', version: 'v2c' },
    },
  ];

  const alarmTemplates: AlarmTemplate[] = [
    { id: id('tpl'), name: '默认告警模板', level: 'critical', subject: '[ape-dts] {{level}} · {{taskName}}', body: '任务 {{taskName}} 触发告警：{{message}}，来源 {{source}}。', updatedAt: isoMinus(intBetween(10, 1000)) },
    { id: id('tpl'), name: '精简模板', level: 'minor', subject: '[ape-dts] {{taskName}} · {{message}}', body: '{{message}}', updatedAt: isoMinus(intBetween(10, 1000)) },
    { id: id('tpl'), name: '合规告警模板', level: 'major', subject: '[合规] {{taskName}} · {{level}}', body: '审计信息：{{detail}}', updatedAt: isoMinus(intBetween(10, 1000)) },
  ];

  const operateLogs = Array.from({ length: 150 }, (_, i) => buildLog('op', i, allTasks)) as OperateLog[];
  const controlLogs = Array.from({ length: 100 }, (_, i) => buildLog('ctrl', i, allTasks)) as ControlLog[];

  const licenses: License[] = [
    buildLicense('perpetual'),
    buildLicense('expiring'),
    buildLicense('expiring'),
    buildLicense('expired'),
  ];

  const resourceGroups: ResourceGroup[] = RESOURCE_GROUPS.map((n, i) => ({
    id: id('rg'),
    name: n,
    description: ['默认资源组', '生产资源组', '预发布资源组', '开发资源组'][i],
    taskCount: allTasks.filter((t) => t.resourceGroup === n).length,
  }));

  const users: User[] = [
    { id: id('u'), username: 'admin', displayName: '超级管理员', role: 'admin', email: 'admin@ape-dts.local', lastLoginAt: new Date().toISOString() },
    { id: id('u'), username: 'alice', displayName: 'Alice', role: 'operator', email: 'alice@ape-dts.local', lastLoginAt: isoMinus(60) },
    { id: id('u'), username: 'bob', displayName: 'Bob', role: 'operator', email: 'bob@ape-dts.local', lastLoginAt: isoMinus(360) },
    { id: id('u'), username: 'carol', displayName: 'Carol', role: 'viewer', email: 'carol@ape-dts.local', lastLoginAt: isoMinus(1440) },
    { id: id('u'), username: 'dave', displayName: 'Dave', role: 'viewer', email: 'dave@ape-dts.local', lastLoginAt: isoMinus(2880) },
  ];

  const hosts: SystemHost[] = Array.from({ length: 6 }, (_, i) => buildHost(i));

  const globalParams: GlobalParam[] = [
    { key: 'checkpoint_interval_secs', value: '10', description: '断点提交间隔', nameZh: '断点提交间隔', nameEn: 'Checkpoint interval (secs)', descZh: '断点提交间隔', descEn: 'Checkpoint interval in seconds', category: 'runtime', updatedAt: isoMinus(1000) },
    { key: 'max_parallel_tasks', value: '20', description: '单集群最大并行任务数', nameZh: '单集群最大并行任务数', nameEn: 'Max parallel tasks per cluster', descZh: '单集群最大并行任务数', descEn: 'Max parallel tasks per cluster', category: 'runtime', updatedAt: isoMinus(2000) },
    { key: 'default_buffer_memory_mb', value: '200', description: '默认内存缓冲', nameZh: '默认内存缓冲', nameEn: 'Default buffer memory (MB)', descZh: '默认内存缓冲', descEn: 'Default buffer memory in MB', category: 'pipeline', updatedAt: isoMinus(500) },
    { key: 'metric_retention_days', value: '30', description: '指标保留天数', nameZh: '指标保留天数', nameEn: 'Metric retention days', descZh: '指标保留天数', descEn: 'Metric retention days', category: 'runtime', updatedAt: isoMinus(100) },
    { key: 'alarm_default_channel', value: '主 Kafka 通道', description: '默认告警外发通道', nameZh: '默认告警外发通道', nameEn: 'Default alarm channel', descZh: '默认告警外发通道', descEn: 'Default alarm dispatch channel', category: 'alarm', updatedAt: isoMinus(700) },
    { key: 'session_timeout_min', value: '60', description: '登录会话超时', nameZh: '登录会话超时', nameEn: 'Session timeout (min)', descZh: '登录会话超时', descEn: 'Session timeout in minutes', category: 'security', updatedAt: isoMinus(100) },
    { key: 'audit_log_days', value: '180', description: '审计日志保留天数', nameZh: '审计日志保留天数', nameEn: 'Audit log retention days', descZh: '审计日志保留天数', descEn: 'Audit log retention days', category: 'security', updatedAt: isoMinus(300) },
    { key: 'log_level', value: 'info', description: '日志级别', nameZh: '日志级别', nameEn: 'Log level', descZh: '日志级别', descEn: 'Log level', category: 'runtime', updatedAt: isoMinus(10) },
  ];

  // Build metric series only for running tasks (performance)
  const metricsByTask: Record<string, Record<string, MetricSeries>> = {};
  const METRICS: { name: string; base: number; amp: number }[] = [
    { name: 'extractor_pushed_rps_avg', base: 600, amp: 180 },
    { name: 'sinker_record_count_avg_by_sec', base: 580, amp: 180 },
    { name: 'sinker_rt_per_query_avg', base: 2800, amp: 1200 },
    { name: 'pipeline_buffer_size_avg', base: 30, amp: 20 },
    { name: 'extractor_pushed_bps_avg', base: 320_000, amp: 120_000 },
    { name: 'latency_ms', base: 900, amp: 500 },
  ];
  for (const t of allTasks) {
    if (t.status !== 'running' && t.status !== 'paused' && t.status !== 'failed') continue;
    const series: Record<string, MetricSeries> = {};
    const scale = jitter(1, 0.5);
    for (const m of METRICS) {
      series[m.name] = buildMetricSeries(t.id, m.name, m.base * scale, m.amp * Math.abs(scale));
    }
    metricsByTask[t.id] = series;
  }

  return {
    snapshotTasks, cdcTasks, checkTasks, structTasks,
    activeAlerts, historyAlerts, events, metricRules,
    alarmChannels, alarmTemplates,
    operateLogs, controlLogs, licenses, license: licenses[0] ?? null, resourceGroups, users, hosts, globalParams,
    metricsByTask,
  };
}

export const db: Db = seed();

/* ========== helpers used by handlers ========== */

export function tasksOf(category: TaskCategory): Task[] {
  switch (category) {
    case 'snapshot': return db.snapshotTasks;
    case 'cdc': return db.cdcTasks;
    case 'check': return db.checkTasks;
    case 'struct': return db.structTasks;
  }
}

export function allTasks(): Task[] {
  return [...db.snapshotTasks, ...db.cdcTasks, ...db.checkTasks, ...db.structTasks];
}

export function findTask(id: string): Task | undefined {
  return db.snapshotTasks.find((t) => t.id === id)
    ?? db.cdcTasks.find((t) => t.id === id)
    ?? db.checkTasks.find((t) => t.id === id)
    ?? db.structTasks.find((t) => t.id === id);
}

export function tickRunningMetrics(): void {
  // Called by the periodic tick handler to jitter metric snapshots so charts feel alive.
  const tasks = allTasks();
  for (const t of tasks) {
    if (t.status === 'running') {
      t.metrics.rpsLatest = Math.max(0, Math.round(jitter(t.metrics.rpsLatest || 500, 0.25)));
      t.metrics.sinkerRpsLatest = Math.max(0, Math.round(jitter(t.metrics.sinkerRpsLatest || 480, 0.25)));
      t.metrics.latencyMs = Math.max(0, Math.round(jitter(t.metrics.latencyMs || 900, 0.2)));
      t.metrics.queryRtUs = Math.max(0, Math.round(jitter(t.metrics.queryRtUs || 2800, 0.2)));
      t.metrics.bufferSize = Math.max(0, Math.round(jitter(t.metrics.bufferSize || 20, 0.3)));
      t.metrics.processedRecords += t.metrics.sinkerRpsLatest || 0;
      t.progressPercent = Math.min(99.5, t.progressPercent + (Math.random() * 0.05));
    }
    t.lastHeartbeatAt = new Date().toISOString();
  }
  // Append latest points to series so charts push forward naturally.
  const now = Math.floor(Date.now() / 60_000) * 60_000;
  for (const t of tasks) {
    const seriesGroup = db.metricsByTask[t.id];
    if (!seriesGroup) continue;
    for (const [, s] of Object.entries(seriesGroup)) {
      if (s.points[s.points.length - 1]?.t === now) continue;
      const base = s.points[s.points.length - 1]?.v ?? 0;
      const next = Math.max(0, Math.round(jitter(base || 100, 0.15) * 100) / 100);
      s.points.push({ t: now, v: next });
      if (s.points.length > 24 * 60) s.points.shift();
    }
  }
}

// Tick every 5s so any running client sees fresh values.
setInterval(tickRunningMetrics, 5000);
