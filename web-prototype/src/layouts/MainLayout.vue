<template>
  <div
    class="main-layout"
    :class="{ collapsed: appStore.sidebarCollapsed, 'mobile-open': mobileSidebarOpen }"
  >
    <div class="main-layout__mobile-bar">
      <button
        type="button"
        class="main-layout__menu-btn"
        :aria-label="t('topbar.openMenu')"
        @click="mobileSidebarOpen = true"
      >
        <IconMenu />
      </button>
      <BrandMark />
    </div>
    <button
      v-if="mobileSidebarOpen"
      type="button"
      class="main-layout__scrim"
      :aria-label="t('topbar.closeMenu')"
      @click="mobileSidebarOpen = false"
    ></button>
    <aside class="main-layout__sidebar">
      <Sidebar @navigate="mobileSidebarOpen = false" />
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
import { ref } from 'vue';
import { useI18n } from 'vue-i18n';
import Sidebar from '@/components/Sidebar.vue';
import TopBar from '@/components/TopBar.vue';
import LicenseBanner from '@/components/LicenseBanner.vue';
import BrandMark from '@/components/BrandMark.vue';
import { useAppStore } from '@/stores/app';
import IconMenu from '~icons/tabler/menu-2';

const appStore = useAppStore();
const { t } = useI18n();
const mobileSidebarOpen = ref(false);
</script>

<style scoped>
.main-layout {
  display: grid;
  grid-template-columns: var(--layout-sidebar-w) 1fr;
  grid-template-rows: var(--layout-header-h) 1fr;
  grid-template-areas:
    'sidebar header'
    'sidebar content';
  height: 100dvh;
  background: var(--color-canvas);
  transition: grid-template-columns var(--dur) var(--ease-soft);
}

.main-layout__mobile-bar {
  display: none;
}

.main-layout__scrim {
  display: none;
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

.main-layout__menu-btn {
  width: 36px;
  height: 36px;
  border-radius: var(--radius);
  border: 1px solid var(--color-border);
  background: var(--color-surface);
  color: var(--color-ink-muted);
  display: inline-flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
  transition: border-color var(--dur) var(--ease-soft), color var(--dur) var(--ease-soft), background var(--dur) var(--ease-soft);
}

.main-layout__menu-btn:hover,
.main-layout__menu-btn:focus-visible {
  border-color: var(--color-primary-500);
  color: var(--color-primary-700);
  outline: none;
}

.main-layout__menu-btn svg {
  width: 18px;
  height: 18px;
}

@media (max-width: 767px) {
  .main-layout {
    grid-template-columns: 1fr;
    grid-template-rows: var(--layout-mobile-header-h) var(--layout-header-h) minmax(0, 1fr);
    grid-template-areas:
      'mobile'
      'header'
      'content';
    min-width: 0;
  }

  .main-layout.collapsed {
    grid-template-columns: 1fr;
  }

  .main-layout__mobile-bar {
    grid-area: mobile;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    padding: 0 var(--space-4);
    background: var(--color-surface);
    border-bottom: 1px solid var(--color-border);
    z-index: 24;
  }

  .main-layout__sidebar {
    position: fixed;
    inset: 0 auto 0 0;
    width: min(304px, calc(100vw - 48px));
    z-index: 32;
    box-shadow: var(--shadow-drop);
    transform: translateX(-100%);
    transition: transform var(--dur-slow) var(--ease-soft);
  }

  .main-layout.mobile-open .main-layout__sidebar {
    transform: translateX(0);
  }

  .main-layout__scrim {
    display: block;
    position: fixed;
    inset: 0;
    z-index: 28;
    border: 0;
    background: rgba(15, 23, 42, 0.38);
    cursor: pointer;
  }

  .main-layout__header {
    min-width: 0;
  }

  .main-layout__content {
    min-width: 0;
  }
}
</style>
