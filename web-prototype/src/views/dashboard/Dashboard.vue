<template>
  <div class="dashboard">
    <PageHeader :title="t('dashboard.title')" :subtitle="t('dashboard.subtitle')">
      <template #actions>
        <el-segmented
          v-model="timeRange"
          :options="timeRangeOptions"
          size="default"
        />
        <el-button :loading="loading" @click="load">
          <template #icon><IconRefresh /></template>
          {{ t('common.refresh') }}
        </el-button>
      </template>
    </PageHeader>

    <div class="ape-dts-console-page dashboard__body">
      <!-- License warning banner (uses /api/license) -->
      <LicenseBanner />

      <!-- Hero / KPI row with embedded sparklines -->
      <section class="dashboard__hero">
        <KpiCard
          accent
          :badge="t('dashboard.badge.live')"
          :label="t('dashboard.kpi.running')"
          :value="summary?.kpi.running.total ?? 0"
          :delta="summary?.kpi.running.delta ?? 0"
          :icon-comp="IconActivity"
          :spark="summary?.kpiSparks?.running"
          tone="success"
          class="dashboard__kpi-item"
          @click="go('/tasks/snapshot?status=running')"
        />
        <KpiCard
          :label="t('dashboard.kpi.todayAlerts')"
          :value="summary?.kpi.todayAlerts.total ?? 0"
          :delta="summary?.kpi.todayAlerts.delta ?? 0"
          :icon-comp="IconAlertTriangle"
          inverse
          :spark="summary?.kpiSparks?.todayAlerts"
          :tone="(summary?.kpi.todayAlerts.total ?? 0) > 20 ? 'warning' : 'default'"
          class="dashboard__kpi-item"
          @click="go('/alerts/current')"
        />
        <KpiCard
          :label="t('dashboard.kpi.throughput')"
          :value="summary?.kpi.totalRps.value ?? 0"
          unit="rows/s"
          :delta="summary?.kpi.totalRps.delta ?? 0"
          :icon-comp="IconBolt"
          :spark="summary?.kpiSparks?.totalRps"
          class="dashboard__kpi-item"
        />
        <KpiCard
          :label="t('dashboard.kpi.latency')"
          :value="summary?.kpi.avgLatencyMs.value ?? 0"
          unit="ms"
          :delta="summary?.kpi.avgLatencyMs.delta ?? 0"
          :icon-comp="IconClock"
          inverse
          :spark="summary?.kpiSparks?.avgLatencyMs"
          class="dashboard__kpi-item"
        />
      </section>

      <!-- Time series · combined RPS + latency, full width -->
      <section class="dashboard__section">
        <div class="dashboard__section-head">
          <h2>{{ t('dashboard.section.timeseries') }}</h2>
          <span class="dashboard__section-hint">{{ timeRangeHint }}</span>
        </div>
        <ChartCard
          :title="t('dashboard.chart.combined')"
          :subtitle="t('dashboard.chart.combinedSub')"
          :height="320"
        >
          <template #extra>
            <span class="dashboard__legend">
              <span class="dashboard__legend-dot" style="background:#0F766E" />{{ t('dashboard.kpi.throughput') }} (rows/s)
            </span>
            <span class="dashboard__legend">
              <span class="dashboard__legend-dot" style="background:#F59E0B" />{{ t('dashboard.kpi.latency') }} (ms)
            </span>
          </template>
          <v-chart
            v-if="hasTimeseries"
            :option="combinedOption"
            autoresize
            class="dashboard__chart"
          />
          <EmptyState v-else :hint="t('dashboard.empty.timeseries')" />
        </ChartCard>
      </section>

      <!-- Distribution row -->
      <section class="dashboard__section">
        <div class="dashboard__section-head">
          <h2>{{ t('dashboard.section.distribution') }}</h2>
          <span class="dashboard__section-hint">{{ t('dashboard.section.distributionHint') }}</span>
        </div>
        <div class="dashboard__grid dashboard__grid--3">
          <ChartCard
            :title="t('dashboard.chart.taskStatus')"
            :subtitle="`${t('dashboard.chart.taskStatusSub')} · ${totalTasks}`"
            :height="260"
            size="sm"
          >
            <v-chart
              v-if="summary?.statusDist?.length"
              :option="statusPieOption"
              autoresize
              class="dashboard__chart"
              @click="onStatusClick"
            />
            <EmptyState v-else :hint="t('dashboard.empty.statusDist')" compact />
          </ChartCard>
          <ChartCard
            :title="t('dashboard.chart.engineDist')"
            :subtitle="t('dashboard.chart.engineDistSub')"
            :height="260"
            size="sm"
          >
            <v-chart
              v-if="summary?.engineDist?.length"
              :option="engineBarOption"
              autoresize
              class="dashboard__chart"
            />
            <EmptyState v-else :hint="t('dashboard.empty.engineDist')" compact />
          </ChartCard>
          <ChartCard
            :title="t('dashboard.chart.alertTrend')"
            :subtitle="t('dashboard.chart.alertTrendSub')"
            :height="260"
            size="sm"
          >
            <v-chart
              v-if="summary?.alertTrend?.length"
              :option="alertTrendOption"
              autoresize
              class="dashboard__chart"
            />
            <EmptyState v-else :hint="t('dashboard.empty.alertTrend')" compact />
          </ChartCard>
        </div>
      </section>

      <!-- Recent activity timeline + running task grid -->
      <section class="dashboard__section">
        <div class="dashboard__section-head">
          <h2>{{ t('dashboard.section.recent') }}</h2>
          <span class="dashboard__section-hint">{{ t('dashboard.section.recentHint') }}</span>
        </div>
        <div class="dashboard__grid dashboard__grid--recent">
          <div class="ape-dts-console-card dashboard__activity-wrap">
            <ActivityTimeline
              :title="t('dashboard.activity.title')"
              :subtitle="t('dashboard.activity.sub')"
              :events="recentEvents"
              :empty-hint="t('dashboard.empty.activity')"
              @select="goToActivity"
            />
          </div>
          <div class="ape-dts-console-card dashboard__activity-wrap">
            <RunningTaskGrid
              :title="t('dashboard.top.title')"
              :subtitle="t('dashboard.top.sub')"
              :tasks="topRunning"
              :empty-hint="t('dashboard.empty.topRunning')"
              :more-label="t('common.more')"
              @select="(task) => go(`/tasks/${task.category}/${task.id}`)"
              @more="go('/tasks/sync?status=running')"
            />
          </div>
        </div>
      </section>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, ref } from 'vue';
