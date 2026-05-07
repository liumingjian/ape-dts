import { defineStore } from 'pinia';
import { ref } from 'vue';
import { setLocale, type LocaleCode } from '@/locales';

export const useAppStore = defineStore(
  'app',
  () => {
    const sidebarCollapsed = ref(false);
    const locale = ref<LocaleCode>('zh-CN');
    const resourceGroup = ref<string>('default');
    const timeRange = ref<'1h' | '24h' | '7d' | '30d'>('24h');

    function toggleSidebar() {
      sidebarCollapsed.value = !sidebarCollapsed.value;
    }

    function changeLocale(l: LocaleCode) {
      locale.value = l;
      setLocale(l);
    }

    return {
      sidebarCollapsed,
      locale,
      resourceGroup,
      timeRange,
      toggleSidebar,
      changeLocale,
    };
  },
  {
    persist: {
      key: 'drs.app',
      pick: ['sidebarCollapsed', 'locale', 'resourceGroup', 'timeRange'],
    },
  },
);
