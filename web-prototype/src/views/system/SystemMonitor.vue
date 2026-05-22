<template>
  <div class="sys-mon">
    <PageHeader :title="t('system.monitor.title')" :subtitle="t('system.monitor.subtitle')">
      <template #actions>
        <el-button @click="loadList">
          <template #icon><IconRefresh /></template>
          {{ t('common.refresh') }}
        </el-button>
      </template>
    </PageHeader>

    <div class="ape-dts-console-page sys-mon__body">
      <div class="sys-mon__summary">
        <div class="sys-mon__sum sys-mon__sum--healthy">
          <div class="sys-mon__sum-label"><IconCircleCheck /> {{ t('system.monitor.status.healthy') }}</div>
          <div class="sys-mon__sum-value tabular-nums">{{ counters.healthy }}</div>
        </div>
        <div class="sys-mon__sum sys-mon__sum--warning">
          <div class="sys-mon__sum-label"><IconAlertTriangle /> {{ t('system.monitor.status.warning') }}</div>
          <div class="sys-mon__sum-value tabular-nums">{{ counters.warning }}</div>
        </div>
        <div class="sys-mon__sum sys-mon__sum--error">
          <div class="sys-mon__sum-label"><IconCircleX /> {{ t('system.monitor.status.error') }}</div>
          <div class="sys-mon__sum-value tabular-nums">{{ counters.error }}</div>
        </div>
        <div class="sys-mon__sum sys-mon__sum--total">
          <div class="sys-mon__sum-label">{{ t('common.total') }}</div>
          <div class="sys-mon__sum-value tabular-nums">{{ list.length }}</div>
        </div>
      </div>

      <div v-loading="loading" class="ape-dts-console-card sys-mon__panel">
        <div class="sys-mon__filters">
          <el-input
            v-model="filter.host"
            :placeholder="t('system.monitor.filter.host')"
            clearable
            style="width: 240px;"
            @keyup.enter="loadList"
            @clear="loadList"
          >
            <template #prefix><IconSearch /></template>
          </el-input>
          <el-input
            v-model="filter.ip"
            :placeholder="t('system.monitor.filter.ip')"
            clearable
            style="width: 240px;"
            @keyup.enter="loadList"
            @clear="loadList"
          >
            <template #prefix><IconSearch /></template>
          </el-input>
        </div>

        <el-table :data="filteredList" stripe class="sys-mon__table">
          <el-table-column :label="t('system.monitor.col.host')" prop="hostname" sortable min-width="180">
            <template #default="{ row }">
              <span class="sys-mon__host">{{ row.hostname }}</span>
            </template>
          </el-table-column>
          <el-table-column :label="t('system.monitor.col.ip')" prop="ip" sortable width="160">
            <template #default="{ row }">
              <span class="sys-mon__ip">{{ row.ip }}</span>
            </template>
          </el-table-column>
          <el-table-column :label="t('system.monitor.col.role')" prop="role" sortable width="120">
            <template #default="{ row }">
              <span class="sys-mon__role" :class="`sys-mon__role--${row.role}`">
                {{ t(`system.monitor.role.${row.role}`) }}
              </span>
            </template>
          </el-table-column>
          <el-table-column :label="t('system.monitor.col.status')" prop="status" sortable width="120">
            <template #default="{ row }">
              <span class="sys-mon__status" :class="`sys-mon__status--${row.status}`">
                <span class="sys-mon__dot" />
                {{ t(`system.monitor.status.${row.status}`) }}
              </span>
            </template>
          </el-table-column>
          <el-table-column :label="t('system.monitor.col.nodeType')" prop="nodeType" sortable width="120">
            <template #default="{ row }">
              <span class="sys-mon__node">{{ t(`system.monitor.nodeType.${row.nodeType}`) }}</span>
            </template>
          </el-table-column>
          <el-table-column :label="t('system.monitor.col.cpu')" prop="cpuPercent" sortable width="200">
            <template #default="{ row }">
              <div class="sys-mon__bar-cell">
                <el-progress :percentage="Number(row.cpuPercent.toFixed(1))" :stroke-width="6" :status="usageStatus(row.cpuPercent)" :show-text="false" />
                <span class="tabular-nums">{{ row.cpuPercent.toFixed(1) }}%</span>
              </div>
            </template>
          </el-table-column>
          <el-table-column :label="t('system.monitor.col.mem')" prop="memoryPercent" sortable width="200">
            <template #default="{ row }">
              <div class="sys-mon__bar-cell">
                <el-progress :percentage="Number(row.memoryPercent.toFixed(1))" :stroke-width="6" :status="usageStatus(row.memoryPercent)" :show-text="false" />
                <span class="tabular-nums">{{ row.memoryPercent.toFixed(1) }}%</span>
              </div>
            </template>
          </el-table-column>
          <el-table-column :label="t('system.monitor.col.disk')" prop="diskPercent" sortable width="200">
            <template #default="{ row }">
              <div class="sys-mon__bar-cell">
                <el-progress :percentage="Number(row.diskPercent.toFixed(1))" :stroke-width="6" :status="usageStatus(row.diskPercent)" :show-text="false" />
                <span class="tabular-nums">{{ row.diskPercent.toFixed(1) }}%</span>
              </div>
            </template>
          </el-table-column>
          <el-table-column :label="t('system.monitor.col.uptime')" prop="uptime" sortable width="140">
            <template #default="{ row }">
              <span class="sys-mon__uptime">{{ formatUptime(row.uptime) }}</span>
            </template>
          </el-table-column>
        </el-table>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, reactive, ref } from 'vue';
