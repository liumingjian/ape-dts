<template>
  <div class="sidebar">
    <BrandMark :compact="collapsed" />
    <el-scrollbar class="sidebar__scroll">
      <el-menu
        :default-active="activeKey"
        :collapse="collapsed"
        :collapse-transition="false"
        class="sidebar__menu"
        background-color="transparent"
        text-color="var(--color-ink)"
        active-text-color="var(--color-primary-700)"
        router
        @select="emit('navigate')"
      >
        <template v-for="item in visibleMenu" :key="item.key">
          <el-sub-menu v-if="item.children?.length" :index="item.key">
            <template #title>
              <component :is="resolveIcon(item.icon)" class="sidebar__icon" />
              <span class="sidebar__label">{{ t(item.labelKey) }}</span>
            </template>
            <el-menu-item
              v-for="child in item.children"
              :key="child.key"
              :index="child.to"
              :route="child.to"
            >
              <span class="sidebar__label">{{ t(child.labelKey) }}</span>
            </el-menu-item>
          </el-sub-menu>
          <el-menu-item v-else :index="item.to" :route="item.to">
            <component :is="resolveIcon(item.icon)" class="sidebar__icon" />
            <template #title>
              <span class="sidebar__label">{{ t(item.labelKey) }}</span>
            </template>
          </el-menu-item>
        </template>
      </el-menu>
    </el-scrollbar>
    <div class="sidebar__footer">
      <button class="sidebar__collapse-btn" @click="appStore.toggleSidebar">
        <component :is="collapsed ? IconChevronRight : IconChevronLeft" />
      </button>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, h } from 'vue';
import { useRoute } from 'vue-router';
import { useI18n } from 'vue-i18n';
import BrandMark from './BrandMark.vue';
import { menu } from '@/config/menu';
import { useAppStore } from '@/stores/app';
import { useRbac } from '@/composables/useRbac';
import type { NavModule } from '@/auth/permissions';

import IconLayoutDashboard from '~icons/tabler/layout-dashboard';
import IconArrowsExchange from '~icons/tabler/arrows-exchange';
import IconBell from '~icons/tabler/bell';
import IconLicense from '~icons/tabler/license';
import IconServer from '~icons/tabler/server-2';
import IconTools from '~icons/tabler/tools';
import IconActivity from '~icons/tabler/activity-heartbeat';
import IconChevronLeft from '~icons/tabler/chevron-left';
import IconChevronRight from '~icons/tabler/chevron-right';

const { t } = useI18n();
const route = useRoute();
const appStore = useAppStore();
const { visibleNav } = useRbac();
const emit = defineEmits<{ navigate: [] }>();

const collapsed = computed(() => appStore.sidebarCollapsed);
const activeKey = computed(() => route.path);

/** Filter menu items by RBAC — v-if ensures zero DOM presence for hidden items. */
const visibleMenu = computed(() =>
  menu.filter((item) => visibleNav.value.includes(item.key as NavModule)),
);

const iconMap: Record<string, any> = {
  'tabler:layout-dashboard': IconLayoutDashboard,
  'tabler:arrows-exchange': IconArrowsExchange,
  'tabler:bell': IconBell,
  'tabler:license': IconLicense,
  'tabler:server-2': IconServer,
  'tabler:tools': IconTools,
  'tabler:activity-heartbeat': IconActivity,
};

function resolveIcon(name?: string) {
  if (!name) return h('span');
  return iconMap[name] ?? h('span');
}
</script>

<style scoped>
.sidebar {
  display: flex;
  flex-direction: column;
  height: 100%;
}
.sidebar__scroll {
  flex: 1;
  min-height: 0;
}
.sidebar__menu {
  border-right: 0 !important;
  padding: 8px 10px;
}
.sidebar__menu :deep(.el-menu-item),
.sidebar__menu :deep(.el-sub-menu__title) {
  height: 40px;
  line-height: 40px;
  border-radius: var(--radius);
  margin: 2px 0;
  font-size: var(--text-base);
  display: flex;
  align-items: center;
  flex-wrap: nowrap;
  gap: 0;
}
.sidebar__menu :deep(.el-sub-menu__title > .el-sub-menu__icon-arrow) {
  margin-left: auto;
}
.sidebar__menu :deep(.el-menu-item:hover),
.sidebar__menu :deep(.el-sub-menu__title:hover) {
  background: var(--color-primary-50);
  color: var(--color-primary-700);
}
.sidebar__menu :deep(.el-menu-item.is-active) {
  background: var(--color-primary-50);
  color: var(--color-primary-700);
  font-weight: 600;
  position: relative;
}
.sidebar__menu :deep(.el-menu-item.is-active)::before {
  content: '';
  position: absolute;
  left: 0;
  top: 8px;
  bottom: 8px;
  width: 3px;
  background: var(--color-primary-600);
  border-radius: 0 3px 3px 0;
}
.sidebar__menu :deep(.el-menu--inline .el-menu-item) {
  padding-left: 44px !important;
}
.sidebar__icon {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 18px;
  height: 18px;
  margin-right: 10px;
  color: var(--color-ink-subtle);
  flex-shrink: 0;
  vertical-align: middle;
}
.sidebar__icon svg {
  width: 100%;
  height: 100%;
  display: block;
}
.sidebar__label {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.sidebar__footer {
  border-top: 1px solid var(--color-border);
  padding: 8px;
  display: flex;
  justify-content: flex-end;
}
.sidebar__collapse-btn {
  width: 32px;
  height: 32px;
  border: 1px solid var(--color-border);
  background: var(--color-surface);
  border-radius: var(--radius);
  cursor: pointer;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  color: var(--color-ink-subtle);
  transition: all var(--dur) var(--ease-soft);
}
.sidebar__collapse-btn:hover {
  border-color: var(--color-primary-500);
  color: var(--color-primary-700);
}
.sidebar__collapse-btn svg {
  width: 16px;
  height: 16px;
}

@media (max-width: 767px) {
  .sidebar__footer {
    display: none;
  }

  .sidebar__menu {
    padding-bottom: var(--space-4);
  }
}
</style>
