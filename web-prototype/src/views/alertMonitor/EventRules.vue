<template>
  <div class="event-rules">
    <PageHeader :title="t('monitor.event.title')" :subtitle="t('monitor.event.subtitle')">
      <template #actions>
        <el-button @click="loadList">
          <template #icon><IconRefresh /></template>
          {{ t('common.refresh') }}
        </el-button>
      </template>
    </PageHeader>

    <div class="ape-dts-console-page event-rules__body">
      <div class="ape-dts-console-card event-rules__panel">
        <div class="event-rules__filters">
          <el-select
            v-model="filter.category"
            :placeholder="t('monitor.event.col.category')"
            clearable
            class="event-rules__filter"
            @change="applyFilter"
          >
            <el-option v-for="c in CATS" :key="c" :value="c" :label="t(`monitor.event.cat.${c}`)" />
          </el-select>
          <el-select
            v-model="filter.level"
            :placeholder="t('monitor.event.col.level')"
            clearable
            class="event-rules__filter"
            @change="applyFilter"
          >
            <el-option v-for="lvl in LEVELS" :key="lvl" :value="lvl" :label="lvl" />
          </el-select>
          <el-input
            v-model="filter.q"
            :placeholder="t('common.search')"
            clearable
            class="event-rules__filter event-rules__filter--grow"
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
          class="event-rules__table"
        >
          <el-table-column :label="t('monitor.event.col.id')" width="160">
            <template #default="{ row }">
              <span class="event-rules__id">{{ row.id }}</span>
            </template>
          </el-table-column>
          <el-table-column :label="t('monitor.event.col.name')" min-width="180">
            <template #default="{ row }">
              <span class="event-rules__name">{{ row.name }}</span>
            </template>
          </el-table-column>
          <el-table-column :label="t('monitor.event.col.category')" width="120">
            <template #default="{ row }">
              <span class="event-rules__cat" :class="`event-rules__cat--${row.category}`">
                {{ t(`monitor.event.cat.${row.category}`) }}
              </span>
            </template>
          </el-table-column>
          <el-table-column :label="t('monitor.event.col.level')" width="110">
            <template #default="{ row }"><LevelBadge :level="row.level" /></template>
          </el-table-column>
          <el-table-column :label="t('monitor.event.col.source')" width="160">
            <template #default="{ row }">
              <span class="event-rules__src">{{ row.source }}</span>
            </template>
          </el-table-column>
          <el-table-column :label="t('monitor.event.col.enabled')" width="100">
            <template #default="{ row }">
              <el-switch
                :model-value="row.status === 'enabled'"
                :disabled="!can('alert.rule.manage')"
                @change="(v: unknown) => onToggle(row, v as boolean)"
              />
            </template>
          </el-table-column>
          <el-table-column :label="t('monitor.event.col.validUntil')" width="180">
            <template #default="{ row }">
              <span class="event-rules__time">{{ formatDate(row.validUntil) }}</span>
            </template>
          </el-table-column>
          <el-table-column :label="t('monitor.event.col.period')" width="120" align="right">
            <template #default="{ row }">
              <span class="tabular-nums">{{ row.periodMin }}</span>
            </template>
          </el-table-column>
          <el-table-column :label="t('monitor.event.col.trigger')" width="120" align="right">
            <template #default="{ row }">
              <span class="tabular-nums">{{ row.triggerCount }}</span>
            </template>
          </el-table-column>
          <el-table-column :label="t('monitor.event.col.desc')" min-width="220" show-overflow-tooltip>
            <template #default="{ row }">
              <span class="event-rules__desc">{{ row.description }}</span>
            </template>
          </el-table-column>
          <el-table-column v-if="can('alert.rule.manage')" label="" width="120" fixed="right">
            <template #default="{ row }">
              <el-button link type="primary" @click="openEdit(row)">{{ t('common.edit') }}</el-button>
            </template>
          </el-table-column>
        </el-table>

        <footer class="event-rules__footer">
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

    <el-drawer v-model="drawerVisible" :title="t('common.edit')" size="480px" destroy-on-close>
      <el-form v-if="form" label-position="top" class="event-rules__form">
        <el-form-item :label="t('monitor.event.col.name')">
          <el-input v-model="form.name" disabled />
        </el-form-item>
        <el-form-item :label="t('monitor.event.col.level')">
          <el-radio-group v-model="form.level">
            <el-radio-button v-for="lvl in LEVELS" :key="lvl" :value="lvl">{{ lvl }}</el-radio-button>
          </el-radio-group>
        </el-form-item>
        <el-row :gutter="12">
          <el-col :span="12">
            <el-form-item :label="t('monitor.event.col.period')">
              <el-input-number v-model="form.periodMin" :min="1" />
            </el-form-item>
          </el-col>
          <el-col :span="12">
            <el-form-item :label="t('monitor.event.col.trigger')">
              <el-input-number v-model="form.triggerCount" :min="1" />
            </el-form-item>
          </el-col>
        </el-row>
        <el-form-item :label="t('monitor.event.col.validUntil')">
          <el-date-picker v-model="form.validUntil" type="datetime" value-format="YYYY-MM-DDTHH:mm:ss" style="width: 100%" />
        </el-form-item>
        <el-form-item :label="t('monitor.event.col.desc')">
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
import { ElMessage } from 'element-plus';
import dayjs from 'dayjs';
import PageHeader from '@/components/PageHeader.vue';
import LevelBadge from '@/components/LevelBadge.vue';
import { api } from '@/api/client';
import { useRbac } from '@/composables/useRbac';
import type { SysEvent, AlertLevel, Paginated } from '@/types/domain';

