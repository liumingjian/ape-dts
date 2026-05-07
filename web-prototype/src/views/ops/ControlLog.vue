<template>
  <div class="ctrl-log">
    <PageHeader :title="t('ops.controlLog.title')" :subtitle="t('ops.controlLog.subtitle')" />

    <div class="ape-dts-console-page ctrl-log__body">
      <div v-loading="loading" class="ape-dts-console-card ctrl-log__panel">
        <div class="ctrl-log__filters">
          <div class="ctrl-log__field">
            <span class="ctrl-log__label">{{ t('ops.controlLog.filter.scope') }}：</span>
            <el-select v-model="scope" style="width: 220px;">
              <el-option value="all" :label="t('ops.controlLog.scope.all')" />
              <el-option value="task" :label="t('ops.controlLog.scope.task')" />
              <el-option value="controller" :label="t('ops.controlLog.scope.controller')" />
              <el-option value="engine" :label="t('ops.controlLog.scope.engine')" />
            </el-select>
          </div>
          <el-button type="primary" @click="loadList">{{ t('ops.controlLog.btn.search') }}</el-button>
        </div>

        <el-table v-if="files.length" :data="files" stripe class="ctrl-log__table">
          <el-table-column :label="t('ops.controlLog.col.file')" prop="file" sortable min-width="320">
            <template #default="{ row }">
              <span class="ctrl-log__file">{{ row.file }}</span>
            </template>
          </el-table-column>
          <el-table-column :label="t('ops.controlLog.col.date')" prop="date" sortable width="200">
            <template #default="{ row }">
              <span class="ctrl-log__date">{{ row.date }}</span>
            </template>
          </el-table-column>
          <el-table-column :label="t('ops.controlLog.col.size')" width="140" align="right">
            <template #default="{ row }">
              <span class="tabular-nums">{{ formatSize(row.size) }}</span>
            </template>
          </el-table-column>
          <el-table-column :label="t('ops.controlLog.col.actions')" width="200" fixed="right">
            <template #default="{ row }">
              <el-button link type="primary" @click="onPreview(row)">{{ t('ops.controlLog.action.preview') }}</el-button>
              <el-button link type="primary" @click="onDownload(row)">{{ t('ops.controlLog.action.download') }}</el-button>
            </template>
          </el-table-column>
        </el-table>

        <div v-else class="ctrl-log__empty">
          <IconInbox />
          <p>{{ t('ops.controlLog.empty') }}</p>
        </div>
      </div>
    </div>

    <el-drawer
      v-model="previewVisible"
      :title="previewing?.file"
      size="640px"
      destroy-on-close
    >
      <pre v-if="previewing" class="ctrl-log__preview">{{ previewing.body }}</pre>
    </el-drawer>
  </div>
</template>

<script setup lang="ts">
import { onMounted, ref } from 'vue';
import { useI18n } from 'vue-i18n';
import dayjs from 'dayjs';
import IconInbox from '~icons/tabler/inbox';
import PageHeader from '@/components/PageHeader.vue';
import { api } from '@/api/client';
import type { ControlLog as ControlLogEntry, Paginated } from '@/types/domain';

const { t } = useI18n();

interface LogFile {
  file: string;
  date: string;
  size: number;
  body: string;
}

const scope = ref<'all' | 'task' | 'controller' | 'engine'>('all');
const files = ref<LogFile[]>([]);
const loading = ref(false);

async function loadList() {
  loading.value = true;
  try {
    const data = await api.get<Paginated<ControlLogEntry>>('/control_logs?page=1&page_size=100');
    // Aggregate logs into virtual files (one per day, per scope).
    const buckets = new Map<string, ControlLogEntry[]>();
    for (const l of data.items) {
      const day = dayjs(l.at).format('YYYY-MM-DD');
      const key = `${day}_${scope.value}`;
      if (!buckets.has(key)) buckets.set(key, []);
      buckets.get(key)!.push(l);
    }
    files.value = Array.from(buckets.entries()).map(([key, items]) => {
      const day = key.split('_')[0];
      return {
        file: `ape-dts-${scope.value}-${day}.log`,
        date: day,
        size: items.length * 1240 + Math.floor(Math.random() * 4096),
        body: items.map((l) =>
          `${dayjs(l.at).format('YYYY-MM-DD HH:mm:ss')} [${l.action.toUpperCase()}] task=${l.taskName} operator=${l.operator} result=${l.result} ${l.detail}`
        ).join('\n'),
      };
    }).sort((a, b) => b.date.localeCompare(a.date));
  } finally {
    loading.value = false;
  }
}

function formatSize(b: number) {
  if (b > 1024 * 1024) return `${(b / 1024 / 1024).toFixed(2)} MB`;
  if (b > 1024) return `${(b / 1024).toFixed(1)} KB`;
  return `${b} B`;
}

function onDownload(row: LogFile) {
  const blob = new Blob([row.body], { type: 'text/plain' });
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = row.file;
  a.click();
  URL.revokeObjectURL(url);
}

const previewVisible = ref(false);
const previewing = ref<LogFile | null>(null);
function onPreview(row: LogFile) {
  previewing.value = row;
  previewVisible.value = true;
}

onMounted(loadList);
</script>

<style scoped>
.ctrl-log { display: flex; flex-direction: column; }
.ctrl-log__body { display: flex; flex-direction: column; gap: 16px; }
.ctrl-log__panel { padding: 16px 20px 12px; display: flex; flex-direction: column; gap: 14px; }
.ctrl-log__filters { display: flex; gap: 12px; align-items: center; }
.ctrl-log__field { display: inline-flex; align-items: center; gap: 6px; }
.ctrl-log__label { color: var(--color-ink-muted); font-size: 12px; }
.ctrl-log__file { font-family: var(--font-mono); font-size: 13px; color: var(--color-ink); }
.ctrl-log__date { font-family: var(--font-mono); font-size: 12px; color: var(--color-ink-muted); }
.ctrl-log__empty {
  display: flex;
  flex-direction: column;
  gap: 12px;
  align-items: center;
  padding: 60px 0;
  color: var(--color-ink-subtle);
}
.ctrl-log__empty :deep(svg) { width: 38px; height: 38px; color: var(--color-ink-faint); }
.ctrl-log__preview {
  margin: 0;
  padding: 16px 20px;
  background: var(--color-surface-2);
  font-family: var(--font-mono);
  font-size: 12px;
  line-height: 1.55;
  color: var(--color-ink);
  white-space: pre-wrap;
  height: calc(100% - 24px);
  overflow-y: auto;
}
</style>
