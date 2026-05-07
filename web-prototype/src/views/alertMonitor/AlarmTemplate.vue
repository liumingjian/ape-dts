<template>
  <div class="alarm-template">
    <PageHeader :title="t('monitor.template.title')" :subtitle="t('monitor.template.subtitle')">
      <template #actions>
        <el-button @click="loadList">
          <template #icon><IconRefresh /></template>
          {{ t('common.refresh') }}
        </el-button>
      </template>
    </PageHeader>

    <div v-loading="loading" class="ape-dts-console-page alarm-template__body">
      <div class="ape-dts-console-card alarm-template__panel">
        <el-form label-width="120px" class="alarm-template__form">
          <el-form-item :label="t('monitor.template.field.kind')">
            <el-radio-group v-model="state.kind" @change="onKindChange">
              <el-radio-button value="kafka">Kafka</el-radio-button>
              <el-radio-button value="snmp">SNMP</el-radio-button>
            </el-radio-group>
          </el-form-item>
          <el-form-item :label="t('monitor.template.field.format')">
            <el-radio-group v-model="state.format" @change="onFormatChange">
              <el-radio-button value="json">{{ t('monitor.template.format.json') }}</el-radio-button>
              <el-radio-button value="split">{{ t('monitor.template.format.split') }}</el-radio-button>
            </el-radio-group>
          </el-form-item>
          <el-form-item :label="t('monitor.template.field.context')">
            <el-input
              v-model="state.context"
              type="textarea"
              :rows="14"
              :placeholder="t('monitor.template.field.contextPh')"
              :disabled="!can('alarm.template.manage')"
              class="alarm-template__textarea"
            />
          </el-form-item>
          <el-form-item v-if="can('alarm.template.manage')" label=" ">
            <el-button type="primary" @click="save">{{ t('common.confirm') }}</el-button>
            <el-button @click="resetToDefault">{{ t('monitor.template.action.init') }}</el-button>
            <el-button @click="previewInterpolation">{{ t('monitor.template.action.preview') }}</el-button>
          </el-form-item>
          <el-form-item v-if="previewText" :label="t('monitor.template.field.preview')">
            <pre class="alarm-template__preview">{{ previewText }}</pre>
          </el-form-item>
        </el-form>
      </div>

      <div class="ape-dts-console-card alarm-template__list">
        <header class="alarm-template__list-head">
          <h3>已保存模板</h3>
          <span class="alarm-template__list-hint">点击切换查看 / 编辑</span>
        </header>
        <ul class="alarm-template__items">
          <li
            v-for="tpl in templates"
            :key="tpl.id"
            class="alarm-template__item"
            :class="{ 'is-active': activeId === tpl.id }"
            @click="selectTemplate(tpl)"
          >
            <div class="alarm-template__item-name">{{ tpl.name }}</div>
            <div class="alarm-template__item-meta">
              <LevelBadge :level="tpl.level" />
              <span class="alarm-template__item-time">{{ formatTime(tpl.updatedAt) }}</span>
            </div>
            <div class="alarm-template__item-subj">{{ tpl.subject }}</div>
          </li>
        </ul>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { onMounted, reactive, ref } from 'vue';
import { useI18n } from 'vue-i18n';
import { ElMessage } from 'element-plus';
import dayjs from 'dayjs';
import PageHeader from '@/components/PageHeader.vue';
import LevelBadge from '@/components/LevelBadge.vue';
import { api } from '@/api/client';
import { useRbac } from '@/composables/useRbac';
import type { AlarmTemplate } from '@/types/domain';

const { t } = useI18n();
const { can } = useRbac();

const DEFAULTS: Record<'kafka' | 'snmp', Record<'json' | 'split', string>> = {
  kafka: {
    json: `{
  "level": "{{level}}",
  "task": "{{taskName}}",
  "source": "{{source}}",
  "message": "{{message}}",
  "firstAt": "{{firstAt}}",
  "lastAt": "{{lastAt}}"
}`,
    split: `level={{level}}|task={{taskName}}|source={{source}}|message={{message}}`,
  },
  snmp: {
    json: `{
  "trapOid": "1.3.6.1.4.1.99999.1.1",
  "level": "{{level}}",
  "task": "{{taskName}}",
  "msg": "{{message}}"
}`,
    split: `OID:1.3.6.1.4.1.99999.1.1|LEVEL:{{level}}|TASK:{{taskName}}|MSG:{{message}}`,
  },
};

