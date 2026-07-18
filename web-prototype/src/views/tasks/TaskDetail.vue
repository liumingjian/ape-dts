<template>
  <div class="detail">
    <header class="detail__header">
      <div class="detail__identity">
        <el-button link @click="onBack">
          <IconArrowLeft /> {{ t('taskDetail.back') }}
        </el-button>
        <div class="detail__identity-main">
          <span class="detail__eyebrow">Task</span>
          <div class="detail__title-row">
            <el-tooltip :content="task?.name ?? ''" placement="top" :disabled="!task?.name">
              <h1 class="detail__title" tabindex="0">{{ task?.name ?? '—' }}</h1>
            </el-tooltip>
            <span v-if="task" class="detail__task-id">{{ task.id }}</span>
          </div>
          <div v-if="task" class="detail__runtime-state" aria-label="Current run state">
            <StatusBadge :status="task.status" />
            <span>Run: {{ detail?.currentRun?.status ?? 'Not started' }}</span>
            <span>Phase: {{ currentPhaseLabel }}</span>
          </div>
        </div>
      </div>
      <div class="detail__actions">
        <div class="detail__actions-primary">
          <el-button
            v-if="rbac.can('task.start') && canStart"
            type="success"
            @click="doLifecycle('start')"
          >
            <template #icon><IconPlayerPlay /></template>
            {{ t('taskDetail.action.start') }}
          </el-button>
          <el-button
            v-else-if="rbac.can('task.resume') && task?.status === 'paused'"
            type="primary"
            @click="doLifecycle('resume')"
          >
            <template #icon><IconPlayerPlay /></template>
            {{ t('taskDetail.action.resume') }}
          </el-button>
        </div>
        <div class="detail__actions-secondary" aria-label="Secondary task actions">
          <el-button
            v-if="rbac.can('task.pause') && task?.status === 'running'"
            @click="doLifecycle('pause')"
          >
            <template #icon><IconPlayerPause /></template>
            {{ t('taskDetail.action.pause') }}
          </el-button>
          <el-button v-if="rbac.can('task.stop') && canStop" @click="confirmStop">
            <template #icon><IconPlayerStop /></template>
            {{ t('taskDetail.action.stop') }}
          </el-button>
          <el-button v-if="rbac.can('task.create')" plain @click="openEditor">
            <template #icon><IconEdit /></template>
            {{ t('taskDetail.action.edit') }}
          </el-button>
        </div>
        <div v-if="rbac.can('task.delete')" class="detail__actions-destructive" aria-label="Destructive task actions">
          <el-button type="danger" plain @click="confirmDelete">
            <template #icon><IconTrash /></template>
            {{ t('taskDetail.action.delete') }}
          </el-button>
        </div>
      </div>
    </header>

    <div v-if="detailError" class="detail__diagnostic ape-dts-console-card" role="alert">
      <h2>Task detail unavailable</h2>
      <dl>
        <dt>Code</dt><dd>{{ detailError.code ?? 'UNKNOWN_ERROR' }}</dd>
        <dt>Message</dt><dd>{{ detailError.message }}</dd>
        <dt>HTTP status</dt><dd>{{ detailError.status }}</dd>
        <dt>Request ID</dt><dd>{{ detailError.requestId ?? '—' }}</dd>
        <dt>Last refresh</dt><dd>{{ lastDetailRefresh ? dayjs(lastDetailRefresh).format('YYYY-MM-DD HH:mm:ss') : '—' }}</dd>
      </dl>
      <div class="detail__diagnostic-actions">
        <el-button type="primary" @click="loadDetail">Retry</el-button>
        <el-button @click="copyDetailDiagnostics">Copy diagnostics</el-button>
      </div>
    </div>

    <div v-else-if="task" class="detail__body ape-dts-console-page">
      <!-- KPI strip + flow diagram -->
      <section class="detail__flow ape-dts-console-card">
        <div class="detail__kpi-row">
          <KpiCard :label="t('taskDetail.kpi.status')" :value="0" :badge="detail?.currentRun?.status ?? task.status" :icon-comp="IconActivity" />
          <KpiCard :label="currentPhase === 'cdc' ? 'Apply throughput' : 'Copy throughput'" :value="throughputValue ?? 0" unit="rows/s" :icon-comp="IconBolt" :sentinel-text="throughputValue === null ? '—' : undefined" />

          <template v-if="currentPhase !== 'cdc'">
            <div class="detail__kpi-progress-card kpi">
              <div class="kpi__head">
                <div class="kpi__label">
                  <IconChartBar class="kpi__icon" />
                  <span>{{ t('taskDetail.kpi.progress') }}</span>
                </div>
              </div>
              <div class="kpi__value">
                <el-progress v-if="progressValue !== null" :percentage="progressValue" :stroke-width="10" :show-text="true" />
                <span v-else>—</span>
              </div>
              <div class="detail__progress-counts">
                <template v-if="progress?.copiedRecords !== null && progress?.copiedRecords !== undefined">
                  <template v-if="progress.estimatedTotalRecords !== null && progress.estimatedTotalRecords !== undefined">
                    {{ progress.copiedRecords }} / {{ progress.totalIsEstimate ? 'estimated ' : '' }}{{ progress.estimatedTotalRecords }} records
                  </template>
                  <template v-else>
                    {{ progress.copiedRecords }} records · Estimating total
                  </template>
                </template>
                <template v-else>No row sample received</template>
              </div>
              <div class="detail__progress-counts">
                {{ snapshotCompletedTables ?? '—' }} / {{ snapshotSelectedTables ?? '—' }} tables
              </div>
            </div>
          </template>

          <template v-if="currentPhase === 'cdc'">
            <KpiCard label="Replication lag" :value="lagHasValue ? (rawLatestMetrics.lag ?? 0) : 0" unit="s" :icon-comp="IconClock" :sentinel-text="lagHasValue ? undefined : '—'" />
            <KpiCard label="Queue backlog" :value="rawLatestMetrics.pipeline_queue_size ?? 0" :icon-comp="IconChartBar" :sentinel-text="rawLatestMetrics.pipeline_queue_size === undefined ? '—' : undefined" />
            <KpiCard label="Applied changes" :value="rawLatestMetrics.sinker_sinked_records ?? 0" :icon-comp="IconChartBar" :sentinel-text="rawLatestMetrics.sinker_sinked_records === undefined ? '—' : undefined" />
          </template>
        </div>
        <div v-if="metricsAreStale" class="detail__metric-state" role="status">
          Metrics stale · Last sample {{ metricsSampledAt ? dayjs(metricsSampledAt).format('YYYY-MM-DD HH:mm:ss') : '—' }}
        </div>
        <div v-if="metricErrors.length" class="detail__diagnostic" data-testid="metric-diagnostics" role="alert">
          <h2>Metric query failed</h2>
          <dl v-for="error in metricErrors" :key="error.metric">
            <dt>Metric</dt><dd>{{ error.metric }}</dd>
            <dt>Code</dt><dd>{{ error.code ?? 'UNKNOWN_ERROR' }}</dd>
            <dt>Message</dt><dd>{{ error.message }}</dd>
            <dt>HTTP status</dt><dd>{{ error.status }}</dd>
            <dt>Request ID</dt><dd>{{ error.requestId ?? '—' }}</dd>
            <dt>Last refresh</dt><dd>{{ lastMetricsRefresh ? dayjs(lastMetricsRefresh).format('YYYY-MM-DD HH:mm:ss') : '—' }}</dd>
          </dl>
          <div class="detail__diagnostic-actions">
            <el-button type="primary" @click="loadMetricSeries">Retry</el-button>
            <el-button @click="copyMetricDiagnostics">Copy diagnostics</el-button>
          </div>
        </div>
        <div v-if="currentPhase === 'cdc'" class="detail__metric-context">
          <span v-if="throughputValue === 0">No new changes</span>
          <span v-else-if="throughputValue === null">No sample received for sinker_rps_avg</span>
          <span v-if="detail?.currentRun?.checkpoint">Checkpoint: {{ formatPosition(detail.currentRun.checkpoint) }}</span>
          <span v-else>Checkpoint: —</span>
          <span>Last event: {{ lastEventText }}</span>
        </div>
      </section>

      <!-- 3 charts -->
      <section class="detail__charts ape-dts-console-card">
        <ChartCard title="Source read throughput · rows/s" :height="200">
          <v-chart v-if="rpsOption" :option="rpsOption" autoresize class="detail__chart" />
          <div v-else class="detail__chart-state">No sample received for extractor_rps_avg</div>
        </ChartCard>
        <ChartCard title="Target apply throughput · rows/s" :height="200">
          <v-chart v-if="sinkRpsOption" :option="sinkRpsOption" autoresize class="detail__chart" />
          <div v-else class="detail__chart-state">No sample received for sinker_rps_avg</div>
        </ChartCard>
        <ChartCard title="Queue backlog · records" :height="200">
          <v-chart v-if="bufferOption" :option="bufferOption" autoresize class="detail__chart" />
          <div v-else class="detail__chart-state">No sample received for pipeline_queue_size</div>
        </ChartCard>
      </section>

      <el-tabs v-model="activeTab" class="detail__tabs ape-dts-console-card" @tab-change="onTabChange as any">
        <el-tab-pane :label="t('taskDetail.tab.overview')" name="overview">
          <section class="detail__topology" aria-labelledby="database-topology-title">
            <h2 id="database-topology-title">Database topology</h2>
            <div class="detail__topology-flow">
              <article class="detail__endpoint" aria-label="Source database">
                <span class="detail__endpoint-role">Source</span>
                <EngineTag :engine="task.source.engine" />
                <dl>
                  <dt>Host</dt><dd>{{ task.source.host || 'Unavailable' }}</dd>
                  <dt>Port</dt><dd>{{ task.source.port || 'Unavailable' }}</dd>
                  <dt>Database</dt><dd>{{ task.source.database || 'Unavailable' }}</dd>
                </dl>
              </article>
              <div class="detail__topology-arrow" aria-label="Replicates to"><IconArrowRight /></div>
              <article class="detail__endpoint" aria-label="Target database">
                <span class="detail__endpoint-role">Target</span>
                <EngineTag :engine="task.target.engine" />
                <dl>
                  <dt>Host</dt><dd>{{ task.target.host || 'Unavailable' }}</dd>
                  <dt>Port</dt><dd>{{ task.target.port || 'Unavailable' }}</dd>
                  <dt>Database</dt><dd>{{ task.target.database || 'Unavailable' }}</dd>
                </dl>
              </article>
            </div>
          </section>
          <div class="detail__config">
            <h2>Configuration summary</h2>
            <dl>
              <dt>Parallel mode</dt><dd>{{ task.config.parallelizer }}</dd>
              <dt>Parallel size</dt><dd>{{ task.config.parallelSize }}</dd>
              <dt>Buffer</dt><dd>{{ task.config.bufferSize }} rows</dd>
              <dt>Checkpoint interval</dt><dd>{{ task.config.checkpointIntervalSecs }} s</dd>
              <dt>Maximum RPS</dt><dd>{{ task.config.maxRps || 'Unlimited' }}</dd>
              <dt>Resume strategy</dt><dd>{{ task.config.resumeType }}</dd>
              <dt>Prometheus</dt><dd>{{ task.config.metricsEnabled ? 'Enabled' : 'Disabled' }}</dd>
              <dt>Selected objects</dt><dd>{{ detail?.task.selectedObjects.length ?? 'Unavailable' }}</dd>
            </dl>
          </div>
        </el-tab-pane>

        <el-tab-pane :label="t('taskDetail.tab.objects')" name="objects">
          <div v-if="objectsError" class="detail__diagnostic" role="alert">
            <h2>Sync objects unavailable</h2>
            <dl>
              <dt>Code</dt><dd>{{ objectsError.code ?? 'UNKNOWN_ERROR' }}</dd>
              <dt>Message</dt><dd>{{ objectsError.message }}</dd>
              <dt>HTTP status</dt><dd>{{ objectsError.status }}</dd>
              <dt>Request ID</dt><dd>{{ objectsError.requestId ?? '—' }}</dd>
            </dl>
            <el-button type="primary" @click="loadObjects">Retry</el-button>
          </div>
          <el-empty v-else-if="objects.length === 0" :description="t('common.empty')" />
          <el-table v-else :data="objects" class="detail__objects">
            <el-table-column :label="t('taskDetail.objects.col.schema')" min-width="180" prop="schema" />
            <el-table-column :label="t('taskDetail.objects.col.table')" min-width="240" prop="table" />
            <el-table-column :label="t('taskDetail.objects.col.state')" width="140">
              <template #default="{ row }">
                <el-tag :type="stateTagType(row.state)" size="small">{{ row.state }}</el-tag>
              </template>
            </el-table-column>
          </el-table>
        </el-tab-pane>

        <!-- Logs (SSE) -->
        <el-tab-pane :label="t('taskDetail.tab.logs')" name="logs">
          <div class="detail__log-context" aria-label="Log context">
            <span>Run ID: <strong>{{ currentRunId || '—' }}</strong></span>
            <span>Phase: <strong>{{ currentPhaseLabel }}</strong></span>
            <span>File: <strong>{{ logFile }}.log</strong></span>
            <span>Last event: <strong>{{ logLastEventText }}</strong></span>
          </div>
          <div class="detail__log-toolbar">
            <div class="detail__log-toolbar-left">
              <el-select v-model="logFile" class="detail__log-file-select" @change="reopenLogStream">
                <el-option v-for="f in logFiles" :key="f" :label="f" :value="f" />
              </el-select>
              <el-select v-model="logLevelFilter" class="detail__log-level-select">
                <el-option label="ALL" value="ALL" />
                <el-option label="ERROR" value="error" />
                <el-option label="WARN" value="warn" />
                <el-option label="INFO" value="info" />
                <el-option label="DEBUG" value="debug" />
              </el-select>
              <el-input
                v-model="logSearch"
                class="detail__log-search"
                clearable
                placeholder="Search logs"
                aria-label="Search logs"
              />
              <span
                class="detail__log-status-pill"
                :class="`detail__log-status-pill--${sseState}`"
              >
                {{ sseStateLabel }}
              </span>
              <el-button
                v-if="sseState === 'disconnected'"
                size="small"
                @click="reopenLogStream"
              >
                {{ t('taskDetail.log.reconnect') }}
              </el-button>
            </div>
            <div class="detail__log-toolbar-right">
              <el-button size="small" @click="downloadLogs">Download</el-button>
              <el-button
                size="small"
                :type="logPaused ? 'primary' : 'default'"
                @click="toggleLogPause"
              >
                {{ logPaused ? t('taskDetail.log.resume') : t('taskDetail.log.pause') }}
              </el-button>
            </div>
          </div>
          <p class="detail__sr-only" role="status" aria-live="polite" aria-atomic="true">{{ logLiveRegionText }}</p>
          <el-alert
            v-if="logNotice"
            type="warning"
            :closable="false"
            show-icon
            class="detail__run-alert"
            :title="logNotice"
          />
          <div v-if="logError" class="detail__diagnostic detail__log-diagnostic" role="alert">
            <h2>Run logs unavailable</h2>
            <dl>
              <dt>Code</dt><dd>{{ logError.code ?? 'UNKNOWN_ERROR' }}</dd>
              <dt>Message</dt><dd>{{ logError.message }}</dd>
              <dt>HTTP status</dt><dd>{{ logError.status }}</dd>
              <dt>Request ID</dt><dd>{{ logError.requestId ?? '—' }}</dd>
              <dt>Run ID</dt><dd>{{ currentRunId || '—' }}</dd>
              <dt>Phase</dt><dd>{{ currentPhaseLabel }}</dd>
              <dt>File</dt><dd>{{ logFile }}.log</dd>
              <dt>Connection</dt><dd>{{ sseStateLabel }}</dd>
              <dt>Last event</dt><dd>{{ logLastEventText }}</dd>
              <dt>Last refresh</dt><dd>{{ lastLogRefresh ? dayjs(lastLogRefresh).format('YYYY-MM-DD HH:mm:ss') : '—' }}</dd>
            </dl>
            <div class="detail__diagnostic-actions">
              <el-button type="primary" @click="reopenLogStream">Retry</el-button>
              <el-button @click="copyLogDiagnostics">Copy diagnostics</el-button>
            </div>
          </div>
          <el-alert
            v-if="latestRun?.status === 'failed' && latestRun.exitCode !== null"
            type="error"
            :closable="false"
            show-icon
            class="detail__run-alert"
          >
            Run {{ latestRun.id }} failed with exit code {{ latestRun.exitCode }}.
          </el-alert>
          <div ref="logPaneRef" class="detail__log-view" @scroll="onLogScroll">
            <div
              v-for="(ln, i) in filteredLogLines"
              :key="i"
              class="detail__log-line"
              :class="`detail__log-line--${ln.level}`"
            >
              <span class="detail__log-time">{{ formatLogTime(ln.timestamp) }}</span>
              <span class="detail__log-level">{{ ln.level.toUpperCase() }}</span>
              <span class="detail__log-source">{{ ln.source }}</span>
              <span class="detail__log-msg">{{ ln.message }}</span>
            </div>
          </div>
          <div v-if="showFollowBtn" class="detail__log-follow">
            <el-button size="small" type="primary" @click="scrollToBottom">
              {{ t('taskDetail.log.follow') }}
            </el-button>
          </div>
        </el-tab-pane>

        <!-- Monitor -->
        <el-tab-pane :label="t('taskDetail.tab.monitoring')" name="monitoring">
          <div class="detail__monitor-toolbar">
            <el-button-group>
              <el-button
                v-for="r in monitorRanges"
                :key="r.value"
                :type="monitorRange === r.value ? 'primary' : 'default'"
                size="small"
                @click="setMonitorRange(r.value)"
              >
                {{ r.label }}
              </el-button>
            </el-button-group>
          </div>
          <div v-if="monitorSeries.length === 0" class="detail__monitor-empty">
            <el-empty :description="t('common.empty')" />
          </div>
          <div v-else class="detail__monitor-charts">
            <ChartCard v-for="ms in monitorSeries" :key="ms.metric" :title="ms.metric" :height="200">
              <v-chart :option="monitorChartOption(ms)" autoresize class="detail__chart" />
            </ChartCard>
          </div>
        </el-tab-pane>

        <el-tab-pane :label="t('taskDetail.tab.history')" name="history">
          <el-table v-loading="historyLoading" :data="historyRuns" class="detail__history">
            <el-table-column label="Run ID" width="200" prop="id" />
            <el-table-column label="状态" width="120">
              <template #default="{ row }">
                <StatusBadge :status="row.status" />
              </template>
            </el-table-column>
            <el-table-column label="开始时间" width="170">
              <template #default="{ row }">{{ row.startedAt ? dayjs(row.startedAt).format('YYYY-MM-DD HH:mm:ss') : '—' }}</template>
            </el-table-column>
            <el-table-column label="结束时间" width="170">
              <template #default="{ row }">{{ row.stoppedAt ? dayjs(row.stoppedAt).format('YYYY-MM-DD HH:mm:ss') : '—' }}</template>
            </el-table-column>
            <el-table-column label="Exit Code" width="120" align="center">
              <template #default="{ row }">{{ row.exitCode ?? '—' }}</template>
            </el-table-column>
            <el-table-column label="断点续传状态" min-width="200">
              <template #default="{ row }">
                <span v-if="row.position" class="detail__mono detail__position">{{ formatPosition(row.position) }}</span>
                <span v-else class="detail__muted">—</span>
              </template>
            </el-table-column>
            <el-table-column label="操作" width="120" fixed="right">
              <template #default="{ row }">
                <el-button link type="primary" @click="viewArchivedLogs(row)">
                  {{ t('taskDetail.history.viewLogs') }}
                </el-button>
              </template>
            </el-table-column>
          </el-table>
          <footer v-if="historyTotal > historyPageSize" class="detail__history-footer">
            <el-pagination
              v-model:current-page="historyPage"
              :page-size="historyPageSize"
              :total="historyTotal"
              layout="prev, pager, next"
              background
              @current-change="loadHistory"
            />
          </footer>
        </el-tab-pane>

        <el-tab-pane :label="t('taskDetail.tab.more')" name="more">
          <section class="detail__more-section">
            <h2>Alerts</h2>
            <el-table v-if="alerts.length" :data="alerts" class="detail__alerts">
              <el-table-column label="Level" width="110">
                <template #default="{ row }"><LevelBadge :level="row.level" /></template>
              </el-table-column>
              <el-table-column label="Source" width="120" prop="source" />
              <el-table-column label="Message" prop="message" />
              <el-table-column label="Service" width="140" prop="service" />
              <el-table-column label="First seen" width="170">
                <template #default="{ row }">{{ dayjs(row.firstAt).format('MM-DD HH:mm:ss') }}</template>
              </el-table-column>
              <el-table-column label="Count" width="80" align="right">
                <template #default="{ row }">{{ row.count }}</template>
              </el-table-column>
            </el-table>
            <el-empty v-else :description="t('taskDetail.alerts.none')" />
          </section>
        </el-tab-pane>
      </el-tabs>
    </div>

    <div v-else class="detail__loading">
      <el-skeleton :rows="6" animated />
    </div>

    <!-- Edit drawer -->
    <el-drawer
      v-model="editorVisible"
      :title="t('taskDetail.editor.title')"
      size="520px"
      direction="rtl"
      append-to-body
      @close="onEditorClose"
    >
      <div v-if="task" class="detail__editor">
        <el-alert type="info" :closable="false" show-icon>{{ t('taskDetail.editor.tip') }}</el-alert>
        <div class="detail__editor-form">
          <label>任务名称</label>
          <el-input v-model="editForm.name" disabled />
          <label>描述</label>
          <el-input v-model="editForm.description" type="textarea" :rows="2" />
          <label>资源组</label>
          <el-select v-model="editForm.resourceGroup" style="width: 100%">
            <el-option v-for="g in resourceGroups" :key="g" :label="g" :value="g" />
          </el-select>
          <label>并行度</label>
          <el-input-number v-model="editForm.config.parallelSize" :min="1" :max="64" style="width: 100%" />
          <label>缓冲区</label>
          <el-input-number v-model="editForm.config.bufferSize" :min="1000" :max="200000" :step="1000" style="width: 100%" />
          <label>断点间隔（秒）</label>
          <el-input-number v-model="editForm.config.checkpointIntervalSecs" :min="1" :max="600" style="width: 100%" />
          <label>最大 RPS (0 = 不限速)</label>
          <el-input-number v-model="editForm.config.maxRps" :min="0" :max="1000000" :step="500" style="width: 100%" />
          <label>续传策略</label>
          <el-select v-model="editForm.config.resumeType" style="width: 100%">
            <el-option label="from_log" value="from_log" />
            <el-option label="from_target" value="from_target" />
            <el-option label="from_db" value="from_db" />
          </el-select>
        </div>
      </div>
      <template #footer>
        <el-button @click="editorVisible = false">取消</el-button>
        <el-button type="primary" :loading="saving" @click="saveEdit">保存</el-button>
      </template>
    </el-drawer>

    <!-- Archived logs dialog -->
    <el-dialog v-model="archivedDialogVisible" :title="t('taskDetail.history.archivedLogs')" width="70%">
      <div v-loading="archivedLoading" class="detail__archived-log-view">
        <div
          v-for="(ln, i) in archivedLines"
          :key="i"
          class="detail__log-line"
          :class="`detail__log-line--${ln.level ?? 'info'}`"
        >
          <span class="detail__log-time">{{ formatLogTime(ln.timestamp) }}</span>
          <span class="detail__log-level">{{ (ln.level ?? 'info').toUpperCase() }}</span>
          <span class="detail__log-msg">{{ ln.message }}</span>
        </div>
      </div>
      <template #footer>
        <el-button @click="archivedDialogVisible = false">{{ t('common.close') }}</el-button>
      </template>
    </el-dialog>
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, reactive, ref, shallowRef, watch } from 'vue';
import { useI18n } from 'vue-i18n';
import { useRoute, useRouter } from 'vue-router';
import type { RouteLocationRaw } from 'vue-router';
import { ElMessage, ElMessageBox } from 'element-plus';
import dayjs from 'dayjs';
import { api, type ApiError } from '@/api/client';
import { useRbac } from '@/composables/useRbac';
import { useDocumentVisibility } from '@/composables/useDocumentVisibility';
import { useLogStream, type LogLine, type LogStreamHandle } from '@/composables/useLogStream';
import type { Task, Alert, ApiAlert, MetricQueryResponse, Run, RunPosition, TableLoadState, TaskCategory, TaskDetailAggregate, TaskDetailPhaseName } from '@/types/domain';
import { mapApiTask, mapApiAlert } from '@/types/domain';
import { listPathForTaskKind } from '@/utils/migrationMode';
import { redactDiagnosticText, redactDiagnosticValue } from '@/utils/redactDiagnostics';
import EngineTag from '@/components/EngineTag.vue';
import KpiCard from '@/components/KpiCard.vue';
import ChartCard from '@/components/ChartCard.vue';
import LevelBadge from '@/components/LevelBadge.vue';
import StatusBadge from '@/components/StatusBadge.vue';
import '@/composables/useEcharts';
import { BRAND_PALETTE, AXIS_BASE } from '@/composables/useEcharts';
import IconBolt from '~icons/tabler/bolt';
import IconClock from '~icons/tabler/clock';
import IconChartBar from '~icons/tabler/chart-bar';
import IconActivity from '~icons/tabler/activity';
import IconArrowRight from '~icons/tabler/arrow-right';

