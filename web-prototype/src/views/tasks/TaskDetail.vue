<template>
  <div class="detail">
    <header class="detail__header">
      <div class="detail__header-left">
        <el-button link @click="onBack">
          <IconArrowLeft /> {{ t('taskDetail.back') }}
        </el-button>
        <span class="detail__sep">|</span>
        <h1 class="detail__title">{{ task?.name ?? '—' }}</h1>
        <StatusBadge v-if="task" :status="task.status" />
      </div>
      <div class="detail__header-right">
        <el-button
          v-if="task?.status === 'running'"
          @click="doAction('pause')"
        >
          <template #icon><IconPlayerPause /></template>
          {{ t('taskDetail.action.pause') }}
        </el-button>
        <el-button
          v-else-if="task?.status === 'paused' || task?.status === 'failed'"
          type="primary"
          @click="doAction('resume')"
        >
          <template #icon><IconPlayerPlay /></template>
          {{ t('taskDetail.action.resume') }}
        </el-button>
        <el-button v-if="task && task.status !== 'completed'" @click="doAction('stop')">
          <template #icon><IconPlayerStop /></template>
          {{ t('taskDetail.action.stop') }}
        </el-button>
        <el-button type="primary" plain @click="editorVisible = true">
          <template #icon><IconEdit /></template>
          {{ t('taskDetail.action.edit') }}
        </el-button>
        <el-button type="danger" plain @click="confirmDelete">
          <template #icon><IconTrash /></template>
          {{ t('taskDetail.action.delete') }}
        </el-button>
      </div>
    </header>

    <div v-if="task" class="detail__body drs-page">
      <section class="detail__flow drs-card">
        <div class="detail__flow-side">
          <span class="detail__flow-label">源端</span>
          <EngineTag :engine="task.source.engine" />
          <code>{{ task.source.host }}:{{ task.source.port }}</code>
          <small>{{ task.source.database || '—' }}</small>
        </div>
        <div class="detail__flow-mid">
          <span class="detail__flow-rate">{{ formatShort(task.metrics.rpsLatest) }} rows/s</span>
          <div class="detail__flow-line">
            <span class="detail__flow-dot" />
            <span class="detail__flow-dot" />
            <span class="detail__flow-dot" />
          </div>
          <span class="detail__flow-lag">lag {{ task.metrics.latencyMs }} ms</span>
        </div>
        <div class="detail__flow-side">
          <span class="detail__flow-label">目标端</span>
          <EngineTag :engine="task.target.engine" />
          <code>{{ task.target.host }}:{{ task.target.port }}</code>
          <small>{{ task.target.database || '—' }}</small>
        </div>
        <div class="detail__flow-meta">
          <div><span>模式</span><strong>{{ t(`taskList.mode.${task.syncMode}`) }}</strong></div>
          <div><span>资源组</span><strong>{{ task.resourceGroup }}</strong></div>
          <div><span>实例 IP</span><strong>{{ task.instanceIp }}</strong></div>
          <div><span>创建于</span><strong>{{ dayjs(task.createdAt).format('YYYY-MM-DD HH:mm') }}</strong></div>
        </div>
      </section>

      <el-tabs v-model="activeTab" class="detail__tabs drs-card">
        <!-- Overview -->
        <el-tab-pane :label="t('taskDetail.tab.overview')" name="overview">
          <div class="detail__kpi">
            <KpiCard :label="t('taskDetail.kpi.rps')" :value="task.metrics.rpsLatest" unit="rows/s" :icon-comp="IconBolt" />
            <KpiCard :label="t('taskDetail.kpi.sinkRps')" :value="task.metrics.sinkerRpsLatest" unit="rows/s" :icon-comp="IconArrowDown" />
            <KpiCard :label="t('taskDetail.kpi.latency')" :value="task.metrics.latencyMs" unit="ms" inverse :icon-comp="IconClock" :tone="task.metrics.latencyMs > 3000 ? 'warning' : 'default'" />
            <KpiCard :label="t('taskDetail.kpi.buffer')" :value="task.metrics.bufferSize" :icon-comp="IconStack" />
            <KpiCard :label="t('taskDetail.kpi.processed')" :value="task.metrics.processedRecords" :icon-comp="IconDatabase" />
            <KpiCard :label="t('taskDetail.kpi.errors')" :value="task.metrics.errorCount" :icon-comp="IconAlertTriangle" :tone="task.metrics.errorCount > 0 ? 'danger' : 'default'" />
          </div>

          <div class="detail__charts">
            <ChartCard :title="t('taskDetail.chart.rps')" :height="240">
              <v-chart v-if="rpsOption" :option="rpsOption" autoresize class="detail__chart" />
            </ChartCard>
            <ChartCard :title="t('taskDetail.chart.latency')" :height="240">
              <v-chart v-if="latencyOption" :option="latencyOption" autoresize class="detail__chart" />
            </ChartCard>
            <ChartCard :title="t('taskDetail.chart.buffer')" :height="240">
              <v-chart v-if="bufferOption" :option="bufferOption" autoresize class="detail__chart" />
            </ChartCard>
          </div>
        </el-tab-pane>

        <!-- Config -->
        <el-tab-pane :label="t('taskDetail.tab.config')" name="config">
          <div class="detail__config">
            <dl>
              <dt>并行模式</dt><dd>{{ task.config.parallelizer }}</dd>
              <dt>并行度</dt><dd>{{ task.config.parallelSize }}</dd>
              <dt>缓冲区</dt><dd>{{ task.config.bufferSize }} rows</dd>
              <dt>断点提交间隔</dt><dd>{{ task.config.checkpointIntervalSecs }} s</dd>
              <dt>最大 RPS</dt><dd>{{ task.config.maxRps || '不限速' }}</dd>
              <dt>续传策略</dt><dd>{{ task.config.resumeType }}</dd>
              <dt>Prometheus</dt><dd>{{ task.config.metricsEnabled ? '已启用' : '未启用' }}</dd>
              <dt>同步对象</dt><dd>{{ task.syncObjects.selectedTables }} / {{ task.syncObjects.totalTables }}</dd>
            </dl>
          </div>
        </el-tab-pane>

        <!-- Objects -->
        <el-tab-pane :label="t('taskDetail.tab.objects')" name="objects">
          <el-table :data="objectRows" class="detail__objects">
            <el-table-column :label="t('taskDetail.objects.col.name')" min-width="240">
              <template #default="{ row }">
                <span class="detail__mono">{{ row.name }}</span>
              </template>
            </el-table-column>
            <el-table-column :label="t('taskDetail.objects.col.type')" width="120" prop="type" />
            <el-table-column :label="t('taskDetail.objects.col.rows')" width="140" align="right">
              <template #default="{ row }">
                <span class="tabular-nums">{{ row.rows.toLocaleString() }}</span>
              </template>
            </el-table-column>
            <el-table-column :label="t('taskDetail.objects.col.status')" width="140">
              <template #default="{ row }">
                <el-tag :type="row.status === '同步中' ? 'success' : row.status === '已完成' ? 'info' : 'warning'" size="small">
                  {{ row.status }}
                </el-tag>
              </template>
            </el-table-column>
          </el-table>
        </el-tab-pane>

        <!-- Logs -->
        <el-tab-pane :label="t('taskDetail.tab.logs')" name="logs">
          <div class="detail__log-filters">
            <el-segmented v-model="logLevel" :options="logLevelOptions" />
            <div class="detail__log-refresh">
              <el-switch v-model="logAuto" :active-text="t('taskDetail.log.autoRefresh')" />
              <el-button :loading="loading" @click="loadLogs">
                <template #icon><IconRefresh /></template>
                {{ t('common.refresh') }}
              </el-button>
            </div>
          </div>
          <div class="detail__log-view">
            <div
              v-for="(ln, i) in filteredLogs"
              :key="i"
              class="detail__log-line"
              :class="`detail__log-line--${ln.level.toLowerCase()}`"
            >
              <span class="detail__log-time">{{ dayjs(ln.t).format('HH:mm:ss') }}</span>
              <span class="detail__log-level">{{ ln.level }}</span>
              <span class="detail__log-source">{{ ln.source }}</span>
              <span class="detail__log-msg">{{ ln.message }}</span>
            </div>
          </div>
        </el-tab-pane>

        <!-- Alerts -->
        <el-tab-pane :label="t('taskDetail.tab.alerts')" name="alerts">
          <el-table v-if="alerts.length" :data="alerts" class="detail__alerts">
            <el-table-column label="级别" width="110">
              <template #default="{ row }"><LevelBadge :level="row.level" /></template>
            </el-table-column>
            <el-table-column label="来源" width="120" prop="source" />
            <el-table-column label="消息" prop="message" />
            <el-table-column label="服务" width="140" prop="service" />
            <el-table-column label="首次发生" width="170">
              <template #default="{ row }">{{ dayjs(row.firstAt).format('MM-DD HH:mm:ss') }}</template>
            </el-table-column>
            <el-table-column label="次数" width="80" align="right">
              <template #default="{ row }">{{ row.count }}</template>
            </el-table-column>
          </el-table>
          <el-empty v-else :description="t('taskDetail.alerts.none')" />
        </el-tab-pane>
      </el-tabs>
    </div>

    <div v-else class="detail__loading">
      <el-skeleton :rows="6" animated />
    </div>

    <!-- Edit drawer -->
    <el-drawer
      v-model="editorVisible"
      :title="t('taskDetail.editor.title')"
      size="520px"
      direction="rtl"
    >
      <div v-if="task" class="detail__editor">
        <el-alert type="info" :closable="false" show-icon>{{ t('taskDetail.editor.tip') }}</el-alert>

        <div class="detail__editor-form">
          <label>任务名称</label>
          <el-input v-model="editForm.name" disabled />
          <label>描述</label>
          <el-input v-model="editForm.description" type="textarea" :rows="2" />
          <label>资源组</label>
          <el-select v-model="editForm.resourceGroup" style="width: 100%">
            <el-option v-for="g in resourceGroups" :key="g" :label="g" :value="g" />
          </el-select>
          <label>并行度</label>
          <el-input-number v-model="editForm.config.parallelSize" :min="1" :max="64" style="width: 100%" />
          <label>缓冲区</label>
          <el-input-number v-model="editForm.config.bufferSize" :min="1000" :max="200000" :step="1000" style="width: 100%" />
          <label>断点间隔（秒）</label>
          <el-input-number v-model="editForm.config.checkpointIntervalSecs" :min="1" :max="600" style="width: 100%" />
          <label>最大 RPS (0 = 不限速)</label>
          <el-input-number v-model="editForm.config.maxRps" :min="0" :max="1000000" :step="500" style="width: 100%" />
          <label>续传策略</label>
          <el-select v-model="editForm.config.resumeType" style="width: 100%">
            <el-option label="from_log" value="from_log" />
            <el-option label="from_target" value="from_target" />
            <el-option label="from_db" value="from_db" />
          </el-select>
        </div>
      </div>
      <template #footer>
        <el-button @click="editorVisible = false">取消</el-button>
        <el-button type="primary" :loading="saving" @click="saveEdit">保存</el-button>
      </template>
    </el-drawer>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, onUnmounted, reactive, ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';
