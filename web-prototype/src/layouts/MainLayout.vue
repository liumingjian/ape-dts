<template>
  <div class="main-layout" :class="{ collapsed: appStore.sidebarCollapsed }">
    <aside class="main-layout__sidebar">
      <Sidebar />
    </aside>
    <header class="main-layout__header">
      <TopBar />
    </header>
    <main class="main-layout__content">
      <LicenseBanner class="main-layout__banner" />
      <router-view v-slot="{ Component }">
        <transition name="fade" mode="out-in">
          <component :is="Component" />
        </transition>
      </router-view>
    </main>
  </div>
</template>

<script setup lang="ts">
import Sidebar from '@/components/Sidebar.vue';
import TopBar from '@/components/TopBar.vue';
import LicenseBanner from '@/components/LicenseBanner.vue';
import { useAppStore } from '@/stores/app';

const appStore = useAppStore();
</script>

<style scoped>
.main-layout {
  display: grid;
  grid-template-columns: var(--layout-sidebar-w) 1fr;
  grid-template-rows: var(--layout-header-h) 1fr;
  grid-template-areas:
    'sidebar header'
    'sidebar content';
  height: 100vh;
  background: var(--color-canvas);
  transition: grid-template-columns var(--dur) var(--ease-soft);
}

.main-layout.collapsed {
  grid-template-columns: var(--layout-sidebar-w-collapsed) 1fr;
}

.main-layout__sidebar {
  grid-area: sidebar;
  background: var(--color-surface);
  border-right: 1px solid var(--color-border);
  overflow: hidden;
  display: flex;
  flex-direction: column;
}

.main-layout__header {
  grid-area: header;
  background: var(--color-surface);
  border-bottom: 1px solid var(--color-border);
  z-index: 10;
}

.main-layout__content {
  grid-area: content;
  overflow-y: auto;
  overflow-x: hidden;
  display: flex;
  flex-direction: column;
}
.main-layout__banner {
  margin-top: 16px;
}
</style>
