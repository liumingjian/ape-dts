<template>
  <el-config-provider :locale="elLocale">
    <router-view v-slot="{ Component }">
      <transition name="fade" mode="out-in">
        <component :is="Component" />
      </transition>
    </router-view>
  </el-config-provider>
</template>

<script setup lang="ts">
import { computed } from 'vue';
import { ElConfigProvider } from 'element-plus';
import zhCn from 'element-plus/es/locale/lang/zh-cn';
import en from 'element-plus/es/locale/lang/en';
import { i18n } from '@/locales';

const elLocale = computed(() => {
  const code = i18n.global.locale.value;
  return code === 'en-US' ? en : zhCn;
});
</script>
