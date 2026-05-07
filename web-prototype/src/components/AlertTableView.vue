<template>
  <div class="alert-view">
    <PageHeader :title="title" :subtitle="subtitle">
      <template #actions>
        <el-button @click="loadList">
          <template #icon><IconRefresh /></template>
          {{ t('common.refresh') }}
        </el-button>
      </template>
    </PageHeader>

    <div class="ape-dts-console-page alert-view__body">
      <!-- summary cards (active mode only) -->
      <div v-if="mode === 'active'" class="alert-view__summary">
        <button
          v-for="lvl in (['critical', 'major', 'minor', 'info'] as AlertLevel[])"
          :key="lvl"
          type="button"
          class="alert-view__sum-card"
          :class="[`alert-view__sum-card--${lvl}`, { 'is-active': filter.level === lvl }]"
          @click="onSummaryToggle(lvl)"
        >
          <div class="alert-view__sum-label">
            <LevelBadge :level="lvl" />
          </div>
          <div class="alert-view__sum-value tabular-nums">{{ summary[lvl] ?? 0 }}</div>
        </button>
        <div class="alert-view__sum-card alert-view__sum-card--total">
          <div class="alert-view__sum-label">{{ t('alerts.summary.total') }}</div>
          <div class="alert-view__sum-value tabular-nums">{{ summaryTotal }}</div>
        </div>
      </div>

      <div class="ape-dts-console-card alert-view__panel">
        <!-- toolbar row -->
        <div class="alert-view__toolbar">
          <div class="alert-view__actions">
            <el-dropdown
              v-if="mode === 'active'"
              trigger="click"
              :disabled="selected.length === 0"
              @command="onBatch"
            >
              <el-button :disabled="selected.length === 0">
                {{ t('taskList.action.batch') }}
                <span v-if="selected.length" class="alert-view__count">· {{ selected.length }}</span>
                <IconChevronDown class="alert-view__dropdown-icon" />
              </el-button>
              <template #dropdown>
                <el-dropdown-menu>
                  <el-dropdown-item command="clear">
                    <IconCheck /> {{ t('alerts.batch.clear') }}
                  </el-dropdown-item>
                </el-dropdown-menu>
              </template>
            </el-dropdown>

            <el-switch
              v-if="mode === 'active'"
              v-model="autoRefresh"
              :active-text="t('alerts.auto.label')"
              inline-prompt
            />
            <span v-if="mode === 'active'" class="alert-view__auto-hint">
              {{ autoRefresh ? t('alerts.auto.on') : t('alerts.auto.off') }}
            </span>

            <el-date-picker
              v-if="mode === 'history'"
              v-model="dateRange"
              type="datetimerange"
              :start-placeholder="t('alerts.filter.range')"
              :end-placeholder="t('alerts.filter.range')"
              format="YYYY-MM-DD HH:mm"
              value-format="YYYY-MM-DDTHH:mm:ss"
              class="alert-view__filter--range"
              @change="applyFilter"
            />
          </div>
        </div>

        <!-- filters row -->
        <div class="alert-view__filters">
          <el-select
            v-model="filter.level"
            :placeholder="t('alerts.filter.level')"
            clearable
            class="alert-view__filter alert-view__filter--sm"
            @change="applyFilter"
          >
            <el-option
              v-for="lvl in (['critical', 'major', 'minor', 'info'] as AlertLevel[])"
              :key="lvl"
              :value="lvl"
              :label="t(`alerts.summary.${lvl}`)"
            >
              <span class="alert-view__opt"><LevelBadge :level="lvl" /></span>
            </el-option>
          </el-select>
          <el-select
            v-model="filter.source"
            :placeholder="t('alerts.filter.source')"
            clearable
            class="alert-view__filter alert-view__filter--sm"
            @change="applyFilter"
          >
            <el-option
              v-for="s in SOURCES"
              :key="s"
              :value="s"
              :label="t(`alerts.source.${s}`)"
            />
          </el-select>
          <el-select
            v-model="filter.engine"
            :placeholder="t('alerts.filter.engine')"
            clearable
            class="alert-view__filter alert-view__filter--sm"
            @change="applyFilter"
          >
            <el-option
              v-for="e in engineOptions"
              :key="e.value"
              :value="e.value"
              :label="e.label"
            />
          </el-select>
          <el-input
            v-model="filter.taskId"
            :placeholder="t('alerts.filter.taskId')"
            clearable
            class="alert-view__filter alert-view__filter--sm"
            @keyup.enter="applyFilter"
            @clear="applyFilter"
          >
            <template #prefix><IconSearch /></template>
          </el-input>
          <el-input
            v-model="filter.alertId"
            :placeholder="t('alerts.filter.alertId')"
            clearable
            class="alert-view__filter alert-view__filter--sm"
            @keyup.enter="applyFilter"
            @clear="applyFilter"
          >
            <template #prefix><IconSearch /></template>
          </el-input>
          <el-input
            v-model="filter.ip"
            :placeholder="t('alerts.filter.ip')"
            clearable
            class="alert-view__filter alert-view__filter--sm"
            @keyup.enter="applyFilter"
            @clear="applyFilter"
          >
            <template #prefix><IconSearch /></template>
          </el-input>
          <el-input
            v-model="filter.q"
            :placeholder="t('alerts.filter.search')"
            clearable
            class="alert-view__filter alert-view__filter--grow"
            @keyup.enter="applyFilter"
            @clear="applyFilter"
          >
            <template #prefix><IconSearch /></template>
          </el-input>
        </div>

        <!-- table -->
        <el-table
          v-loading="loading"
          :data="list"
          row-key="id"
          stripe
          class="alert-view__table"
          :default-sort="{ prop: 'lastAt', order: 'descending' }"
          @selection-change="onSelectionChange"
        >
          <el-table-column v-if="mode === 'active'" type="selection" width="44" />
          <el-table-column :label="t('alerts.col.id')" prop="id" sortable width="180">
            <template #default="{ row }">
              <span class="alert-view__id">{{ row.id }}</span>
            </template>
          </el-table-column>
          <el-table-column :label="t('alerts.col.level')" prop="level" width="100" sortable>
            <template #default="{ row }">
              <LevelBadge :level="row.level" />
            </template>
          </el-table-column>
          <el-table-column :label="t('alerts.col.message')" min-width="220" show-overflow-tooltip>
            <template #default="{ row }">
              <span class="alert-view__msg">{{ row.message }}</span>
            </template>
          </el-table-column>
          <el-table-column :label="t('alerts.col.task')" min-width="180" show-overflow-tooltip>
            <template #default="{ row }">
              <el-link type="primary" :underline="false" @click="goTask(row)">
                {{ row.taskName }}
              </el-link>
            </template>
          </el-table-column>
          <el-table-column :label="t('alerts.col.ip')" width="140">
            <template #default="{ row }">
              <span class="alert-view__ip">{{ row.instanceIp }}</span>
            </template>
          </el-table-column>
          <el-table-column :label="t('alerts.col.engine')" width="130">
            <template #default="{ row }">
              <EngineTag :engine="row.engine" />
            </template>
          </el-table-column>
          <el-table-column :label="t('alerts.col.source')" width="140">
            <template #default="{ row }">
              <AlertSourceTag :source="row.source" />
            </template>
          </el-table-column>
          <el-table-column :label="t('alerts.col.first')" prop="firstAt" sortable width="170">
            <template #default="{ row }">
              <span class="alert-view__time">{{ formatTime(row.firstAt) }}</span>
            </template>
          </el-table-column>
          <el-table-column :label="t('alerts.col.last')" prop="lastAt" sortable width="170">
            <template #default="{ row }">
              <span class="alert-view__time">{{ formatTime(row.lastAt) }}</span>
            </template>
          </el-table-column>
          <el-table-column
            v-if="mode === 'history'"
            :label="t('alerts.col.cleared')"
            prop="clearedAt"
            sortable
            width="170"
          >
            <template #default="{ row }">
              <span class="alert-view__time">{{ row.clearedAt ? formatTime(row.clearedAt) : '—' }}</span>
            </template>
          </el-table-column>
          <el-table-column :label="t('alerts.col.count')" prop="count" sortable width="90" align="right">
            <template #default="{ row }">
              <span class="tabular-nums">{{ row.count }}</span>
            </template>
          </el-table-column>
          <el-table-column :label="t('alerts.col.service')" width="140">
            <template #default="{ row }">
              <span class="alert-view__svc">{{ row.service }}</span>
            </template>
          </el-table-column>
          <el-table-column
            v-if="mode === 'active'"
            :label="t('alerts.col.actions')"
            width="160"
            fixed="right"
          >
            <template #default="{ row }">
              <el-button link type="primary" @click="goTask(row)">
                {{ t('alerts.action.viewTask') }}
              </el-button>
              <el-button link type="danger" @click="confirmClear(row)">
                {{ t('alerts.action.clear') }}
              </el-button>
            </template>
          </el-table-column>

          <template #empty>
            <div class="alert-view__empty">
              <IconBellOff class="alert-view__empty-icon" />
              <p>{{ t(`alerts.empty.${mode === 'active' ? 'current' : 'history'}`) }}</p>
            </div>
          </template>
        </el-table>

        <!-- pagination -->
        <footer class="alert-view__footer">
          <span class="alert-view__total">{{ t('common.total') }}：{{ total }}</span>
          <el-pagination
            v-model:current-page="page"
            v-model:page-size="pageSize"
            :page-sizes="[10, 20, 50]"
            :total="total"
            layout="sizes, prev, pager, next, jumper"
            background
            @current-change="loadList"
            @size-change="onSizeChange"
          />
        </footer>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, onUnmounted, reactive, ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';
