<template>
  <div class="empty-state" :class="{ 'empty-state--compact': compact }">
    <div class="empty-state__art" aria-hidden="true">
      <svg viewBox="0 0 64 64" fill="none">
        <circle cx="32" cy="32" r="30" stroke="currentColor" stroke-opacity="0.18" stroke-width="1.4" stroke-dasharray="2 4" />
        <path
          d="M14 38 L24 28 L34 36 L46 22"
          stroke="currentColor"
          stroke-width="1.6"
          stroke-linecap="round"
          stroke-linejoin="round"
          stroke-opacity="0.6"
          fill="none"
        />
        <circle cx="46" cy="22" r="2.4" fill="currentColor" fill-opacity="0.6" />
      </svg>
    </div>
    <p class="empty-state__hint">{{ hint }}</p>
  </div>
</template>

<script setup lang="ts">
withDefaults(defineProps<{ hint: string; compact?: boolean }>(), { compact: false });
</script>

<style scoped>
.empty-state {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 10px;
  padding: 28px 16px;
  color: var(--color-ink-subtle);
  text-align: center;
  min-height: 180px;
}
.empty-state--compact {
  min-height: 140px;
  gap: 8px;
  padding: 18px 12px;
}
.empty-state__art {
  width: 48px;
  height: 48px;
  color: var(--color-primary-700, #0F766E);
  display: inline-flex;
}
.empty-state--compact .empty-state__art { width: 36px; height: 36px; }
.empty-state__art svg { width: 100%; height: 100%; }
.empty-state__hint {
  margin: 0;
  font-size: 13px;
  color: var(--color-ink-subtle);
  line-height: 1.5;
}
</style>
