<template>
  <div class="brand" :class="{ 'brand--compact': compact }">
    <svg class="brand__logo" viewBox="0 0 32 32" aria-hidden="true">
      <defs>
        <linearGradient id="brand-grad" x1="0" y1="0" x2="1" y2="1">
          <stop offset="0%" stop-color="#0F766E" />
          <stop offset="100%" stop-color="#06B6D4" />
        </linearGradient>
      </defs>
      <rect x="2" y="2" width="28" height="28" rx="7" fill="url(#brand-grad)" />
      <path
        d="M9 10 L16 7 L23 10 L23 22 L16 25 L9 22 Z"
        fill="none"
        stroke="#fff"
        stroke-width="1.6"
        stroke-linejoin="round"
      />
      <circle cx="16" cy="16" r="2.8" fill="#fff" />
    </svg>
    <div v-if="!compact" class="brand__text">
      <div class="brand__name">{{ t('brand.name') }}</div>
      <div class="brand__tag">{{ t('brand.tagline') }}</div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { useI18n } from 'vue-i18n';
defineProps<{ compact?: boolean }>();
const { t } = useI18n();
</script>

<style scoped>
.brand {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 0 16px;
  height: var(--layout-header-h);
}
.brand__logo {
  width: 28px;
  height: 28px;
  flex-shrink: 0;
}
.brand__text {
  display: flex;
  flex-direction: column;
  line-height: 1.15;
}
.brand__name {
  font-size: 17px;
  font-weight: 600;
  letter-spacing: 0.02em;
  color: var(--color-ink);
}
.brand__tag {
  font-size: 11px;
  color: var(--color-ink-subtle);
  letter-spacing: 0.02em;
}
.brand--compact {
  justify-content: center;
  padding: 0;
}
</style>
