<template>
  <div class="task-list" :class="`task-list--${density}`">
    <PageHeader :title="title" :subtitle="subtitle">
      <template #actions>
        <el-button @click="loadList">
          <template #icon><IconRefresh /></template>
          {{ t('common.refresh') }}
        </el-button>
        <el-button type="primary" @click="onCreate">
          <template #icon><IconPlus /></template>
          {{ createLabel }}
        </el-button>
      </template>
    </PageHeader>

    <div class="ape-dts-console-page task-list__body">
      <div class="ape-dts-console-card task-list__panel">
        <!-- toolbar row -->
        <div class="task-list__toolbar">
          <div class="task-list__actions">
            <el-dropdown trigger="click" :disabled="selected.length === 0" @command="onBatch">
              <el-button :disabled="selected.length === 0">
                {{ t('taskList.action.batch') }}
                <span v-if="selected.length" class="task-list__selected-count">· {{ selected.length }}</span>
                <IconChevronDown class="task-list__dropdown-icon" />
              </el-button>
              <template #dropdown>
                <el-dropdown-menu>
                  <el-dropdown-item v-if="can('task.start')" command="start">
                    <IconPlayerPlay /> {{ t('taskList.batch.start') }}
                  </el-dropdown-item>
                  <el-dropdown-item v-if="can('task.pause')" command="pause">
                    <IconPlayerPause /> {{ t('taskList.batch.pause') }}
                  </el-dropdown-item>
                  <el-dropdown-item v-if="can('task.resume')" command="resume">
                    <IconPlayerPlay /> {{ t('taskList.batch.resume') }}
                  </el-dropdown-item>
                  <el-dropdown-item v-if="can('task.stop')" command="stop" divided>
                    <IconPlayerStop /> {{ t('taskList.batch.stop') }}
                  </el-dropdown-item>
                  <el-dropdown-item v-if="can('task.delete')" command="delete" divided>
                    <IconTrash /> {{ t('taskList.batch.delete') }}
                  </el-dropdown-item>
                </el-dropdown-menu>
              </template>
            </el-dropdown>
            <el-button @click="onExport">
              <template #icon><IconFileExport /></template>
              {{ t('taskList.action.export') }}
            </el-button>
            <el-button @click="onImport">
              <template #icon><IconFileImport /></template>
              {{ t('taskList.action.import') }}
            </el-button>
            <el-button :disabled="selected.length === 0" @click="onExportTemplate">
              <template #icon><IconTemplate /></template>
              {{ t('taskList.action.exportTpl') }}
            </el-button>
            <el-button @click="onDownloadTemplate">
              <template #icon><IconDownload /></template>
              {{ t('taskList.action.downloadTpl') }}
            </el-button>
          </div>

          <!-- right side: density + column manager -->
          <div class="task-list__view-controls">
            <el-tooltip :content="t('taskList.density.tip')" placement="top">
              <el-button-group>
                <el-button
                  :type="density === 'compact' ? 'primary' : 'default'"
                  size="small"
                  @click="setDensity('compact')"
                >
                  <IconLayoutRows />
                </el-button>
                <el-button
                  :type="density === 'default' ? 'primary' : 'default'"
                  size="small"
                  @click="setDensity('default')"
                >
                  <IconLayoutList />
                </el-button>
              </el-button-group>
            </el-tooltip>

            <el-popover trigger="click" placement="bottom-end" :width="240">
              <template #reference>
                <el-button size="small">
                  <template #icon><IconColumns /></template>
                  {{ t('taskList.cols.manage') }}
                </el-button>
              </template>
              <div class="task-list__cols-pop">
                <div class="task-list__cols-pop-head">
                  <span>{{ t('taskList.cols.title') }}</span>
                  <el-link type="primary" :underline="false" @click="resetCols">
                    {{ t('common.reset') }}
                  </el-link>
                </div>
                <div class="task-list__cols-pop-body">
                  <el-checkbox
                    v-for="col in optionalCols"
                    :key="col.key"
                    :model-value="visibleColSet.has(col.key)"
                    @change="(v) => toggleCol(col.key, !!v)"
                  >
                    {{ col.label }}
                  </el-checkbox>
                </div>
                <div class="task-list__cols-pop-foot">{{ t('taskList.cols.fixedHint') }}</div>
              </div>
            </el-popover>
          </div>
        </div>

        <!-- filters row -->
        <div class="task-list__filters">
          <el-select
            v-model="filter.resourceGroup"
            :placeholder="t('taskList.filter.rg')"
            clearable
            class="task-list__filter task-list__filter--sm"
            @change="applyFilter"
          >
            <el-option v-for="g in resourceGroups" :key="g" :label="g" :value="g" />
          </el-select>
          <el-select
            v-model="filter.engine"
            :placeholder="t('taskList.filter.engine')"
            clearable
            class="task-list__filter task-list__filter--sm"
            @change="applyFilter"
          >
            <el-option
              v-for="e in engineOptions"
              :key="e.value"
              :label="e.label"
              :value="e.value"
            >
              <span class="task-list__engine-option">
                <EngineTag :engine="e.value" icon-only />
                <span>{{ e.label }}</span>
              </span>
            </el-option>
          </el-select>
          <el-select
            v-model="filter.status"
            :placeholder="t('taskList.filter.status')"
            clearable
            class="task-list__filter task-list__filter--sm"
            @change="applyFilter"
          >
            <el-option
              v-for="s in statusOptions"
              :key="s"
              :label="t(`task.status.${s}`)"
              :value="s"
            />
          </el-select>
          <el-input
            v-model="filter.q"
            :placeholder="t('taskList.filter.search')"
            clearable
            class="task-list__filter task-list__filter--grow"
            @keyup.enter="applyFilter"
            @clear="applyFilter"
          >
            <template #prefix><IconSearch /></template>
          </el-input>
        </div>

        <!-- filter chips -->
        <div v-if="activeChips.length" class="task-list__chips">
          <el-tag
            v-for="chip in activeChips"
            :key="chip.key"
            closable
            class="task-list__chip"
            @close="removeChip(chip.key)"
          >
            {{ chip.label }}: {{ chip.value }}
          </el-tag>
          <el-button link type="primary" size="small" @click="clearAllChips">
            {{ t('taskList.filter.clearAll') }}
          </el-button>
        </div>

        <!-- table -->
        <el-table
          v-loading="loading"
          :data="list"
          row-key="id"
          class="task-list__table"
          :row-class-name="rowClassName"
          :size="density === 'compact' ? 'small' : 'default'"
          stripe
          @selection-change="onSelectionChange"
          @sort-change="onSortChange"
        >
          <el-table-column type="selection" width="44" align="center" />
          <el-table-column :label="t('taskList.col.name')" min-width="240" header-align="left">
            <template #default="{ row }">
              <div class="task-list__name-cell">
                <el-link type="primary" :underline="false" @click="goDetail(row)">
                  {{ row.name }}
                </el-link>
                <span class="task-list__id">{{ row.id }}</span>
              </div>
            </template>
          </el-table-column>
          <el-table-column
            v-if="visibleColSet.has('mode') && showModeCol"
            :label="t('taskList.col.mode')"
            width="132"
            header-align="left"
          >
            <template #default="{ row }">
              <span class="task-list__mode" :class="`task-list__mode--${row.syncMode}`">
                {{ t(`taskList.mode.${row.syncMode}`) }}
              </span>
            </template>
          </el-table-column>
          <el-table-column :label="t('taskList.col.source')" width="148" header-align="left" sortable="custom" sort-by="source.engine">
            <template #default="{ row }">
              <EngineTag :engine="row.source.engine" />
            </template>
          </el-table-column>
          <el-table-column :label="t('taskList.col.target')" width="148" header-align="left">
            <template #default="{ row }">
              <EngineTag :engine="row.target.engine" />
            </template>
          </el-table-column>
          <el-table-column :label="t('taskList.col.status')" width="120" header-align="left" align="left" sortable="custom" sort-by="status">
            <template #default="{ row }">
              <StatusBadge :status="row.status" />
            </template>
          </el-table-column>
          <el-table-column :label="t('taskList.col.rps')" width="100" header-align="right" align="right" sortable="custom" sort-by="metrics.rpsLatest">
            <template #default="{ row }">
              <span class="task-list__rps tabular-nums">{{ row.metrics?.rpsLatest ?? 0 }}</span>
            </template>
          </el-table-column>
          <el-table-column :label="t('taskList.col.progress')" width="160" header-align="left">
            <template #default="{ row }">
              <div class="task-list__progress">
                <el-progress
                  :percentage="Number(row.progressPercent.toFixed(1))"
                  :stroke-width="6"
                  :status="progressStatus(row.status)"
                  :show-text="false"
                />
                <span class="task-list__progress-val tabular-nums">{{ row.progressPercent.toFixed(1) }}%</span>
              </div>
            </template>
          </el-table-column>

          <!-- optional columns (toggle from column manager) -->
          <el-table-column
            v-if="visibleColSet.has('rg')"
            :label="t('taskList.col.rg')"
            width="120"
            header-align="left"
          >
            <template #default="{ row }">
              <span class="task-list__rg">{{ row.resourceGroup }}</span>
            </template>
          </el-table-column>
          <el-table-column
            v-if="visibleColSet.has('ip')"
            :label="t('taskList.col.ip')"
            width="140"
            header-align="left"
          >
            <template #default="{ row }">
              <span class="task-list__ip tabular-nums">{{ row.instanceIp }}</span>
            </template>
          </el-table-column>
          <el-table-column
            v-if="visibleColSet.has('createdAt')"
            :label="t('taskList.col.createdAt')"
            width="160"
            header-align="left"
          >
            <template #default="{ row }">
              <span class="task-list__time tabular-nums">{{ formatDate(row.createdAt) }}</span>
            </template>
          </el-table-column>
          <el-table-column
            v-if="visibleColSet.has('updatedAt')"
            :label="t('taskList.col.updatedAt')"
            width="160"
            header-align="left"
          >
            <template #default="{ row }">
              <span class="task-list__time tabular-nums">{{ formatDate(row.updatedAt) }}</span>
            </template>
          </el-table-column>
          <el-table-column
            v-if="visibleColSet.has('desc')"
            :label="t('taskList.col.desc')"
            min-width="200"
            show-overflow-tooltip
            header-align="left"
          >
            <template #default="{ row }">
              <span class="task-list__desc">{{ row.description || '—' }}</span>
            </template>
          </el-table-column>

          <el-table-column :label="t('taskList.col.actions')" width="200" fixed="right" header-align="left" align="left">
            <template #default="{ row }">
              <div class="task-list__row-actions">
                <el-button link type="primary" @click="goDetail(row)">
                  {{ t('task.action.view') }}
                </el-button>
                <el-button
                  v-if="can('task.pause') && row.status === 'running'"
                  link
                  type="warning"
                  @click="doAction(row, 'pause')"
                >
                  {{ t('task.action.pause') }}
                </el-button>
                <el-button
                  v-if="can('task.resume') && (row.status === 'paused' || row.status === 'failed')"
                  link
                  type="success"
                  @click="doAction(row, 'resume')"
                >
                  {{ t('task.action.resume') }}
                </el-button>
                <el-button
                  v-if="can('task.start') && (row.status === 'draft' || row.status === 'ready' || row.status === 'stopped' || row.status === 'pending' || row.status === 'creating')"
                  link
                  type="success"
                  @click="doAction(row, 'start')"
                >
                  {{ t('task.action.start') }}
                </el-button>
                <el-button
                  v-if="can('task.stop') && row.status !== 'stopped' && row.status !== 'completed' && row.status !== 'draft' && row.status !== 'ready'"
                  link
                  type="danger"
                  @click="confirmStop(row)"
                >
                  {{ t('task.action.stop') }}
                </el-button>
                <el-dropdown v-if="can('task.delete') || can('task.create')" trigger="click" @command="(cmd: string) => onRowMore(row, cmd)">
                  <el-button link>
                    <IconDotsVertical />
                  </el-button>
                  <template #dropdown>
                    <el-dropdown-menu>
                      <el-dropdown-item v-if="can('task.create')" command="edit">
                        <IconEdit /> {{ t('task.action.edit') }}
                      </el-dropdown-item>
                      <el-dropdown-item v-if="can('task.delete')" command="delete" divided>
                        <IconTrash /> {{ t('task.action.delete') }}
                      </el-dropdown-item>
                    </el-dropdown-menu>
                  </template>
                </el-dropdown>
              </div>
            </template>
          </el-table-column>

          <template #empty>
            <div class="task-list__empty">
              <IconInbox class="task-list__empty-icon" />
              <p>{{ t('taskList.empty') }}</p>
              <el-button type="primary" @click="onCreate">
                {{ createLabel }}
              </el-button>
            </div>
          </template>
        </el-table>

        <!-- pagination -->
        <footer class="task-list__footer">
          <span class="task-list__count">{{ t('common.total') }}：{{ total }}</span>
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
import { api } from '@/api/client';
import { useRbac } from '@/composables/useRbac';
import { useDocumentVisibility } from '@/composables/useDocumentVisibility';
import type {
  Task, TaskCategory, TaskStatus, EngineType, Paginated, ApiTask,
} from '@/types/domain';
import { mapApiTask } from '@/types/domain';
import { ENGINE_LABELS } from '@/types/domain';

