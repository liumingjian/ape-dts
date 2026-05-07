<template>
  <span class="src-tag" :class="`src-tag--${source}`">
    <component :is="iconComp" class="src-tag__icon" />
    <span>{{ t(`alerts.source.${source}`) }}</span>
  </span>
</template>

<script setup lang="ts">
import { computed } from 'vue';
import { useI18n } from 'vue-i18n';
import IconBolt from '~icons/tabler/bolt';
import IconClock from '~icons/tabler/clock';
import IconAlertTriangle from '~icons/tabler/alert-triangle';
import IconPlugConnectedX from '~icons/tabler/plug-connected-x';
import IconDatabase from '~icons/tabler/database';
import IconAdjustments from '~icons/tabler/adjustments';
import type { AlertSource } from '@/types/domain';

const props = defineProps<{ source: AlertSource }>();
const { t } = useI18n();

const ICONS: Record<AlertSource, unknown> = {
  rps: IconBolt,
  latency: IconClock,
  error_rate: IconAlertTriangle,
  connection: IconPlugConnectedX,
  disk: IconDatabase,
  custom: IconAdjustments,
};
const iconComp = computed(() => ICONS[props.source]);
</script>

<style scoped>
.src-tag {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 1px 8px 1px 4px;
  border-radius: 999px;
  font-size: 12px;
  background: var(--color-surface-2);
  color: var(--color-ink-muted);
  border: 1px solid var(--color-border);
}
.src-tag__icon {
  width: 12px;
  height: 12px;
}
.src-tag--rps        { color: var(--color-primary-700); background: var(--color-primary-50); border-color: var(--color-primary-200); }
.src-tag--latency    { color: var(--color-warning); background: var(--color-warning-soft); border-color: color-mix(in oklab, var(--color-warning) 20%, transparent); }
.src-tag--error_rate { color: var(--color-danger); background: var(--color-danger-soft); border-color: color-mix(in oklab, var(--color-danger) 20%, transparent); }
.src-tag--connection { color: #6D28D9; background: #F5F3FF; border-color: #DDD6FE; }
.src-tag--disk       { color: #B45309; background: #FFFBEB; border-color: #FDE68A; }
.src-tag--custom     { color: var(--color-ink-muted); background: var(--color-surface-2); border-color: var(--color-border); }
</style>