import { useRouter, useRoute } from 'vue-router';
import { ElMessage, ElMessageBox } from 'element-plus';
import dayjs from 'dayjs';
import IconBellOff from '~icons/tabler/bell-off';
import PageHeader from './PageHeader.vue';
import LevelBadge from './LevelBadge.vue';
import EngineTag from './EngineTag.vue';
import AlertSourceTag from './AlertSourceTag.vue';
import { api } from '@/api/client';
import {
  ENGINE_LABELS,
  type Alert,
  type AlertLevel,
  type AlertSource,
  type EngineType,
  type Paginated,
} from '@/types/domain';

const props = defineProps<{ mode: 'active' | 'history' }>();
const { t } = useI18n();
const router = useRouter();
const route = useRoute();

const title = computed(() =>
  t(props.mode === 'active' ? 'nav.alerts.current' : 'nav.alerts.history'),
);
const subtitle = computed(() =>
  t(props.mode === 'active' ? 'alerts.subtitle.current' : 'alerts.subtitle.history'),
);

const SOURCES: AlertSource[] = ['rps', 'latency', 'error_rate', 'connection', 'disk', 'custom'];
const engineOptions = (Object.keys(ENGINE_LABELS) as EngineType[]).map((k) => ({
  value: k,
  label: ENGINE_LABELS[k],
}));

