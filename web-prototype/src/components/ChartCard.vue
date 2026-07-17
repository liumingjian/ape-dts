<template>
  <section class="chart-card" :class="{ 'chart-card--sm': size === 'sm' }">
    <header class="chart-card__head">
      <div class="chart-card__title-wrap">
        <h3 class="chart-card__title">{{ title }}</h3>
        <p v-if="subtitle" class="chart-card__subtitle">{{ subtitle }}</p>
      </div>
      <div v-if="$slots.extra" class="chart-card__extra">
        <slot name="extra" />
      </div>
    </header>
    <div class="chart-card__body" :style="{ height: bodyHeight }">
      <slot />
    </div>
    <footer v-if="$slots.footer" class="chart-card__footer">
      <slot name="footer" />
    </footer>
  </section>
</template>

<script setup lang="ts">
import { computed } from 'vue';
const props = withDefaults(defineProps<{
  title: string;
  subtitle?: string;
  size?: 'default' | 'sm';
  height?: number;
}>(), { size: 'default' });

const bodyHeight = computed(() => `${props.height ?? (props.size === 'sm' ? 220 : 288)}px`);
</script>

<style scoped>
.chart-card {
  background: var(--color-surface);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  box-shadow: var(--shadow-card);
  display: flex;
  flex-direction: column;
  overflow: hidden;
  transition: box-shadow var(--dur) var(--ease-soft), border-color var(--dur) var(--ease-soft);
}
.chart-card:hover {
  box-shadow: var(--shadow-elevated);
}
.chart-card__head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 12px;
  padding: 16px 20px 8px;
}
.chart-card__title {
  margin: 0;
  font-size: var(--text-md);
  font-weight: 600;
  color: var(--color-ink);
  letter-spacing: -0.01em;
}
.chart-card__subtitle {
  margin: 2px 0 0;
  font-size: 12px;
  color: var(--color-ink-subtle);
}
.chart-card__extra {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
  justify-content: flex-end;
}
.chart-card__body {
  flex: 1;
  min-height: 0;
  padding: 4px 8px 8px;
  display: flex;
  flex-direction: column;
}
.chart-card__body > * {
  flex: 1;
  min-height: 0;
}
.chart-card__footer {
  padding: 8px 20px 14px;
  border-top: 1px solid var(--color-border);
  color: var(--color-ink-subtle);
  font-size: 12px;
}

@media (max-width: 767px) {
  .chart-card__head {
    flex-direction: column;
    gap: var(--space-2);
    padding: var(--space-4) var(--space-4) var(--space-2);
  }

  .chart-card__extra {
    justify-content: flex-start;
  }
}
</style>