import { useRoute, useRouter } from 'vue-router';
import { ElMessage, ElMessageBox } from 'element-plus';
import dayjs from 'dayjs';
import { api } from '@/api/client';
import type { Task, Alert, MetricSeries } from '@/types/domain';
import KpiCard from '@/components/KpiCard.vue';
import ChartCard from '@/components/ChartCard.vue';
import LevelBadge from '@/components/LevelBadge.vue';
import StatusBadge from '@/components/StatusBadge.vue';
import EngineTag from '@/components/EngineTag.vue';
import '@/composables/useEcharts';
import { BRAND_PALETTE, AXIS_BASE } from '@/composables/useEcharts';
import IconBolt from '~icons/tabler/bolt';
import IconArrowDown from '~icons/tabler/arrow-down';
import IconClock from '~icons/tabler/clock';
import IconStack from '~icons/tabler/stack-2';
import IconDatabase from '~icons/tabler/database';
import IconAlertTriangle from '~icons/tabler/alert-triangle';

const { t } = useI18n();
const route = useRoute();
const router = useRouter();

const taskId = computed(() => String(route.params.id));

const task = ref<Task | null>(null);
const series = ref<MetricSeries[]>([]);
const alerts = ref<Alert[]>([]);
const logs = ref<{ t: string; level: string; source: string; message: string }[]>([]);
const loading = ref(false);
const activeTab = ref<string>((route.query.tab as string) || 'overview');
const editorVisible = ref(false);
const saving = ref(false);
const resourceGroups = ['default', 'production', 'staging', 'dev'];