import { useI18n } from 'vue-i18n';
import { useRouter } from 'vue-router';
import VChart from 'vue-echarts';
import dayjs from 'dayjs';
import PageHeader from '@/components/PageHeader.vue';
import KpiCard from '@/components/KpiCard.vue';
import ChartCard from '@/components/ChartCard.vue';
import EmptyState from '@/components/EmptyState.vue';
import ActivityTimeline from '@/components/ActivityTimeline.vue';
import RunningTaskGrid from '@/components/RunningTaskGrid.vue';
import LicenseBanner from '@/components/LicenseBanner.vue';
import { useDashboardData } from '@/composables/useDashboardData';
import { ENGINE_LABELS, type ActivityEvent, type TaskStatus, type TaskCategory } from '@/types/domain';
import { AXIS_BASE } from '@/composables/useEcharts';
import IconActivity from '~icons/tabler/activity';
import IconAlertTriangle from '~icons/tabler/alert-triangle';
import IconBolt from '~icons/tabler/bolt';
import IconClock from '~icons/tabler/clock';
import IconRefresh from '~icons/tabler/refresh';

const { t } = useI18n();
const router = useRouter();

const { summary, loading, load } = useDashboardData();

const timeRange = ref<string>('24h');
const timeRangeOptions = [
  { label: '1h', value: '1h' },
  { label: '6h', value: '6h' },
  { label: '24h', value: '24h' },
  { label: '7d', value: '7d' },
];

