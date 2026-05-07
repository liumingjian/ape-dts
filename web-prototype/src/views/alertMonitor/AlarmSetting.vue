<template>
  <div class="alarm-setting">
    <PageHeader :title="t('monitor.channel.title')" :subtitle="t('monitor.channel.subtitle')">
      <template #actions>
        <el-button @click="loadList">
          <template #icon><IconRefresh /></template>
          {{ t('common.refresh') }}
        </el-button>
        <el-button v-if="can('alarm.channel.manage')" type="primary" @click="openCreate">
          <template #icon><IconPlus /></template>
          {{ t('monitor.channel.create') }}
        </el-button>
      </template>
    </PageHeader>

    <div v-loading="loading" class="ape-dts-console-page alarm-setting__body">
      <article
        v-for="ch in channels"
        :key="ch.id"
        class="ape-dts-console-card alarm-setting__card"
      >
        <header class="alarm-setting__head">
          <div class="alarm-setting__title">
            <component :is="kindIcon(ch.kind)" class="alarm-setting__title-icon" />
            <h3>{{ ch.name }}</h3>
            <span class="alarm-setting__kind">{{ ch.kind === 'kafka' ? 'Kafka' : 'SNMP' }}</span>
          </div>
          <div class="alarm-setting__actions">
            <el-button v-if="can('alarm.channel.manage')" link type="primary" @click="openEdit(ch)">{{ t('common.edit') }}</el-button>
            <el-button v-if="can('alarm.channel.manage')" link @click="onTest(ch)">{{ t('monitor.channel.action.test') }}</el-button>
            <el-button v-if="can('alarm.channel.manage')" link type="danger" @click="confirmDelete(ch)">{{ t('common.delete') }}</el-button>
          </div>
        </header>

        <el-form label-width="120px" class="alarm-setting__form">
          <el-form-item :label="t('monitor.channel.field.enabled')">
            <el-switch :model-value="ch.enabled" :disabled="!can('alarm.channel.manage')" @change="(v: unknown) => onToggle(ch, v as boolean)" />
          </el-form-item>
          <el-form-item :label="t('monitor.channel.field.range')">
            <div class="alarm-setting__range">
              <el-input
                :model-value="`${formatTime(ch.startAt)} → ${formatTime(ch.endAt)}`"
                readonly
                style="width: 360px;"
              />
              <span class="alarm-setting__hint alarm-setting__hint--warn">
                {{ t('monitor.channel.field.rangeHint') }}
              </span>
            </div>
          </el-form-item>
          <el-form-item :label="t('monitor.channel.field.period')">
            <div class="alarm-setting__row">
              <el-input :model-value="String(ch.periodMin)" readonly style="width: 120px;" />
              <span class="alarm-setting__hint">{{ t('monitor.channel.field.periodHint') }}</span>
            </div>
          </el-form-item>

          <template v-if="ch.kind === 'kafka' && ch.kafka">
            <el-form-item :label="t('monitor.channel.field.brokers')">
              <el-input :model-value="ch.kafka.brokers" readonly />
            </el-form-item>
            <el-form-item :label="t('monitor.channel.field.topic')">
              <el-input :model-value="ch.kafka.topic" readonly />
            </el-form-item>
            <el-form-item :label="t('monitor.channel.field.distinguish')">
              <el-switch :model-value="ch.kafka.distinguishType" disabled />
            </el-form-item>
            <el-form-item :label="t('monitor.channel.field.ssl')">
              <el-switch :model-value="ch.kafka.ssl" disabled />
            </el-form-item>
          </template>
          <template v-else-if="ch.kind === 'snmp' && ch.snmp">
            <el-form-item :label="t('monitor.channel.field.snmp.agent')">
              <el-input :model-value="ch.snmp.agent" readonly />
            </el-form-item>
            <el-form-item :label="t('monitor.channel.field.snmp.community')">
              <el-input :model-value="ch.snmp.community" readonly />
            </el-form-item>
            <el-form-item :label="t('monitor.channel.field.snmp.version')">
              <el-input :model-value="ch.snmp.version" readonly />
            </el-form-item>
          </template>
        </el-form>
      </article>
    </div>

    <el-drawer
      v-model="drawerVisible"
      :title="editing?.id ? t('monitor.channel.edit') : t('monitor.channel.create')"
      size="540px"
      destroy-on-close
    >
      <el-form v-if="form" label-position="top" class="alarm-setting__edit">
        <el-form-item :label="t('monitor.channel.field.name')" required>
          <el-input v-model="form.name" />
        </el-form-item>
        <el-form-item :label="t('monitor.channel.field.enabled')">
          <el-switch v-model="form.enabled" />
        </el-form-item>
        <el-form-item :label="t('monitor.channel.field.range')">
          <el-date-picker
            v-model="rangeRaw"
            type="datetimerange"
            value-format="YYYY-MM-DDTHH:mm:ss"
            style="width: 100%;"
            @change="onRangeChange"
          />
          <small class="alarm-setting__hint alarm-setting__hint--warn">
            {{ t('monitor.channel.field.rangeHint') }}
          </small>
        </el-form-item>
        <el-form-item :label="t('monitor.channel.field.period')">
          <el-input-number v-model="form.periodMin" :min="1" :max="60" />
          <small class="alarm-setting__hint">{{ t('monitor.channel.field.periodHint') }}</small>
        </el-form-item>
        <el-form-item :label="t('monitor.channel.field.kind')">
          <el-radio-group v-model="form.kind" @change="onKindChange">
            <el-radio-button value="kafka">Kafka</el-radio-button>
            <el-radio-button value="snmp">SNMP</el-radio-button>
          </el-radio-group>
        </el-form-item>

        <template v-if="form.kind === 'kafka'">
          <el-form-item :label="t('monitor.channel.field.brokers')">
            <el-input v-model="form.kafka!.brokers" placeholder="host:9092,host2:9092" />
          </el-form-item>
          <el-form-item :label="t('monitor.channel.field.topic')">
            <el-input v-model="form.kafka!.topic" placeholder="ape-dts-alarm" />
          </el-form-item>
          <el-form-item :label="t('monitor.channel.field.distinguish')">
            <el-switch v-model="form.kafka!.distinguishType" />
          </el-form-item>
          <el-form-item :label="t('monitor.channel.field.ssl')">
            <el-switch v-model="form.kafka!.ssl" />
          </el-form-item>
        </template>
        <template v-else-if="form.kind === 'snmp'">
          <el-form-item :label="t('monitor.channel.field.snmp.agent')">
            <el-input v-model="form.snmp!.agent" placeholder="10.250.0.50:162" />
          </el-form-item>
          <el-form-item :label="t('monitor.channel.field.snmp.community')">
            <el-input v-model="form.snmp!.community" />
          </el-form-item>
          <el-form-item :label="t('monitor.channel.field.snmp.version')">
            <el-radio-group v-model="form.snmp!.version">
              <el-radio-button value="v1">v1</el-radio-button>
              <el-radio-button value="v2c">v2c</el-radio-button>
              <el-radio-button value="v3">v3</el-radio-button>
            </el-radio-group>
          </el-form-item>
        </template>
      </el-form>
      <template #footer>
        <el-button @click="onTestForm">{{ t('monitor.channel.action.test') }}</el-button>
        <el-button @click="drawerVisible = false">{{ t('common.cancel') }}</el-button>
        <el-button type="primary" @click="save">{{ t('common.save') }}</el-button>
      </template>
    </el-drawer>
  </div>
