<template>
  <div class="brand" :class="{ 'brand--compact': compact }">
    <div class="brand__mark" :class="{ 'brand__mark--compact': compact }" aria-hidden="true">
      <span class="brand__mark-node brand__mark-node--one" />
      <span class="brand__mark-node brand__mark-node--two" />
      <span class="brand__mark-node brand__mark-node--three" />
    </div>
    <div v-if="!compact" class="brand__text">
      <div class="brand__name">{{ t('brand.name') }}</div>
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
  flex-direction: row;
  align-items: center;
  justify-content: flex-start;
  gap: 10px;
  padding: 14px 16px;
  min-height: var(--layout-header-h);
}
.brand__mark {
  position: relative;
  width: 34px;
  height: 34px;
  border-radius: var(--radius-md);
  background:
    linear-gradient(135deg, rgba(20, 184, 166, 0.16), rgba(6, 182, 212, 0.08)),
    var(--color-surface);
  border: 1px solid var(--color-primary-200);
  box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.75), var(--shadow-card);
  flex-shrink: 0;
}
.brand__mark::before,
.brand__mark::after {
  content: "";
  position: absolute;
  height: 2px;
  background: var(--color-primary-600);
  transform-origin: left center;
}
.brand__mark::before {
  width: 18px;
  left: 9px;
  top: 12px;
  transform: rotate(28deg);
}
.brand__mark::after {
  width: 17px;
  left: 9px;
  top: 21px;
  transform: rotate(-24deg);
}
.brand__mark-node {
  position: absolute;
  width: 7px;
  height: 7px;
  border-radius: 50%;
  background: var(--color-primary-700);
  box-shadow: 0 0 0 3px var(--color-primary-50);
  z-index: 1;
}
.brand__mark-node--one {
  left: 7px;
  top: 8px;
}
.brand__mark-node--two {
  right: 7px;
  top: 14px;
}
.brand__mark-node--three {
  left: 9px;
  bottom: 7px;
}
.brand__mark--compact {
  width: 32px;
  height: 32px;
}
.brand__text {
  display: flex;
  justify-content: flex-start;
  line-height: 1.15;
  min-width: 0;
}
.brand__name {
  font-size: 15px;
  font-weight: 600;
  letter-spacing: 0;
  color: var(--color-ink);
  white-space: nowrap;
}
.brand--compact {
  padding: 0;
  gap: 0;
  min-height: 0;
}

@media (min-width: 768px) {
  .brand:not(.brand--compact) {
    min-height: calc(var(--layout-header-h) + 28px);
  }
}
</style>
