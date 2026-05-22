<template>
  <transition name="fade">
    <el-alert
      v-if="shouldWarn"
      class="lic-banner"
      type="warning"
      :closable="false"
      show-icon
    >
      <template #title>
        <div class="lic-banner__content">
          <span>{{ t('dashboard.licenseWarn', { n: license?.expireAt ? Math.ceil((new Date(license.expireAt).getTime() - Date.now()) / (24 * 60 * 60 * 1000)) : 0 }) }}</span>
          <el-button type="warning" link @click="go">
            {{ t('dashboard.handle') }}
            <IconArrowRight class="lic-banner__arrow" />
          </el-button>
        </div>
      </template>
    </el-alert>
  </transition>
</template>

<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue';
import { useI18n } from 'vue-i18n';
import { useRouter } from 'vue-router';
import IconArrowRight from '~icons/tabler/arrow-right';
import { api } from '@/api/client';
import { useDocumentVisibility } from '@/composables/useDocumentVisibility';

const THIRTY_DAYS_MS = 30 * 24 * 60 * 60 * 1000;

interface LicensePayload {
  sku?: string;
  maxTasks?: number;
  expireAt?: string;
  status?: 'active' | 'expiring_soon' | 'expired' | 'missing';
}

const { t } = useI18n();
const router = useRouter();
const { isVisible } = useDocumentVisibility();

const license = ref<LicensePayload | null>(null);

const shouldWarn = computed(() => {
  if (!license.value) return false;
  const s = license.value.status;
  if (s === 'expired' || s === 'expiring_soon') return true;
  if (!license.value.expireAt) return false;
  const diff = new Date(license.value.expireAt).getTime() - Date.now();
  return diff > 0 && diff <= THIRTY_DAYS_MS;
});

async function load() {
  try {
    license.value = await api.get<LicensePayload>('/license');
  } catch {
    /* noop — banner simply won't render */
  }
}

function go() {
  router.push('/license');
}

onMounted(load);
let pollHandle: ReturnType<typeof setInterval> | null = null;
onMounted(() => {
  pollHandle = setInterval(() => {
    if (isVisible.value) load();
  }, 30_000);
});
onUnmounted(() => {
  if (pollHandle !== null) clearInterval(pollHandle);
});
</script>

<style scoped>
.lic-banner {
  margin: 0 24px;
  border-radius: var(--radius-md);
}
.lic-banner__content {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 12px;
  width: 100%;
}
.lic-banner__arrow { width: 14px; height: 14px; vertical-align: -2px; }
.fade-enter-from, .fade-leave-to { opacity: 0; }
.fade-enter-active, .fade-leave-active { transition: opacity 0.2s var(--ease-soft); }
</style>