const { t } = useI18n();
const route = useRoute();
const router = useRouter();
const rbac = useRbac();
const { isVisible } = useDocumentVisibility();

const VALID_TABS = ['overview', 'objects', 'logs', 'monitoring', 'history', 'more'] as const;
type TabName = (typeof VALID_TABS)[number];

function resolveTab(value: unknown): TabName {
  return typeof value === 'string' && VALID_TABS.includes(value as TabName) ? value as TabName : 'overview';
}

const taskId = computed(() => String(route.params.id));
const taskCategory = computed<TaskCategory>(() => {
  const category = String(route.params.category ?? 'snapshot');
  return category === 'cdc' || category === 'check' || category === 'struct' ? category : 'snapshot';
});

const detail = ref<TaskDetailAggregate | null>(null);
const task = ref<Task | null>(null);
const detailError = ref<ApiError | null>(null);
const lastDetailRefresh = ref<string | null>(null);
const activeTab = ref<TabName>(resolveTab(route.query.tab));
const editorVisible = ref(Boolean(route.query.edit === '1'));
const saving = ref(false);
const resourceGroups = ['default', 'production', 'staging', 'dev'];

/* ---------- computed helpers ---------- */
const canStart = computed(() => {
  const s = task.value?.status;
  return s === 'draft' || s === 'ready' || s === 'stopped' || s === 'failed' || s === 'completed';
});
const canStop = computed(() => {
  const s = task.value?.status;
  return s === 'running' || s === 'paused' || s === 'stopping';
});