type ViewKind = TaskCategory;

const props = defineProps<{ viewKind: ViewKind }>();
const { t } = useI18n();
const router = useRouter();
const route = useRoute();
const { can } = useRbac();
const { isVisible } = useDocumentVisibility();

const showModeCol = computed(() => false);
const apiCategory = computed(() => props.viewKind);

const title = computed(() => t(`nav.tasks.${props.viewKind}`));
const subtitle = computed(() => t(`taskList.subtitle.${props.viewKind}`));
const createLabel = computed(() =>
  t('taskList.action.create', { type: t(`task.type.${props.viewKind}`) }),
);

const list = ref<Task[]>([]);
const total = ref(0);
const page = ref(1);
const pageSize = ref(10);
const loading = ref(false);
const selected = ref<Task[]>([]);
const sortKey = ref<string>('');
const sortDir = ref<'asc' | 'desc'>('asc');

const filter = reactive({
  resourceGroup: '',
  engine: '' as EngineType | '',
  status: '' as TaskStatus | '',
  q: '',
});

/* ---------- column manager ---------- */
type OptionalColKey = 'mode' | 'rg' | 'ip' | 'createdAt' | 'updatedAt' | 'desc';
const STORAGE_KEY = `task-list:cols:${props.viewKind}`;
const DENSITY_KEY = 'task-list:density';

