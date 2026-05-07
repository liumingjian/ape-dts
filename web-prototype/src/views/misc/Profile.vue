<template>
  <PagePlaceholder :title="t('topbar.profile')" subtitle="个人中心 · 原型占位" />
</template>
<script setup lang="ts">
import { useI18n } from 'vue-i18n';
import PagePlaceholder from '@/components/PagePlaceholder.vue';
const { t } = useI18n();
</script>