const { t } = useI18n();
const { can } = useRbac();

const CATS = ['task', 'system', 'security'] as const;
const LEVELS: AlertLevel[] = ['critical', 'major', 'minor', 'info'];

const list = ref<SysEvent[]>([]);
const total = ref(0);
const page = ref(1);
const pageSize = ref(10);
const loading = ref(false);

const filter = reactive({ category: '' as 'task' | 'system' | 'security' | '', level: '' as AlertLevel | '', q: '' });

const filteredList = computed(() => {
  let l = list.value;
  if (filter.level) l = l.filter((e) => e.level === filter.level);
  return l;
});

async function loadList() {
  loading.value = true;
  try {
    const params = new URLSearchParams({ page: String(page.value), size: String(pageSize.value) });
    if (filter.category) params.set('category', filter.category);
    if (filter.q) params.set('q', filter.q);
    const data = await api.get<Paginated<SysEvent>>(`/alert_rules?kind=event&${params.toString()}`);
    list.value = data.items;
    total.value = data.total;
  } finally {
    loading.value = false;
  }
}
function applyFilter() { page.value = 1; loadList(); }

const drawerVisible = ref(false);
const editing = ref<SysEvent | null>(null);
const form = ref<SysEvent | null>(null);

function openEdit(row: SysEvent) {
  editing.value = row;
  form.value = { ...row };
  drawerVisible.value = true;
}

async function save() {
  if (!form.value || !editing.value) return;
  await api.patch(`/alert_rules/${editing.value.id}`, form.value);
  ElMessage.success(t('common.save'));
  drawerVisible.value = false;
  loadList();
}

async function onToggle(row: SysEvent, v: boolean) {
  await api.patch(`/alert_rules/${row.id}`, { enabled: v });
  ElMessage.success(t('monitor.metric.toast.toggled'));
  loadList();
}

function formatDate(s: string) { return dayjs(s).format('YYYY-MM-DD HH:mm'); }

onMounted(loadList);
</script>

<style scoped>
.event-rules { display: flex; flex-direction: column; }
.event-rules__body { display: flex; flex-direction: column; gap: 16px; }
.event-rules__panel { padding: 16px 20px 12px; display: flex; flex-direction: column; gap: 14px; }
.event-rules__filters { display: flex; gap: 12px; flex-wrap: wrap; align-items: center; }
.event-rules__filter { width: 200px; }
.event-rules__filter--grow { flex: 1; min-width: 240px; }
.event-rules__id { font-family: var(--font-mono); font-size: 12px; color: var(--color-ink-muted); }
.event-rules__name { color: var(--color-ink); font-weight: 500; }
.event-rules__cat { display: inline-flex; padding: 2px 8px; border-radius: 4px; font-size: 12px; background: var(--color-surface-2); color: var(--color-ink-muted); }
.event-rules__cat--task     { background: var(--color-primary-50); color: var(--color-primary-700); }
.event-rules__cat--system   { background: var(--color-info-soft); color: var(--color-info); }
.event-rules__cat--security { background: var(--color-danger-soft); color: var(--color-danger); }
.event-rules__src { font-family: var(--font-mono); font-size: 12px; color: var(--color-ink-muted); }
.event-rules__time { font-family: var(--font-mono); font-size: 12px; color: var(--color-ink-muted); }
.event-rules__desc { color: var(--color-ink-muted); font-size: 13px; }
.event-rules__form { padding: 0 8px; }
.event-rules__footer { display: flex; justify-content: space-between; align-items: center; padding-top: 8px; color: var(--color-ink-subtle); font-size: 12px; }
</style>
