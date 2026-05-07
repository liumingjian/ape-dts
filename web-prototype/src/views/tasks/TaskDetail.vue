<template>
  <div class="detail">
    <!-- persistent action bar -->
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
          v-if="rbac.can('task.start') && canStart"
          type="success"
          @click="doLifecycle('start')"
        >
          <template #icon><IconPlayerPlay /></template>
          {{ t('taskDetail.action.start') }}
        </el-button>
        <el-button
          v-if="rbac.can('task.pause') && task?.status === 'running'"
          @click="doLifecycle('pause')"
        >
          <template #icon><IconPlayerPause /></template>
          {{ t('taskDetail.action.pause') }}
        </el-button>
        <el-button
          v-if="rbac.can('task.resume') && (task?.status === 'paused')"
          type="primary"
          @click="doLifecycle('resume')"
        >
          <template #icon><IconPlayerPlay /></template>
          {{ t('taskDetail.action.resume') }}
        </el-button>
        <el-button
          v-if="rbac.can('task.stop') && canStop"
          @click="confirmStop"
        >
          <template #icon><IconPlayerStop /></template>
          {{ t('taskDetail.action.stop') }}
        </el-button>
        <el-button
          v-if="rbac.can('task.delete')"
          type="danger"
          plain
          @click="confirmDelete"
        >
          <template #icon><IconTrash /></template>
          {{ t('taskDetail.action.delete') }}
        </el-button>
        <el-button
          v-if="rbac.can('task.create')"
          type="primary"
          plain
          @click="editorVisible = true"
        >
          <template #icon><IconEdit /></template>
          {{ t('taskDetail.action.edit') }}
        </el-button>
      </div>
    </header>

    <div v-if="task" class="detail__body ape-dts-console-page">
      <!-- KPI strip + flow diagram -->
      <section class="detail__flow ape-dts-console-card">
        <div class="detail__kpi-row">
          <KpiCard :label="t('taskDetail.kpi.status')" :value="0" :badge="t(`task.status.${task.status}`)" :icon-comp="IconActivity" />
          <KpiCard :label="t('taskDetail.kpi.rps')" :value="task.metrics.rpsLatest" unit="rows/s" :icon-comp="IconBolt" />
          <KpiCard :label="t('taskDetail.kpi.latency')" :value="task.metrics.latencyMs" unit="ms" inverse :icon-comp="IconClock" :tone="task.metrics.latencyMs > 3000 ? 'warning' : 'default'" />
          <KpiCard :label="t('taskDetail.kpi.progress')" :value="Math.round(task.progressPercent)" unit="%" :icon-comp="IconChartBar" />
        </div>
      </section>

      <!-- 3 charts -->
      <section class="detail__charts ape-dts-console-card">
        <ChartCard :title="'extractor_rps_avg'" :height="200">
          <v-chart v-if="rpsOption" :option="rpsOption" autoresize class="detail__chart" />
          <el-empty v-else :description="t('common.empty')" :image-size="40" />
        </ChartCard>
        <ChartCard :title="'sinker_record_count_avg_by_sec'" :height="200">
          <v-chart v-if="sinkRpsOption" :option="sinkRpsOption" autoresize class="detail__chart" />
          <el-empty v-else :description="t('common.empty')" :image-size="40" />
        </ChartCard>
        <ChartCard :title="'pipeline_buffer_size_avg'" :height="200">
          <v-chart v-if="bufferOption" :option="bufferOption" autoresize class="detail__chart" />
          <el-empty v-else :description="t('common.empty')" :image-size="40" />
        </ChartCard>
      </section>

      <!-- 6 tabs -->
      <el-tabs v-model="activeTab" class="detail__tabs ape-dts-console-card" @tab-change="onTabChange as any">
        <!-- Config -->
        <el-tab-pane :label="t('taskDetail.tab.config')" name="config">
          <div class="detail__config">
            <dl>
              <dt>源端连接</dt><dd class="detail__mono">{{ task.sourceUrl || '—' }}</dd>
              <dt>目标连接</dt><dd class="detail__mono">{{ task.targetUrl || '—' }}</dd>
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

        <!-- Logs (SSE) -->
        <el-tab-pane :label="t('taskDetail.tab.logs')" name="logs">
          <div class="detail__log-toolbar">
            <div class="detail__log-toolbar-left">
              <el-select v-model="logFile" class="detail__log-file-select" @change="reopenLogStream">
                <el-option v-for="f in logFiles" :key="f" :label="f" :value="f" />
              </el-select>
              <el-select v-model="logLevelFilter" class="detail__log-level-select">
                <el-option label="ALL" value="ALL" />
                <el-option label="ERROR" value="error" />
                <el-option label="WARN" value="warn" />
                <el-option label="INFO" value="info" />
                <el-option label="DEBUG" value="debug" />
              </el-select>
              <span
                class="detail__log-status-pill"
                :class="`detail__log-status-pill--${sseState}`"
              >
                {{ sseStateLabel }}
              </span>
              <el-button
                v-if="sseState === 'disconnected'"
                size="small"
                @click="reopenLogStream"
              >
                {{ t('taskDetail.log.reconnect') }}
              </el-button>
            </div>
            <div class="detail__log-toolbar-right">
              <el-button
                size="small"
                :type="logPaused ? 'primary' : 'default'"
                @click="logPaused = !logPaused"
              >
                {{ logPaused ? t('taskDetail.log.resume') : t('taskDetail.log.pause') }}
              </el-button>
            </div>
          </div>
          <div ref="logPaneRef" class="detail__log-view" @scroll="onLogScroll">
            <div
              v-for="(ln, i) in filteredLogLines"
              :key="i"
              class="detail__log-line"
              :class="`detail__log-line--${ln.level}`"
            >
              <span class="detail__log-time">{{ formatLogTime(ln.ts) }}</span>
              <span class="detail__log-level">{{ ln.level.toUpperCase() }}</span>
              <span class="detail__log-msg">{{ ln.message }}</span>
            </div>
          </div>
          <div v-if="showFollowBtn" class="detail__log-follow">
            <el-button size="small" type="primary" @click="scrollToBottom">
              {{ t('taskDetail.log.follow') }}
            </el-button>
          </div>
        </el-tab-pane>

        <!-- Monitor -->
        <el-tab-pane :label="t('taskDetail.tab.monitor')" name="monitor">
          <div class="detail__monitor-toolbar">
            <el-button-group>
              <el-button
                v-for="r in monitorRanges"
                :key="r.value"
                :type="monitorRange === r.value ? 'primary' : 'default'"
                size="small"
                @click="setMonitorRange(r.value)"
              >
                {{ r.label }}
              </el-button>
            </el-button-group>
          </div>
          <div v-if="monitorLoading" class="detail__monitor-loading">
            <el-skeleton :rows="3" animated />
          </div>
          <div v-else-if="monitorSeries.length === 0" class="detail__monitor-empty">
            <el-empty :description="t('common.empty')" />
          </div>
          <div v-else class="detail__monitor-charts">
            <ChartCard v-for="ms in monitorSeries" :key="ms.metric" :title="ms.metric" :height="200">
              <v-chart :option="monitorChartOption(ms)" autoresize class="detail__chart" />
            </ChartCard>
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

        <!-- History -->
        <el-tab-pane :label="t('taskDetail.tab.history')" name="history">
          <el-table v-loading="historyLoading" :data="historyRuns" class="detail__history">
            <el-table-column label="Run ID" width="200" prop="id" />
            <el-table-column label="状态" width="120">
              <template #default="{ row }">
                <StatusBadge :status="row.status" />
              </template>
            </el-table-column>
            <el-table-column label="开始时间" width="170">
              <template #default="{ row }">{{ row.startedAt ? dayjs(row.startedAt).format('YYYY-MM-DD HH:mm:ss') : '—' }}</template>
            </el-table-column>
            <el-table-column label="结束时间" width="170">
              <template #default="{ row }">{{ row.stoppedAt ? dayjs(row.stoppedAt).format('YYYY-MM-DD HH:mm:ss') : '—' }}</template>
            </el-table-column>
            <el-table-column label="Exit Status" width="120" align="center">
              <template #default="{ row }">{{ row.exitStatus ?? '—' }}</template>
            </el-table-column>
            <el-table-column label="断点续传状态" min-width="200">
              <template #default="{ row }">
                <span v-if="row.position" class="detail__mono detail__position">{{ formatPosition(row.position) }}</span>
                <span v-else class="detail__muted">—</span>
              </template>
            </el-table-column>
            <el-table-column label="操作" width="120" fixed="right">
              <template #default="{ row }">
                <el-button link type="primary" @click="viewArchivedLogs(row)">
                  {{ t('taskDetail.history.viewLogs') }}
                </el-button>
              </template>
            </el-table-column>
          </el-table>
          <footer v-if="historyTotal > historyPageSize" class="detail__history-footer">
            <el-pagination
              v-model:current-page="historyPage"
              :page-size="historyPageSize"
              :total="historyTotal"
              layout="prev, pager, next"
              background
              @current-change="loadHistory"
            />
          </footer>
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
      @close="onEditorClose"
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

    <!-- Archived logs dialog -->
    <el-dialog v-model="archivedDialogVisible" :title="t('taskDetail.history.archivedLogs')" width="70%">
      <div v-loading="archivedLoading" class="detail__archived-log-view">
        <div
          v-for="(ln, i) in archivedLines"
          :key="i"
          class="detail__log-line"
          :class="`detail__log-line--${ln.level ?? 'info'}`"
        >
          <span class="detail__log-time">{{ formatLogTime(ln.ts) }}</span>
          <span class="detail__log-level">{{ (ln.level ?? 'info').toUpperCase() }}</span>
          <span class="detail__log-msg">{{ ln.message }}</span>
        </div>
      </div>
      <template #footer>
        <el-button @click="archivedDialogVisible = false">{{ t('common.close') }}</el-button>
      </template>
    </el-dialog>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, onUnmounted, reactive, ref, shallowRef, unref, watch } from 'vue';