const optionalCols = computed<{ key: OptionalColKey; label: string }[]>(() => {
  const base: { key: OptionalColKey; label: string }[] = [
    { key: 'rg', label: t('taskList.col.rg') },
    { key: 'ip', label: t('taskList.col.ip') },
    { key: 'createdAt', label: t('taskList.col.createdAt') },
    { key: 'updatedAt', label: t('taskList.col.updatedAt') },
    { key: 'desc', label: t('taskList.col.desc') },
  ];
  if (showModeCol.value) base.unshift({ key: 'mode', label: t('taskList.col.mode') });
  return base;
});

const DEFAULT_VISIBLE: OptionalColKey[] = ['mode', 'rg'];

function loadVisible(): OptionalColKey[] {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw) return JSON.parse(raw) as OptionalColKey[];
  } catch { /* ignore */ }
  return DEFAULT_VISIBLE.filter((k) => k !== 'mode' || showModeCol.value);
}

const visibleCols = ref<OptionalColKey[]>(loadVisible());
const visibleColSet = computed(() => new Set(visibleCols.value));

function persistCols() {
  try { localStorage.setItem(STORAGE_KEY, JSON.stringify(visibleCols.value)); }
  catch { /* ignore */ }
}
function toggleCol(key: OptionalColKey, on: boolean) {
  if (on) {
    if (!visibleCols.value.includes(key)) visibleCols.value = [...visibleCols.value, key];
  } else {
    visibleCols.value = visibleCols.value.filter((k) => k !== key);
  }
  persistCols();
}
function resetCols() {
  visibleCols.value = DEFAULT_VISIBLE.filter((k) => k !== 'mode' || showModeCol.value);
  persistCols();
}

