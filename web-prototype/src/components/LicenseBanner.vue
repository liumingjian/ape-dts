<template>
  <transition name="fade">
    <el-alert
      v-if="warnCount > 0"
      class="lic-banner"
      type="warning"
      :closable="false"
      show-icon
    >
      <template #title>
        <div class="lic-banner__content">
          <span>{{ t('dashboard.licenseWarn', { n: warnCount }) }}</span>
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
import { onMounted, ref } from 'vue';
import { useI18n } from 'vue-i18n';
import { useRouter } from 'vue-router';
import IconArrowRight from '~icons/tabler/arrow-right';
import { api } from '@/api/client';
import type { License } from '@/types/domain';

const { t } = useI18n();
const router = useRouter();
const warnCount = ref(0);

async function load() {
  try {
    const data = await api.get<{ items: License[] }>('/licenses');
    warnCount.value = data.items.filter((l) => l.status === 'expiring' || l.status === 'expired').length;
  } catch {
    /* noop */
  }
}

function go() {
  router.push('/license');
}

onMounted(load);
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