import { useI18n } from 'vue-i18n';
import { useRoute, useRouter } from 'vue-router';
import { ElMessage, ElMessageBox } from 'element-plus';
import dayjs from 'dayjs';
import { api } from '@/api/client';
import { useRbac } from '@/composables/useRbac';
import { useDocumentVisibility } from '@/composables/useDocumentVisibility';
import { useLogStream, type LogLine, type LogStreamHandle } from '@/composables/useLogStream';
import type { Task, Alert, MetricQueryResponse, Run, RunPosition, ApiTask } from '@/types/domain';
import { mapApiTask } from '@/types/domain';
import KpiCard from '@/components/KpiCard.vue';
import ChartCard from '@/components/ChartCard.vue';
import LevelBadge from '@/components/LevelBadge.vue';
import StatusBadge from '@/components/StatusBadge.vue';
import '@/composables/useEcharts';
import { BRAND_PALETTE, AXIS_BASE } from '@/composables/useEcharts';
import IconBolt from '~icons/tabler/bolt';
import IconClock from '~icons/tabler/clock';
import IconChartBar from '~icons/tabler/chart-bar';
import IconActivity from '~icons/tabler/activity';

const { t } = useI18n();
const route = useRoute();
const router = useRouter();
const rbac = useRbac();
const { isVisible } = useDocumentVisibility();