/* ---------- density ---------- */
type Density = 'compact' | 'default';
const density = ref<Density>(((): Density => {
  try {
    const raw = localStorage.getItem(DENSITY_KEY);
    return raw === 'compact' || raw === 'default' ? raw : 'default';
  } catch { return 'default'; }
})());

function setDensity(d: Density) {
  density.value = d;
  try { localStorage.setItem(DENSITY_KEY, d); } catch { /* ignore */ }
}

/* ---------- options ---------- */
const resourceGroups = ['default', 'production', 'staging', 'dev'];
const engineOptions = (Object.keys(ENGINE_LABELS) as EngineType[])
  .map((k) => ({ value: k, label: ENGINE_LABELS[k] }));
const statusOptions: TaskStatus[] = ['draft', 'ready', 'running', 'paused', 'stopping', 'stopped', 'failed'];

/* ---------- filter chips ---------- */
const activeChips = computed(() => {
  const chips: { key: string; label: string; value: string }[] = [];
  if (filter.status) {
    chips.push({ key: 'status', label: t('taskList.filter.status'), value: t(`task.status.${filter.status}`) });
  }
  if (filter.engine) {
    const eng = ENGINE_LABELS[filter.engine as EngineType];
    chips.push({ key: 'engine', label: t('taskList.filter.engine'), value: eng ?? filter.engine });
  }
  if (filter.q) {
    chips.push({ key: 'q', label: t('taskList.filter.search'), value: filter.q });
  }
  return chips;
});