/* ---------- edit form ---------- */
const editForm = reactive({
  name: '',
  description: '',
  resourceGroup: 'default',
  config: { parallelSize: 4, bufferSize: 16000, checkpointIntervalSecs: 10, maxRps: 0, resumeType: 'from_log' as Task['config']['resumeType'] },
});

watch(task, (v) => {
  if (!v) return;
  editForm.name = v.name;
  editForm.description = v.description ?? '';
  editForm.resourceGroup = v.resourceGroup;
  editForm.config.parallelSize = v.config.parallelSize;
  editForm.config.bufferSize = v.config.bufferSize;
  editForm.config.checkpointIntervalSecs = v.config.checkpointIntervalSecs;
  editForm.config.maxRps = v.config.maxRps;
  editForm.config.resumeType = v.config.resumeType;
}, { immediate: true });

/* ---------- load authoritative detail ---------- */
async function loadDetail() {
  try {
    const aggregate = await api.get<TaskDetailAggregate>(`/tasks/${taskId.value}/detail`);
    detail.value = aggregate;
    task.value = mapApiTask(aggregate.task);
    latestRun.value = aggregate.currentRun ? {
      id: aggregate.currentRun.id,
      taskId: aggregate.task.id,
      status: aggregate.currentRun.status,
      startedAt: aggregate.currentRun.startedAt,
      stoppedAt: aggregate.currentRun.stoppedAt,
      exitCode: aggregate.currentRun.exitCode,
      logDir: null,
      iniPath: null,
      pid: null,
      position: aggregate.currentRun.checkpoint,
      createdAt: aggregate.currentRun.startedAt ?? aggregate.task.createdAt,
    } : null;
    currentRunId.value = aggregate.currentRun?.id ?? '';
    rawLatestMetrics.value = aggregate.metricsSnapshot?.values ?? {};
    metricsSampledAt.value = aggregate.metricsSnapshot?.sampledAt ?? null;
    accumulateMetrics(aggregate.metricsSnapshot?.sampledAt);
    const shouldLoadSeries = currentRunId.value !== metricSeriesRunId.value;
    if (shouldLoadSeries) metricSeriesRunId.value = currentRunId.value;
    await Promise.all([
      shouldLoadSeries ? loadMetricSeries() : Promise.resolve(),
      aggregate.currentRun?.currentPhase === 'snapshot' ? loadObjects() : Promise.resolve(),
    ]);
    detailError.value = null;
  } catch (error) {
    detailError.value = error as ApiError;
  } finally {
    lastDetailRefresh.value = new Date().toISOString();
  }
}

