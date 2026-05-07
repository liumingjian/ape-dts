<template>
  <div class="page-header">
    <div class="page-header__text">
      <h1 class="page-header__title">{{ title }}</h1>
      <p v-if="subtitle" class="page-header__subtitle">{{ subtitle }}</p>
    </div>
    <div v-if="$slots.actions" class="page-header__actions">
      <slot name="actions" />
    </div>
  </div>
</template>

<script setup lang="ts">
defineProps<{ title: string; subtitle?: string }>();
</script>

<style scoped>
.page-header {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 24px;
  padding: 20px 24px 16px;
}
.page-header__title {
  font-size: var(--text-2xl);
  font-weight: 600;
  color: var(--color-ink);
  line-height: 1.3;
  margin: 0;
  letter-spacing: -0.01em;
}
.page-header__subtitle {
  margin: 4px 0 0;
  font-size: var(--text-sm);
  color: var(--color-ink-subtle);
}
.page-header__actions {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-shrink: 0;
}
</style>
