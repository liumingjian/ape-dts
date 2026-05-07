<template>
  <div class="login">
    <div class="login__hero">
      <div class="login__hero-top">
        <BrandMark />
      </div>
      <div class="login__hero-center">
        <h1 class="login__hero-title">{{ t('auth.heroTitle') }}</h1>
        <p class="login__hero-subtitle">{{ t('auth.heroSubtitle') }}</p>
        <div class="login__hero-badges">
          <span class="login__badge">MySQL · PostgreSQL · MongoDB · Redis</span>
          <span class="login__badge">Kafka · StarRocks · ClickHouse · Doris</span>
          <span class="login__badge">Oracle · GaussDB · TiDB · Foxlake</span>
        </div>
      </div>
      <div class="login__hero-bottom">{{ t('auth.copyright') }}</div>
      <div class="login__hero-mesh" aria-hidden="true" />
    </div>
    <div class="login__panel">
      <div class="login__panel-top">
        <el-dropdown trigger="click" @command="onLocaleChange">
          <button class="login__lang">
            <IconWorld />
            <span>{{ currentLocaleLabel }}</span>
          </button>
          <template #dropdown>
            <el-dropdown-menu>
              <el-dropdown-item v-for="l in SUPPORTED_LOCALES" :key="l.code" :command="l.code">
                {{ l.label }}
              </el-dropdown-item>
            </el-dropdown-menu>
          </template>
        </el-dropdown>
      </div>
      <div class="login__card">
        <h2 class="login__title">{{ t('auth.loginTitle') }}</h2>
        <el-form :model="form" label-position="top" size="large" @submit.prevent="onSubmit">
          <el-form-item :label="t('auth.username')">
            <el-input v-model="form.username" placeholder="admin" autocomplete="username">
              <template #prefix><IconUser /></template>
            </el-input>
          </el-form-item>
          <el-form-item :label="t('auth.password')">
            <el-input
              v-model="form.password"
              type="password"
              show-password
              placeholder="admin123"
              autocomplete="current-password"
            >
              <template #prefix><IconLock /></template>
            </el-input>
          </el-form-item>
          <el-button
            type="primary"
            size="large"
            native-type="submit"
            :loading="loading"
            class="login__submit"
          >
            {{ t('auth.login') }}
          </el-button>
        </el-form>
        <div class="login__hint">{{ t('auth.hint') }}</div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { reactive, ref, computed } from 'vue';
import { useRouter, useRoute } from 'vue-router';
import { useI18n } from 'vue-i18n';
import { ElMessage } from 'element-plus';
import { useAuthStore } from '@/stores/auth';
import { useAppStore } from '@/stores/app';
import { SUPPORTED_LOCALES, type LocaleCode } from '@/locales';
import BrandMark from '@/components/BrandMark.vue';

import IconUser from '~icons/tabler/user';
import IconLock from '~icons/tabler/lock';
import IconWorld from '~icons/tabler/world';

const { t } = useI18n();
const router = useRouter();
const route = useRoute();
const auth = useAuthStore();
const appStore = useAppStore();

const form = reactive({ username: 'admin', password: 'admin123' });
const loading = ref(false);

const currentLocaleLabel = computed(
  () => SUPPORTED_LOCALES.find((l) => l.code === appStore.locale)?.label ?? '中文（简体）',
);

function onLocaleChange(code: LocaleCode) {
  appStore.changeLocale(code);
}

async function onSubmit() {
  if (!form.username || !form.password) {
    ElMessage.warning('请输入账号和密码');
    return;
  }
  loading.value = true;
  await new Promise((r) => setTimeout(r, 700));
  auth.login(form.username, form.password);
  loading.value = false;
  ElMessage.success('登录成功');
  const redirect = (route.query.redirect as string) || '/dashboard';
  router.push(redirect);
}
</script>

<style scoped>
.login {
  display: grid;
  grid-template-columns: 1.2fr 1fr;
  min-height: 100vh;
  background: var(--color-canvas);
}

@media (max-width: 960px) {
  .login { grid-template-columns: 1fr; }
  .login__hero { display: none; }
}

.login__hero {
  position: relative;
  background:
    radial-gradient(circle at 80% 20%, rgba(20, 184, 166, 0.18), transparent 50%),
    radial-gradient(circle at 20% 80%, rgba(6, 182, 212, 0.18), transparent 50%),
    linear-gradient(135deg, #0B1120 0%, #0F172A 50%, #134E4A 100%);
  color: #fff;
  padding: 32px 48px;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.login__hero-top :deep(.brand__name),
.login__hero-top :deep(.brand__tag) {
  color: #fff;
}
.login__hero-top :deep(.brand__tag) { opacity: 0.7; }

.login__hero-center {
  flex: 1;
  display: flex;
  flex-direction: column;
  justify-content: center;
  gap: 16px;
  max-width: 640px;
  position: relative;
  z-index: 1;
}

.login__hero-title {
  font-size: 44px;
  line-height: 1.2;
  font-weight: 700;
  letter-spacing: -0.02em;
  margin: 0;
  background: linear-gradient(90deg, #fff 0%, #99F6E4 100%);
  -webkit-background-clip: text;
  -webkit-text-fill-color: transparent;
}

.login__hero-subtitle {
  font-size: 17px;
  line-height: 1.6;
  opacity: 0.78;
  margin: 0;
  max-width: 540px;
}

.login__hero-badges {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  margin-top: 12px;
}
.login__badge {
  padding: 6px 12px;
  border-radius: 999px;
  border: 1px solid rgba(255, 255, 255, 0.14);
  background: rgba(255, 255, 255, 0.04);
  font-size: 12px;
  letter-spacing: 0.02em;
  color: rgba(255, 255, 255, 0.82);
  backdrop-filter: blur(8px);
}

.login__hero-bottom {
  font-size: 12px;
  opacity: 0.45;
  position: relative;
  z-index: 1;
}

.login__hero-mesh {
  position: absolute;
  inset: 0;
  background-image:
    linear-gradient(rgba(255, 255, 255, 0.04) 1px, transparent 1px),
    linear-gradient(90deg, rgba(255, 255, 255, 0.04) 1px, transparent 1px);
  background-size: 48px 48px;
  mask-image: radial-gradient(circle at 60% 50%, #000 0%, transparent 80%);
  pointer-events: none;
}

.login__panel {
  display: flex;
  flex-direction: column;
  padding: 20px 48px;
}

.login__panel-top {
  display: flex;
  justify-content: flex-end;
}

.login__lang {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 6px 12px;
  border-radius: var(--radius);
  background: transparent;
  border: 1px solid transparent;
  color: var(--color-ink-muted);
  font-size: 13px;
  cursor: pointer;
  transition: all var(--dur) var(--ease-soft);
}
.login__lang:hover {
  border-color: var(--color-border);
  background: var(--color-surface);
}
.login__lang svg { width: 16px; height: 16px; }

.login__card {
  margin: auto;
  max-width: 420px;
  width: 100%;
  padding: 40px 36px;
  background: var(--color-surface);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-card);
}

.login__title {
  margin: 0 0 28px;
  font-size: 22px;
  font-weight: 600;
  color: var(--color-ink);
  text-align: center;
  letter-spacing: -0.01em;
}

.login__submit {
  width: 100%;
  height: 44px;
  font-size: 15px;
  letter-spacing: 0.04em;
  margin-top: 8px;
}

.login__hint {
  margin-top: 20px;
  padding: 10px 12px;
  border-radius: var(--radius);
  background: var(--color-primary-50);
  color: var(--color-primary-800);
  font-size: 12px;
  text-align: center;
  line-height: 1.5;
}
</style>
