<template>
  <span class="engine-tag" :style="{ background: meta.bg, color: meta.fg, borderColor: meta.border }">
    <span class="engine-tag__dot" :style="{ background: meta.fg }" />
    <span v-if="!iconOnly" class="engine-tag__label">{{ ENGINE_LABELS[engine] }}</span>
  </span>
</template>

<script setup lang="ts">
import { computed } from 'vue';
import { ENGINE_LABELS, type EngineType } from '@/types/domain';

const ENGINE_COLORS: Record<EngineType, { bg: string; fg: string; border: string }> = {
  mysql:      { bg: '#EFF6FF', fg: '#1D4ED8', border: '#BFDBFE' },
  postgres:   { bg: '#ECFEFF', fg: '#0E7490', border: '#A5F3FC' },
  mongo:      { bg: '#F0FDF4', fg: '#047857', border: '#BBF7D0' },
  redis:      { bg: '#FEF2F2', fg: '#B91C1C', border: '#FCA5A5' },
  kafka:      { bg: '#F5F3FF', fg: '#6D28D9', border: '#DDD6FE' },
  oracle:     { bg: '#FFF7ED', fg: '#C2410C', border: '#FED7AA' },
  gaussdb:    { bg: '#F0F9FF', fg: '#0369A1', border: '#BAE6FD' },
  tidb:       { bg: '#FFFBEB', fg: '#B45309', border: '#FDE68A' },
  starrocks:  { bg: '#FDF4FF', fg: '#A21CAF', border: '#F5D0FE' },
  clickhouse: { bg: '#FEFCE8', fg: '#A16207', border: '#FEF08A' },
  doris:      { bg: '#F1F5F9', fg: '#475569', border: '#CBD5E1' },
  foxlake:    { bg: '#FDF2F8', fg: '#9D174D', border: '#FBCFE8' },
};

const props = withDefaults(defineProps<{ engine: EngineType; iconOnly?: boolean }>(), {});
const meta = computed(() => ENGINE_COLORS[props.engine]);
</script>

<style scoped>
.engine-tag {
  display: inline-flex;
  align-items: center;
  justify-content: flex-start;
  gap: 6px;
  padding: 2px 10px;
  min-width: 104px;
  height: 22px;
  font-size: 12px;
  font-weight: 500;
  border-radius: 999px;
  border: 1px solid transparent;
  line-height: 1.4;
  white-space: nowrap;
  box-sizing: border-box;
}
.engine-tag:where(:not(:has(.engine-tag__label))) {
  min-width: 0;
  padding: 2px;
  width: 22px;
  justify-content: center;
}
.engine-tag__dot {
  width: 6px;
  height: 6px;
  border-radius: 999px;
  flex-shrink: 0;
}
.engine-tag__label {
  letter-spacing: 0;
}
</style>
