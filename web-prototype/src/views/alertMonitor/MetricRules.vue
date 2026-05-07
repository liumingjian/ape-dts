<template>
  <div class="metric-rules">
    <PageHeader :title="t('monitor.metric.title')" :subtitle="t('monitor.metric.subtitle')">
      <template #actions>
        <el-button @click="loadList">
          <template #icon><IconRefresh /></template>
          {{ t('common.refresh') }}
        </el-button>
        <el-button v-if="can('alert.rule.manage')" type="primary" @click="openCreate">
          <template #icon><IconPlus /></template>
          {{ t('monitor.metric.action.create') }}
        </el-button>
      </template>
    </PageHeader>

    <div class="ape-dts-console-page metric-rules__body">
      <div class="ape-dts-console-card metric-rules__panel">
        <div class="metric-rules__filters">
          <el-select
            v-model="filter.status"
            :placeholder="t('monitor.metric.col.status')"
            clearable
            class="metric-rules__filter"
            @change="applyFilter"
          >
            <el-option value="enabled" :label="t('common.enable')" />
            <el-option value="disabled" :label="t('common.disable')" />
          </el-select>
          <el-select
            v-model="filter.level"
            :placeholder="t('monitor.metric.col.threshold')"
            clearable
            class="metric-rules__filter"
            @change="applyFilter"
          >
            <el-option v-for="lvl in LEVELS" :key="lvl" :value="lvl">
              <LevelBadge :level="lvl" />
            </el-option>
          </el-select>
          <el-input
            v-model="filter.q"
            :placeholder="t('common.search')"
            clearable
            class="metric-rules__filter metric-rules__filter--grow"
            @keyup.enter="applyFilter"
            @clear="applyFilter"
          >
            <template #prefix><IconSearch /></template>
          </el-input>
        </div>

        <el-table
          v-loading="loading"
          :data="filteredList"
          row-key="id"
          stripe
          class="metric-rules__table"
        >
          <el-table-column :label="t('monitor.metric.col.id')" width="160">
            <template #default="{ row }">
              <span class="metric-rules__id">{{ row.id }}</span>
            </template>
          </el-table-column>
          <el-table-column :label="t('monitor.metric.col.name')" min-width="220">
            <template #default="{ row }">
              <div class="metric-rules__name">
                <span class="metric-rules__name-text">{{ row.name }}</span>
                <span class="metric-rules__metric">{{ row.metric }}</span>
              </div>
            </template>
          </el-table-column>
          <el-table-column :label="t('monitor.metric.col.category')" width="110">
            <template #default="{ row }"><LevelBadge :level="row.level" /></template>
          </el-table-column>
          <el-table-column :label="t('monitor.metric.col.status')" width="120">
            <template #default="{ row }">
              <span :class="`metric-rules__status metric-rules__status--${row.status}`">
                <span class="metric-rules__status-dot" />
                {{ row.status === 'enabled' ? t('common.enable') : t('common.disable') }}
              </span>
            </template>
          </el-table-column>
          <el-table-column :label="t('monitor.metric.col.threshold')" min-width="200">
            <template #default="{ row }">
              <span class="tabular-nums metric-rules__thresh">
                {{ row.metric }} <strong>{{ row.operator }}</strong> {{ formatNumber(row.threshold) }}
              </span>
            </template>
          </el-table-column>
          <el-table-column :label="t('monitor.metric.col.enabled')" width="100">
            <template #default="{ row }">
              <el-switch
                :model-value="row.status === 'enabled'"
                :disabled="!can('alert.rule.manage')"
                @change="(v: unknown) => onToggle(row, v as boolean)"
              />
            </template>
          </el-table-column>
          <el-table-column :label="t('monitor.metric.col.period')" width="120" align="right">
            <template #default="{ row }">
              <span class="tabular-nums">{{ row.periodMin }}</span>
            </template>
          </el-table-column>
          <el-table-column :label="t('monitor.metric.col.trigger')" width="120" align="right">
            <template #default="{ row }">
              <span class="tabular-nums">{{ row.triggerCount }}</span>
            </template>
          </el-table-column>
          <el-table-column :label="t('monitor.metric.col.recovery')" width="140" align="right">
            <template #default="{ row }">
              <span class="tabular-nums">{{ formatNumber(row.recoveryThreshold) }}</span>
            </template>
          </el-table-column>
          <el-table-column v-if="can('alert.rule.manage')" :label="t('monitor.metric.col.actions')" width="160" fixed="right">
            <template #default="{ row }">
              <el-button link type="primary" @click="openEdit(row)">{{ t('common.edit') }}</el-button>
              <el-button link type="danger" @click="confirmDelete(row)">{{ t('common.delete') }}</el-button>
            </template>
          </el-table-column>

          <template #empty>
            <div class="metric-rules__empty">
              <IconBellRinging class="metric-rules__empty-icon" />
              <p>{{ t('monitor.metric.empty') }}</p>
              <el-button type="primary" @click="openCreate">{{ t('monitor.metric.action.create') }}</el-button>
            </div>
          </template>
        </el-table>

        <footer class="metric-rules__footer">
          <span>{{ t('common.total') }}：{{ total }}</span>
          <el-pagination
            v-model:current-page="page"
            v-model:page-size="pageSize"
            :total="total"
            :page-sizes="[10, 20, 50]"
            layout="sizes, prev, pager, next, jumper"
            background
            @current-change="loadList"
            @size-change="(s: number) => { pageSize = s; loadList(); }"
          />
        </footer>
      </div>
    </div>

    <el-drawer
      v-model="drawerVisible"
      :title="editing?.id ? t('monitor.metric.action.edit') : t('monitor.metric.action.create')"
      size="540px"
      append-to-body
      destroy-on-close
    >
      <el-form v-if="form" label-width="120px" label-position="top" class="metric-rules__form">
        <el-form-item :label="t('monitor.metric.field.name')" required>
          <el-input v-model="form.name" />
        </el-form-item>
        <el-form-item :label="t('monitor.metric.field.metric')" required>
          <el-select v-model="form.metric" filterable allow-create>
            <el-option v-for="m in METRIC_OPTIONS" :key="m" :value="m" :label="m" />
          </el-select>
        </el-form-item>
        <el-row :gutter="12">
          <el-col :span="8">
            <el-form-item :label="t('monitor.metric.field.operator')">
              <el-select v-model="form.operator">
                <el-option v-for="op in (['>', '<', '>=', '<=', '=='] as const)" :key="op" :value="op" :label="op" />
              </el-select>
            </el-form-item>
          </el-col>
          <el-col :span="16">
            <el-form-item :label="t('monitor.metric.field.threshold')">
              <el-input-number v-model="form.threshold" :min="0" :precision="0" />
            </el-form-item>
          </el-col>
        </el-row>
        <el-form-item :label="t('monitor.metric.field.level')">
          <el-radio-group v-model="form.level">
            <el-radio-button v-for="lvl in LEVELS" :key="lvl" :value="lvl">{{ levelLabel(lvl) }}</el-radio-button>
          </el-radio-group>
        </el-form-item>
        <el-row :gutter="12">
          <el-col :span="12">
            <el-form-item :label="t('monitor.metric.field.period')">
              <el-input-number v-model="form.periodMin" :min="1" :precision="0" />
            </el-form-item>
          </el-col>
          <el-col :span="12">
            <el-form-item :label="t('monitor.metric.field.trigger')">
              <el-input-number v-model="form.triggerCount" :min="1" :precision="0" />
            </el-form-item>
          </el-col>
        </el-row>
        <el-form-item :label="t('monitor.metric.field.recovery')">
          <el-input-number v-model="form.recoveryThreshold" :min="0" :precision="0" />
        </el-form-item>
        <el-form-item :label="t('monitor.metric.field.description')">
          <el-input v-model="form.description" type="textarea" :rows="3" />
        </el-form-item>
      </el-form>

      <template #footer>
        <el-button @click="drawerVisible = false">{{ t('common.cancel') }}</el-button>
        <el-button type="primary" @click="save">{{ t('common.save') }}</el-button>
      </template>
    </el-drawer>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, reactive, ref } from 'vue';