function accumulateMetrics(sampledAt?: string) {
  const timestamp = sampledAt ? dayjs(sampledAt).valueOf() : Date.now();
  for (const [metric, value] of Object.entries(rawLatestMetrics.value)) {
    if (typeof value !== 'number' || !Number.isFinite(value)) continue;
    const arr = metricsHistory.value[metric] ?? [];
    arr.push({ ts: timestamp, value });
    if (arr.length > MAX_HISTORY_POINTS) arr.shift();
    metricsHistory.value[metric] = arr;
  }
}

async function copyDetailDiagnostics() {
  if (!detailError.value) return;
  await navigator.clipboard.writeText(JSON.stringify({
    code: detailError.value.code,
    message: detailError.value.message,
    status: detailError.value.status,
    requestId: detailError.value.requestId,
    lastRefresh: lastDetailRefresh.value,
    taskId: taskId.value,
  }, null, 2));
  ElMessage.success('Diagnostics copied');
}

function backToListPath(): RouteLocationRaw {
  const cat = taskCategory.value;
  return listPathForTaskKind(cat, task.value?.syncMode);
}

/* ---------- lifecycle actions ---------- */
async function doLifecycle(action: string) {
  try {
    await api.post(`/tasks/${taskId.value}/${action}`);
    ElMessage.success('操作成功');
    await loadDetail();
  } catch (err: unknown) {
    const msg = (err as { message?: string })?.message ?? '操作失败';
    ElMessage.error(msg);
  }
}

function confirmStop() {
  if (!task.value) return;
  ElMessageBox.confirm(
    `确定停止任务「${task.value.name}」？`,
    '停止任务',
    { type: 'warning' },
  ).then(() => doLifecycle('stop')).catch(() => {});
}