const editForm = reactive({
  name: '',
  description: '',
  resourceGroup: 'default',
  config: { parallelSize: 4, bufferSize: 16000, checkpointIntervalSecs: 10, maxRps: 0, resumeType: 'from_log' as Task['config']['resumeType'] },
});

watch(task, (t) => {
  if (!t) return;
  editForm.name = t.name;
  editForm.description = t.description ?? '';
  editForm.resourceGroup = t.resourceGroup;
  editForm.config.parallelSize = t.config.parallelSize;
  editForm.config.bufferSize = t.config.bufferSize;
  editForm.config.checkpointIntervalSecs = t.config.checkpointIntervalSecs;
  editForm.config.maxRps = t.config.maxRps;
  editForm.config.resumeType = t.config.resumeType;
}, { immediate: true });

/* load */
async function loadTask() {
  try {
    task.value = await api.get<Task>(`/tasks/${taskId.value}`);
  } catch {
    ElMessage.error('任务不存在');
    router.push(backToListPath());
  }
}

function backToListPath(): string {
  const cat = task.value?.category ?? (route.params.category as string | undefined);
  if (cat === 'check') return '/tasks/check';
  if (cat === 'struct') return '/tasks/struct';
  return '/tasks/sync';
}

async function loadMetrics() {
  try {
    const res = await api.get<{ series: MetricSeries[] }>(`/tasks/${taskId.value}/metrics`);
    series.value = res.series ?? [];
  } catch { /* ignore */ }
}

