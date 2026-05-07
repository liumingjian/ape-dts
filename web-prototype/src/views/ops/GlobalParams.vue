<template>
  <div class="gparams">
    <PageHeader :title="t('ops.globalParams.title')" :subtitle="t('ops.globalParams.subtitle')">
      <template #actions>
        <el-button @click="loadList">
          <template #icon><IconRefresh /></template>
          {{ t('common.refresh') }}
        </el-button>
      </template>
    </PageHeader>

    <div class="drs-page gparams__body">
      <div v-loading="loading" class="drs-card gparams__panel">
        <div class="gparams__filters">
          <el-select
            v-model="filter.category"
            :placeholder="t('common.all')"
            clearable
            style="width: 200px;"
          >
            <el-option v-for="c in CATS" :key="c" :value="c" :label="t(`ops.globalParams.cat.${c}`)" />
          </el-select>
          <el-input
            v-model="filter.q"
            :placeholder="t('common.search')"
            clearable
            style="width: 240px;"
          >
            <template #prefix><IconSearch /></template>
          </el-input>
        </div>

        <el-table :data="filteredList" stripe class="gparams__table">
          <el-table-column :label="t('ops.globalParams.col.key')" prop="key" sortable min-width="240">
            <template #default="{ row }">
              <div class="gparams__key">
                <span class="gparams__key-text">{{ row.key }}</span>
                <span class="gparams__key-cat" :class="`gparams__key-cat--${row.category}`">
                  {{ t(`ops.globalParams.cat.${row.category}`) }}
                </span>
              </div>
            </template>
          </el-table-column>
          <el-table-column :label="t('ops.globalParams.col.value')" min-width="160">
            <template #default="{ row }">
              <el-input v-if="editingKey === row.key" v-model="editingValue" size="small" />
              <span v-else class="gparams__value">{{ row.value }}</span>
            </template>
          </el-table-column>
          <el-table-column :label="t('ops.globalParams.col.desc')" min-width="280" show-overflow-tooltip>
            <template #default="{ row }">
              <span class="gparams__desc">{{ row.description }}</span>
            </template>
          </el-table-column>
          <el-table-column :label="t('ops.globalParams.col.createdAt')" width="180">
            <template #default>
              <span class="gparams__time">2024-12-01 09:00</span>
            </template>
          </el-table-column>
          <el-table-column :label="t('ops.globalParams.col.updatedAt')" prop="updatedAt" sortable width="180">
            <template #default="{ row }">
              <span class="gparams__time">{{ formatTime(row.updatedAt) }}</span>
            </template>
          </el-table-column>
          <el-table-column :label="t('ops.globalParams.col.actions')" width="160" fixed="right">
            <template #default="{ row }">
              <template v-if="editingKey === row.key">
                <el-button link type="primary" @click="save(row)">{{ t('common.save') }}</el-button>
                <el-button link @click="cancelEdit">{{ t('common.cancel') }}</el-button>
              </template>
              <template v-else>
                <el-button link type="primary" @click="startEdit(row)">{{ t('common.edit') }}</el-button>
              </template>
            </template>
          </el-table-column>
        </el-table>

        <footer class="gparams__footer">
          <span>{{ t('common.total') }}：{{ filteredList.length }}</span>
        </footer>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, reactive, ref } from 'vue';
import { useI18n } from 'vue-i18n';
import { ElMessage } from 'element-plus';
import dayjs from 'dayjs';
import PageHeader from '@/components/PageHeader.vue';
import { api } from '@/api/client';
import type { GlobalParam } from '@/types/domain';

const { t } = useI18n();

const CATS: GlobalParam['category'][] = ['runtime', 'pipeline', 'security', 'alarm'];

const list = ref<GlobalParam[]>([]);
const loading = ref(false);
const filter = reactive({ category: '' as GlobalParam['category'] | '', q: '' });

const filteredList = computed(() => {
  let l = list.value;
  if (filter.category) l = l.filter((p) => p.category === filter.category);
  if (filter.q) l = l.filter((p) => p.key.includes(filter.q) || p.description.includes(filter.q));
  return l;
});

async function loadList() {
  loading.value = true;
  try {
    const res = await api.get<{ items: GlobalParam[] }>('/global-params');
    list.value = res.items;
  } finally {
    loading.value = false;
  }
}

const editingKey = ref<string | null>(null);
const editingValue = ref('');
function startEdit(row: GlobalParam) {
  editingKey.value = row.key;
  editingValue.value = row.value;
}
function cancelEdit() {
  editingKey.value = null;
  editingValue.value = '';
}
async function save(row: GlobalParam) {
  await api.patch(`/global-params/${row.key}`, { value: editingValue.value });
  ElMessage.success(t('ops.globalParams.toast.saved'));
  editingKey.value = null;
  loadList();
}

function formatTime(s: string) { return dayjs(s).format('YYYY-MM-DD HH:mm'); }

onMounted(loadList);
</script>

<style scoped>
.gparams { display: flex; flex-direction: column; }
.gparams__body { display: flex; flex-direction: column; gap: 16px; }
.gparams__panel { padding: 16px 20px 12px; display: flex; flex-direction: column; gap: 14px; }
.gparams__filters { display: flex; gap: 12px; align-items: center; }
.gparams__key { display: flex; align-items: center; gap: 8px; }
.gparams__key-text { font-family: var(--font-mono); font-size: 13px; color: var(--color-ink); font-weight: 500; }
.gparams__key-cat {
  font-size: 11px; padding: 1px 8px; border-radius: 999px;
  background: var(--color-surface-2); color: var(--color-ink-muted);
  border: 1px solid var(--color-border);
}
.gparams__key-cat--runtime  { background: var(--color-primary-50); color: var(--color-primary-700); border-color: var(--color-primary-200); }
.gparams__key-cat--pipeline { background: var(--color-info-soft); color: var(--color-info); }
.gparams__key-cat--security { background: var(--color-danger-soft); color: var(--color-danger); }
.gparams__key-cat--alarm    { background: var(--color-warning-soft); color: var(--color-warning); }
.gparams__value { font-family: var(--font-mono); font-size: 13px; color: var(--color-ink); }
.gparams__desc { color: var(--color-ink-muted); font-size: 13px; }
.gparams__time { font-family: var(--font-mono); font-size: 12px; color: var(--color-ink-muted); }
.gparams__footer { display: flex; justify-content: space-between; align-items: center; padding-top: 8px; color: var(--color-ink-subtle); font-size: 12px; }
</style>
