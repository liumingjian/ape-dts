<template>
  <div class="activity">
    <header class="activity__head">
      <div>
        <h3 class="activity__title">{{ title }}</h3>
        <p class="activity__sub">{{ subtitle }}</p>
      </div>
      <div class="activity__chip-row">
        <button
          v-for="opt in chips"
          :key="opt.value"
          type="button"
          class="activity__chip"
          :class="{ 'activity__chip--active': filter === opt.value }"
          @click="filter = opt.value"
        >
          {{ opt.label }}
          <span class="activity__chip-count">{{ opt.count }}</span>
        </button>
      </div>
    </header>
    <ol v-if="filtered.length" class="activity__list">
      <li
        v-for="(event, idx) in filtered"
        :key="event.id"
        class="activity__item"
        :class="[
          `activity__item--${event.tone}`,
          { 'activity__item--clickable': !!event.taskId },
        ]"
        @click="event.taskId ? $emit('select', event) : null"
      >
        <div class="activity__rail" aria-hidden="true">
          <span class="activity__dot" />
          <span v-if="idx !== filtered.length - 1" class="activity__line" />
        </div>
        <div class="activity__icon">
          <component :is="iconFor(event.type)" />
        </div>
        <div class="activity__body">
          <div class="activity__line-1">
            <span class="activity__event-title">{{ event.title }}</span>
            <span class="activity__time">{{ formatRelative(event.occurredAt) }}</span>
          </div>
          <div v-if="event.description || event.sourceEngine" class="activity__line-2">
            <template v-if="event.sourceEngine && event.targetEngine">
              <EngineTag :engine="event.sourceEngine" icon-only />
              <IconArrowNarrowRight class="activity__arrow" />
              <EngineTag :engine="event.targetEngine" icon-only />
              <span class="activity__divider">·</span>
            </template>
            <span v-if="event.description" class="activity__desc">{{ event.description }}</span>
          </div>
        </div>
      </li>
    </ol>
    <EmptyState v-else :hint="emptyHint" compact />
  </div>
</template>

<script setup lang="ts">
import { computed, ref } from 'vue';
import { useI18n } from 'vue-i18n';
import EngineTag from './EngineTag.vue';
import EmptyState from './EmptyState.vue';
import IconArrowNarrowRight from '~icons/tabler/arrow-narrow-right';
import IconCheck from '~icons/tabler/check';
import IconAlertOctagon from '~icons/tabler/alert-octagon';
import IconAlertTriangle from '~icons/tabler/alert-triangle';
import IconPlayerPlay from '~icons/tabler/player-play';
import IconPlayerPause from '~icons/tabler/player-pause';
import IconLicense from '~icons/tabler/license';
import IconSettings from '~icons/tabler/settings';
import type { ActivityEvent, ActivityEventCategory, ActivityEventType } from '@/types/domain';

const props = withDefaults(defineProps<{
  title: string;
  subtitle?: string;
  events: ActivityEvent[];
  emptyHint: string;
}>(), {});

defineEmits<{ (e: 'select', event: ActivityEvent): void }>();

const { t } = useI18n();
const filter = ref<'all' | ActivityEventCategory>('all');

const counts = computed(() => {
  const total = props.events.length;
  const byCat: Record<ActivityEventCategory, number> = { task: 0, alert: 0, system: 0 };
  for (const e of props.events) byCat[e.category] += 1;
  return { total, byCat };
});

const chips = computed(() => [
  { label: t('dashboard.filter.all'), value: 'all' as const, count: counts.value.total },
  { label: t('dashboard.activity.cat.task'), value: 'task' as const, count: counts.value.byCat.task },
  { label: t('dashboard.activity.cat.alert'), value: 'alert' as const, count: counts.value.byCat.alert },
  { label: t('dashboard.activity.cat.system'), value: 'system' as const, count: counts.value.byCat.system },
]);

const filtered = computed(() => {
  if (filter.value === 'all') return props.events;
  return props.events.filter((e) => e.category === filter.value);
});

const ICON_MAP: Record<ActivityEventType, unknown> = {
  'task.started': IconPlayerPlay,
  'task.completed': IconCheck,
  'task.failed': IconAlertOctagon,
  'task.paused': IconPlayerPause,
  'task.resumed': IconPlayerPlay,
  'alert.triggered': IconAlertTriangle,
  'alert.cleared': IconCheck,
  'license.expiring': IconLicense,
  'system.deploy': IconSettings,
};

function iconFor(type: ActivityEventType) {
  return ICON_MAP[type] ?? IconSettings;
}