const state = reactive({
  kind: 'kafka' as 'kafka' | 'snmp',
  format: 'json' as 'json' | 'split',
  context: DEFAULTS.kafka.json,
});

const templates = ref<AlarmTemplate[]>([]);
const activeId = ref<string | null>(null);
const loading = ref(false);
const previewText = ref('');

async function loadList() {
  loading.value = true;
  try {
    const res = await api.get<{ items: AlarmTemplate[] }>('/alarm_templates');
    templates.value = res.items;
    if (!activeId.value && templates.value.length > 0) selectTemplate(templates.value[0]);
  } finally {
    loading.value = false;
  }
}

function selectTemplate(tpl: AlarmTemplate) {
  activeId.value = tpl.id;
  state.context = tpl.body;
}

function onKindChange() {
  state.context = DEFAULTS[state.kind][state.format];
}
function onFormatChange() {
  state.context = DEFAULTS[state.kind][state.format];
}

function resetToDefault() {
  state.context = DEFAULTS[state.kind][state.format];
  ElMessage.success(t('monitor.template.toast.reset'));
}

async function save() {
  if (activeId.value) {
    await api.patch(`/alarm_templates/${activeId.value}`, { body: state.context });
  } else {
    const created = await api.post<AlarmTemplate>('/alarm_templates', {
      name: `${state.kind}-${state.format} 模板`,
      body: state.context,
      level: 'major',
    });
    activeId.value = created.id;
  }
  ElMessage.success(t('monitor.template.toast.saved'));
  loadList();
}

async function previewInterpolation() {
  try {
    const res = await api.post<{ rendered: string }>('/alarm_templates/preview', {
      body: state.context,
      sample: true,
    });
    previewText.value = res.rendered;
  } catch {
    previewText.value = state.context;
  }
}

function formatTime(s: string) { return dayjs(s).format('YYYY-MM-DD HH:mm'); }

onMounted(loadList);
</script>

<style scoped>
.alarm-template { display: flex; flex-direction: column; }
.alarm-template__body {
  display: grid;
  grid-template-columns: minmax(0, 1fr) 360px;
  gap: 16px;
}
.alarm-template__panel { padding: 24px 28px; }
.alarm-template__form :deep(.el-form-item__label) { color: var(--color-ink-muted); }
.alarm-template__textarea :deep(textarea) {
  font-family: var(--font-mono);
  font-size: 13px;
  line-height: 1.55;
}
.alarm-template__list { padding: 0; display: flex; flex-direction: column; }
.alarm-template__list-head {
  padding: 14px 20px;
  display: flex;
  justify-content: space-between;
  align-items: baseline;
  border-bottom: 1px solid var(--color-border);
}
.alarm-template__list-head h3 { margin: 0; font-size: 14px; font-weight: 600; }
.alarm-template__list-hint { font-size: 12px; color: var(--color-ink-subtle); }
.alarm-template__items { list-style: none; padding: 8px 0; margin: 0; max-height: 540px; overflow-y: auto; }
.alarm-template__item {
  padding: 10px 20px;
  cursor: pointer;
  display: flex;
  flex-direction: column;
  gap: 4px;
  border-left: 3px solid transparent;
  transition: background var(--dur) var(--ease-soft);
}
.alarm-template__item:hover { background: var(--color-surface-2); }
.alarm-template__item.is-active {
  background: var(--color-primary-50);
  border-left-color: var(--color-primary-500);
}
.alarm-template__item-name { font-size: 13px; font-weight: 500; color: var(--color-ink); }
.alarm-template__item-meta { display: flex; align-items: center; gap: 8px; }
.alarm-template__item-time { font-size: 11px; color: var(--color-ink-faint); font-family: var(--font-mono); }
.alarm-template__item-subj { font-size: 11px; color: var(--color-ink-subtle); font-family: var(--font-mono); }
.alarm-template__preview {
  background: var(--color-surface-2);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  padding: 12px 16px;
  font-family: var(--font-mono);
  font-size: 13px;
  line-height: 1.55;
  margin: 0;
  white-space: pre-wrap;
  max-height: 320px;
  overflow-y: auto;
}
@media (max-width: 1100px) {
  .alarm-template__body { grid-template-columns: 1fr; }
}
</style>
