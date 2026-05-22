<template>
  <div class="brand" :class="{ 'brand--compact': compact }">
    <img
      class="brand__logo"
      :src="atiLogo"
      alt="安图特"
      :class="{ 'brand__logo--compact': compact }"
    />
    <div v-if="!compact" class="brand__text">
      <div class="brand__name">{{ t('brand.name') }}</div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { useI18n } from 'vue-i18n';
import atiLogo from '../assets/ati-logo.png';
defineProps<{ compact?: boolean }>();
const { t } = useI18n();
</script>

<style scoped>
.brand {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 6px;
  padding: 12px 16px;
  min-height: calc(var(--layout-header-h) + 28px);
}
.brand__logo {
  width: 132px;
  height: auto;
  max-height: 44px;
  object-fit: contain;
  flex-shrink: 0;
}
.brand__logo--compact {
  width: 40px;
  max-height: 28px;
}
.brand__text {
  display: flex;
  justify-content: center;
  line-height: 1.15;
}
.brand__name {
  font-size: 15px;
  font-weight: 600;
  letter-spacing: 0.06em;
  color: var(--color-ink);
  white-space: nowrap;
}
.brand--compact {
  padding: 10px 0;
  gap: 0;
  min-height: var(--layout-header-h);
}
</style>
