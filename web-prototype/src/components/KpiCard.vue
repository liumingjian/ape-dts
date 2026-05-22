<template>
  <div
    class="kpi"
    :class="[
      `kpi--${tone}`,
      { 'kpi--accent': accent, 'kpi--clickable': isClickable },
    ]"
    :role="isClickable ? 'link' : undefined"
    :tabindex="isClickable ? 0 : undefined"
    @keydown.enter.prevent="onKeyActivate"
    @keydown.space.prevent="onKeyActivate"
  >
    <div class="kpi__head">
      <div class="kpi__label">
        <component :is="iconComp" v-if="iconComp" class="kpi__icon" />
        <span>{{ label }}</span>
      </div>
      <span v-if="badge" class="kpi__badge">{{ badge }}</span>
    </div>
    <div class="kpi__value tabular-nums">
      <span class="kpi__number">{{ displayValue }}</span>
      <span v-if="unit" class="kpi__unit">{{ unit }}</span>
    </div>
    <div class="kpi__foot">
      <div v-if="delta !== undefined" class="kpi__delta" :class="deltaClass">
        <IconTrendingUp v-if="delta > 0" class="kpi__delta-icon" />
        <IconTrendingDown v-else-if="delta < 0" class="kpi__delta-icon" />
        <IconMinus v-else class="kpi__delta-icon" />
        <span class="tabular-nums">{{ deltaText }}</span>
        <span class="kpi__delta-label">{{ compareLabel }}</span>
      </div>
      <svg
        v-if="spark && spark.length > 1"
        class="kpi__spark"
        :viewBox="`0 0 ${sparkWidth} ${sparkHeight}`"
        preserveAspectRatio="none"
        aria-hidden="true"
      >
        <defs>
          <linearGradient :id="`kpi-grad-${gradId}`" x1="0" y1="0" x2="0" y2="1">
            <stop offset="0%" :stop-color="sparkColor" stop-opacity="0.32" />
            <stop offset="100%" :stop-color="sparkColor" stop-opacity="0" />
          </linearGradient>
        </defs>
        <path :d="sparkAreaPath" :fill="`url(#kpi-grad-${gradId})`" />
        <path
          :d="sparkLinePath"
          fill="none"
          :stroke="sparkColor"
          stroke-width="1.6"
          stroke-linecap="round"
          stroke-linejoin="round"
        />
      </svg>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, getCurrentInstance } from 'vue';
import IconTrendingUp from '~icons/tabler/trending-up';
import IconTrendingDown from '~icons/tabler/trending-down';
import IconMinus from '~icons/tabler/minus';

const props = withDefaults(defineProps<{
  label: string;
  value: number;
  unit?: string;
  delta?: number;
  tone?: 'default' | 'warning' | 'danger' | 'success';
  compareLabel?: string;
  iconComp?: unknown;
  inverse?: boolean;
  spark?: number[];
  badge?: string;
  accent?: boolean;
}>(), {
  tone: 'default',
  compareLabel: '',
  unit: '',
  accent: false,
});

const sparkWidth = 140;
const sparkHeight = 40;
const gradId = computed(() => Math.abs(hashString(props.label)).toString(36));

function hashString(s: string): number {
  let h = 0;
  for (let i = 0; i < s.length; i++) h = ((h << 5) - h + s.charCodeAt(i)) | 0;
  return h;
}

const sparkColor = computed(() => {
  switch (props.tone) {
    case 'danger': return '#EF4444';
    case 'warning': return '#F59E0B';
    case 'success': return '#10B981';
    default: return '#0F766E';
  }
});

const sparkPoints = computed(() => {
  const data = props.spark ?? [];
  if (data.length < 2) return [];
  const min = Math.min(...data);
  const max = Math.max(...data);
  const span = max - min || 1;
  const stepX = sparkWidth / (data.length - 1);
  return data.map((v, i) => {
    const x = i * stepX;
    const y = sparkHeight - ((v - min) / span) * (sparkHeight - 4) - 2;
    return [x, y] as const;
  });
});

const sparkLinePath = computed(() => {
  const pts = sparkPoints.value;
  if (!pts.length) return '';
  return pts.map(([x, y], i) => `${i === 0 ? 'M' : 'L'}${x.toFixed(2)},${y.toFixed(2)}`).join(' ');
});