import { useI18n } from 'vue-i18n';
import { ElMessage, ElMessageBox } from 'element-plus';
import IconBellRinging from '~icons/tabler/bell-ringing';
import PageHeader from '@/components/PageHeader.vue';
import LevelBadge from '@/components/LevelBadge.vue';
import { api } from '@/api/client';
import { useRbac } from '@/composables/useRbac';
import type { MetricRule, AlertLevel, Paginated } from '@/types/domain';

const { t } = useI18n();
const { can } = useRbac();

const LEVELS: AlertLevel[] = ['critical', 'major', 'minor', 'info'];
const METRIC_OPTIONS = [
  'extractor_pushed_rps_avg',
  'extractor_pushed_bps_avg',
  'sinker_record_count_avg_by_sec',
  'sinker_rt_per_query_avg',
  'sinker_rt_per_query_max',
  'sinker_records_per_query_avg',
  'sinker_bps_avg_by_sec',
  'pipeline_buffer_size_avg',
  'pipeline_record_size_avg',
  'pipeline_sinked_count_latest',
  'latency_ms',
];

const list = ref<MetricRule[]>([]);
const total = ref(0);
const page = ref(1);
const pageSize = ref(10);
const loading = ref(false);

const filter = reactive({ status: '' as 'enabled' | 'disabled' | '', level: '' as AlertLevel | '', q: '' });

