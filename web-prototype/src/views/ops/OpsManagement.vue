<template>
  <div class="ops-mgmt">
    <PageHeader :title="t('ops.management.title')" :subtitle="t('ops.management.subtitle')" />

    <div class="drs-page ops-mgmt__body">
      <div class="drs-card ops-mgmt__panel">
        <el-tabs v-model="active" class="ops-mgmt__tabs" @tab-change="onTabChange">
          <el-tab-pane name="sync" :label="t('ops.management.tab.sync')" />
          <el-tab-pane name="check" :label="t('ops.management.tab.check')" />
          <el-tab-pane name="struct" :label="t('ops.management.tab.struct')" />
        </el-tabs>

        <TaskListView v-if="active === 'sync'" key="sync" view-kind="sync" />
        <TaskListView v-else-if="active === 'check'" key="check" view-kind="check" />
        <TaskListView v-else key="struct" view-kind="struct" />
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref } from 'vue';
import { useI18n } from 'vue-i18n';
import PageHeader from '@/components/PageHeader.vue';
import TaskListView from '@/components/TaskListView.vue';

type OpsTab = 'sync' | 'check' | 'struct';

const { t } = useI18n();
const active = ref<OpsTab>('sync');

function onTabChange(name: string | number) {
  active.value = String(name) as OpsTab;
}
</script>

<style scoped>
.ops-mgmt { display: flex; flex-direction: column; }
.ops-mgmt__body { display: flex; flex-direction: column; gap: 16px; }
.ops-mgmt__panel { padding: 4px 0 0 0; overflow: hidden; }
.ops-mgmt__tabs { padding: 0 20px; }
.ops-mgmt__panel :deep(.task-list .page-header) { padding-top: 0; }
</style>