async function loadAlerts() {
  try {
    const res = await api.get<{ items: Alert[] }>(`/alerts/active?taskId=${taskId.value}`);
    alerts.value = res.items ?? [];
  } catch { /* ignore */ }
}

async function loadLogs() {
  loading.value = true;
  try {
    const url = logLevel.value === 'ALL' ? `/tasks/${taskId.value}/logs` : `/tasks/${taskId.value}/logs?level=${logLevel.value}`;
    const res = await api.get<{ lines: typeof logs.value }>(url);
    logs.value = res.lines ?? [];
  } finally { loading.value = false; }
}

/* charts */
function baseLine(title: string, xs: string[], series: { name: string; data: number[]; color: string }[]) {
  return {
    grid: { left: 36, right: 16, top: 18, bottom: 22 },
    tooltip: { trigger: 'axis' as const },
    legend: { bottom: 0, icon: 'roundRect', itemWidth: 8, itemHeight: 8, textStyle: { color: '#64748B', fontSize: 11 } },
    xAxis: { type: 'category', data: xs, axisLine: AXIS_BASE.axisLine, axisLabel: { ...AXIS_BASE.axisLabel, fontSize: 10 }, axisTick: { show: false } },
    yAxis: { type: 'value', axisLine: { show: false }, axisLabel: AXIS_BASE.axisLabel, splitLine: AXIS_BASE.splitLine },
    series: series.map((s) => ({
      name: s.name,
      type: 'line' as const,
      data: s.data,
      smooth: true,
      symbol: 'none',
      lineStyle: { width: 1.6, color: s.color },
      areaStyle: { color: s.color, opacity: 0.08 },
    })),
  };
}

const rpsOption = computed(() => {
  const ex = series.value.find((s) => s.metric === 'extractor_pushed_rps_avg');
  const sk = series.value.find((s) => s.metric === 'sinker_record_count_avg_by_sec');
  if (!ex) return null;
  const xs = ex.points.map((p) => dayjs(p.t).format('HH:mm'));
  return baseLine('RPS', xs, [
    { name: '抽取 RPS', data: ex.points.map((p) => p.v), color: BRAND_PALETTE[0] },
    ...(sk ? [{ name: '写入 RPS', data: sk.points.map((p) => p.v), color: BRAND_PALETTE[1] }] : []),
  ]);
});

const latencyOption = computed(() => {
  const la = series.value.find((s) => s.metric === 'latency_ms');
  if (!la) return null;
  const xs = la.points.map((p) => dayjs(p.t).format('HH:mm'));
  return baseLine('Latency', xs, [{ name: '延迟', data: la.points.map((p) => p.v), color: BRAND_PALETTE[2] }]);
});

const bufferOption = computed(() => {
  const bf = series.value.find((s) => s.metric === 'pipeline_buffer_size_avg');
  if (!bf) return null;
  const xs = bf.points.map((p) => dayjs(p.t).format('HH:mm'));
  return baseLine('Buffer', xs, [{ name: '缓冲队列', data: bf.points.map((p) => p.v), color: BRAND_PALETTE[4] }]);
});