const list = ref<Alert[]>([]);
const total = ref(0);
const page = ref(1);
const pageSize = ref(10);
const loading = ref(false);
const selected = ref<Alert[]>([]);

const autoRefresh = ref(true);
const dateRange = ref<[string, string] | null>(null);

const filter = reactive({
  level: '' as AlertLevel | '',
  source: '' as AlertSource | '',
  engine: '' as EngineType | '',
  taskId: '',
  alertId: '',
  ip: '',
  q: '',
});

const summary = ref<Record<AlertLevel, number>>({ critical: 0, major: 0, minor: 0, info: 0 });
const summaryTotal = computed(
  () => summary.value.critical + summary.value.major + summary.value.minor + summary.value.info,
);

async function refreshSummary() {
  if (props.mode !== 'active') return;
  try {
    const data = await api.get<Paginated<Alert>>('/alerts/active?page=1&size=200');
    const s: Record<AlertLevel, number> = { critical: 0, major: 0, minor: 0, info: 0 };
    for (const a of data.items) s[a.level] += 1;
    summary.value = s;
  } catch {
    /* noop */
  }
}

async function loadList() {
  loading.value = true;
  try {
    const params = new URLSearchParams({
      page: String(page.value),
      size: String(pageSize.value),
    });
    if (filter.level) params.set('level', filter.level);
    if (filter.source) params.set('source', filter.source);
    if (filter.engine) params.set('engine', filter.engine);
    if (filter.taskId) params.set('taskId', filter.taskId);
    if (filter.q) params.set('q', filter.q);
    const path = props.mode === 'active' ? '/alerts/active' : '/alerts/history';
    const data = await api.get<Paginated<Alert>>(`${path}?${params.toString()}`);
    let items = data.items;
    // client-side refinement for fields not handled server-side
    if (filter.alertId) items = items.filter((a) => a.id.includes(filter.alertId));
    if (filter.ip) items = items.filter((a) => a.instanceIp.includes(filter.ip));
    if (props.mode === 'history' && dateRange.value) {
      const [from, to] = dateRange.value;
      items = items.filter((a) => a.lastAt >= from && a.lastAt <= to);
    }
    list.value = items;
    total.value = filter.alertId || filter.ip || dateRange.value ? items.length : data.total;
  } catch {
    ElMessage.error('加载告警失败');
  } finally {
    loading.value = false;
  }
  if (props.mode === 'active') refreshSummary();
}

