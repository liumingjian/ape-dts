<template>
  <div class="ape-dts-console-page">
    <PageHeader :title="title" :subtitle="subtitle" />
    <div class="ape-dts-console-card placeholder">
      <div class="placeholder__icon">
        <IconTool />
      </div>
      <div class="placeholder__title">{{ t('page.wip') }}</div>
      <div class="placeholder__desc">{{ t('page.wipDesc') }}</div>
      <div v-if="tier" class="placeholder__tier" :class="`tier-${tier.toLowerCase()}`">
        {{ tier }} · 分档实现中
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { useI18n } from 'vue-i18n';
import PageHeader from './PageHeader.vue';
import IconTool from '~icons/tabler/tool';

defineProps<{
  title: string;
  subtitle?: string;
  tier?: 'P0' | 'P1' | 'P2';
}>();
const { t } = useI18n();
</script>

<style scoped>
.placeholder {
  margin: 0 24px 24px;
  padding: 72px 24px;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 10px;
}
.placeholder__icon {
  width: 64px;
  height: 64px;
  border-radius: 50%;
  background: var(--color-primary-50);
  color: var(--color-primary-700);
  display: inline-flex;
  align-items: center;
  justify-content: center;
  margin-bottom: 8px;
}
.placeholder__icon svg { width: 28px; height: 28px; }
.placeholder__title {
  font-size: var(--text-lg);
  font-weight: 600;
  color: var(--color-ink);
}
.placeholder__desc {
  font-size: var(--text-sm);
  color: var(--color-ink-subtle);
  max-width: 520px;
  text-align: center;
}
.placeholder__tier {
  margin-top: 16px;
  padding: 4px 10px;
  border-radius: 999px;
  font-size: var(--text-xs);
  font-weight: 500;
  letter-spacing: 0.03em;
}
.tier-p0 { background: var(--color-primary-50); color: var(--color-primary-700); }
.tier-p1 { background: var(--color-info-soft); color: var(--color-info); }
.tier-p2 { background: var(--color-surface-2); color: var(--color-ink-muted); }
</style>