function removeChip(key: string) {
  if (key === 'status') filter.status = '';
  else if (key === 'engine') filter.engine = '';
  else if (key === 'q') filter.q = '';
  applyFilter();
}

function clearAllChips() {
  filter.status = '';
  filter.engine = '';
  filter.q = '';
  applyFilter();
}

/* ---------- URL sync ---------- */
function syncFiltersToUrl() {
  const query: Record<string, string> = {};
  if (filter.status) query.status = filter.status;
  if (filter.engine) query.engine = filter.engine;
  if (filter.q) query.q = filter.q;
  router.replace({ query });
}

function readFiltersFromUrl() {
  if (route.query.status) filter.status = route.query.status as TaskStatus;
  if (route.query.engine) filter.engine = route.query.engine as EngineType;
  if (route.query.q) filter.q = String(route.query.q);
}

async function loadList() {
  loading.value = true;
  try {
    const params = new URLSearchParams({
      category: apiCategory.value,
      page: String(page.value),
      size: String(pageSize.value),
    });
    if (filter.resourceGroup) params.set('resourceGroup', filter.resourceGroup);
    if (filter.engine) params.set('engine', filter.engine);
    if (filter.status) params.set('status', filter.status);
    if (filter.q) params.set('q', filter.q);
    if (sortKey.value) {
      params.set('sort', sortKey.value);
      params.set('order', sortDir.value);
    }
    const data = await api.get<Paginated<ApiTask>>(`/tasks?${params.toString()}`);
    list.value = (data.items ?? []).map(mapApiTask);
    total.value = data.total;
  } catch {
    ElMessage.error(t('taskList.toast.loadFailed'));
  } finally {
    loading.value = false;
  }
}

function applyFilter() {
  page.value = 1;
  syncFiltersToUrl();
  loadList();
}

