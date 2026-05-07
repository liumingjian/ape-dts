<template>
  <section class="conn-card drs-card">
    <header class="conn-card__head">
      <h3>{{ title }}</h3>
      <span class="conn-card__badge">{{ t('wizard.source.editable') }}</span>
    </header>
    <div class="conn-card__form">
      <div class="conn-card__row">
        <label>{{ t('wizard.source.host') }}</label>
        <el-input :model-value="endpoint.host" readonly />
      </div>
      <div class="conn-card__row">
        <label>{{ t('wizard.source.port') }}</label>
        <el-input :model-value="String(endpoint.port)" readonly />
      </div>
      <div class="conn-card__row">
        <label>{{ t('wizard.source.username') }}</label>
        <el-input :model-value="endpoint.username" readonly />
      </div>
      <div class="conn-card__row">
        <label>{{ t('wizard.source.password') }}</label>
        <el-input :model-value="endpoint.password" type="password" readonly />
      </div>
      <div class="conn-card__actions">
        <el-button
          type="primary"
          :loading="result.status === 'running'"
          :disabled="result.status === 'running'"
          @click="emit('test')"
        >
          {{ result.status === 'running' ? t('wizard.test.running') : t('wizard.test.run') }}
        </el-button>
        <span v-if="result.status === 'ok'" class="conn-card__status conn-card__status--ok">
          <IconCircleCheck />
          {{ t('wizard.test.success', { ms: result.latency }) }}
        </span>
        <span v-else-if="result.status === 'fail'" class="conn-card__status conn-card__status--fail">
          <IconCircleX />
          {{ t('wizard.test.failure', { msg: result.message ?? '' }) }}
        </span>
        <span v-else-if="result.status === 'running'" class="conn-card__status conn-card__status--running">
          <IconLoader2 class="conn-card__spin" />
          {{ t('wizard.test.running') }}
        </span>
      </div>
    </div>
  </section>
</template>

<script setup lang="ts">
import { useI18n } from 'vue-i18n';

defineProps<{
  title: string;
  endpoint: { host: string; port: number; username: string; password: string };
  result: { status: 'idle' | 'running' | 'ok' | 'fail'; latency?: number; message?: string };
}>();
const emit = defineEmits<{ (e: 'test'): void }>();
const { t } = useI18n();
</script>

<style scoped>
.conn-card {
  padding: 0;
}
.conn-card__head {
  padding: 14px 20px;
  display: flex;
  justify-content: space-between;
  align-items: center;
  border-bottom: 1px solid var(--color-border);
}
.conn-card__head h3 {
  margin: 0;
  font-size: 14px;
  font-weight: 600;
}
.conn-card__badge {
  font-size: 11px;
  padding: 2px 8px;
  background: var(--color-success-soft);
  color: var(--color-success);
  border-radius: 999px;
}
.conn-card__form {
  padding: 16px 20px 18px;
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.conn-card__row {
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.conn-card__row label {
  font-size: 12px;
  color: var(--color-ink-subtle);
}
.conn-card__actions {
  display: flex;
  align-items: center;
  gap: 12px;
  padding-top: 4px;
  flex-wrap: wrap;
}
.conn-card__status {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  font-size: 13px;
}
.conn-card__status--ok { color: var(--color-success); }
.conn-card__status--fail { color: var(--color-danger); }
.conn-card__status--running { color: var(--color-info); }
.conn-card__spin { animation: spin 1s linear infinite; }
@keyframes spin {
  from { transform: rotate(0); }
  to { transform: rotate(360deg); }
}
</style>
