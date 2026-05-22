<template>
  <span class="status-badge" :class="`status-badge--${status}`">
    <span class="status-badge__dot" />
    <span>{{ labelText }}</span>
  </span>
</template>

<script setup lang="ts">
import { computed } from 'vue';
import { useI18n } from 'vue-i18n';
import type { TaskStatus } from '@/types/domain';

const props = withDefaults(defineProps<{
  status: TaskStatus;
  label?: string;
}>(), {});

const { t } = useI18n();
const labelText = computed(() => props.label ?? t(`task.status.${props.status}`));
</script>

<style scoped>
.status-badge {
  display: inline-flex;
  align-items: center;
  justify-content: flex-start;
  gap: 6px;
  padding: 2px 10px;
  min-width: 80px;
  height: 22px;
  border-radius: 999px;
  font-size: 12px;
  font-weight: 500;
  white-space: nowrap;
  border: 1px solid transparent;
  background: var(--color-surface-2);
  color: var(--color-ink-muted);
  box-sizing: border-box;
}
.status-badge__dot {
  width: 6px;
  height: 6px;
  border-radius: 999px;
  background: currentColor;
}
.status-badge--running {
  background: var(--color-success-soft);
  color: var(--color-success);
  border-color: color-mix(in oklab, var(--color-success) 25%, transparent);
}
.status-badge--running .status-badge__dot {
  animation: pulse 1.8s infinite var(--ease-soft);
}
.status-badge--paused {
  background: var(--color-warning-soft);
  color: var(--color-warning);
  border-color: color-mix(in oklab, var(--color-warning) 25%, transparent);
}
.status-badge--failed {
  background: var(--color-danger-soft);
  color: var(--color-danger);
  border-color: color-mix(in oklab, var(--color-danger) 25%, transparent);
}
.status-badge--completed {
  background: var(--color-info-soft);
  color: var(--color-info);
  border-color: color-mix(in oklab, var(--color-info) 25%, transparent);
}
.status-badge--creating,
.status-badge--pending {
  background: var(--color-surface-2);
  color: var(--color-ink-subtle);
  border-color: var(--color-border);
}

@keyframes pulse {
  0%, 100% { opacity: 1; transform: scale(1); }
  50% { opacity: 0.55; transform: scale(0.82); }
}
</style>
