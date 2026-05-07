<template>
  <div class="monitor-setting">
    <PageHeader :title="t('monitor.setting.title')" :subtitle="t('monitor.setting.subtitle')" />

    <div class="drs-page monitor-setting__body">
      <div v-loading="loading" class="drs-card monitor-setting__panel">
        <el-form
          v-if="form"
          ref="formRef"
          :model="form"
          label-width="220px"
          class="monitor-setting__form"
        >
          <el-form-item :label="t('monitor.setting.dataRetentionDays')">
            <div class="monitor-setting__row">
              <el-input-number v-model="form.retentionDays" :min="1" :max="365" />
              <span class="monitor-setting__hint">{{ t('monitor.setting.dataRetentionHelp') }}</span>
            </div>
          </el-form-item>
          <el-form-item :label="t('monitor.setting.aggregationWindowMin')">
            <div class="monitor-setting__row">
              <el-input-number v-model="form.aggregationWindowMin" :min="1" :max="60" />
              <span class="monitor-setting__hint">{{ t('monitor.setting.aggregationHelp') }}</span>
            </div>
          </el-form-item>
          <el-form-item :label="t('monitor.setting.defaultChannel')">
            <el-select v-model="form.defaultChannelId" placeholder="—" style="width: 320px;">
              <el-option v-for="c in channels" :key="c.id" :value="c.id" :label="c.name" />
            </el-select>
          </el-form-item>
          <el-form-item :label="t('monitor.setting.defaultTemplate')">
            <el-select v-model="form.defaultTemplateId" placeholder="—" style="width: 320px;">
              <el-option v-for="t2 in templates" :key="t2.id" :value="t2.id" :label="t2.name" />
            </el-select>
          </el-form-item>
          <el-form-item :label="t('monitor.setting.silenceStart')">
            <el-time-select
              v-model="form.silenceStart"
              start="00:00"
              step="00:30"
              end="23:30"
              style="width: 200px;"
            />
          </el-form-item>
          <el-form-item :label="t('monitor.setting.silenceEnd')">
            <el-time-select
              v-model="form.silenceEnd"
              start="00:00"
              step="00:30"
              end="23:30"
              style="width: 200px;"
            />
          </el-form-item>
          <el-form-item label=" ">
            <el-button type="primary" @click="save">{{ t('common.save') }}</el-button>
            <el-button @click="loadAll">{{ t('common.reset') }}</el-button>
          </el-form-item>
        </el-form>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { onMounted, ref } from 'vue';
import { useI18n } from 'vue-i18n';
import { ElMessage } from 'element-plus';
import PageHeader from '@/components/PageHeader.vue';
import { api } from '@/api/client';
import type { AlarmChannel, AlarmTemplate } from '@/types/domain';

const { t } = useI18n();

interface Setting {
  retentionDays: number;
  aggregationWindowMin: number;
  defaultChannelId?: string;
  defaultTemplateId?: string;
  silenceStart: string;
  silenceEnd: string;
}

const form = ref<Setting | null>(null);
const channels = ref<AlarmChannel[]>([]);
const templates = ref<AlarmTemplate[]>([]);
const loading = ref(false);

interface RawSetting {
  globalEnabled: boolean;
  aggregationWindowMin: number;
  defaultChannelId?: string;
  defaultTemplateId?: string;
  silenceStart: string;
  silenceEnd: string;
  retentionDays?: number;
}

async function loadAll() {
  loading.value = true;
  try {
    const [setting, ch, tpl] = await Promise.all([
      api.get<RawSetting>('/alert-monitor/setting'),
      api.get<{ items: AlarmChannel[] }>('/alert-monitor/channels'),
      api.get<{ items: AlarmTemplate[] }>('/alert-monitor/templates'),
    ]);
    form.value = {
      retentionDays: setting.retentionDays ?? 7,
      aggregationWindowMin: setting.aggregationWindowMin,
      defaultChannelId: setting.defaultChannelId,
      defaultTemplateId: setting.defaultTemplateId,
      silenceStart: setting.silenceStart,
      silenceEnd: setting.silenceEnd,
    };
    channels.value = ch.items;
    templates.value = tpl.items;
  } finally {
    loading.value = false;
  }
}

async function save() {
  if (!form.value) return;
  await api.patch('/alert-monitor/setting', form.value);
  ElMessage.success(t('monitor.setting.toast.saved'));
}

onMounted(loadAll);
</script>

<style scoped>
.monitor-setting { display: flex; flex-direction: column; }
.monitor-setting__body { display: flex; flex-direction: column; gap: 16px; }
.monitor-setting__panel { padding: 24px 28px; max-width: 880px; }
.monitor-setting__form :deep(.el-form-item__label) { color: var(--color-ink-muted); }
.monitor-setting__row { display: flex; align-items: center; gap: 12px; flex-wrap: wrap; }
.monitor-setting__hint { color: var(--color-ink-subtle); font-size: 12px; }
</style>
