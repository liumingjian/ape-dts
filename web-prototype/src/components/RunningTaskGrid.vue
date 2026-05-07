<template>
  <div class="rt-grid">
    <header class="rt-grid__head">
      <div>
        <h3 class="rt-grid__title">{{ title }}</h3>
        <p class="rt-grid__sub">{{ subtitle }}</p>
      </div>
      <button v-if="moreLabel" type="button" class="rt-grid__more" @click="$emit('more')">
        {{ moreLabel }}
        <IconChevronRight class="rt-grid__more-icon" />
      </button>
    </header>
    <div v-if="tasks.length" class="rt-grid__body">
      <button
        v-for="(item, idx) in tasks"
        :key="item.id"
        type="button"
        class="rt-grid__card"
        @click="$emit('select', item)"
      >
        <div class="rt-grid__card-head">
          <span class="rt-grid__rank">#{{ idx + 1 }}</span>
          <EngineTag :engine="item.sourceEngine" icon-only />
          <IconArrowNarrowRight class="rt-grid__arrow" />
          <EngineTag :engine="item.targetEngine" icon-only />
        </div>
        <div class="rt-grid__name">{{ item.name }}</div>
        <div class="rt-grid__metrics">
          <div class="rt-grid__rps">
            <span class="rt-grid__rps-num tabular-nums">{{ formatShort(item.rps) }}</span>
            <span class="rt-grid__rps-unit">rows/s</span>
          </div>
          <div class="rt-grid__lat tabular-nums">
            <IconClock class="rt-grid__lat-icon" />
            {{ formatShort(item.latencyMs) }} ms
          </div>
        </div>
        <svg
          class="rt-grid__spark"
          viewBox="0 0 100 24"
          preserveAspectRatio="none"
          aria-hidden="true"
        >
          <defs>
            <linearGradient :id="`rt-grad-${item.id}`" x1="0" y1="0" x2="0" y2="1">
              <stop offset="0%" stop-color="#0F766E" stop-opacity="0.32" />
              <stop offset="100%" stop-color="#0F766E" stop-opacity="0" />
            </linearGradient>
          </defs>
          <path :d="areaPath(item.spark)" :fill="`url(#rt-grad-${item.id})`" />
          <path :d="linePath(item.spark)" fill="none" stroke="#0F766E" stroke-width="1.4" stroke-linecap="round" />
        </svg>
      </button>
    </div>
    <EmptyState v-else :hint="emptyHint" compact />
  </div>
</template>

<script setup lang="ts">
import EngineTag from './EngineTag.vue';
import EmptyState from './EmptyState.vue';
import IconArrowNarrowRight from '~icons/tabler/arrow-narrow-right';
import IconClock from '~icons/tabler/clock';
import IconChevronRight from '~icons/tabler/chevron-right';
import type { DashboardTopTask } from '@/types/domain';

withDefaults(defineProps<{
  title: string;
  subtitle?: string;
  tasks: DashboardTopTask[];
  emptyHint: string;
  moreLabel?: string;
}>(), {});

defineEmits<{
  (e: 'select', task: DashboardTopTask): void;
  (e: 'more'): void;
}>();

function points(values: number[]): Array<[number, number]> {
  if (!values || values.length < 2) return [];
  const min = Math.min(...values);
  const max = Math.max(...values);
  const span = max - min || 1;
  const stepX = 100 / (values.length - 1);
  return values.map((v, i) => [i * stepX, 24 - ((v - min) / span) * 22 - 1]);
}

function linePath(values: number[]): string {
  return points(values).map(([x, y], i) => `${i === 0 ? 'M' : 'L'}${x.toFixed(2)},${y.toFixed(2)}`).join(' ');
}

function areaPath(values: number[]): string {
  const pts = points(values);
  if (!pts.length) return '';
  const head = `M${pts[0][0].toFixed(2)},24`;
  const line = pts.map(([x, y]) => `L${x.toFixed(2)},${y.toFixed(2)}`).join(' ');
  const tail = `L${pts[pts.length - 1][0].toFixed(2)},24Z`;
  return `${head} ${line} ${tail}`;
}

function formatShort(v: number | undefined): string {
  if (v == null || Number.isNaN(v)) return '0';
  if (v >= 1_000_000) return `${(v / 1_000_000).toFixed(1)}M`;
  if (v >= 1_000) return `${(v / 1_000).toFixed(1)}K`;
  return `${Math.round(v)}`;
}
</script>

<style scoped>
.rt-grid {
  display: flex;
  flex-direction: column;
  height: 100%;
}
.rt-grid__head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 14px 20px 8px;
}
.rt-grid__title {
  margin: 0;
  font-size: var(--text-md);
  font-weight: 600;
  color: var(--color-ink);
}
.rt-grid__sub {
  margin: 2px 0 0;
  font-size: 12px;
  color: var(--color-ink-subtle);
}
.rt-grid__more {
  border: 0;
  background: transparent;
  display: inline-flex;
  align-items: center;
  gap: 2px;
  font-size: 12px;
  color: var(--color-primary-700, #0F766E);
  cursor: pointer;
}
.rt-grid__more-icon { width: 14px; height: 14px; }

.rt-grid__body {
  flex: 1;
  display: grid;
  grid-template-columns: repeat(2, 1fr);
  gap: 10px;
  padding: 4px 14px 14px;
  overflow: auto;
  align-content: start;
}
.rt-grid__card {
  border: 1px solid var(--color-border);
  background: var(--color-surface);
  border-radius: var(--radius-sm);
  padding: 10px 12px 8px;
  display: flex;
  flex-direction: column;
  gap: 6px;
  cursor: pointer;
  text-align: left;
  transition: all var(--dur) var(--ease-soft);
  min-height: 110px;
}
.rt-grid__card:hover {
  border-color: var(--color-primary-200, #99F6E4);
  box-shadow: 0 4px 16px rgba(15, 118, 110, 0.08);
  transform: translateY(-1px);
}
.rt-grid__card-head {
  display: flex;
  align-items: center;
  gap: 4px;
}
.rt-grid__rank {
  font-size: 10px;
  font-weight: 700;
  color: var(--color-primary-700, #0F766E);
  background: var(--color-primary-50, #ECFDF5);
  padding: 2px 6px;
  border-radius: 999px;
  margin-right: 4px;
}
.rt-grid__arrow {
  width: 12px;
  height: 12px;
  color: var(--color-ink-faint);
}
.rt-grid__name {
  font-size: var(--text-sm);
  font-weight: 500;
  color: var(--color-ink);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.rt-grid__metrics {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: 8px;
}
.rt-grid__rps {
  display: flex;
  align-items: baseline;
  gap: 3px;
}
.rt-grid__rps-num {
  font-size: 18px;
  font-weight: 600;
  color: var(--color-primary-700, #0F766E);
  letter-spacing: -0.02em;
}
.rt-grid__rps-unit {
  font-size: 11px;
  color: var(--color-ink-subtle);
}
.rt-grid__lat {
  display: inline-flex;
  align-items: center;
  gap: 3px;
  font-size: 12px;
  color: var(--color-ink-subtle);
}
.rt-grid__lat-icon {
  width: 12px;
  height: 12px;
}
.rt-grid__spark {
  width: 100%;
  height: 22px;
  margin-top: auto;
}
</style>
