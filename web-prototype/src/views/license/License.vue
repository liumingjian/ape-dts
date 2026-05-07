<template>
  <div class="license">
    <PageHeader :title="t('license.title')" :subtitle="t('license.subtitle')">
      <template #actions>
        <el-button @click="loadList">
          <template #icon><IconRefresh /></template>
          {{ t('common.refresh') }}
        </el-button>
        <el-button type="primary" @click="activateVisible = true">
          <template #icon><IconKey /></template>
          {{ t('license.action.activate') }}
        </el-button>
      </template>
    </PageHeader>

    <div v-loading="loading" class="ape-dts-console-page license__body">
      <div class="license__summary">
        <div class="license__sum license__sum--active">
          <div class="license__sum-label">{{ t('license.summary.active') }}</div>
          <div class="license__sum-value tabular-nums">{{ counters.active }}</div>
        </div>
        <div class="license__sum license__sum--expiring">
          <div class="license__sum-label">{{ t('license.summary.expiring') }}</div>
          <div class="license__sum-value tabular-nums">{{ counters.expiring }}</div>
        </div>
        <div class="license__sum license__sum--expired">
          <div class="license__sum-label">{{ t('license.summary.expired') }}</div>
          <div class="license__sum-value tabular-nums">{{ counters.expired }}</div>
        </div>
        <div class="license__sum license__sum--quota">
          <div class="license__sum-label">{{ t('license.summary.maxTasks') }}</div>
          <div class="license__sum-value tabular-nums">{{ totalQuota }}</div>
        </div>
      </div>

      <div class="ape-dts-console-card license__panel">
        <div class="license__filters">
          <label class="license__esn">
            <span>{{ t('license.esn') }}：</span>
            <el-input v-model="esn" :placeholder="t('license.esnPh')" style="width: 280px;" />
          </label>
          <el-select v-model="filter.status" :placeholder="t('license.filter.status')" clearable class="license__filter">
            <el-option v-for="s in STATUSES" :key="s" :value="s" :label="t(`license.status.${s}`)" />
          </el-select>
          <el-select v-model="filter.sku" :placeholder="t('license.filter.model')" clearable class="license__filter">
            <el-option v-for="s in skus" :key="s" :value="s" :label="s" />
          </el-select>
        </div>

        <el-table :data="filteredList" stripe class="license__table">
          <el-table-column :label="t('license.col.name')" min-width="180">
            <template #default="{ row }">
              <span class="license__name">{{ row.sku }}</span>
              <div class="license__id">{{ row.id }}</div>
            </template>
          </el-table-column>
          <el-table-column :label="t('license.col.status')" width="120">
            <template #default="{ row }">
              <span class="license__status" :class="`license__status--${row.status}`">
                <component :is="statusIcon(row.status)" />
                {{ t(`license.status.${row.status}`) }}
              </span>
            </template>
          </el-table-column>
          <el-table-column :label="t('license.col.curUse')" width="160" align="right">
            <template #default="{ row }">
              <span class="tabular-nums">{{ tasksInUse(row) }}</span>
            </template>
          </el-table-column>
          <el-table-column :label="t('license.col.maxUse')" prop="maxTasks" width="160" align="right">
            <template #default="{ row }">
              <span class="tabular-nums">{{ row.maxTasks }}</span>
            </template>
          </el-table-column>
          <el-table-column :label="t('license.col.unit')" width="100">
            <template #default>
              <span class="license__unit">tasks</span>
            </template>
          </el-table-column>
          <el-table-column :label="t('license.col.file')" min-width="220">
            <template #default="{ row }">
              <span class="license__file">{{ fileNameOf(row) }}</span>
            </template>
          </el-table-column>
          <el-table-column :label="t('license.col.expire')" width="180">
            <template #default="{ row }">
              <span class="license__time">{{ formatExpire(row.expireAt) }}</span>
            </template>
          </el-table-column>
          <el-table-column :label="t('license.col.actions')" width="180" fixed="right">
            <template #default="{ row }">
              <el-button link type="primary" @click="downloadFile(row)">{{ t('license.action.download') }}</el-button>
              <el-button link type="primary" @click="activateVisible = true">{{ t('license.action.activate') }}</el-button>
            </template>
          </el-table-column>
        </el-table>

        <footer class="license__footer">
          <span>{{ t('common.total') }}：{{ filteredList.length }}</span>
        </footer>
      </div>
    </div>

    <el-dialog v-model="activateVisible" :title="t('license.activate.title')" width="520px" append-to-body>
      <el-form label-position="top">
        <el-form-item :label="t('license.activate.codeLabel')">
          <el-input
            v-model="activateCode"
            :placeholder="t('license.activate.codePh')"
            type="textarea"
            :rows="4"
          />
        </el-form-item>
      </el-form>
      <template #footer>
        <el-button @click="activateVisible = false">{{ t('common.cancel') }}</el-button>
        <el-button type="primary" :loading="activating" @click="activate">{{ t('common.confirm') }}</el-button>
      </template>
    </el-dialog>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, reactive, ref } from 'vue';
import { useI18n } from 'vue-i18n';
import { ElMessage } from 'element-plus';
import dayjs from 'dayjs';
import IconKey from '~icons/tabler/key';
import IconCircleCheck from '~icons/tabler/circle-check';
import IconAlertTriangle from '~icons/tabler/alert-triangle';
import IconCircleX from '~icons/tabler/circle-x';
import IconInfinity from '~icons/tabler/infinity';
import PageHeader from '@/components/PageHeader.vue';
import { api } from '@/api/client';
import type { License } from '@/types/domain';