import { useI18n } from 'vue-i18n';
import IconCircleCheck from '~icons/tabler/circle-check';
import IconAlertTriangle from '~icons/tabler/alert-triangle';
import IconCircleX from '~icons/tabler/circle-x';
import PageHeader from '@/components/PageHeader.vue';
import { api } from '@/api/client';
import type { SystemHost, Paginated } from '@/types/domain';

const { t } = useI18n();

const list = ref<SystemHost[]>([]);
const loading = ref(false);
const filter = reactive({ host: '', ip: '' });

const filteredList = computed(() => {
  let l = list.value;
  if (filter.host) l = l.filter((h) => h.hostname.includes(filter.host));
  if (filter.ip) l = l.filter((h) => h.ip.includes(filter.ip));
  return l;
});

const counters = computed(() => {
  const c = { healthy: 0, warning: 0, error: 0 };
  for (const h of list.value) c[h.status] += 1;
  return c;
});

async function loadList() {
  loading.value = true;
  try {
    const res = await api.get<Paginated<SystemHost>>('/system/hosts?page=1&page_size=100');
    list.value = res.items;
  } finally {
    loading.value = false;
  }
}

function usageStatus(v: number): '' | 'success' | 'warning' | 'exception' {
  if (v >= 85) return 'exception';
  if (v >= 70) return 'warning';
  return 'success';
}

function formatUptime(secs: number) {
  const d = Math.floor(secs / 86400);
  const h = Math.floor((secs % 86400) / 3600);
  return `${d}d ${h}h`;
}

onMounted(loadList);
</script>

<style scoped>
.sys-mon { display: flex; flex-direction: column; }
.sys-mon__body { display: flex; flex-direction: column; gap: 16px; }
.sys-mon__summary {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: 12px;
}
.sys-mon__sum {
  background: var(--color-surface);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  padding: 14px 18px;
  display: flex;
  flex-direction: column;
  gap: 4px;
  position: relative;
}
.sys-mon__sum::before {
  content: ""; position: absolute; inset: 0 auto 0 0; width: 3px;
  border-radius: var(--radius-md) 0 0 var(--radius-md);
}
.sys-mon__sum--healthy::before { background: var(--color-success); }
.sys-mon__sum--warning::before { background: var(--color-warning); }
.sys-mon__sum--error::before   { background: var(--color-danger); }
.sys-mon__sum--total::before   { background: var(--color-primary-500); }
.sys-mon__sum-label { color: var(--color-ink-subtle); font-size: 12px; display: inline-flex; align-items: center; gap: 4px; }
.sys-mon__sum-label :deep(svg) { width: 14px; height: 14px; }
.sys-mon__sum-value { font-size: 26px; font-weight: 600; letter-spacing: -0.02em; line-height: 1.1; }
.sys-mon__panel { padding: 16px 20px 12px; display: flex; flex-direction: column; gap: 14px; }
.sys-mon__filters { display: flex; gap: 12px; flex-wrap: wrap; align-items: center; }
.sys-mon__host { color: var(--color-ink); font-weight: 500; }
.sys-mon__ip { font-family: var(--font-mono); font-size: 12px; color: var(--color-ink-muted); }
.sys-mon__role { display: inline-flex; padding: 2px 8px; border-radius: 4px; font-size: 12px; background: var(--color-surface-2); color: var(--color-ink-muted); }
.sys-mon__role--master  { background: var(--color-primary-50); color: var(--color-primary-700); }
.sys-mon__role--manager { background: var(--color-warning-soft); color: var(--color-warning); }
.sys-mon__status { display: inline-flex; align-items: center; gap: 6px; font-size: 12px; }
.sys-mon__dot { width: 6px; height: 6px; border-radius: 999px; background: currentColor; }
.sys-mon__status--healthy { color: var(--color-success); }
.sys-mon__status--warning { color: var(--color-warning); }
.sys-mon__status--error   { color: var(--color-danger); }
.sys-mon__node { font-size: 12px; color: var(--color-ink-muted); }
.sys-mon__bar-cell { display: flex; align-items: center; gap: 8px; }
.sys-mon__bar-cell :deep(.el-progress) { flex: 1; min-width: 80px; }
.sys-mon__uptime { font-family: var(--font-mono); font-size: 12px; color: var(--color-ink-muted); }
@media (max-width: 1080px) {
  .sys-mon__summary { grid-template-columns: repeat(2, minmax(0, 1fr)); }
}
</style>