function confirmDelete() {
  if (!task.value) return;
  ElMessageBox.confirm(
    `确定删除任务「${task.value.name}」？该操作不可撤销。`,
    '删除任务',
    { type: 'warning' },
  ).then(async () => {
    await api.del(`/tasks/${taskId.value}`);
    ElMessage.success('任务已删除');
    router.push(backToListPath());
  }).catch(() => {});
}

async function saveEdit() {
  saving.value = true;
  try {
    await api.patch(`/tasks/${taskId.value}`, {
      description: editForm.description,
      resourceGroup: editForm.resourceGroup,
      config: {
        ...(task.value?.config ?? {}),
        parallelSize: editForm.config.parallelSize,
        bufferSize: editForm.config.bufferSize,
        checkpointIntervalSecs: editForm.config.checkpointIntervalSecs,
        maxRps: editForm.config.maxRps,
        resumeType: editForm.config.resumeType,
      },
    });
    ElMessage.success('保存成功');
    editorVisible.value = false;
    await loadDetail();
  } finally { saving.value = false; }
}

function openEditor() {
  router.replace({ query: { ...route.query, tab: 'overview', edit: '1' } });
}

function onEditorClose() {
  router.replace({ query: { ...route.query, edit: undefined } });
}

/* ---------- tab deep link ---------- */
function onTabChange(tab: string | number) {
  const tabStr = String(tab);
  const query: Record<string, string | undefined> = { ...route.query, tab: tabStr };
  if (tabStr !== 'overview') delete (query as Partial<typeof query>).edit;
  router.replace({ query });
  if (tabStr === 'logs') {
    reopenLogStream();
  }
}

watch(() => route.query.tab, (v) => {
  activeTab.value = resolveTab(v);
});

watch(() => route.query.edit, (v) => {
  editorVisible.value = v === '1';
});

watch(activeTab, (tab) => {
  if (tab === 'objects') loadObjects();
});

/* ---------- KPI metrics ---------- */
const currentRunId = ref('');
const latestRun = ref<Run | null>(null);
const rawLatestMetrics = ref<Record<string, number>>({});
const metricsHistory = ref<Record<string, { ts: number; value: number }[]>>({});
const metricsSampledAt = ref<string | null>(null);
const lastMetricsRefresh = ref<string | null>(null);
type MetricQueryError = ApiError & { metric: string };
const metricErrors = ref<MetricQueryError[]>([]);
const metricSeriesRunId = ref('');
const MAX_HISTORY_POINTS = 720; // ~1 h at 5 s interval
const METRIC_STALE_AFTER_MS = 30_000;

const DETAIL_METRIC_NAMES = ['extractor_rps_avg', 'sinker_rps_avg', 'pipeline_queue_size'];
const activeMetricNames = computed(() => {
  const names = new Set(DETAIL_METRIC_NAMES);
  names.add('sinker_sinked_records');
  names.add('sinker_rt_avg');
  if (currentPhase.value === 'snapshot') {
    names.add('progress');
    names.add('extractor_plan_records');
  }
  if (currentPhase.value === 'cdc') {
    names.add('lag');
    names.add('timestamp');
  }
  return [...names];
});

async function loadMetricSeries() {
  if (!currentRunId.value) return;
  const to = Date.now();
  const from = to - 3600_000;
  const results = await Promise.all(activeMetricNames.value.map(async (metric) => {
    try {
      const response = await api.get<MetricQueryResponse>(
        `/runs/${currentRunId.value}/metrics?metric=${metric}&from=${from}&to=${to}&step=60`,
      );
      return { metric, response } as const;
    } catch (error) {
      return { metric, error: error as ApiError } as const;
    }
  }));
  const errors: MetricQueryError[] = [];
  for (const result of results) {
    const error = 'error' in result ? result.error : undefined;
    if (error) {
      errors.push({
        metric: result.metric,
        status: error.status ?? 0,
        code: error.code,
        message: error.message ?? 'Metric query failed',
        details: error.details,
        requestId: error.requestId,
      });
      continue;
    }
    const response = 'response' in result ? result.response : undefined;
    if (response) {
      metricsHistory.value[result.metric] = response.data ?? [];
    }
  }
  metricErrors.value = errors;
  lastMetricsRefresh.value = new Date().toISOString();
}

async function copyMetricDiagnostics() {
  await navigator.clipboard.writeText(JSON.stringify({
    runId: currentRunId.value,
    errors: metricErrors.value,
    lastRefresh: lastMetricsRefresh.value,
  }, null, 2));
  ElMessage.success('Diagnostics copied');
}

const detailMetricSeries = computed<MetricQueryResponse[]>(() =>
  DETAIL_METRIC_NAMES
    .filter(m => (metricsHistory.value[m]?.length ?? 0) > 0)
    .map(m => ({ metric: m, data: metricsHistory.value[m] ?? [] })),
);

/* ---------- KPI computed helpers ---------- */
const currentPhase = computed<TaskDetailPhaseName | null>(() => detail.value?.currentRun?.currentPhase ?? null);
const currentPhaseLabel = computed(() => {
  if (currentPhase.value === 'snapshot') return 'Snapshot';
  if (currentPhase.value === 'transitioning_to_cdc') return 'Transitioning to CDC';
  if (currentPhase.value === 'cdc') return 'CDC';
  return 'Not started';
});
const progress = computed(() => detail.value?.progress ?? null);
const progressValue = computed<number | null>(() => {
  const percent = progress.value?.phase === 'snapshot' ? progress.value.percent : null;
  if (percent === null || percent === undefined || !Number.isFinite(percent)) return null;
  return Math.round(Math.max(0, Math.min(100, percent)));
});

const lagHasValue = computed(() => {
  return 'lag' in rawLatestMetrics.value;
});

const throughputValue = computed<number | null>(() => {
  const metric = currentPhase.value === 'cdc' ? 'sinker_rps_avg' : 'extractor_rps_avg';
  const value = rawLatestMetrics.value[metric];
  return typeof value === 'number' && Number.isFinite(value) ? value : null;
});

const snapshotCompletedTables = computed(() => {
  if (!objects.value.length) return null;
  return objects.value.filter((object) => object.state === 'completed').length;
});
const snapshotSelectedTables = computed(() => detail.value?.task.selectedObjects.length ?? null);
const lastEventText = computed(() => {
  const timestamp = rawLatestMetrics.value.timestamp;
  if (typeof timestamp !== 'number' || !Number.isFinite(timestamp)) return '—';
  const milliseconds = timestamp < 10_000_000_000 ? timestamp * 1000 : timestamp;
  return dayjs(milliseconds).format('YYYY-MM-DD HH:mm:ss');
});
const metricsAreStale = computed(() => {
  if (!metricsSampledAt.value) return false;
  const sampledAt = dayjs(metricsSampledAt.value).valueOf();
  return Number.isFinite(sampledAt) && Date.now() - sampledAt > METRIC_STALE_AFTER_MS;
});

function baseLine(name: string, xs: string[], sData: { name: string; data: number[]; color: string }[]) {
  return {
    grid: { left: 36, right: 16, top: 18, bottom: 22 },
    tooltip: { trigger: 'axis' as const },
    legend: { bottom: 0, icon: 'roundRect', itemWidth: 8, itemHeight: 8, textStyle: { color: '#64748B', fontSize: 11 } },
    xAxis: { type: 'category', data: xs, axisLine: AXIS_BASE.axisLine, axisLabel: { ...AXIS_BASE.axisLabel, fontSize: 10 }, axisTick: { show: false } },
    yAxis: { type: 'value', name: name, axisLine: { show: false }, axisLabel: AXIS_BASE.axisLabel, splitLine: AXIS_BASE.splitLine },
    series: sData.map((s) => ({
      name: s.name,
      type: 'line' as const,
      data: s.data,
      smooth: true,
      symbol: 'none',
      lineStyle: { width: 1.6, color: s.color },
      areaStyle: { color: s.color, opacity: 0.08 },
    })),
  };
}