function onSizeChange(size: number) {
  pageSize.value = size;
  page.value = 1;
  loadList();
}

function onSelectionChange(rows: Task[]) {
  selected.value = rows;
}

function rowClassName({ row }: { row: Task }) {
  if (row.status === 'failed') return 'task-list__row--failed';
  if (row.status === 'paused') return 'task-list__row--paused';
  return '';
}

function progressStatus(s: TaskStatus): '' | 'success' | 'exception' | 'warning' {
  if (s === 'failed') return 'exception';
  if (s === 'paused') return 'warning';
  if (s === 'completed') return 'success';
  return '';
}

function formatDate(iso: string | undefined): string {
  if (!iso) return '—';
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return '—';
  const pad = (n: number) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

function goDetail(row: Task) {
  router.push({ path: `/tasks/${row.category}/${row.id}` });
}

async function doAction(row: Task, action: string) {
  try {
    const lifecycleMap: Record<string, string> = {
      start: 'start',
      stop: 'stop',
      pause: 'pause',
      resume: 'resume',
    };
    const endpoint = lifecycleMap[action];
    if (endpoint) {
      await api.post(`/tasks/${row.id}/${endpoint}`);
    }
    ElMessage.success(t(`taskList.toast.action.${mapToastKey(action)}`));
    loadList();
  } catch {
    ElMessage.error(t('taskList.toast.actionFailed'));
  }
}

function mapToastKey(action: string): string {
  if (action === 'resume' || action === 'start') return 'resumed';
  if (action === 'pause') return 'paused';
  if (action === 'stop') return 'stopped';
  return 'started';
}

function confirmDelete(row: Task) {
  ElMessageBox.confirm(
    t('taskList.confirm.delete', { name: row.name }),
    t('task.action.delete'),
    { type: 'warning', confirmButtonText: t('common.confirm'), cancelButtonText: t('common.cancel') },
  ).then(async () => {
    await api.del(`/tasks/${row.id}`);
    ElMessage.success(t('taskList.toast.deleted'));
    loadList();
  }).catch(() => {});
}

function onRowMore(row: Task, cmd: string) {
  if (cmd === 'edit') {
    router.push({ path: `/tasks/${row.category}/${row.id}`, query: { tab: 'config', edit: '1' } });
  } else if (cmd === 'delete') {
    confirmDelete(row);
  }
}

function confirmStop(row: Task) {
  ElMessageBox.confirm(
    t('taskList.confirm.stop', { name: row.name }),
    t('task.action.stop'),
    { type: 'warning', confirmButtonText: t('common.confirm'), cancelButtonText: t('common.cancel') },
  ).then(() => doAction(row, 'stop')).catch(() => {});
}

async function onBatch(cmd: string) {
  if (!selected.value.length) return;
  if (cmd === 'delete') {
    ElMessageBox.confirm(
      t('taskList.confirm.batchDelete', { n: selected.value.length }),
      t('taskList.batch.delete'),
      { type: 'warning' },
    ).then(async () => {
      await Promise.all(selected.value.map((row) => api.del(`/tasks/${row.id}`)));
      ElMessage.success(t('taskList.toast.deleted'));
      loadList();
    }).catch(() => {});
    return;
  }
  const lifecycleMap: Record<string, string> = {
    start: 'start',
    pause: 'pause',
    resume: 'resume',
    stop: 'stop',
  };
  const endpoint = lifecycleMap[cmd];
  if (!endpoint) return;
  await Promise.all(
    selected.value.map((row) => api.post(`/tasks/${row.id}/${endpoint}`)),
  );
  ElMessage.success(t(`taskList.toast.action.${mapToastKey(cmd)}`));
  loadList();
}

function onExport() {
  const payload = JSON.stringify(list.value, null, 2);
  const blob = new Blob([payload], { type: 'application/json' });
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = `ape-dts-${props.viewKind}-tasks.json`;
  a.click();
  URL.revokeObjectURL(url);
  ElMessage.success(t('taskList.toast.exported'));
}

function onImport() {
  ElMessage.info(t('taskList.toast.importHint'));
}

function onExportTemplate() {
  const sample = selected.value[0] ?? list.value[0];
  if (!sample) return;
  api.post<{ ini: string }>('/tasks/preview-ini', sample).then(({ ini }) => {
    const blob = new Blob([ini], { type: 'text/plain' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = `ape-dts-${sample.id}.ini`;
    a.click();
    URL.revokeObjectURL(url);
  });
}

function onDownloadTemplate() {
  const tpl = `# ape-dts task template · ${props.viewKind}\n[extractor]\ndb_type=mysql\nextract_type=snapshot_cdc\nurl=*******************************/app_db\n\n[sinker]\ndb_type=postgres\nsink_type=write\nurl=**********************************/app_db\nbatch_size=200\n\n[pipeline]\nbuffer_size=16000\ncheckpoint_interval_secs=10\n`;
  const blob = new Blob([tpl], { type: 'text/plain' });
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = `ape-dts-${props.viewKind}-template.ini`;
  a.click();
  URL.revokeObjectURL(url);
}

function onCreate() {
  router.push({ path: `/tasks/create/${props.viewKind}` });
}

/* ---------- sorting ---------- */
function onSortChange({ prop, order }: { prop: string; order: string | null }) {
  if (!order) {
    sortKey.value = '';
    sortDir.value = 'asc';
  } else {
    sortKey.value = prop;
    sortDir.value = order === 'ascending' ? 'asc' : 'desc';
  }
  loadList();
}

let pollId: ReturnType<typeof setInterval> | null = null;
const POLL_INTERVAL_MS = 5_000;

onMounted(() => {
  readFiltersFromUrl();
  loadList();
  pollId = setInterval(() => {
    if (isVisible.value) loadList();
  }, POLL_INTERVAL_MS);
});
onUnmounted(() => {
  if (pollId) clearInterval(pollId);
});

watch(() => props.viewKind, () => {
  page.value = 1;
  visibleCols.value = loadVisible();
  loadList();
});

watch(() => route.query, () => {
  readFiltersFromUrl();
});
</script>

<style scoped>
.task-list {
  display: flex;
  flex-direction: column;
}
.task-list__body {
  display: flex;
  flex-direction: column;
  gap: 16px;
  flex: 1;
}
.task-list__panel {
  padding: 16px 20px 12px;
  display: flex;
  flex-direction: column;
  gap: 14px;
}
.task-list__toolbar {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 12px;
}
.task-list__actions {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}
.task-list__view-controls {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-shrink: 0;
}
.task-list__selected-count {
  font-size: 12px;
  color: var(--color-primary-700);
  margin-left: 4px;
}
.task-list__dropdown-icon {
  margin-left: 4px;
  color: var(--color-ink-subtle);
}
.task-list__filters {
  display: flex;
  gap: 12px;
  flex-wrap: wrap;
  align-items: center;
}
.task-list__filter--sm {
  width: 160px;
}
.task-list__filter--grow {
  flex: 1;
  min-width: 240px;
}
.task-list__engine-option {
  display: inline-flex;
  align-items: center;
  gap: 8px;
}
.task-list__chips {
  display: flex;
  gap: 8px;
  align-items: center;
  flex-wrap: wrap;
}
.task-list__chip {
  font-size: 12px;
}
.task-list__rps {
  font-family: var(--font-mono);
  font-size: 12px;
  color: var(--color-ink-muted);
}

.task-list__cols-pop {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.task-list__cols-pop-head {
  display: flex;
  justify-content: space-between;
  align-items: center;
  font-size: var(--text-sm);
  font-weight: 600;
  color: var(--color-ink);
}
.task-list__cols-pop-body {
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 4px 0;
  max-height: 240px;
  overflow-y: auto;
}
.task-list__cols-pop-foot {
  font-size: 12px;
  color: var(--color-ink-faint);
  border-top: 1px dashed var(--color-border);
  padding-top: 6px;
}

/* table — horizontal scroll fallback when content is wider than container */
.task-list__table {
  border-radius: var(--radius-md);
  overflow: hidden;
}
.task-list__table :deep(.el-table__body-wrapper),
.task-list__table :deep(.el-table__header-wrapper) {
  overflow-x: auto;
}

.task-list__name-cell {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
  align-items: flex-start;
}
.task-list__name-cell :deep(.el-link__inner) {
  font-weight: 600;
  letter-spacing: 0;
}
.task-list__id {
  font-family: var(--font-mono);
  font-size: 11px;
  color: var(--color-ink-faint);
  line-height: 1.3;
}
.task-list__rg {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  padding: 2px 8px;
  min-width: 76px;
  height: 22px;
  background: var(--color-surface-2);
  color: var(--color-ink-muted);
  border: 1px solid var(--color-border);
  border-radius: 999px;
  font-size: 12px;
  font-weight: 500;
  white-space: nowrap;
  box-sizing: border-box;
}
.task-list__mode {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  padding: 2px 10px;
  min-width: 96px;
  height: 22px;
  border-radius: 999px;
  font-size: 12px;
  font-weight: 500;
  background: var(--color-surface-2);
  color: var(--color-ink-muted);
  border: 1px solid var(--color-border);
  white-space: nowrap;
  box-sizing: border-box;
}
.task-list__mode--cdc {
  background: #ECFEFF;
  color: #0E7490;
  border-color: #A5F3FC;
}
.task-list__mode--snapshot {
  background: #FEF3C7;
  color: #92400E;
  border-color: #FDE68A;
}
.task-list__mode--snapshot_cdc {
  background: var(--color-primary-50, #ECFDF5);
  color: var(--color-primary-700, #0F766E);
  border-color: var(--color-primary-200, #99F6E4);
}
.task-list__progress {
  display: flex;
  align-items: center;
  gap: 8px;
}
.task-list__progress-val {
  font-size: 12px;
  color: var(--color-ink-muted);
  min-width: 44px;
  text-align: right;
}
.task-list__ip,
.task-list__time {
  font-family: var(--font-mono);
  font-size: 12px;
  color: var(--color-ink-muted);
}
.task-list__desc {
  color: var(--color-ink-muted);
  font-size: 13px;
}
.task-list__row-actions {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  flex-wrap: nowrap;
  white-space: nowrap;
}
.task-list__row-actions :deep(.el-button) {
  padding: 0;
  height: 22px;
  font-size: 13px;
}
.task-list__row-actions :deep(.el-button + .el-button) {
  margin-left: 0;
}
.task-list__val--danger { color: var(--color-danger); }
.task-list__val--warning { color: var(--color-warning); }

/* keep header & cell horizontally aligned with a uniform horizontal rhythm */
.task-list__table :deep(.el-table .cell) {
  display: flex;
  align-items: center;
  padding-left: 12px;
  padding-right: 12px;
}
.task-list__table :deep(.el-table th .cell) {
  font-weight: 600;
  color: var(--color-ink);
}
.task-list__table :deep(.el-table__cell) {
  vertical-align: middle;
}
:deep(.task-list__row--failed) {
  background: color-mix(in oklab, var(--color-danger) 3%, transparent);
}
:deep(.task-list__row--paused) {
  background: color-mix(in oklab, var(--color-warning) 3%, transparent);
}

/* compact density: tighten cell padding */
.task-list--compact :deep(.el-table .cell) {
  padding-top: 4px;
  padding-bottom: 4px;
}

.task-list__empty {
  display: flex;
  flex-direction: column;
  gap: 12px;
  align-items: center;
  padding: 32px 0;
  color: var(--color-ink-subtle);
}
.task-list__empty-icon {
  width: 40px;
  height: 40px;
  color: var(--color-ink-faint);
}
.task-list__footer {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding-top: 8px;
  color: var(--color-ink-subtle);
  font-size: 12px;
}
</style>