const filteredList = computed(() => {
  let l = list.value;
  if (filter.level) l = l.filter((r) => r.level === filter.level);
  return l;
});

async function loadList() {
  loading.value = true;
  try {
    const params = new URLSearchParams({ page: String(page.value), size: String(pageSize.value) });
    if (filter.status) params.set('status', filter.status);
    if (filter.q) params.set('q', filter.q);
    const data = await api.get<Paginated<MetricRule>>(`/alert_rules?${params.toString()}`);
    list.value = data.items;
    total.value = data.total;
  } catch {
    ElMessage.error('加载失败');
  } finally {
    loading.value = false;
  }
}
function applyFilter() { page.value = 1; loadList(); }

const drawerVisible = ref(false);
const editing = ref<MetricRule | null>(null);
const form = ref<MetricRule | null>(null);

function openCreate() {
  editing.value = null;
  form.value = {
    id: '',
    name: '',
    metric: 'extractor_pushed_rps_avg',
    operator: '>',
    threshold: 100,
    level: 'major',
    status: 'enabled',
    periodMin: 5,
    triggerCount: 1,
    recoveryThreshold: 80,
    description: '',
  };
  drawerVisible.value = true;
}
function openEdit(row: MetricRule) {
  editing.value = row;
  form.value = { ...row };
  drawerVisible.value = true;
}

async function save() {
  if (!form.value) return;
  try {
    if (editing.value) {
      await api.patch(`/alert_rules/${editing.value.id}`, form.value);
      ElMessage.success(t('monitor.metric.toast.updated'));
    } else {
      await api.post('/alert_rules', form.value);
      ElMessage.success(t('monitor.metric.toast.created'));
    }
    drawerVisible.value = false;
    loadList();
  } catch {
    ElMessage.error('保存失败');
  }
}

async function onToggle(row: MetricRule, v: boolean) {
  await api.patch(`/alert_rules/${row.id}`, { status: v ? 'enabled' : 'disabled' });
  ElMessage.success(t('monitor.metric.toast.toggled'));
  loadList();
}

function confirmDelete(row: MetricRule) {
  ElMessageBox.confirm(`确定删除规则「${row.name}」？`, t('common.delete'), { type: 'warning' })
    .then(async () => {
      await api.del(`/alert_rules/${row.id}`);
      ElMessage.success(t('monitor.metric.toast.deleted'));
      loadList();
    })
    .catch(() => {});
}

function formatNumber(v: number) {
  if (v >= 1_000_000) return `${(v / 1_000_000).toFixed(1)}M`;
  if (v >= 1_000) return `${(v / 1_000).toFixed(1)}K`;
  return String(v);
}
function levelLabel(lvl: AlertLevel) {
  return ({ critical: '紧急', major: '重要', minor: '次要', info: '提示' } as Record<AlertLevel, string>)[lvl];
}

onMounted(loadList);
</script>

<style scoped>
.metric-rules { display: flex; flex-direction: column; }
.metric-rules__body { display: flex; flex-direction: column; gap: 16px; }
.metric-rules__panel { padding: 16px 20px 12px; display: flex; flex-direction: column; gap: 14px; }
.metric-rules__filters { display: flex; gap: 12px; flex-wrap: wrap; align-items: center; }
.metric-rules__filter { width: 200px; }
.metric-rules__filter--grow { flex: 1; min-width: 240px; }
.metric-rules__id { font-family: var(--font-mono); font-size: 12px; color: var(--color-ink-muted); }
.metric-rules__name { display: flex; flex-direction: column; gap: 2px; }
.metric-rules__name-text { color: var(--color-ink); font-weight: 500; }
.metric-rules__metric { font-family: var(--font-mono); font-size: 11px; color: var(--color-ink-faint); }
.metric-rules__status { display: inline-flex; align-items: center; gap: 6px; font-size: 12px; }
.metric-rules__status-dot { width: 6px; height: 6px; border-radius: 999px; background: currentColor; }
.metric-rules__status--enabled { color: var(--color-success); }
.metric-rules__status--disabled { color: var(--color-ink-faint); }
.metric-rules__thresh { font-size: 12px; color: var(--color-ink-muted); }
.metric-rules__thresh strong { color: var(--color-primary-700); margin: 0 4px; }
.metric-rules__form { padding: 0 8px; }
.metric-rules__empty { display: flex; flex-direction: column; gap: 12px; align-items: center; padding: 36px 0; color: var(--color-ink-subtle); }
.metric-rules__empty-icon { width: 38px; height: 38px; color: var(--color-ink-faint); }
.metric-rules__footer { display: flex; justify-content: space-between; align-items: center; padding-top: 8px; color: var(--color-ink-subtle); font-size: 12px; }
</style>