</template>

<script setup lang="ts">
import { onMounted, ref } from 'vue';
import { useI18n } from 'vue-i18n';
import { ElMessage, ElMessageBox } from 'element-plus';
import dayjs from 'dayjs';
import IconBrandKafka from '~icons/tabler/topology-star';
import IconNetwork from '~icons/tabler/network';
import PageHeader from '@/components/PageHeader.vue';
import { api } from '@/api/client';
import { useRbac } from '@/composables/useRbac';
import type { AlarmChannel } from '@/types/domain';

const { t } = useI18n();
const { can } = useRbac();

const channels = ref<AlarmChannel[]>([]);
const loading = ref(false);

async function loadList() {
  loading.value = true;
  try {
    const res = await api.get<{ items: AlarmChannel[] }>('/alarm_channels');
    channels.value = res.items;
  } finally {
    loading.value = false;
  }
}

const drawerVisible = ref(false);
const editing = ref<AlarmChannel | null>(null);
const form = ref<AlarmChannel | null>(null);
const rangeRaw = ref<[string, string] | null>(null);

function ensureKafka(c: AlarmChannel) {
  c.kafka ??= { brokers: '', topic: '', ssl: false, distinguishType: false };
}
function ensureSnmp(c: AlarmChannel) {
  c.snmp ??= { agent: '', community: 'public', version: 'v2c' };
}