const rpsOption = computed(() => {
  const ms = detailMetricSeries.value.find((s) => s.metric === 'extractor_rps_avg');
  if (!ms || ms.data.length === 0) return null;
  const xs = ms.data.map((p) => dayjs(p.ts).format('HH:mm'));
  return baseLine('extractor_rps_avg', xs, [{ name: 'extractor_rps_avg', data: ms.data.map((p) => p.value), color: BRAND_PALETTE[0] }]);
});

const sinkRpsOption = computed(() => {
  const ms = detailMetricSeries.value.find((s) => s.metric === 'sinker_rps_avg');
  if (!ms || ms.data.length === 0) return null;
  const xs = ms.data.map((p) => dayjs(p.ts).format('HH:mm'));
  return baseLine('sinker_rps_avg', xs, [{ name: 'sinker_rps_avg', data: ms.data.map((p) => p.value), color: BRAND_PALETTE[1] }]);
});

const bufferOption = computed(() => {
  const ms = detailMetricSeries.value.find((s) => s.metric === 'pipeline_queue_size');
  if (!ms || ms.data.length === 0) return null;
  const xs = ms.data.map((p) => dayjs(p.ts).format('HH:mm'));
  return baseLine('pipeline_queue_size', xs, [{ name: 'pipeline_queue_size', data: ms.data.map((p) => p.value), color: BRAND_PALETTE[4] }]);
});

/* ---------- objects (per-table state from /runs/:id/objects) ---------- */
const objects = ref<TableLoadState[]>([]);
const objectsLoading = ref(false);
const objectsError = ref<ApiError | null>(null);

function stateTagType(state: TableLoadState['state']): 'info' | 'warning' | 'success' {
  if (state === 'pending') return 'info';
  if (state === 'loading') return 'warning';
  return 'success'; // completed
}

async function loadObjects() {
  if (!currentRunId.value) return;
  objectsLoading.value = true;
  objectsError.value = null;
  try {
    const res = await api.get<TableLoadState[]>(`/runs/${currentRunId.value}/objects`);
    objects.value = Array.isArray(res) ? res : [];
  } catch (error) {
    objects.value = [];
    objectsError.value = error as ApiError;
  } finally {
    objectsLoading.value = false;
  }
}

/* ---------- Logs tab (SSE) ---------- */
const logFile = ref('default');
const logFiles = ['task', 'default', 'position', 'monitor', 'commit', 'finished', 'http'];
const logLevelFilter = ref('ALL');
const logSearch = ref('');
const logPaused = ref(false);
const logPaneRef = ref<HTMLElement | null>(null);
const showFollowBtn = ref(false);
const logStreamHandle = shallowRef<LogStreamHandle | null>(null);
const persistedLogLines = ref<LogLine[]>([]);
const pausedLogLines = ref<LogLine[]>([]);
const logNotice = ref('');
const logError = ref<ApiError | null>(null);
const lastLogRefresh = ref<string | null>(null);
const logLiveRegionText = ref('');
const FALLBACK_NOTICE = 'Live stream unavailable; showing persisted logs.';

const sseState = computed(() => logStreamHandle.value?.state.value ?? 'disconnected');

const sseStateLabel = computed(() => {
  if (sseState.value === 'connecting') return 'Connecting';
  if (sseState.value === 'connected') return t('taskDetail.log.connected');
  if (sseState.value === 'reconnecting') return t('taskDetail.log.reconnecting');
  return t('taskDetail.log.disconnected');
});

const logLastEventText = computed(() => {
  const timestamp = logStreamHandle.value?.lastEventAt.value;
  return timestamp ? dayjs(timestamp).format('YYYY-MM-DD HH:mm:ss') : '—';
});

function logLineKey(line: LogLine): string {
  return [line.timestamp, line.level, line.source, line.file, line.message].join('\u0000');
}

const combinedLogLines = computed<LogLine[]>(() => {
  const seen = new Set<string>();
  const receivedLiveLines = logStreamHandle.value?.lines.value ?? [];
  const visibleLiveLines = logPaused.value && pausedLogLines.value.length
    ? receivedLiveLines.slice(0, -pausedLogLines.value.length)
    : receivedLiveLines;
  const lines = [...persistedLogLines.value, ...visibleLiveLines];
  return lines.filter((line) => {
    const key = logLineKey(line);
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  }).slice(-500);
});

const filteredLogLines = computed<LogLine[]>(() => {
  const search = logSearch.value.trim().toLowerCase();
  return combinedLogLines.value.filter((line) => {
    if (logLevelFilter.value !== 'ALL' && line.level !== logLevelFilter.value) return false;
    if (!search) return true;
    return [line.message, line.source, line.file, line.level]
      .some((value) => value.toLowerCase().includes(search));
  });
});

function formatLogTime(timestamp: string): string {
  const value = dayjs(timestamp);
  return value.isValid() ? value.format('HH:mm:ss') : '—';
}

async function loadPersistedLogs(runId: string): Promise<boolean> {
  try {
    const text = await api.get<string>(`/runs/${runId}/logs?file=${logFile.value}`, { parseAs: 'text' });
    persistedLogLines.value = parseLogText(text, logFile.value);
    logError.value = null;
    return true;
  } catch (error) {
    persistedLogLines.value = [];
    logError.value = error as ApiError;
    return false;
  } finally {
    lastLogRefresh.value = new Date().toISOString();
  }
}

async function fallbackToPersistedLogs() {
  logNotice.value = FALLBACK_NOTICE;
  logLiveRegionText.value = FALLBACK_NOTICE;
  await loadPersistedLogs(currentRunId.value);
}

async function reopenLogStream() {
  logStreamHandle.value?.close();
  logStreamHandle.value = null;
  logNotice.value = '';
  logError.value = null;
  if (!currentRunId.value) return;
  await loadPersistedLogs(currentRunId.value);
  if (latestRun.value && !['running', 'paused'].includes(latestRun.value.status)) return;

  logLiveRegionText.value = 'Connecting to live logs.';
  logStreamHandle.value = useLogStream({
    runId: currentRunId.value,
    file: logFile.value,
    bufferLimit: 500,
    onLine: (line) => {
      if (logPaused.value) pausedLogLines.value.push(line);
      if (!logPaused.value && !showFollowBtn.value) nextTick(scrollToBottom);
      logLiveRegionText.value = `Live logs connected. Latest ${line.level} event received.`;
    },
    onUnavailable: () => { void fallbackToPersistedLogs(); },
  });
}

function toggleLogPause() {
  logPaused.value = !logPaused.value;
  if (!logPaused.value && pausedLogLines.value.length) {
    pausedLogLines.value = [];
    nextTick(scrollToBottom);
  }
}

function onLogScroll() {
  if (!logPaneRef.value) return;
  const el = logPaneRef.value;
  showFollowBtn.value = el.scrollHeight - el.scrollTop - el.clientHeight >= 40;
}

function scrollToBottom() {
  if (!logPaneRef.value) return;
  logPaneRef.value.scrollTop = logPaneRef.value.scrollHeight;
  showFollowBtn.value = false;
}

function serializeLogLines(lines: LogLine[]): string {
  return lines.map((line) => redactDiagnosticText(
    `${line.timestamp} - ${line.level.toUpperCase()} - [${line.source}] - ${line.message}`,
  )).join('\n');
}

function downloadLogs() {
  const blob = new Blob([serializeLogLines(filteredLogLines.value)], { type: 'text/plain;charset=utf-8' });
  const url = URL.createObjectURL(blob);
  const link = document.createElement('a');
  link.href = url;
  link.download = `run-${currentRunId.value}-${logFile.value}.log`;
  link.click();
  URL.revokeObjectURL(url);
}

