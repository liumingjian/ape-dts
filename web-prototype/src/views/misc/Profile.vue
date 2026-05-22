<template>
  <div class="profile">
    <PageHeader :title="t('topbar.profile')" />

    <div class="profile__content">
      <el-card shadow="never">
        <div class="profile__grid">
          <div class="profile__field">
            <span class="profile__label">{{ t('profile.username') }}</span>
            <span class="profile__value">{{ auth.user?.username ?? '—' }}</span>
          </div>
          <div class="profile__field">
            <span class="profile__label">{{ t('profile.displayName') }}</span>
            <span class="profile__value">{{ auth.user?.displayName ?? '—' }}</span>
          </div>
          <div class="profile__field">
            <span class="profile__label">{{ t('profile.role.label') }}</span>
            <span class="profile__value">
              <el-tag :type="roleTagType" size="small" effect="dark">{{ roleLabel }}</el-tag>
            </span>
          </div>
        </div>
      </el-card>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue';
import { useI18n } from 'vue-i18n';
import { useAuthStore } from '@/stores/auth';
import PageHeader from '@/components/PageHeader.vue';

const { t } = useI18n();
const auth = useAuthStore();

const roleLabel = computed(() => {
  const key = `profile.role.${auth.user?.role}`;
  const val = t(key);
  return val !== key ? val : (auth.user?.role ?? '—');
});

const roleTagType = computed(() => {
  switch (auth.user?.role) {
    case 'admin': return 'danger';
    case 'operator': return 'warning';
    case 'viewer': return 'info';
    default: return 'info';
  }
});
</script>

<style scoped>
.profile {
  padding: 20px;
}
.profile__content {
  max-width: 600px;
}
.profile__grid {
  display: flex;
  flex-direction: column;
  gap: 16px;
}
.profile__field {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 8px 0;
  border-bottom: 1px solid var(--color-border);
}
.profile__field:last-child {
  border-bottom: none;
}
.profile__label {
  font-size: 14px;
  color: var(--color-ink-subtle);
}
.profile__value {
  font-size: 14px;
  font-weight: 500;
  color: var(--color-ink);
}
</style>
