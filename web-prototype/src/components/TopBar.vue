<template>
  <div class="topbar">
    <div class="topbar__left">
      <el-breadcrumb separator="/" class="topbar__crumb">
        <el-breadcrumb-item
          v-for="(key, idx) in breadcrumb"
          :key="idx"
        >
          {{ t(key) }}
        </el-breadcrumb-item>
      </el-breadcrumb>
    </div>
    <div class="topbar__right">
      <el-badge :value="alertCount" :hidden="alertCount === 0" :offset="[-4, 6]" class="topbar__item">
        <el-tooltip :content="t('topbar.notifications')" placement="bottom">
          <button class="topbar__icon-btn" @click="router.push('/alerts/current')">
            <IconBell />
          </button>
        </el-tooltip>
      </el-badge>
      <el-tooltip :content="t('topbar.help')" placement="bottom">
        <button class="topbar__icon-btn topbar__item">
          <IconHelp />
        </button>
      </el-tooltip>
      <el-dropdown class="topbar__item" trigger="click" @command="onLocaleChange">
        <button class="topbar__lang-btn">
          <IconWorld />
          <span>{{ currentLocaleLabel }}</span>
          <IconChevronDown class="topbar__chev" />
        </button>
        <template #dropdown>
          <el-dropdown-menu>
            <el-dropdown-item v-for="l in SUPPORTED_LOCALES" :key="l.code" :command="l.code">
              {{ l.label }}
            </el-dropdown-item>
          </el-dropdown-menu>
        </template>
      </el-dropdown>
      <el-dropdown class="topbar__item" trigger="click" @command="onUserCommand">
        <div class="topbar__user">
          <div class="topbar__avatar">{{ avatarLetter }}</div>
          <span class="topbar__user-name">{{ auth.user?.displayName ?? 'Guest' }}</span>
          <el-tag :type="roleTagType" size="small" effect="dark" class="topbar__role-tag">{{ roleLabel }}</el-tag>
          <IconChevronDown class="topbar__chev" />
        </div>
        <template #dropdown>
          <el-dropdown-menu>
            <el-dropdown-item command="profile">
              <IconUser class="topbar__menu-icon" />{{ t('topbar.profile') }}
            </el-dropdown-item>
            <el-dropdown-item command="logout" divided>
              <IconLogout class="topbar__menu-icon" />{{ t('auth.logout') }}
            </el-dropdown-item>
          </el-dropdown-menu>
        </template>
      </el-dropdown>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, ref } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { useI18n } from 'vue-i18n';
import { ElNotification } from 'element-plus';
import { useAppStore } from '@/stores/app';
import { useAuthStore } from '@/stores/auth';
import { useAlertStream } from '@/composables/useAlertStream';
import { SUPPORTED_LOCALES, type LocaleCode } from '@/locales';

import IconBell from '~icons/tabler/bell';
import IconHelp from '~icons/tabler/help-circle';
import IconWorld from '~icons/tabler/world';
import IconChevronDown from '~icons/tabler/chevron-down';
import IconUser from '~icons/tabler/user';
import IconLogout from '~icons/tabler/logout';

const { t } = useI18n();
const route = useRoute();
const router = useRouter();
const appStore = useAppStore();
const auth = useAuthStore();
const isMockMode = import.meta.env.VITE_USE_MOCK === 'true';

// Global SSE alert stream — badge counter + toast
const alertCount = ref(0);
const alertStream = auth.isAuthenticated && !isMockMode
  ? useAlertStream({
      url: '/api/alerts/stream',
      onAlert: (e) => {
        alertCount.value += 1;
        ElNotification({
          title: t('alerts.toast.newAlert'),
          message: e.message,
          type: e.level === 'critical' || e.level === 'major' ? 'error' : 'warning',
          duration: 4000,
          onClick: () => router.push('/alerts/current'),
        });
      },
      bufferLimit: 1,
    })
  : null;

onBeforeUnmount(() => alertStream?.close());

const breadcrumb = computed<string[]>(() => {
  const b = (route.meta?.breadcrumb as string[] | undefined) ?? [];
  return b.length ? b : [route.meta?.title as string ?? ''];
});

const currentLocaleLabel = computed(
  () => SUPPORTED_LOCALES.find((l) => l.code === appStore.locale)?.label ?? '中文（简体）',
);

const avatarLetter = computed(() =>
  (auth.user?.displayName ?? auth.user?.username ?? 'G').slice(0, 1).toUpperCase(),
);

const roleLabel = computed(() => {
  const key = `profile.role.${auth.user?.role}`;
  const val = t(key);
  return val !== key ? val : (auth.user?.role ?? '');
});

const roleTagType = computed(() => {
  switch (auth.user?.role) {
    case 'admin': return 'danger';
    case 'operator': return 'warning';
    case 'viewer': return 'info';
    default: return 'info';
  }
});

function onLocaleChange(code: LocaleCode) {
  appStore.changeLocale(code);
}

async function onUserCommand(cmd: string) {
  if (cmd === 'logout') {
    await auth.logout();
    router.push('/login');
  } else if (cmd === 'profile') {
    router.push('/profile');
  }
}
</script>

<style scoped>
.topbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  height: var(--layout-header-h);
  padding: 0 20px;
}
.topbar__left {
  display: flex;
  align-items: center;
  min-width: 0;
}
.topbar__crumb :deep(.el-breadcrumb__item),
.topbar__crumb :deep(.el-breadcrumb__inner) {
  font-size: var(--text-base);
  color: var(--color-ink-muted);
}
.topbar__crumb :deep(.el-breadcrumb__item:last-child .el-breadcrumb__inner) {
  color: var(--color-ink);
  font-weight: 600;
}
.topbar__right {
  display: flex;
  align-items: center;
  gap: 6px;
}
.topbar__item { display: inline-flex; align-items: center; }
.topbar__icon-btn {
  width: 36px;
  height: 36px;
  border-radius: var(--radius);
  border: none;
  background: transparent;
  cursor: pointer;
  color: var(--color-ink-subtle);
  display: inline-flex;
  align-items: center;
  justify-content: center;
  transition: all var(--dur) var(--ease-soft);
}
.topbar__icon-btn:hover {
  background: var(--color-surface-2);
  color: var(--color-primary-700);
}
.topbar__icon-btn svg { width: 18px; height: 18px; }
.topbar__lang-btn {
  height: 36px;
  padding: 0 10px;
  border-radius: var(--radius);
  border: none;
  background: transparent;
  cursor: pointer;
  display: inline-flex;
  align-items: center;
  gap: 6px;
  color: var(--color-ink-muted);
  font-size: var(--text-sm);
  transition: all var(--dur) var(--ease-soft);
}
.topbar__lang-btn:hover { background: var(--color-surface-2); }
.topbar__lang-btn svg { width: 16px; height: 16px; }
.topbar__chev { width: 14px !important; height: 14px !important; opacity: 0.6; }
.topbar__user {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  padding: 6px 10px;
  border-radius: var(--radius);
  cursor: pointer;
  transition: background var(--dur) var(--ease-soft);
}
.topbar__user:hover { background: var(--color-surface-2); }
.topbar__avatar {
  width: 28px;
  height: 28px;
  border-radius: 50%;
  background: linear-gradient(135deg, #0F766E, #06B6D4);
  color: #fff;
  font-weight: 600;
  font-size: 13px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
}
.topbar__user-name {
  font-size: var(--text-sm);
  color: var(--color-ink);
}
.topbar__role-tag {
  margin-left: 4px;
}
.topbar__menu-icon {
  width: 16px;
  height: 16px;
  margin-right: 8px;
  vertical-align: -3px;
}
</style>