async function copyLogDiagnostics() {
  await navigator.clipboard.writeText(JSON.stringify(redactDiagnosticValue({
    taskId: taskId.value,
    runId: currentRunId.value,
    phase: currentPhase.value,
    file: `${logFile.value}.log`,
    connectionState: sseState.value,
    lastEvent: logStreamHandle.value?.lastEventAt.value ?? null,
    lastRefresh: lastLogRefresh.value,
    code: logError.value?.code,
    message: logError.value?.message,
    status: logError.value?.status,
    requestId: logError.value?.requestId,
  }), null, 2));
  ElMessage.success('Diagnostics copied');
}

/* ---------- Alerts tab ---------- */
const alerts = ref<Alert[]>([]);

async function loadAlerts() {
  try {
    const res = await api.get<{ items: ApiAlert[] }>(`/alerts?taskId=${taskId.value}`);
    alerts.value = (res.items ?? []).map(mapApiAlert);
  } catch { /* ignore */ }
}

/* ---------- Monitor tab ---------- */
const monitorRange = ref<'1h' | '6h' | '24h'>('1h');
const monitorRanges = [
  { value: '1h' as const, label: '1h' },
  { value: '6h' as const, label: '6h' },
  { value: '24h' as const, label: '24h' },
];

const MONITOR_METRIC_NAMES = computed(() => {
  const base = [
    'extractor_rps_avg',
    'sinker_rps_avg',
    'pipeline_queue_size',
    'sinker_rt_avg',
  ];
  if (currentPhase.value === 'cdc') {
    base.push('lag');
  }
  return base;
});

const monitorSeries = computed<MetricQueryResponse[]>(() => {
  const rangeMs = monitorRange.value === '1h' ? 3600_000 : monitorRange.value === '6h' ? 6 * 3600_000 : 24 * 3600_000;
  const cutoff = Date.now() - rangeMs;
  return MONITOR_METRIC_NAMES.value
    .filter(m => (metricsHistory.value[m]?.length ?? 0) > 0)
    .map(m => ({
      metric: m,
      data: (metricsHistory.value[m] ?? []).filter(p => p.ts >= cutoff),
    }));
});

function setMonitorRange(r: '1h' | '6h' | '24h') {
  monitorRange.value = r;
}

function monitorChartOption(ms: MetricQueryResponse) {
  const xs = ms.data.map((p) => dayjs(p.ts).format('HH:mm'));
  return {
    grid: { left: 36, right: 16, top: 18, bottom: 22 },
    tooltip: { trigger: 'axis' as const },
    xAxis: { type: 'category', data: xs, axisLine: AXIS_BASE.axisLine, axisLabel: { ...AXIS_BASE.axisLabel, fontSize: 10 }, axisTick: { show: false } },
    yAxis: { type: 'value', name: ms.metric, axisLine: { show: false }, axisLabel: AXIS_BASE.axisLabel, splitLine: AXIS_BASE.splitLine },
    series: [{
      name: ms.metric,
      type: 'line' as const,
      data: ms.data.map((p) => p.value),
      smooth: true,
      symbol: 'none',
      lineStyle: { width: 1.6, color: BRAND_PALETTE[0] },
      areaStyle: { color: BRAND_PALETTE[0], opacity: 0.08 },
    }],
  };
}

/* ---------- History tab ---------- */
const historyRuns = ref<Run[]>([]);
const historyTotal = ref(0);
const historyPage = ref(1);
const historyPageSize = 25;
const historyLoading = ref(false);

async function loadHistory() {
  historyLoading.value = true;
  try {
    const res = await api.get<{ items: Run[]; total: number }>(`/tasks/${taskId.value}/runs?page=${historyPage.value}&size=${historyPageSize}`);
    historyRuns.value = res.items ?? [];
    historyTotal.value = res.total ?? 0;
  } catch { /* ignore */ }
  finally { historyLoading.value = false; }
}

function formatPosition(pos: RunPosition): string {
  if (pos.kind === 'binlog') return `${pos.file}:${pos.pos}${pos.gtid ? ` gtid=${pos.gtid}` : ''}`;
  if (pos.kind === 'lsn') return `LSN ${pos.lsn}${pos.slot ? ` slot=${pos.slot}` : ''}`;
  if (pos.kind === 'scn') return `SCN ${pos.scn}`;
  if (pos.kind === 'resume_token') return `token=${pos.token}`;
  if (pos.kind === 'unknown') return pos.raw ?? '—';
  return JSON.stringify(pos);
}

/* archived logs dialog */
const archivedDialogVisible = ref(false);
const archivedLines = ref<LogLine[]>([]);
const archivedLoading = ref(false);

async function viewArchivedLogs(run: Run) {
  archivedDialogVisible.value = true;
  archivedLoading.value = true;
  try {
    const text = await api.get<string>(`/runs/${run.id}/logs?file=${logFile.value}`, { parseAs: 'text' });
    archivedLines.value = parseLogText(text, logFile.value);
  } catch { archivedLines.value = []; }
  finally { archivedLoading.value = false; }
}

function parseLogText(text: string, file: string): LogLine[] {
  return text
    .split('\n')
    .filter((line) => line.trim().length > 0)
    .map((line) => parseLogLine(line, file));
}

function parseLogLine(line: string, file: string): LogLine {
  const match = line.match(/^(\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}(?:\.\d+)?) - (DEBUG|INFO|WARN|ERROR) - (?:\[([^\]]+)\] - )?(.*)$/);
  if (!match) {
    return {
      timestamp: new Date().toISOString(),
      level: 'info',
      source: 'dt-main',
      file: `${file}.log`,
      message: line,
    };
  }
  return {
    timestamp: `${match[1].replace(' ', 'T')}Z`,
    level: match[2].toLowerCase() as LogLine['level'],
    source: match[3] ?? 'dt-main',
    file: `${file}.log`,
    message: match[4],
  };
}

/* ---------- lifecycle ---------- */
let pollId: ReturnType<typeof setInterval> | null = null;
const POLL_INTERVAL_MS = 5_000;

onMounted(async () => {
  await loadDetail();
  if (latestRun.value?.status === 'failed') {
    await loadPersistedLogs(latestRun.value.id);
  }
  loadAlerts();
  loadHistory();

  if (activeTab.value === 'logs' && currentRunId.value) reopenLogStream();
  if (activeTab.value === 'objects') loadObjects();

  pollId = setInterval(() => {
    if (isVisible.value) {
      loadDetail();
      if (activeTab.value === 'more') loadAlerts();
    }
  }, POLL_INTERVAL_MS);
});

onUnmounted(() => {
  if (pollId) clearInterval(pollId);
  logStreamHandle.value?.close();
});

/* ---------- navigation ---------- */
function onBack() {
  router.push(backToListPath());
}
</script>