/* objects sample */
const objectRows = computed(() => {
  if (!task.value) return [];
  const n = task.value.syncObjects.selectedTables;
  const tables = ['orders', 'users', 'payments', 'products', 'shipments', 'inventory', 'logs'];
  return Array.from({ length: Math.min(n, 8) }, (_, i) => ({
    name: `${task.value!.source.database ?? 'app_db'}.${tables[i % tables.length]}`,
    type: '表',
    rows: Math.round(100_000 * Math.random() * (i + 1)),
    status: task.value!.status === 'completed' ? '已完成'
      : task.value!.status === 'running' ? '同步中'
      : task.value!.status === 'failed' ? '失败' : '等待中',
  }));
});

/* logs */
const logLevel = ref('ALL');
const logLevelOptions = ['ALL', 'INFO', 'WARN', 'ERROR', 'DEBUG'].map((v) => ({ label: v, value: v }));
const logAuto = ref(true);
const filteredLogs = computed(() => logs.value.slice(0, 200));
watch(logLevel, loadLogs);

/* actions */
async function doAction(action: string) {
  try {
    await api.post(`/tasks/${taskId.value}/action`, { action });
    ElMessage.success('操作成功');
    await loadTask();
  } catch { ElMessage.error('操作失败'); }
}

function confirmDelete() {
  if (!task.value) return;
  ElMessageBox.confirm(
    `确定删除任务「${task.value.name}」？该操作不可撤销。`,
    '删除任务',
    { type: 'warning' },
  ).then(async () => {
    await api.del(`/tasks/${taskId.value}`);
    ElMessage.success('任务已删除');
    router.push(backToListPath());
  }).catch(() => {});
}

async function saveEdit() {
  saving.value = true;
  try {
    await api.patch(`/tasks/${taskId.value}`, {
      description: editForm.description,
      resourceGroup: editForm.resourceGroup,
      config: {
        ...(task.value?.config ?? {}),
        parallelSize: editForm.config.parallelSize,
        bufferSize: editForm.config.bufferSize,
        checkpointIntervalSecs: editForm.config.checkpointIntervalSecs,
        maxRps: editForm.config.maxRps,
        resumeType: editForm.config.resumeType,
      },
    });
    ElMessage.success('保存成功');
    editorVisible.value = false;
    await loadTask();
  } finally { saving.value = false; }
}

function onBack() {
  router.push(backToListPath());
}

function formatShort(v: number) {
  if (v >= 1000) return `${(v / 1000).toFixed(1)}k`;
  return String(v);
}

/* lifecycle */
let pollId: ReturnType<typeof setInterval> | null = null;
let logPollId: ReturnType<typeof setInterval> | null = null;

onMounted(async () => {
  await loadTask();
  loadMetrics();
  loadAlerts();
  loadLogs();
  pollId = setInterval(() => { loadTask(); loadMetrics(); }, 5000);
  logPollId = setInterval(() => { if (logAuto.value) loadLogs(); }, 8000);
});

onUnmounted(() => {
  if (pollId) clearInterval(pollId);
  if (logPollId) clearInterval(logPollId);
});
</script>