function applyFilter() {
  page.value = 1;
  loadList();
}

function onSummaryToggle(lvl: AlertLevel) {
  filter.level = filter.level === lvl ? '' : lvl;
  applyFilter();
}

function onSizeChange(size: number) {
  pageSize.value = size;
  page.value = 1;
  loadList();
}

function onSelectionChange(rows: Alert[]) {
  selected.value = rows;
}

function formatTime(s: string) {
  return dayjs(s).format('YYYY-MM-DD HH:mm:ss');
}

function goTask(row: Alert) {
  // category isn't on the alert; infer from the task id prefix and fall back to snapshot.
  const category = row.taskId.startsWith('cdc') ? 'cdc'
    : row.taskId.startsWith('check') ? 'check'
    : row.taskId.startsWith('struct') ? 'struct'
    : 'snapshot';
  router.push({ path: `/tasks/${category}/${row.taskId}`, query: { tab: 'alerts' } });
}

function confirmClear(row: Alert) {
  ElMessageBox.confirm(
    t('alerts.confirm.clear', { id: row.id }),
    t('alerts.action.clear'),
    { type: 'warning', confirmButtonText: t('common.confirm'), cancelButtonText: t('common.cancel') },
  ).then(async () => {
    await api.post(`/alerts/${row.id}/clear`);
    ElMessage.success(t('alerts.toast.cleared'));
    loadList();
  }).catch(() => {});
}

async function onBatch(cmd: string) {
  if (cmd !== 'clear' || selected.value.length === 0) return;
  const ids = selected.value.map((r) => r.id);
  ElMessageBox.confirm(
    t('alerts.confirm.clearBatch', { n: ids.length }),
    t('alerts.batch.clear'),
    { type: 'warning' },
  ).then(async () => {
    const res = await api.post<{ cleared: number }>('/alerts/clear-batch', { ids });
    ElMessage.success(t('alerts.toast.clearedBatch', { n: res.cleared }));
    loadList();
  }).catch(() => {});
}

let pollId: ReturnType<typeof setInterval> | null = null;
function startPoll() {
  if (pollId) return;
  pollId = setInterval(loadList, 8000);
}
function stopPoll() {
  if (pollId) {
    clearInterval(pollId);
    pollId = null;
  }
}

watch(autoRefresh, (v) => {
  if (props.mode !== 'active') return;
  if (v) startPoll();
  else stopPoll();
});

onMounted(() => {
  // honor cross-page query (?level=critical from dashboard)
  if (route.query.level) filter.level = route.query.level as AlertLevel;
  if (route.query.taskId) filter.taskId = String(route.query.taskId);
  loadList();
  if (props.mode === 'active' && autoRefresh.value) startPoll();
});

onUnmounted(stopPoll);
</script>