const timeRangeHint = computed(() => {
  const map: Record<string, string> = { '1h': '近 1 小时', '6h': '近 6 小时', '24h': '近 24 小时', '7d': '近 7 天' };
  return map[timeRange.value];
});

const topRunning = computed(() => summary.value?.topRunningTasks ?? []);
const recentEvents = computed(() => summary.value?.recentEvents ?? []);
const totalTasks = computed(() => (summary.value?.statusDist ?? []).reduce((s, d) => s + d.count, 0));
const hasTimeseries = computed(() => (summary.value?.rpsSeries?.length ?? 0) > 0
  || (summary.value?.latencySeries?.length ?? 0) > 0);

/* ---- ECharts options ---- */

const STATUS_LABELS: Record<TaskStatus, string> = {
  draft: '草稿', ready: '就绪', running: '运行中', paused: '已暂停', stopping: '停止中', stopped: '已停止', failed: '错误', completed: '已完成',
  creating: '创建中', pending: '等待',
};
const STATUS_COLORS: Record<TaskStatus, string> = {
  draft: '#94A3B8', ready: '#67E8F9', running: '#10B981', paused: '#F59E0B', stopping: '#F97316', stopped: '#64748B', failed: '#EF4444', completed: '#0EA5E9',
  creating: '#94A3B8', pending: '#CBD5E1',
};

/* Combined RPS + latency overlay — primary axis = rps (left teal),
 * secondary axis = latency (right amber), aggregated across top-running tasks. */
const combinedOption = computed(() => {
  const rps = summary.value?.rpsSeries ?? [];
  const lat = summary.value?.latencySeries ?? [];
  const xLen = Math.max(
    rps[0]?.points?.length ?? 0,
    lat[0]?.points?.length ?? 0,
  );

  function aggregate(seriesList: typeof rps, len: number, mode: 'sum' | 'avg'): { xs: string[]; ys: number[] } {
    const xs: string[] = [];
    const ys: number[] = [];
    for (let i = 0; i < len; i++) {
      let acc = 0;
      let n = 0;
      let ts = 0;
      for (const s of seriesList) {
        const p = s.points[i];
        if (!p) continue;
        ts = ts || p.t;
        acc += p.v;
        n += 1;
      }
      xs.push(ts ? dayjs(ts).format('HH:mm') : '');
      ys.push(mode === 'sum' ? acc : (n ? acc / n : 0));
    }
    return { xs, ys };
  }

  const rpsAgg = aggregate(rps, xLen, 'sum');
  const latAgg = aggregate(lat, xLen, 'avg');

  return {
    color: ['#0F766E', '#F59E0B'],
    tooltip: {
      trigger: 'axis',
      backgroundColor: '#FFFFFF',
      borderColor: '#E2E8F0',
      borderWidth: 1,
      textStyle: { color: '#0F172A', fontSize: 12 },
      padding: [8, 12],
      formatter: (params: { axisValue: string; seriesName: string; value: number; color: string }[]) => {
        const x = params[0]?.axisValue ?? '';
        const lines = params.map((p) => {
          const unit = p.seriesName.includes('rps') || p.seriesName.includes('throughput') ? 'rows/s' : 'ms';
          return `<div style="display:flex;align-items:center;gap:6px"><span style="display:inline-block;width:8px;height:8px;border-radius:2px;background:${p.color}"></span><span style="color:#475569">${p.seriesName}</span><strong style="margin-left:auto;color:#0F172A">${formatShort(p.value)} ${unit}</strong></div>`;
        });
        return `<div style="font-weight:600;margin-bottom:6px;color:#0F172A">${x}</div>${lines.join('')}`;
      },
    },
    grid: { top: 24, left: 16, right: 16, bottom: 24, containLabel: true },
    xAxis: {
      type: 'category',
      data: rpsAgg.xs.length ? rpsAgg.xs : latAgg.xs,
      boundaryGap: false,
      ...AXIS_BASE,
    },
    yAxis: [
      {
        type: 'value',
        name: 'rows/s',
        nameTextStyle: { color: '#94A3B8', fontSize: 11 },
        ...AXIS_BASE,
        axisLabel: { ...AXIS_BASE.axisLabel, formatter: (v: number) => formatShort(v) },
      },
      {
        type: 'value',
        name: 'ms',
        position: 'right',
        nameTextStyle: { color: '#94A3B8', fontSize: 11 },
        ...AXIS_BASE,
        splitLine: { show: false },
        axisLabel: { ...AXIS_BASE.axisLabel, formatter: (v: number) => `${Math.round(v)}` },
      },
    ],
    series: [
      {
        name: t('dashboard.kpi.throughput'),
        type: 'line',
        data: rpsAgg.ys,
        showSymbol: false,
        smooth: true,
        lineStyle: { width: 2 },
        areaStyle: {
          opacity: 0.18,
          color: {
            type: 'linear', x: 0, y: 0, x2: 0, y2: 1,
            colorStops: [
              { offset: 0, color: 'rgba(15,118,110,0.4)' },
              { offset: 1, color: 'rgba(15,118,110,0.02)' },
            ],
          },
        },
        yAxisIndex: 0,
      },
      {
        name: t('dashboard.kpi.latency'),
        type: 'line',
        data: latAgg.ys,
        showSymbol: false,
        smooth: true,
        lineStyle: { width: 1.6, type: 'dashed' },
        yAxisIndex: 1,
      },
    ],
  };
});