function openCreate() {
  editing.value = null;
  form.value = {
    id: '',
    name: '',
    kind: 'kafka',
    enabled: true,
    startAt: new Date().toISOString(),
    endAt: dayjs().add(1, 'year').toISOString(),
    periodMin: 1,
    kafka: { brokers: '', topic: '', ssl: false, distinguishType: false },
  };
  rangeRaw.value = [form.value.startAt, form.value.endAt];
  drawerVisible.value = true;
}
function openEdit(ch: AlarmChannel) {
  editing.value = ch;
  form.value = JSON.parse(JSON.stringify(ch)) as AlarmChannel;
  if (form.value.kind === 'kafka') ensureKafka(form.value);
  if (form.value.kind === 'snmp') ensureSnmp(form.value);
  rangeRaw.value = [form.value.startAt, form.value.endAt];
  drawerVisible.value = true;
}
function onKindChange() {
  if (!form.value) return;
  if (form.value.kind === 'kafka') ensureKafka(form.value);
  if (form.value.kind === 'snmp') ensureSnmp(form.value);
}
function onRangeChange(v: [string, string] | null) {
  if (!form.value || !v) return;
  form.value.startAt = v[0];
  form.value.endAt = v[1];
}

async function save() {
  if (!form.value) return;
  if (editing.value) {
    await api.patch(`/alarm_channels/${editing.value.id}`, form.value);
    ElMessage.success(t('monitor.channel.toast.saved'));
  } else {
    await api.post('/alarm_channels', form.value);
    ElMessage.success(t('monitor.channel.toast.saved'));
  }
  drawerVisible.value = false;
  loadList();
}

async function onToggle(ch: AlarmChannel, v: boolean) {
  await api.patch(`/alarm_channels/${ch.id}`, { enabled: v });
  loadList();
}

async function onTest(ch: AlarmChannel) {
  const res = await api.post<{ ok: boolean; message: string }>(`/alarm_channels/${ch.id}/test`);
  if (res.ok) ElMessage.success(t('monitor.channel.toast.testOk') + '：' + res.message);
  else ElMessage.error(t('monitor.channel.toast.testFail') + '：' + res.message);
}
async function onTestForm() {
  if (!editing.value) {
    ElMessage.info('请先保存通道后再测试');
    return;
  }
  await onTest(editing.value);
}

function confirmDelete(ch: AlarmChannel) {
  ElMessageBox.confirm(`确定删除通道「${ch.name}」？`, t('common.delete'), { type: 'warning' })
    .then(async () => {
      await api.del(`/alarm_channels/${ch.id}`);
      ElMessage.success(t('monitor.channel.toast.deleted'));
      loadList();
    })
    .catch(() => {});
}

function formatTime(s: string) { return dayjs(s).format('YYYY-MM-DD HH:mm'); }
function kindIcon(kind: 'kafka' | 'snmp') { return kind === 'kafka' ? IconBrandKafka : IconNetwork; }

onMounted(loadList);
</script>

<style scoped>
.alarm-setting { display: flex; flex-direction: column; }
.alarm-setting__body { display: flex; flex-direction: column; gap: 16px; }
.alarm-setting__card { padding: 0; overflow: hidden; }
.alarm-setting__head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 14px 20px;
  border-bottom: 1px solid var(--color-border);
  background: linear-gradient(180deg, var(--color-surface), var(--color-surface-2) 200%);
}
.alarm-setting__title { display: flex; align-items: center; gap: 10px; }
.alarm-setting__title h3 { margin: 0; font-size: 14px; font-weight: 600; }
.alarm-setting__title-icon { width: 18px; height: 18px; color: var(--color-primary-700); }
.alarm-setting__kind {
  font-size: 11px; padding: 2px 8px; border-radius: 999px;
  background: var(--color-primary-50); color: var(--color-primary-700);
  border: 1px solid var(--color-primary-200);
}
.alarm-setting__actions { display: flex; gap: 6px; }
.alarm-setting__form { padding: 18px 20px 12px; }
.alarm-setting__row, .alarm-setting__range { display: flex; align-items: center; gap: 12px; flex-wrap: wrap; }
.alarm-setting__hint { color: var(--color-ink-subtle); font-size: 12px; }
.alarm-setting__hint--warn { color: var(--color-danger); }
.alarm-setting__edit { padding: 0 8px; }
</style>