<style scoped>
.alert-view {
  display: flex;
  flex-direction: column;
}
.alert-view__body {
  display: flex;
  flex-direction: column;
  gap: 16px;
}
.alert-view__summary {
  display: grid;
  grid-template-columns: repeat(5, minmax(0, 1fr));
  gap: 12px;
}
.alert-view__sum-card {
  text-align: left;
  background: var(--color-surface);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  padding: 14px 18px;
  cursor: pointer;
  transition: box-shadow var(--dur) var(--ease-soft), transform var(--dur) var(--ease-soft);
  display: flex;
  flex-direction: column;
  gap: 6px;
  position: relative;
}
.alert-view__sum-card:hover { box-shadow: var(--shadow-elevated); transform: translateY(-1px); }
.alert-view__sum-card.is-active {
  border-color: var(--color-primary-500);
  box-shadow: 0 0 0 3px color-mix(in oklab, var(--color-primary-500) 14%, transparent);
}
.alert-view__sum-card--critical::before,
.alert-view__sum-card--major::before,
.alert-view__sum-card--minor::before,
.alert-view__sum-card--info::before {
  content: ""; position: absolute; inset: 0 auto 0 0; width: 3px;
  border-radius: var(--radius-md) 0 0 var(--radius-md);
}
.alert-view__sum-card--critical::before { background: var(--color-danger); }
.alert-view__sum-card--major::before { background: var(--color-warning); }
.alert-view__sum-card--minor::before { background: var(--color-info); }
.alert-view__sum-card--info::before { background: var(--color-ink-faint); }
.alert-view__sum-card--total {
  background: linear-gradient(135deg, var(--color-primary-50), var(--color-surface));
  border-color: var(--color-primary-200);
  cursor: default;
}
.alert-view__sum-label {
  font-size: 12px;
  color: var(--color-ink-subtle);
  display: flex;
  align-items: center;
}
.alert-view__sum-value {
  font-size: 28px;
  font-weight: 600;
  color: var(--color-ink);
  letter-spacing: -0.02em;
  line-height: 1.1;
}
.alert-view__panel {
  padding: 16px 20px 12px;
  display: flex;
  flex-direction: column;
  gap: 14px;
}
.alert-view__toolbar {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 12px;
  flex-wrap: wrap;
}
.alert-view__actions {
  display: flex;
  align-items: center;
  gap: 12px;
  flex-wrap: wrap;
}
.alert-view__count {
  font-size: 12px;
  color: var(--color-primary-700);
  margin-left: 4px;
}
.alert-view__dropdown-icon {
  margin-left: 4px;
  color: var(--color-ink-subtle);
}
.alert-view__auto-hint {
  font-size: 12px;
  color: var(--color-ink-subtle);
}
.alert-view__filters {
  display: flex;
  gap: 12px;
  flex-wrap: wrap;
  align-items: center;
}
.alert-view__filter--sm { width: 180px; }
.alert-view__filter--grow { flex: 1; min-width: 240px; }
.alert-view__filter--range { width: 380px; }
.alert-view__opt { display: inline-flex; align-items: center; }
.alert-view__id {
  font-family: var(--font-mono);
  font-size: 12px;
  color: var(--color-ink-muted);
}
.alert-view__msg {
  color: var(--color-ink);
  font-size: 13px;
}
.alert-view__ip {
  font-family: var(--font-mono);
  font-size: 12px;
  color: var(--color-ink-muted);
}
.alert-view__time {
  font-family: var(--font-mono);
  font-size: 12px;
  color: var(--color-ink-muted);
}
.alert-view__svc {
  font-size: 12px;
  color: var(--color-ink-muted);
}
.alert-view__empty {
  display: flex;
  flex-direction: column;
  gap: 12px;
  align-items: center;
  padding: 36px 0;
  color: var(--color-ink-subtle);
}
.alert-view__empty-icon {
  width: 38px;
  height: 38px;
  color: var(--color-ink-faint);
}
.alert-view__footer {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding-top: 8px;
  color: var(--color-ink-subtle);
  font-size: 12px;
}
@media (max-width: 1280px) {
  .alert-view__summary { grid-template-columns: repeat(3, minmax(0, 1fr)); }
}
@media (max-width: 880px) {
  .alert-view__summary { grid-template-columns: repeat(2, minmax(0, 1fr)); }
}
</style>