const statusPieOption = computed(() => {
  const data = (summary.value?.statusDist ?? []).map((d) => ({
    name: STATUS_LABELS[d.status],
    value: d.count,
    status: d.status,
    itemStyle: { color: STATUS_COLORS[d.status] },
  }));
  return {
    tooltip: { trigger: 'item', formatter: '{b}: {c} ({d}%)' },
    legend: { orient: 'vertical', right: 8, top: 'middle', itemWidth: 10, itemHeight: 10, textStyle: { color: '#475569', fontSize: 11 } },
    series: [
      {
        type: 'pie',
        radius: ['58%', '82%'],
        center: ['38%', '50%'],
        avoidLabelOverlap: true,
        label: {
          show: true,
          position: 'center',
          formatter: () => `{count|${totalTasks.value}}\n{label|${t('dashboard.chart.totalTasks')}}`,
          rich: {
            count: { fontSize: 24, fontWeight: 700, color: '#0F172A' },
            label: { fontSize: 11, color: '#94A3B8', lineHeight: 18 },
          },
        },
        labelLine: { show: false },
        emphasis: { scale: true, scaleSize: 4 },
        data,
      },
    ],
  };
});

const engineBarOption = computed(() => {
  const items = summary.value?.engineDist ?? [];
  return {
    tooltip: { trigger: 'axis', axisPointer: { type: 'shadow' } },
    grid: { top: 10, left: 70, right: 28, bottom: 16, containLabel: true },
    xAxis: { type: 'value', ...AXIS_BASE },
    yAxis: {
      type: 'category',
      data: items.map((i) => ENGINE_LABELS[i.engine]),
      ...AXIS_BASE,
      axisLabel: { ...AXIS_BASE.axisLabel, fontSize: 11 },
    },
    series: [
      {
        type: 'bar',
        data: items.map((i) => i.count),
        barMaxWidth: 16,
        itemStyle: {
          color: {
            type: 'linear', x: 0, y: 0, x2: 1, y2: 0,
            colorStops: [
              { offset: 0, color: '#0F766E' },
              { offset: 1, color: '#06B6D4' },
            ],
          },
          borderRadius: [0, 3, 3, 0],
        },
        label: { show: true, position: 'right', color: '#475569', fontSize: 11 },
      },
    ],
  };
});