<style scoped>
.detail {
  display: flex;
  flex-direction: column;
  min-height: 100%;
}
.detail__diagnostic {
  margin: 24px;
  padding: 20px;
}
.detail__diagnostic h2 { margin-top: 0; }
.detail__diagnostic dl {
  display: grid;
  grid-template-columns: 120px 1fr;
  gap: 8px 16px;
}
.detail__diagnostic dd { margin: 0; font-family: var(--font-mono); overflow-wrap: anywhere; }
.detail__diagnostic-actions { display: flex; gap: 8px; margin-top: 16px; }
.detail__header {
  background: var(--color-surface);
  border-bottom: 1px solid var(--color-border);
  padding: 12px 24px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  flex-wrap: wrap;
}
.detail__identity {
  display: flex;
  align-items: flex-start;
  gap: 16px;
  min-width: 0;
}
.detail__identity-main { display: flex; flex-direction: column; gap: 6px; min-width: 0; }
.detail__eyebrow,
.detail__endpoint-role {
  color: var(--color-ink-subtle);
  font-size: 11px;
  font-weight: 600;
  letter-spacing: 0.08em;
  text-transform: uppercase;
}
.detail__title-row,
.detail__runtime-state,
.detail__actions,
.detail__actions-primary,
.detail__actions-secondary,
.detail__actions-destructive {
  display: flex;
  align-items: center;
  gap: 8px;
}
.detail__title-row { min-width: 0; flex-wrap: nowrap; }
.detail__title-row > :first-child { min-width: 0; }
.detail__runtime-state { color: var(--color-ink-muted); font-size: 13px; flex-wrap: wrap; }
.detail__task-id { color: var(--color-ink-subtle); font-family: var(--font-mono); font-size: 12px; }
.detail__actions { flex-wrap: wrap; justify-content: flex-end; }
.detail__actions-destructive { border-left: 1px solid var(--color-border); padding-left: 12px; }
.detail__title {
  margin: 0;
  min-width: 0;
  max-width: min(42vw, 560px);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: var(--text-xl);
  font-weight: 600;
}
.detail__header-right {
  display: inline-flex;
  gap: 8px;
}
.detail__body {
  display: flex;
  flex-direction: column;
  gap: 16px;
}
.detail__flow {
  padding: 16px 20px;
}
.detail__kpi-row {
  display: grid;
  grid-template-columns: repeat(4, 1fr);
  gap: 12px;
}
@media (max-width: 800px) { .detail__kpi-row { grid-template-columns: repeat(2, 1fr); } }
.detail__charts {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: 12px;
  padding: 0 0 0 0;
}
@media (max-width: 1200px) { .detail__charts { grid-template-columns: repeat(2, 1fr); } }
@media (max-width: 800px) { .detail__charts { grid-template-columns: 1fr; } }
.detail__chart {
  height: 100%;
}
.detail__tabs {
  padding: 8px 20px 20px;
}
.detail__topology { padding: 12px 0 20px; }
.detail__topology h2,
.detail__config h2,
.detail__more-section h2 { margin: 0 0 12px; font-size: var(--text-lg); }
.detail__topology-flow {
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto minmax(0, 1fr);
  align-items: center;
  gap: 20px;
}
.detail__endpoint {
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  padding: 16px;
  min-width: 0;
}
.detail__endpoint .engine-tag { margin: 8px 0 14px; }
.detail__endpoint dl {
  display: grid;
  grid-template-columns: 80px minmax(0, 1fr);
  gap: 8px 12px;
  margin: 0;
}
.detail__endpoint dt { color: var(--color-ink-subtle); font-size: 12px; }
.detail__endpoint dd { margin: 0; overflow-wrap: anywhere; font-family: var(--font-mono); font-size: 12px; }
.detail__topology-arrow { color: var(--color-ink-muted); display: flex; align-items: center; font-size: 24px; }
.detail__more-section { padding-top: 12px; }
.detail__config dl {
  display: grid;
  grid-template-columns: 180px 1fr;
  gap: 12px 24px;
  padding: 12px 0;
  margin: 0;
}
.detail__config dt { color: var(--color-ink-subtle); font-size: 13px; }
.detail__config dd { margin: 0; color: var(--color-ink); font-size: 13px; font-family: var(--font-mono); }
@media (max-width: 800px) {
  .detail__header { align-items: stretch; padding: 12px 16px; }
  .detail__identity { width: 100%; }
  .detail__title { max-width: 100%; }
  .detail__title-row { flex-wrap: wrap; }
  .detail__actions { width: 100%; justify-content: flex-start; }
  .detail__actions-destructive { margin-left: auto; }
  .detail__topology-flow { grid-template-columns: 1fr; }
  .detail__topology-arrow { justify-content: center; }
  .detail__topology-arrow svg { transform: rotate(90deg); }
  .detail__config dl { grid-template-columns: 1fr; gap: 4px; }
}
.detail__mono { font-family: var(--font-mono); font-size: 12px; }
.detail__muted { color: var(--color-ink-subtle); font-size: 12px; }
.detail__position { font-size: 11px; word-break: break-all; }

/* logs */
.detail__log-context {
  display: flex;
  flex-wrap: wrap;
  gap: 8px 20px;
  margin: 12px 0;
  color: var(--color-ink-muted);
  font-size: 12px;
}
.detail__log-context strong { color: var(--color-ink); font-family: var(--font-mono); }
.detail__log-toolbar {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 8px 0;
  gap: 12px;
  flex-wrap: wrap;
}
.detail__log-toolbar-left {
  display: inline-flex;
  gap: 8px;
  align-items: center;
}
.detail__log-toolbar-right {
  display: inline-flex;
  gap: 8px;
  align-items: center;
}
.detail__log-file-select { width: 140px; }
.detail__log-level-select { width: 100px; }
.detail__log-search { width: min(260px, 45vw); }
.detail__log-diagnostic { margin: 12px 0; }
.detail__sr-only {
  position: absolute;
  width: 1px;
  height: 1px;
  padding: 0;
  margin: -1px;
  overflow: hidden;
  clip: rect(0, 0, 0, 0);
  white-space: nowrap;
  border: 0;
}
.detail__log-status-pill--connecting { background: #EFF6FF; color: #1D4ED8; }
.detail__log-status-pill {
  display: inline-flex;
  align-items: center;
  padding: 2px 10px;
  border-radius: 999px;
  font-size: 12px;
  font-weight: 500;
}
.detail__log-status-pill--connected { background: #ECFDF5; color: #0F766E; }
.detail__log-status-pill--reconnecting { background: #FEF3C7; color: #92400E; }
.detail__log-status-pill--disconnected { background: #FEF2F2; color: #991B1B; }
.detail__log-view {
  background: #0F172A;
  color: #CBD5E1;
  border-radius: var(--radius-md);
  padding: 8px 12px;
  font-family: var(--font-mono);
  font-size: 12px;
  max-height: 480px;
  overflow: auto;
  position: relative;
}
.detail__log-line {
  display: grid;
  grid-template-columns: 80px 60px 1fr;
  gap: 10px;
  padding: 3px 0;
}
.detail__log-line--warn .detail__log-level { color: #FBBF24; }
.detail__log-line--error .detail__log-level { color: #F87171; }
.detail__log-line--info .detail__log-level { color: #67E8F9; }
.detail__log-line--debug .detail__log-level { color: #94A3B8; }
.detail__log-time { color: #64748B; }
.detail__log-source { color: #94A3B8; }
.detail__log-msg { color: #E2E8F0; overflow-wrap: anywhere; }
.detail__log-follow {
  position: sticky;
  bottom: 8px;
  text-align: center;
  z-index: 10;
  margin-top: 4px;
}

/* monitor */
.detail__monitor-toolbar {
  display: flex;
  gap: 12px;
  align-items: center;
  padding: 8px 0;
}
.detail__monitor-charts {
  display: grid;
  grid-template-columns: repeat(2, 1fr);
  gap: 12px;
}
@media (max-width: 800px) { .detail__monitor-charts { grid-template-columns: 1fr; } }
.detail__monitor-empty {
  padding: 40px 0;
}

/* history */
.detail__history-footer {
  display: flex;
  justify-content: center;
  padding-top: 12px;
}

/* archived logs */
.detail__archived-log-view {
  background: #0F172A;
  color: #CBD5E1;
  border-radius: var(--radius-md);
  padding: 8px 12px;
  font-family: var(--font-mono);
  font-size: 12px;
  max-height: 480px;
  overflow: auto;
}

/* editor */
.detail__editor {
  display: flex;
  flex-direction: column;
  gap: 12px;
  padding: 0 4px;
}
.detail__editor-form {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.detail__editor-form label {
  font-size: 13px;
  color: var(--color-ink-muted);
  margin-top: 6px;
}
.detail__loading {
  padding: 40px 24px;
}
.detail__kpi-progress-card {
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.detail__kpi-progress-card .kpi__value {
  padding-top: 4px;
}
.detail__progress-counts {
  font-size: 12px;
  color: var(--color-ink-subtle);
  font-family: var(--font-mono);
}
</style>