const VALID_TABS = ['config', 'objects', 'logs', 'monitor', 'alerts', 'history'] as const;
type TabName = (typeof VALID_TABS)[number];

const taskId = computed(() => String(route.params.id));
const taskCategory = computed(() => String(route.params.category ?? 'snapshot'));

const task = ref<Task | null>(null);
const activeTab = ref<TabName>((route.query.tab as TabName) || 'config');
const editorVisible = ref(Boolean(route.query.edit === '1'));
const saving = ref(false);
const resourceGroups = ['default', 'production', 'staging', 'dev'];

/* ---------- computed helpers ---------- */
const canStart = computed(() => {
  const s = task.value?.status;
  return s === 'draft' || s === 'ready' || s === 'stopped' || s === 'failed' || s === 'completed';
});
const canStop = computed(() => {
  const s = task.value?.status;
  return s === 'running' || s === 'paused' || s === 'stopping';
});

/* ---------- edit form ---------- */
const editForm = reactive({
  name: '',
  description: '',
  resourceGroup: 'default',
  config: { parallelSize: 4, bufferSize: 16000, checkpointIntervalSecs: 10, maxRps: 0, resumeType: 'from_log' as Task['config']['resumeType'] },
});

watch(task, (v) => {
  if (!v) return;
  editForm.name = v.name;
  editForm.description = v.description ?? '';
  editForm.resourceGroup = v.resourceGroup;
  editForm.config.parallelSize = v.config.parallelSize;
  editForm.config.bufferSize = v.config.bufferSize;
  editForm.config.checkpointIntervalSecs = v.config.checkpointIntervalSecs;
  editForm.config.maxRps = v.config.maxRps;
  editForm.config.resumeType = v.config.resumeType;
}, { immediate: true });

