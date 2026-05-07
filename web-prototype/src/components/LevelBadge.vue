<template>
  <span class="level-badge" :class="`level-badge--${level}`">
    <span class="level-badge__dot" />
    <span>{{ labelText }}</span>
  </span>
</template>

<script setup lang="ts">
import { computed } from 'vue';
import type { AlertLevel } from '@/types/domain';

const LABELS: Record<AlertLevel, string> = {
  critical: '紧急',
  major: '重要',
  minor: '次要',
  info: '提示',
};

const props = withDefaults(defineProps<{ level: AlertLevel; label?: string }>(), {});
const labelText = computed(() => props.label ?? LABELS[props.level]);
</script>

<style scoped>
.level-badge {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 2px 10px;
  border-radius: 999px;
  font-size: 12px;
  font-weight: 500;
  white-space: nowrap;
  border: 1px solid transparent;
}
.level-badge__dot {
  width: 6px;
  height: 6px;
  border-radius: 999px;
  background: currentColor;
}
.level-badge--critical {
  background: var(--color-danger-soft);
  color: var(--color-danger);
  border-color: color-mix(in oklab, var(--color-danger) 22%, transparent);
}
.level-badge--major {
  background: var(--color-warning-soft);
  color: var(--color-warning);
  border-color: color-mix(in oklab, var(--color-warning) 22%, transparent);
}
.level-badge--minor {
  background: var(--color-info-soft);
  color: var(--color-info);
  border-color: color-mix(in oklab, var(--color-info) 22%, transparent);
}
.level-badge--info {
  background: var(--color-surface-2);
  color: var(--color-ink-subtle);
  border-color: var(--color-border);
}
</style>