function formatRelative(iso: string): string {
  const diff = Date.now() - new Date(iso).getTime();
  if (diff < 60_000) return '刚刚';
  if (diff < 3_600_000) return `${Math.floor(diff / 60_000)} 分钟前`;
  if (diff < 86_400_000) return `${Math.floor(diff / 3_600_000)} 小时前`;
  return `${Math.floor(diff / 86_400_000)} 天前`;
}
</script>

<style scoped>
.activity {
  display: flex;
  flex-direction: column;
  height: 100%;
}
.activity__head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 14px 20px 8px;
  flex-wrap: wrap;
}
.activity__title {
  margin: 0;
  font-size: var(--text-md);
  font-weight: 600;
  color: var(--color-ink);
}
.activity__sub {
  margin: 2px 0 0;
  font-size: 12px;
  color: var(--color-ink-subtle);
}
.activity__chip-row {
  display: inline-flex;
  background: var(--color-surface-2, #F1F5F9);
  border-radius: var(--radius-sm);
  padding: 2px;
  gap: 2px;
}
.activity__chip {
  border: 0;
  background: transparent;
  padding: 4px 10px;
  font-size: 12px;
  border-radius: 6px;
  color: var(--color-ink-subtle);
  cursor: pointer;
  display: inline-flex;
  align-items: center;
  gap: 6px;
  transition: all var(--dur) var(--ease-soft);
}
.activity__chip:hover { color: var(--color-ink); }
.activity__chip--active {
  background: var(--color-surface);
  color: var(--color-primary-700, #0F766E);
  box-shadow: 0 1px 2px rgba(15, 23, 42, 0.06);
  font-weight: 500;
}
.activity__chip-count {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-width: 18px;
  height: 16px;
  padding: 0 5px;
  border-radius: 999px;
  background: var(--color-surface-2, #F1F5F9);
  color: var(--color-ink-subtle);
  font-size: 10px;
  font-weight: 600;
  line-height: 1;
}
.activity__chip--active .activity__chip-count {
  background: var(--color-primary-50, #ECFDF5);
  color: var(--color-primary-700, #0F766E);
}

.activity__list {
  list-style: none;
  margin: 0;
  padding: 6px 12px 14px;
  flex: 1;
  overflow: auto;
  display: flex;
  flex-direction: column;
}
.activity__item {
  display: grid;
  grid-template-columns: 18px 28px 1fr;
  align-items: stretch;
  gap: 10px;
  padding: 10px 8px 10px 6px;
  border-radius: var(--radius-sm);
  position: relative;
  transition: background var(--dur) var(--ease-soft);
}
.activity__item--clickable { cursor: pointer; }
.activity__item--clickable:hover { background: var(--color-surface-2, #F8FAFC); }

.activity__rail {
  position: relative;
  display: flex;
  flex-direction: column;
  align-items: center;
  width: 18px;
}
.activity__dot {
  width: 10px;
  height: 10px;
  border-radius: 999px;
  background: var(--color-surface);
  border: 2px solid var(--dot-color, #0F766E);
  margin-top: 6px;
  z-index: 1;
}
.activity__line {
  flex: 1;
  width: 2px;
  background: var(--color-border, #E2E8F0);
  margin-top: 2px;
}
.activity__item--success { --dot-color: #10B981; --icon-bg: #ECFDF5; --icon-color: #047857; }
.activity__item--info    { --dot-color: #0EA5E9; --icon-bg: #F0F9FF; --icon-color: #0369A1; }
.activity__item--warning { --dot-color: #F59E0B; --icon-bg: #FFFBEB; --icon-color: #B45309; }
.activity__item--danger  { --dot-color: #EF4444; --icon-bg: #FEF2F2; --icon-color: #B91C1C; }
.activity__item--neutral { --dot-color: #64748B; --icon-bg: #F1F5F9; --icon-color: #475569; }

.activity__icon {
  width: 28px;
  height: 28px;
  border-radius: 8px;
  background: var(--icon-bg, #F1F5F9);
  color: var(--icon-color, #475569);
  display: inline-flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
}
.activity__icon :deep(svg) { width: 16px; height: 16px; }

.activity__body { min-width: 0; display: flex; flex-direction: column; gap: 4px; }
.activity__line-1 {
  display: flex;
  align-items: baseline;
  gap: 12px;
  justify-content: space-between;
}
.activity__event-title {
  font-size: var(--text-sm);
  font-weight: 500;
  color: var(--color-ink);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.activity__time {
  font-size: 12px;
  color: var(--color-ink-subtle);
  white-space: nowrap;
  flex-shrink: 0;
}
.activity__line-2 {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 12px;
  color: var(--color-ink-subtle);
  flex-wrap: wrap;
}
.activity__arrow { width: 12px; height: 12px; color: var(--color-ink-faint); }
.activity__divider { color: var(--color-ink-faint); }
.activity__desc {
  flex: 1;
  min-width: 0;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
</style>