<style scoped>
.detail {
  display: flex;
  flex-direction: column;
  min-height: 100%;
}
.detail__header {
  background: var(--color-surface);
  border-bottom: 1px solid var(--color-border);
  padding: 12px 24px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  flex-wrap: wrap;
}
.detail__header-left {
  display: inline-flex;
  align-items: center;
  gap: 12px;
}
.detail__sep {
  color: var(--color-border);
}
.detail__title {
  margin: 0;
  font-size: var(--text-xl);
  font-weight: 600;
}
.detail__header-right {
  display: inline-flex;
  gap: 8px;
}
.detail__body {
  display: flex;
  flex-direction: column;
  gap: 16px;
}
.detail__flow {
  display: grid;
  grid-template-columns: 1fr 1.4fr 1fr;
  gap: 16px;
  padding: 16px 20px;
}
.detail__flow-side {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 6px;
  padding: 12px 16px;
  background: var(--color-primary-50);
  border: 1px solid var(--color-primary-200);
  border-radius: var(--radius-md);
}
.detail__flow-label {
  font-size: 11px;
  color: var(--color-primary-700);
  font-weight: 500;
}
.detail__flow-side code {
  font-family: var(--font-mono);
  font-size: 12px;
  color: var(--color-ink);
}
.detail__flow-side small {
  font-size: 12px;
  color: var(--color-ink-subtle);
}
.detail__flow-mid {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 6px;
  position: relative;
}
.detail__flow-rate {
  font-size: 13px;
  color: var(--color-primary-700);
  font-weight: 500;
}
.detail__flow-line {
  width: 100%;
  height: 2px;
  background: linear-gradient(to right, var(--color-primary-500), var(--color-accent));
  display: flex;
  align-items: center;
  justify-content: space-around;
  position: relative;
}
.detail__flow-dot {
  width: 6px;
  height: 6px;
  background: var(--color-accent);
  border-radius: 50%;
  animation: flow 1.6s infinite ease-in-out;
}
.detail__flow-dot:nth-child(2) { animation-delay: 0.3s; }
.detail__flow-dot:nth-child(3) { animation-delay: 0.6s; }
@keyframes flow {
  0% { transform: translateX(-10px) scale(0.8); opacity: 0.5; }
  50% { transform: translateX(0) scale(1); opacity: 1; }
  100% { transform: translateX(10px) scale(0.8); opacity: 0.5; }
}
.detail__flow-lag {
  font-size: 12px;
  color: var(--color-ink-subtle);
}
.detail__flow-meta {
  grid-column: 1 / -1;
  display: grid;
  grid-template-columns: repeat(4, 1fr);
  gap: 12px;
  padding-top: 12px;
  border-top: 1px solid var(--color-border);
}
.detail__flow-meta > div {
  display: flex;
  gap: 8px;
  font-size: 12px;
}
.detail__flow-meta span { color: var(--color-ink-subtle); }
.detail__flow-meta strong { color: var(--color-ink); font-weight: 500; }
.detail__tabs {
  padding: 8px 20px 20px;
}
.detail__kpi {
  display: grid;
  grid-template-columns: repeat(6, 1fr);
  gap: 12px;
  padding: 8px 0 16px;
}
@media (max-width: 1400px) { .detail__kpi { grid-template-columns: repeat(3, 1fr); } }
@media (max-width: 800px) { .detail__kpi { grid-template-columns: repeat(2, 1fr); } }
.detail__charts {
  display: grid;
  grid-template-columns: repeat(2, 1fr);
  gap: 12px;
}
.detail__charts :nth-child(3) {
  grid-column: 1 / -1;
}
.detail__chart {
  height: 100%;
}
.detail__config dl {
  display: grid;
  grid-template-columns: 180px 1fr;
  gap: 12px 24px;
  padding: 12px 0;
  margin: 0;
}
.detail__config dt {
  color: var(--color-ink-subtle);
  font-size: 13px;
}
.detail__config dd {
  margin: 0;
  color: var(--color-ink);
  font-size: 13px;
  font-family: var(--font-mono);
}
.detail__mono { font-family: var(--font-mono); font-size: 12px; }
.detail__log-filters {
  display: flex;
  justify-content: space-between;
  padding: 8px 0;
  gap: 12px;
  flex-wrap: wrap;
}
.detail__log-refresh {
  display: inline-flex;
  gap: 12px;
  align-items: center;
}
.detail__log-view {
  background: #0F172A;
  color: #CBD5E1;
  border-radius: var(--radius-md);
  padding: 8px 12px;
  font-family: var(--font-mono);
  font-size: 12px;
  max-height: 480px;
  overflow: auto;
}
.detail__log-line {
  display: grid;
  grid-template-columns: 80px 60px 120px 1fr;
  gap: 10px;
  padding: 3px 0;
}
.detail__log-line--warn .detail__log-level { color: #FBBF24; }
.detail__log-line--error .detail__log-level { color: #F87171; }
.detail__log-line--info .detail__log-level { color: #67E8F9; }
.detail__log-line--debug .detail__log-level { color: #94A3B8; }
.detail__log-time { color: #64748B; }
.detail__log-source { color: #A78BFA; }
.detail__log-msg { color: #E2E8F0; overflow-wrap: anywhere; }
.detail__editor {
  display: flex;
  flex-direction: column;
  gap: 12px;
  padding: 0 4px;
}
.detail__editor-form {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.detail__editor-form label {
  font-size: 13px;
  color: var(--color-ink-muted);
  margin-top: 6px;
}
.detail__loading {
  padding: 40px 24px;
}
</style>