/* ---------- load task ---------- */
async function loadTask() {
  try {
    const raw = await api.get<ApiTask>(`/tasks/${taskId.value}`);
    task.value = mapApiTask(raw);
  } catch {
    ElMessage.error('任务不存在');
    router.push(backToListPath());
  }
}

function backToListPath(): string {
  const cat = taskCategory.value;
  if (cat === 'check') return '/tasks/check';
  if (cat === 'struct') return '/tasks/struct';
  if (cat === 'cdc') return '/tasks/cdc';
  return '/tasks/snapshot';
}

/* ---------- lifecycle actions ---------- */
async function doLifecycle(action: string) {
  try {
    await api.post(`/tasks/${taskId.value}/${action}`);
    ElMessage.success('操作成功');
    await loadTask();
  } catch {
    ElMessage.error('操作失败');
  }
}

function confirmStop() {
  if (!task.value) return;
  ElMessageBox.confirm(
    `确定停止任务「${task.value.name}」？`,
    '停止任务',
    { type: 'warning' },
  ).then(() => doLifecycle('stop')).catch(() => {});
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

function onEditorClose() {
  router.replace({ query: { ...route.query, edit: undefined } });
}

/* ---------- tab deep link ---------- */
function onTabChange(tab: string | number) {
  const tabStr = String(tab);
  const query: Record<string, string | undefined> = { ...route.query, tab: tabStr };
  if (tabStr !== 'config') delete (query as Partial<typeof query>).edit;
  router.replace({ query });
}

watch(() => route.query.tab, (v) => {
  if (v && VALID_TABS.includes(v as TabName)) activeTab.value = v as TabName;
});

watch(() => route.query.edit, (v) => {
  editorVisible.value = v === '1';
});

/* ---------- KPI charts (detail overview) ---------- */
async function loadDetailMetrics() {
  try {
    const now = Date.now();
    const from = now - 3600_000; // last 1h
    const metrics = ['extractor_rps_avg', 'sinker_record_count_avg_by_sec', 'pipeline_buffer_size_avg'];
    const results: MetricQueryResponse[] = [];
    for (const m of metrics) {
      try {
        const res = await api.get<MetricQueryResponse>(`/runs/${currentRunId.value}/metrics?metric=${m}&from=${from}&to=${now}&step=60`);
        results.push(res);
      } catch { /* ignore individual metric errors */ }
    }
    detailMetricSeries.value = results;
  } catch { /* ignore */ }
}

const detailMetricSeries = ref<MetricQueryResponse[]>([]);
const currentRunId = ref('');

function baseLine(name: string, xs: string[], sData: { name: string; data: number[]; color: string }[]) {
  return {
    grid: { left: 36, right: 16, top: 18, bottom: 22 },
    tooltip: { trigger: 'axis' as const },
    legend: { bottom: 0, icon: 'roundRect', itemWidth: 8, itemHeight: 8, textStyle: { color: '#64748B', fontSize: 11 } },
    xAxis: { type: 'category', data: xs, axisLine: AXIS_BASE.axisLine, axisLabel: { ...AXIS_BASE.axisLabel, fontSize: 10 }, axisTick: { show: false } },
    yAxis: { type: 'value', name: name, axisLine: { show: false }, axisLabel: AXIS_BASE.axisLabel, splitLine: AXIS_BASE.splitLine },
    series: sData.map((s) => ({
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
  const ms = detailMetricSeries.value.find((s) => s.metric === 'extractor_rps_avg');
  if (!ms || ms.data.length === 0) return null;
  const xs = ms.data.map((p) => dayjs(p.ts).format('HH:mm'));
  return baseLine('extractor_rps_avg', xs, [{ name: 'extractor_rps_avg', data: ms.data.map((p) => p.value), color: BRAND_PALETTE[0] }]);
});

const sinkRpsOption = computed(() => {
  const ms = detailMetricSeries.value.find((s) => s.metric === 'sinker_record_count_avg_by_sec');
  if (!ms || ms.data.length === 0) return null;
  const xs = ms.data.map((p) => dayjs(p.ts).format('HH:mm'));
  return baseLine('sinker_record_count_avg_by_sec', xs, [{ name: 'sinker_record_count_avg_by_sec', data: ms.data.map((p) => p.value), color: BRAND_PALETTE[1] }]);
});

const bufferOption = computed(() => {
  const ms = detailMetricSeries.value.find((s) => s.metric === 'pipeline_buffer_size_avg');
  if (!ms || ms.data.length === 0) return null;
  const xs = ms.data.map((p) => dayjs(p.ts).format('HH:mm'));
  return baseLine('pipeline_buffer_size_avg', xs, [{ name: 'pipeline_buffer_size_avg', data: ms.data.map((p) => p.value), color: BRAND_PALETTE[4] }]);
});

/* ---------- objects sample ---------- */
const objectRows = computed(() => {
  if (!task.value) return [];
  const n = task.value.syncObjects.selectedTables;
  const tables = ['orders', 'users', 'payments', 'products', 'shipments', 'inventory', 'logs'];
  return Array.from({ length: Math.min(n, 8) }, (_, i) => ({
    name: `${task.value!.source.database ?? 'app_db'}.${tables[i % tables.length]}`,
    type: '表',
    rows: Math.round(100_000 * Math.random() * (i + 1)),
    status: task.value!.status === 'stopped' || task.value!.status === 'completed' ? '已完成'
      : task.value!.status === 'running' ? '同步中'
      : task.value!.status === 'failed' ? '失败' : '等待中',
  }));
});

/* ---------- Logs tab (SSE) ---------- */
const logFile = ref('default');
const logFiles = ['default', 'position', 'monitor', 'commit', 'finished', 'task', 'http'];
const logLevelFilter = ref('ALL');
const logPaused = ref(false);
const logPaneRef = ref<HTMLElement | null>(null);
const showFollowBtn = ref(false);
const logStreamHandle = shallowRef<LogStreamHandle | null>(null);

/** Derive SSE state from the handle's state ref so it stays in sync after reconnect. */
const sseState = computed<'connected' | 'reconnecting' | 'disconnected'>(() => {
  if (!logStreamHandle.value) return 'disconnected';
  return logStreamHandle.value.state.value;
});

const sseStateLabel = computed(() => {
  if (sseState.value === 'connected') return t('taskDetail.log.connected');
  if (sseState.value === 'reconnecting') return t('taskDetail.log.reconnecting');
  return t('taskDetail.log.disconnected');
});

const filteredLogLines = computed<LogLine[]>(() => {
  const handle = logStreamHandle.value;
  if (!handle) return [];
  // handle.lines is Ref<LogLine[]>, need .value to unwrap
  const rawLines: LogLine[] = unref(handle.lines);
  if (logLevelFilter.value === 'ALL') return rawLines;
  return rawLines.filter((l: LogLine) => l.level === logLevelFilter.value);
});

function formatLogTime(ts: number): string {
  return dayjs(ts).format('HH:mm:ss');
}

function reopenLogStream() {
  logStreamHandle.value?.close();
  if (!currentRunId.value) return;
  logStreamHandle.value = useLogStream({
    runId: currentRunId.value,
    file: logFile.value,
    level: logLevelFilter.value !== 'ALL' ? logLevelFilter.value as 'debug' | 'info' | 'warn' | 'error' : undefined,
    bufferLimit: 500,
  });
}

function onLogScroll() {
  if (!logPaneRef.value) return;
  const el = logPaneRef.value;
  const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
  showFollowBtn.value = !atBottom;
}

function scrollToBottom() {
  if (!logPaneRef.value) return;
  logPaneRef.value.scrollTop = logPaneRef.value.scrollHeight;
  showFollowBtn.value = false;
}

/* ---------- Alerts tab ---------- */
const alerts = ref<Alert[]>([]);

async function loadAlerts() {
  try {
    const res = await api.get<{ items: Alert[] }>(`/alerts?taskId=${taskId.value}`);
    alerts.value = res.items ?? [];
  } catch { /* ignore */ }
}

/* ---------- Monitor tab ---------- */
const monitorRange = ref<'1h' | '6h' | '24h'>('1h');
const monitorRanges = [
  { value: '1h' as const, label: '1h' },
  { value: '6h' as const, label: '6h' },
  { value: '24h' as const, label: '24h' },
];
const monitorSeries = ref<MetricQueryResponse[]>([]);
const monitorLoading = ref(false);

const MONITOR_METRICS = [
  'extractor_rps_avg',
  'sinker_record_count_avg_by_sec',
  'pipeline_buffer_size_avg',
  'sinker_rt_per_query_avg',
];

async function loadMonitorMetrics() {
  if (!currentRunId.value) return;
  monitorLoading.value = true;
  try {
    const now = Date.now();
    const rangeMs = monitorRange.value === '1h' ? 3600_000 : monitorRange.value === '6h' ? 6 * 3600_000 : 24 * 3600_000;
    const from = now - rangeMs;
    const step = monitorRange.value === '1h' ? 60 : monitorRange.value === '6h' ? 300 : 600;
    const results: MetricQueryResponse[] = [];
    for (const m of MONITOR_METRICS) {
      try {
        const res = await api.get<MetricQueryResponse>(`/runs/${currentRunId.value}/metrics?metric=${m}&from=${from}&to=${now}&step=${step}`);
        if (res.data.length > 0) results.push(res);
      } catch { /* ignore individual metric errors */ }
    }
    monitorSeries.value = results;
  } catch { /* ignore */ }
  finally { monitorLoading.value = false; }
}

function setMonitorRange(r: '1h' | '6h' | '24h') {
  monitorRange.value = r;
  loadMonitorMetrics();
}

function monitorChartOption(ms: MetricQueryResponse) {
  const xs = ms.data.map((p) => dayjs(p.ts).format('HH:mm'));
  return {
    grid: { left: 36, right: 16, top: 18, bottom: 22 },
    tooltip: { trigger: 'axis' as const },
    xAxis: { type: 'category', data: xs, axisLine: AXIS_BASE.axisLine, axisLabel: { ...AXIS_BASE.axisLabel, fontSize: 10 }, axisTick: { show: false } },
    yAxis: { type: 'value', name: ms.metric, axisLine: { show: false }, axisLabel: AXIS_BASE.axisLabel, splitLine: AXIS_BASE.splitLine },
    series: [{
      name: ms.metric,
      type: 'line' as const,
      data: ms.data.map((p) => p.value),
      smooth: true,
      symbol: 'none',
      lineStyle: { width: 1.6, color: BRAND_PALETTE[0] },
      areaStyle: { color: BRAND_PALETTE[0], opacity: 0.08 },
    }],
  };
}

/* ---------- History tab ---------- */
const historyRuns = ref<Run[]>([]);
const historyTotal = ref(0);
const historyPage = ref(1);
const historyPageSize = 25;
const historyLoading = ref(false);

async function loadHistory() {
  historyLoading.value = true;
  try {
    const res = await api.get<{ items: Run[]; total: number }>(`/tasks/${taskId.value}/runs?page=${historyPage.value}&size=${historyPageSize}`);
    historyRuns.value = res.items ?? [];
    historyTotal.value = res.total ?? 0;
  } catch { /* ignore */ }
  finally { historyLoading.value = false; }
}

function formatPosition(pos: RunPosition): string {
  if (pos.kind === 'binlog') return `${pos.file}:${pos.pos}${pos.gtid ? ` gtid=${pos.gtid}` : ''}`;
  if (pos.kind === 'lsn') return `LSN ${pos.lsn}${pos.slot ? ` slot=${pos.slot}` : ''}`;
  if (pos.kind === 'scn') return `SCN ${pos.scn}`;
  if (pos.kind === 'resume_token') return `token=${pos.token}`;
  if (pos.kind === 'unknown') return pos.raw ?? '—';
  return JSON.stringify(pos);
}

/* archived logs dialog */
const archivedDialogVisible = ref(false);
const archivedLines = ref<LogLine[]>([]);
const archivedLoading = ref(false);

async function viewArchivedLogs(run: Run) {
  archivedDialogVisible.value = true;
  archivedLoading.value = true;
  try {
    const res = await api.get<{ lines: LogLine[] }>(`/runs/${run.id}/logs?file=default`);
    archivedLines.value = res.lines ?? [];
  } catch { archivedLines.value = []; }
  finally { archivedLoading.value = false; }
}

/* ---------- load current run ---------- */
async function loadCurrentRunId() {
  try {
    const res = await api.get<{ items: Run[] }>(`/tasks/${taskId.value}/runs?page=1&size=1`);
    const runs = res.items ?? [];
    if (runs.length > 0 && (runs[0].status === 'running' || runs[0].status === 'paused')) {
      currentRunId.value = runs[0].id;
    } else {
      currentRunId.value = '';
    }
  } catch { currentRunId.value = ''; }
}

/* ---------- lifecycle ---------- */
let pollId: ReturnType<typeof setInterval> | null = null;
const POLL_INTERVAL_MS = 5_000;

onMounted(async () => {
  await loadTask();
  await loadCurrentRunId();
  loadDetailMetrics();
  loadAlerts();
  loadHistory();

  // open SSE if on logs tab
  if (activeTab.value === 'logs' && currentRunId.value) reopenLogStream();

  pollId = setInterval(() => {
    if (isVisible.value) {
      loadTask();
      loadCurrentRunId();
      loadDetailMetrics();
      if (activeTab.value === 'monitor') loadMonitorMetrics();
      if (activeTab.value === 'alerts') loadAlerts();
    }
  }, POLL_INTERVAL_MS);
});

onUnmounted(() => {
  if (pollId) clearInterval(pollId);
  logStreamHandle.value?.close();
});

/* ---------- navigation ---------- */
function onBack() {
  router.push(backToListPath());
}
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
  padding: 16px 20px;
}
.detail__kpi-row {
  display: grid;
  grid-template-columns: repeat(4, 1fr);
  gap: 12px;
}
@media (max-width: 800px) { .detail__kpi-row { grid-template-columns: repeat(2, 1fr); } }
.detail__charts {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: 12px;
  padding: 0 0 0 0;
}
@media (max-width: 1200px) { .detail__charts { grid-template-columns: repeat(2, 1fr); } }
@media (max-width: 800px) { .detail__charts { grid-template-columns: 1fr; } }
.detail__chart {
  height: 100%;
}
.detail__tabs {
  padding: 8px 20px 20px;
}
.detail__config dl {
  display: grid;
  grid-template-columns: 180px 1fr;
  gap: 12px 24px;
  padding: 12px 0;
  margin: 0;
}
.detail__config dt { color: var(--color-ink-subtle); font-size: 13px; }
.detail__config dd { margin: 0; color: var(--color-ink); font-size: 13px; font-family: var(--font-mono); }
.detail__mono { font-family: var(--font-mono); font-size: 12px; }
.detail__muted { color: var(--color-ink-subtle); font-size: 12px; }
.detail__position { font-size: 11px; word-break: break-all; }

/* logs */
.detail__log-toolbar {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 8px 0;
  gap: 12px;
  flex-wrap: wrap;
}
.detail__log-toolbar-left {
  display: inline-flex;
  gap: 8px;
  align-items: center;
}
.detail__log-toolbar-right {
  display: inline-flex;
  gap: 8px;
  align-items: center;
}
.detail__log-file-select { width: 140px; }
.detail__log-level-select { width: 100px; }
.detail__log-status-pill {
  display: inline-flex;
  align-items: center;
  padding: 2px 10px;
  border-radius: 999px;
  font-size: 12px;
  font-weight: 500;
}
.detail__log-status-pill--connected { background: #ECFDF5; color: #0F766E; }
.detail__log-status-pill--reconnecting { background: #FEF3C7; color: #92400E; }
.detail__log-status-pill--disconnected { background: #FEF2F2; color: #991B1B; }
.detail__log-view {
  background: #0F172A;
  color: #CBD5E1;
  border-radius: var(--radius-md);
  padding: 8px 12px;
  font-family: var(--font-mono);
  font-size: 12px;
  max-height: 480px;
  overflow: auto;
  position: relative;
}
.detail__log-line {
  display: grid;
  grid-template-columns: 80px 60px 1fr;
  gap: 10px;
  padding: 3px 0;
}
.detail__log-line--warn .detail__log-level { color: #FBBF24; }
.detail__log-line--error .detail__log-level { color: #F87171; }
.detail__log-line--info .detail__log-level { color: #67E8F9; }
.detail__log-line--debug .detail__log-level { color: #94A3B8; }
.detail__log-time { color: #64748B; }
.detail__log-msg { color: #E2E8F0; overflow-wrap: anywhere; }
.detail__log-follow {
  position: sticky;
  bottom: 8px;
  text-align: center;
  z-index: 10;
  margin-top: 4px;
}

/* monitor */
.detail__monitor-toolbar {
  display: flex;
  gap: 12px;
  align-items: center;
  padding: 8px 0;
}
.detail__monitor-charts {
  display: grid;
  grid-template-columns: repeat(2, 1fr);
  gap: 12px;
}
@media (max-width: 800px) { .detail__monitor-charts { grid-template-columns: 1fr; } }
.detail__monitor-loading,
.detail__monitor-empty {
  padding: 40px 0;
}

/* history */
.detail__history-footer {
  display: flex;
  justify-content: center;
  padding-top: 12px;
}

/* archived logs */
.detail__archived-log-view {
  background: #0F172A;
  color: #CBD5E1;
  border-radius: var(--radius-md);
  padding: 8px 12px;
  font-family: var(--font-mono);
  font-size: 12px;
  max-height: 480px;
  overflow: auto;
}

/* editor */
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