const sparkAreaPath = computed(() => {
  const pts = sparkPoints.value;
  if (!pts.length) return '';
  const first = pts[0];
  const last = pts[pts.length - 1];
  const head = `M${first[0].toFixed(2)},${sparkHeight}`;
  const line = pts.map(([x, y]) => `L${x.toFixed(2)},${y.toFixed(2)}`).join(' ');
  const tail = `L${last[0].toFixed(2)},${sparkHeight}Z`;
  return `${head} ${line} ${tail}`;
});

const displayValue = computed(() => {
  const v = props.value;
  if (v >= 1_000_000) return `${(v / 1_000_000).toFixed(2)}M`;
  if (v >= 10_000) return `${(v / 1_000).toFixed(1)}K`;
  return new Intl.NumberFormat('zh-CN').format(v);
});

const deltaText = computed(() => {
  const d = props.delta ?? 0;
  if (d === 0) return '0';
  const sign = d > 0 ? '+' : '';
  const abs = Math.abs(d);
  if (abs >= 1_000) return `${sign}${(d / 1_000).toFixed(1)}K`;
  return `${sign}${d}`;
});

const deltaClass = computed(() => {
  const d = props.delta ?? 0;
  if (d === 0) return 'kpi__delta--flat';
  const positive = d > 0;
  const good = props.inverse ? !positive : positive;
  return good ? 'kpi__delta--up' : 'kpi__delta--down';
});

const instance = getCurrentInstance();
const isClickable = computed(() => Boolean(instance?.vnode.props?.onClick));

function onKeyActivate(e: Event) {
  if (!isClickable.value) return;
  const el = e.currentTarget as HTMLElement | null;
  el?.click();
}
</script>

<style scoped>
.kpi {
  background: var(--color-surface);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  padding: 18px 20px 14px;
  box-shadow: var(--shadow-card);
  display: flex;
  flex-direction: column;
  gap: 10px;
  position: relative;
  overflow: hidden;
  transition: box-shadow var(--dur) var(--ease-soft), transform var(--dur) var(--ease-soft), border-color var(--dur) var(--ease-soft);
}
.kpi:hover {
  box-shadow: var(--shadow-elevated);
  transform: translateY(-1px);
  border-color: var(--color-primary-200, #99F6E4);
}
.kpi--clickable {
  cursor: pointer;
}
.kpi--clickable:focus-visible {
  outline: 2px solid var(--color-primary-500, #14B8A6);
  outline-offset: 2px;
}
.kpi--accent {
  background: linear-gradient(135deg, #ECFDF5 0%, #FFFFFF 60%);
  border-color: #99F6E4;
}
.kpi--warning::before,
.kpi--danger::before,
.kpi--success::before {
  content: "";
  position: absolute;
  inset: 0 auto 0 0;
  width: 3px;
  border-radius: var(--radius-md) 0 0 var(--radius-md);
}
.kpi--warning::before { background: var(--color-warning); }
.kpi--danger::before { background: var(--color-danger); }
.kpi--success::before { background: var(--color-success); }
.kpi__head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}
.kpi__label {
  display: flex;
  align-items: center;
  gap: 6px;
  color: var(--color-ink-subtle);
  font-size: var(--text-sm);
}
.kpi__icon {
  width: 16px;
  height: 16px;
  color: var(--color-primary-700);
}
.kpi__badge {
  font-size: 10px;
  font-weight: 600;
  padding: 2px 8px;
  border-radius: 999px;
  background: var(--color-primary-50, #ECFDF5);
  color: var(--color-primary-700, #0F766E);
  letter-spacing: 0.04em;
}
.kpi__value {
  display: flex;
  align-items: baseline;
  gap: 6px;
  color: var(--color-ink);
}
.kpi__number {
  font-size: 30px;
  font-weight: 600;
  letter-spacing: -0.02em;
  line-height: 1.1;
}
.kpi__unit {
  font-size: var(--text-sm);
  color: var(--color-ink-subtle);
}
.kpi__foot {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  margin-top: auto;
}
.kpi__delta {
  display: flex;
  align-items: center;
  gap: 4px;
  font-size: var(--text-sm);
  white-space: nowrap;
}
.kpi__delta--up { color: var(--color-success); }
.kpi__delta--down { color: var(--color-danger); }
.kpi__delta--flat { color: var(--color-ink-subtle); }
.kpi__delta-icon {
  width: 14px;
  height: 14px;
}
.kpi__delta-label {
  color: var(--color-ink-faint);
  margin-left: 4px;
}
.kpi__spark {
  width: 120px;
  height: 36px;
  flex-shrink: 0;
}
</style>