const { t } = useI18n();

const STATUSES: License['status'][] = ['active', 'expiring', 'expired', 'perpetual'];

const list = ref<License[]>([]);
const loading = ref(false);
const esn = ref('');

const filter = reactive({ status: '' as License['status'] | '', sku: '' });

const skus = computed(() => Array.from(new Set(list.value.map((l) => l.sku))));

const filteredList = computed(() => {
  let l = list.value;
  if (filter.status) l = l.filter((x) => x.status === filter.status);
  if (filter.sku) l = l.filter((x) => x.sku === filter.sku);
  return l;
});

const counters = computed(() => {
  const c = { active: 0, expiring: 0, expired: 0, perpetual: 0 };
  for (const l of list.value) c[l.status] += 1;
  return c;
});
const totalQuota = computed(() => list.value.reduce((s, l) => s + l.maxTasks, 0));

async function loadList() {
  loading.value = true;
  try {
    const res = await api.get<{ items: License[] }>('/licenses');
    list.value = res.items;
  } finally {
    loading.value = false;
  }
}

function tasksInUse(row: License) {
  // mock: distribute proportional to position
  const idx = list.value.indexOf(row);
  return Math.min(row.maxTasks, Math.floor(row.maxTasks * (0.2 + idx * 0.15)));
}
function fileNameOf(row: License) {
  return `ape-dts-${row.sku.toLowerCase()}-${row.id}.lic`;
}
function formatExpire(s: string) {
  if (s.startsWith('2099')) return '永久';
  return dayjs(s).format('YYYY-MM-DD');
}
function statusIcon(s: License['status']) {
  if (s === 'active') return IconCircleCheck;
  if (s === 'expiring') return IconAlertTriangle;
  if (s === 'expired') return IconCircleX;
  return IconInfinity;
}
function downloadFile(row: License) {
  const blob = new Blob([
    `# ape-dts Console License File\nSKU=${row.sku}\nLicenseId=${row.id}\nMaxTasks=${row.maxTasks}\nExpireAt=${row.expireAt}\nIssuedTo=${row.issuedTo}\n`,
  ], { type: 'text/plain' });
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = fileNameOf(row);
  a.click();
  URL.revokeObjectURL(url);
}

const activateVisible = ref(false);
const activateCode = ref('');
const activating = ref(false);
async function activate() {
  if (!activateCode.value) return;
  activating.value = true;
  try {
    const res = await api.post<{ ok: boolean; message: string }>('/licenses/activate', { key: activateCode.value });
    if (res.ok) {
      ElMessage.success(`${t('license.activate.toast.success')}：${res.message}`);
      activateVisible.value = false;
      activateCode.value = '';
      loadList();
    } else {
      ElMessage.error(`${t('license.activate.toast.fail')}：${res.message}`);
    }
  } finally {
    activating.value = false;
  }
}

onMounted(loadList);
</script>

<style scoped>
.license { display: flex; flex-direction: column; }
.license__body { display: flex; flex-direction: column; gap: 16px; }
.license__summary {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: 12px;
}
.license__sum {
  background: var(--color-surface);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  padding: 14px 18px;
  display: flex;
  flex-direction: column;
  gap: 4px;
  position: relative;
}
.license__sum::before {
  content: ""; position: absolute; inset: 0 auto 0 0; width: 3px;
  border-radius: var(--radius-md) 0 0 var(--radius-md);
}
.license__sum--active::before { background: var(--color-success); }
.license__sum--expiring::before { background: var(--color-warning); }
.license__sum--expired::before { background: var(--color-danger); }
.license__sum--quota::before { background: var(--color-primary-500); }
.license__sum-label { color: var(--color-ink-subtle); font-size: 12px; }
.license__sum-value { font-size: 28px; font-weight: 600; letter-spacing: -0.02em; line-height: 1.1; }
.license__panel { padding: 16px 20px 12px; display: flex; flex-direction: column; gap: 14px; }
.license__filters { display: flex; gap: 12px; flex-wrap: wrap; align-items: center; }
.license__esn { display: inline-flex; align-items: center; gap: 8px; color: var(--color-ink-muted); font-size: 13px; }
.license__filter { width: 200px; }
.license__name { color: var(--color-ink); font-weight: 500; }
.license__id { font-family: var(--font-mono); font-size: 11px; color: var(--color-ink-faint); }
.license__status { display: inline-flex; align-items: center; gap: 6px; font-size: 12px; }
.license__status :deep(svg) { width: 14px; height: 14px; }
.license__status--active    { color: var(--color-success); }
.license__status--expiring  { color: var(--color-warning); }
.license__status--expired   { color: var(--color-danger); }
.license__status--perpetual { color: var(--color-primary-700); }
.license__unit { font-size: 12px; color: var(--color-ink-subtle); }
.license__file { font-family: var(--font-mono); font-size: 12px; color: var(--color-ink-muted); }
.license__time { font-family: var(--font-mono); font-size: 12px; color: var(--color-ink-muted); }
.license__footer { display: flex; justify-content: space-between; align-items: center; padding-top: 8px; color: var(--color-ink-subtle); font-size: 12px; }
@media (max-width: 1080px) {
  .license__summary { grid-template-columns: repeat(2, minmax(0, 1fr)); }
}
</style>
