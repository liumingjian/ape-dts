<template>
  <div class="op-log">
    <PageHeader :title="t('system.operateLog.title')" :subtitle="t('system.operateLog.subtitle')" />

    <div class="ape-dts-console-page op-log__body">
      <div v-loading="loading" class="ape-dts-console-card op-log__panel">
        <div class="op-log__filters">
          <div class="op-log__field">
            <span class="op-log__label">{{ t('system.operateLog.action') }}：</span>
            <el-select v-model="filter.action" clearable class="op-log__filter">
              <el-option v-for="a in ACTIONS" :key="a" :value="a" :label="a" />
            </el-select>
          </div>
          <div class="op-log__field">
            <span class="op-log__label">{{ t('system.operateLog.level') }}：</span>
            <el-select v-model="filter.level" clearable class="op-log__filter">
              <el-option value="info" label="信息" />
              <el-option value="warn" label="警告" />
              <el-option value="error" label="错误" />
            </el-select>
          </div>
          <div class="op-log__field">
            <span class="op-log__label">{{ t('system.operateLog.result._label') }}：</span>
            <el-select v-model="filter.result" clearable class="op-log__filter">
              <el-option value="success" :label="t('system.operateLog.result.success')" />
              <el-option value="failure" :label="t('system.operateLog.result.failure')" />
            </el-select>
          </div>
          <div class="op-log__field">
            <span class="op-log__label">{{ t('system.operateLog.target') }}：</span>
            <el-input v-model="filter.target" clearable class="op-log__filter" />
          </div>
          <div class="op-log__field">
            <span class="op-log__label">{{ t('system.operateLog.user') }}：</span>
            <el-input v-model="filter.user" clearable class="op-log__filter" />
          </div>
          <div class="op-log__field">
            <span class="op-log__label">{{ t('system.operateLog.ip') }}：</span>
            <el-input v-model="filter.ip" clearable class="op-log__filter" />
          </div>
          <div class="op-log__field op-log__field--wide">
            <span class="op-log__label">{{ t('system.operateLog.range') }}：</span>
            <el-date-picker
              v-model="filter.range"
              type="datetimerange"
              value-format="YYYY-MM-DDTHH:mm:ss"
              style="width: 360px;"
            />
          </div>
          <div class="op-log__actions">
            <el-button @click="resetFilter">{{ t('system.operateLog.btn.reset') }}</el-button>
            <el-button type="primary" @click="loadList">{{ t('system.operateLog.btn.search') }}</el-button>
          </div>
        </div>

        <el-table :data="list" stripe class="op-log__table">
          <el-table-column :label="t('system.operateLog.col.action')" prop="action" sortable min-width="160">
            <template #default="{ row }">
              <span class="op-log__action">{{ row.action }}</span>
            </template>
          </el-table-column>
          <el-table-column :label="t('system.operateLog.col.level')" width="120">
            <template #default>
              <span class="op-log__level">信息</span>
            </template>
          </el-table-column>
          <el-table-column :label="t('system.operateLog.col.target')" prop="target" width="140">
            <template #default="{ row }">
              <span class="op-log__target">{{ row.target }}</span>
            </template>
          </el-table-column>
          <el-table-column :label="t('system.operateLog.col.type')" width="120">
            <template #default>
              <span class="op-log__type">控制台</span>
            </template>
          </el-table-column>
          <el-table-column :label="t('system.operateLog.col.result')" prop="result" width="120">
            <template #default="{ row }">
              <span class="op-log__result" :class="`op-log__result--${row.result}`">
                {{ t(`system.operateLog.result.${row.result}`) }}
              </span>
            </template>
          </el-table-column>
          <el-table-column :label="t('system.operateLog.col.user')" prop="user" width="120">
            <template #default="{ row }">
              <span class="op-log__user">{{ row.user }}</span>
            </template>
          </el-table-column>
          <el-table-column :label="t('system.operateLog.col.at')" prop="at" sortable width="180">
            <template #default="{ row }">
              <span class="op-log__time">{{ formatTime(row.at) }}</span>
            </template>
          </el-table-column>
          <el-table-column :label="t('system.operateLog.col.ip')" prop="ip" width="160">
            <template #default="{ row }">
              <span class="op-log__ip">{{ row.ip }}</span>
            </template>
          </el-table-column>
        </el-table>

        <footer class="op-log__footer">
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
  </div>