const alertTrendOption = computed(() => {
  const items = summary.value?.alertTrend ?? [];
  return {
    tooltip: { trigger: 'axis', axisPointer: { type: 'shadow' } },
    legend: { bottom: 0, itemWidth: 10, itemHeight: 10, textStyle: { color: '#475569', fontSize: 11 } },
    grid: { top: 10, left: 30, right: 16, bottom: 32, containLabel: true },
    xAxis: {
      type: 'category',
      data: items.map((i) => i.date.slice(5)),
      ...AXIS_BASE,
    },
    yAxis: { type: 'value', ...AXIS_BASE },
    series: [
      { name: t('alerts.summary.critical'), type: 'bar', stack: 'all', data: items.map((i) => i.critical), itemStyle: { color: '#EF4444' }, barMaxWidth: 14 },
      { name: t('alerts.summary.major'), type: 'bar', stack: 'all', data: items.map((i) => i.major), itemStyle: { color: '#F59E0B' } },
      { name: t('alerts.summary.minor'), type: 'bar', stack: 'all', data: items.map((i) => i.minor), itemStyle: { color: '#0EA5E9' } },
      { name: t('alerts.summary.info'), type: 'bar', stack: 'all', data: items.map((i) => i.info), itemStyle: { color: '#94A3B8' } },
    ],
  };
});

/* ---- Interactions ---- */
function go(path: string) { router.push(path); }
function onStatusClick(ev: unknown) {
  const data = (ev as { data?: { status?: TaskStatus } } | undefined)?.data;
  if (data?.status) router.push({ path: '/tasks/sync', query: { status: data.status } });
}
function goToActivity(event: ActivityEvent) {
  if (!event.taskId) return;
  const cat: TaskCategory = (event.taskCategory ?? 'snapshot');
  if (event.category === 'alert') {
    router.push({ path: `/tasks/${cat}/${event.taskId}`, query: { tab: 'alerts' } });
  } else {
    router.push({ path: `/tasks/${cat}/${event.taskId}` });
  }
}

/* ---- Formatters ---- */
function formatShort(v: number): string {
  if (v == null || Number.isNaN(v)) return '0';
  if (v >= 1_000_000) return `${(v / 1_000_000).toFixed(1)}M`;
  if (v >= 1_000) return `${(v / 1_000).toFixed(1)}K`;
  return `${Math.round(v)}`;
}
</script>

<style scoped>
.dashboard__body {
  display: flex;
  flex-direction: column;
  gap: 20px;
}
.dashboard__banner {
  border-radius: var(--radius-md);
}
.dashboard__banner-content {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  width: 100%;
}
.dashboard__banner-arrow { width: 14px; height: 14px; margin-left: 4px; }

.dashboard__hero {
  display: grid;
  grid-template-columns: repeat(4, 1fr);
  gap: 16px;
}
.dashboard__kpi-item { cursor: pointer; min-height: 138px; }

.dashboard__section {
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.dashboard__section-head {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  padding: 4px 2px;
}
.dashboard__section-head h2 {
  margin: 0;
  font-size: var(--text-lg);
  font-weight: 600;
  color: var(--color-ink);
  letter-spacing: -0.01em;
}
.dashboard__section-hint {
  color: var(--color-ink-subtle);
  font-size: 12px;
}

.dashboard__legend {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  font-size: 12px;
  color: var(--color-ink-subtle);
}
.dashboard__legend-dot {
  width: 8px;
  height: 8px;
  border-radius: 2px;
  display: inline-block;
}

.dashboard__grid {
  display: grid;
  gap: 16px;
}
.dashboard__grid--3 { grid-template-columns: repeat(3, 1fr); }
.dashboard__grid--recent { grid-template-columns: 2fr 1fr; }

.dashboard__chart {
  width: 100%;
  height: 100%;
  min-height: 240px;
}

.dashboard__activity-wrap {
  display: flex;
  flex-direction: column;
  min-height: 420px;
  max-height: 520px;
}

@media (max-width: 1280px) {
  .dashboard__hero { grid-template-columns: repeat(2, 1fr); }
  .dashboard__grid--3,
  .dashboard__grid--recent { grid-template-columns: 1fr; }
}
</style>
