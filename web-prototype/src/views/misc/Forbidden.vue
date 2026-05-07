<template>
  <div class="forbidden">
    <div class="forbidden__code">403</div>
    <div class="forbidden__title">{{ t('forbidden.title') }}</div>
    <div class="forbidden__desc">{{ t('forbidden.desc') }}</div>
    <el-button type="primary" @click="$router.push('/dashboard')">
      {{ t('forbidden.back') }}
    </el-button>
  </div>
</template>

<script setup lang="ts">
import { useI18n } from 'vue-i18n';
const { t } = useI18n();
</script>

<style scoped>
.forbidden {
  height: 100vh;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 12px;
  background: var(--color-canvas);
}
.forbidden__code {
  font-size: 96px;
  font-weight: 700;
  letter-spacing: -0.03em;
  background: linear-gradient(135deg, #EF4444, #F59E0B);
  -webkit-background-clip: text;
  -webkit-text-fill-color: transparent;
  line-height: 1;
}
.forbidden__title {
  font-size: 20px;
  font-weight: 600;
  color: var(--color-ink);
}
.forbidden__desc {
  font-size: 14px;
  color: var(--color-ink-subtle);
  margin-bottom: 12px;
}
</style>