</template>

<script setup lang="ts">
import { onMounted, reactive, ref } from 'vue';
import { useI18n } from 'vue-i18n';
import dayjs from 'dayjs';
import PageHeader from '@/components/PageHeader.vue';
import { api } from '@/api/client';
import type { OperateLog, Paginated } from '@/types/domain';

const { t } = useI18n();

const ACTIONS = ['登录', '创建任务', '编辑任务', '删除任务', '修改告警规则', '修改全局参数', '导出日志'];

const list = ref<OperateLog[]>([]);
const total = ref(0);
const page = ref(1);
const pageSize = ref(10);
const loading = ref(false);

const filter = reactive({
  action: '', level: '', result: '', target: '', user: '', ip: '',
  range: null as [string, string] | null,
});

async function loadList() {
  loading.value = true;
  try {
    const params = new URLSearchParams({ page: String(page.value), size: String(pageSize.value) });
    if (filter.user) params.set('q', filter.user);
    const data = await api.get<Paginated<OperateLog>>(`/logs/operate?${params.toString()}`);
    let items = data.items;
    if (filter.action) items = items.filter((l) => l.action === filter.action);
    if (filter.result) items = items.filter((l) => l.result === filter.result);
    if (filter.target) items = items.filter((l) => l.target.includes(filter.target));
    if (filter.ip) items = items.filter((l) => l.ip.includes(filter.ip));
    if (filter.range) {
      const [from, to] = filter.range;
      items = items.filter((l) => l.at >= from && l.at <= to);
    }
    list.value = items;
    total.value = items.length === data.items.length ? data.total : items.length;
  } finally {
    loading.value = false;
  }
}

function resetFilter() {
  Object.assign(filter, {
    action: '', level: '', result: '', target: '', user: '', ip: '', range: null,
  });
  loadList();
}

function formatTime(s: string) { return dayjs(s).format('YYYY-MM-DD HH:mm:ss'); }

onMounted(loadList);
</script>

<style scoped>
.op-log { display: flex; flex-direction: column; }
.op-log__body { display: flex; flex-direction: column; gap: 16px; }
.op-log__panel { padding: 16px 20px 12px; display: flex; flex-direction: column; gap: 14px; }
.op-log__filters {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 12px 16px;
  padding: 8px 0;
  align-items: center;
}
.op-log__field { display: flex; align-items: center; gap: 6px; }
.op-log__field--wide { grid-column: span 2; }
.op-log__label { color: var(--color-ink-muted); font-size: 12px; flex-shrink: 0; min-width: 64px; }
.op-log__filter { flex: 1; }
.op-log__actions { display: flex; gap: 8px; justify-self: end; align-self: center; }
.op-log__action { color: var(--color-ink); font-weight: 500; }
.op-log__level { display: inline-flex; padding: 2px 8px; background: var(--color-info-soft); color: var(--color-info); border-radius: 4px; font-size: 12px; }
.op-log__target { color: var(--color-ink-muted); font-size: 12px; }
.op-log__type { color: var(--color-ink-muted); font-size: 12px; }
.op-log__result { display: inline-flex; padding: 2px 8px; border-radius: 4px; font-size: 12px; }
.op-log__result--success { background: var(--color-success-soft); color: var(--color-success); }
.op-log__result--failure { background: var(--color-danger-soft); color: var(--color-danger); }
.op-log__user, .op-log__ip { font-family: var(--font-mono); font-size: 12px; color: var(--color-ink-muted); }
.op-log__time { font-family: var(--font-mono); font-size: 12px; color: var(--color-ink-muted); }
.op-log__footer { display: flex; justify-content: space-between; align-items: center; padding-top: 8px; color: var(--color-ink-subtle); font-size: 12px; }
</style>
