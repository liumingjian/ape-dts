/**
 * Alert Monitor (告警监控) — config center handlers.
 * Covers metric rules, system events, alarm channels, alarm templates.
 */
import { http } from 'msw';
import { pause, ok, notFound, parsePage, paginate, q } from './_shared';
import { db } from '../db';
import type { MetricRule, SysEvent, AlarmChannel, AlarmTemplate, ApiAlertRule } from '@/types/domain';
import { id, isoMinus } from '../fake';

function toApiAlertRule(rule: MetricRule): ApiAlertRule {
  return {
    id: rule.id,
    name: rule.name,
    metricName: rule.metric,
    operator: rule.operator,
    threshold: rule.threshold,
    recoveryThreshold: rule.recoveryThreshold,
    severity: rule.level,
    dwellSecs: rule.periodMin * 60,
    channelIds: [],
    enabled: rule.status === 'enabled',
    resourceGroupId: undefined,
    createdAt: new Date().toISOString(),
    updatedAt: new Date().toISOString(),
  };
}

export const alertMonitorHandlers = [
  /* ---- Metric rules (指标告警) ---- */
  http.get('/api/alert-monitor/metric-rules', async ({ request }) => {
    await pause();
    const url = new URL(request.url);
    const key = q(url, 'q')?.toLowerCase();
    const status = q(url, 'status');
    let items = [...db.metricRules];
    if (key) items = items.filter((r) => r.name.toLowerCase().includes(key) || r.metric.toLowerCase().includes(key));
    if (status) items = items.filter((r) => r.status === status);
    const { page, size } = parsePage(url);
    return ok(paginate(items, page, size));
  }),

  http.post('/api/alert-monitor/metric-rules', async ({ request }) => {
    await pause();
    const body = (await request.json().catch(() => ({}))) as Partial<MetricRule>;
    const rule: MetricRule = {
      id: id('rule'),
      name: body.name ?? '未命名规则',
      metric: body.metric ?? 'extractor_pushed_rps_avg',
      operator: body.operator ?? '>',
      threshold: body.threshold ?? 100,
      level: body.level ?? 'minor',
      status: body.status ?? 'enabled',
      periodMin: body.periodMin ?? 5,
      triggerCount: body.triggerCount ?? 1,
      recoveryThreshold: body.recoveryThreshold ?? 80,
      description: body.description ?? '',
    };
    db.metricRules.unshift(rule);
    return ok(rule);
  }),

  http.patch('/api/alert-monitor/metric-rules/:id', async ({ params, request }) => {
    await pause();
    const r = db.metricRules.find((x) => x.id === String(params.id));
    if (!r) return notFound();
    const patch = (await request.json().catch(() => ({}))) as Partial<MetricRule>;
    Object.assign(r, patch);
    return ok(r);
  }),

  http.delete('/api/alert-monitor/metric-rules/:id', async ({ params }) => {
    await pause();
    const idx = db.metricRules.findIndex((r) => r.id === String(params.id));
    if (idx < 0) return notFound();
    db.metricRules.splice(idx, 1);
    return ok({ ok: true });
  }),

  /* ---- System events (事件告警) ---- */
  http.get('/api/alert-monitor/events', async ({ request }) => {
    await pause();
    const url = new URL(request.url);
    const key = q(url, 'q')?.toLowerCase();
    const category = q(url, 'category');
    let items = [...db.events];
    if (category) items = items.filter((e) => e.category === category);
    if (key) items = items.filter((e) => e.name.toLowerCase().includes(key));
    const { page, size } = parsePage(url);
    return ok(paginate(items, page, size));
  }),

  http.patch('/api/alert-monitor/events/:id', async ({ params, request }) => {
    await pause();
    const e = db.events.find((x) => x.id === String(params.id));
    if (!e) return notFound();
    const patch = (await request.json().catch(() => ({}))) as Partial<SysEvent>;
    Object.assign(e, patch);
    return ok(e);
  }),

  /* ---- Alarm channels (告警通道) ---- */
  http.get('/api/alert-monitor/channels', async () => {
    await pause();
    return ok({ items: db.alarmChannels, total: db.alarmChannels.length, page: 1, size: 50 });
  }),

  http.post('/api/alert-monitor/channels', async ({ request }) => {
    await pause();
    const body = (await request.json().catch(() => ({}))) as Partial<AlarmChannel>;
    const ch: AlarmChannel = {
      id: id('chan'),
      name: body.name ?? '新通道',
      kind: body.kind ?? 'kafka',
      enabled: body.enabled ?? true,
      startAt: body.startAt ?? new Date().toISOString(),
      endAt: body.endAt ?? '2099-12-31T23:59:59.000Z',
      periodMin: body.periodMin ?? 1,
      kafka: body.kafka,
      snmp: body.snmp,
    };
    db.alarmChannels.unshift(ch);
    return ok(ch);
  }),

  http.patch('/api/alert-monitor/channels/:id', async ({ params, request }) => {
    await pause();
    const c = db.alarmChannels.find((x) => x.id === String(params.id));
    if (!c) return notFound();
    const patch = (await request.json().catch(() => ({}))) as Partial<AlarmChannel>;
    Object.assign(c, patch);
    return ok(c);
  }),

  http.delete('/api/alert-monitor/channels/:id', async ({ params }) => {
    await pause();
    const idx = db.alarmChannels.findIndex((c) => c.id === String(params.id));
    if (idx < 0) return notFound();
    db.alarmChannels.splice(idx, 1);
    return ok({ ok: true });
  }),

  http.post('/api/alert-monitor/channels/:id/test', async ({ params }) => {
    await pause(500, 1200);
    const c = db.alarmChannels.find((x) => x.id === String(params.id));
    if (!c) return notFound();
    const success = Math.random() > 0.15;
    return ok({ ok: success, message: success ? '连通性正常，测试消息已发送' : '无法到达：连接被拒绝' });
  }),

  /* ---- Alarm templates (告警模板) ---- */
  http.get('/api/alert-monitor/templates', async () => {
    await pause();
    return ok({ items: db.alarmTemplates, total: db.alarmTemplates.length, page: 1, size: 50 });
  }),

  http.post('/api/alert-monitor/templates', async ({ request }) => {
    await pause();
    const body = (await request.json().catch(() => ({}))) as Partial<AlarmTemplate>;
    const tpl: AlarmTemplate = {
      id: id('tpl'),
      name: body.name ?? '新模板',
      level: body.level ?? 'minor',
      subject: body.subject ?? '[ape-dts] {{level}} · {{taskName}}',
      body: body.body ?? '告警信息：{{message}}',
      updatedAt: new Date().toISOString(),
    };
    db.alarmTemplates.unshift(tpl);
    return ok(tpl);
  }),

  http.patch('/api/alert-monitor/templates/:id', async ({ params, request }) => {
    await pause();
    const t = db.alarmTemplates.find((x) => x.id === String(params.id));
    if (!t) return notFound();
    const patch = (await request.json().catch(() => ({}))) as Partial<AlarmTemplate>;
    Object.assign(t, patch, { updatedAt: new Date().toISOString() });
    return ok(t);
  }),

  http.delete('/api/alert-monitor/templates/:id', async ({ params }) => {
    await pause();
    const idx = db.alarmTemplates.findIndex((t) => t.id === String(params.id));
    if (idx < 0) return notFound();
    db.alarmTemplates.splice(idx, 1);
    return ok({ ok: true });
  }),

  /* ---- Monitor settings (监控设置) — single document with global knobs ---- */
  http.get('/api/alert-monitor/setting', async () => {
    await pause();
    return ok({
      globalEnabled: true,
      aggregationWindowMin: 5,
      defaultChannelId: db.alarmChannels[0]?.id,
      defaultTemplateId: db.alarmTemplates[0]?.id,
      silenceStart: '00:00',
      silenceEnd: '00:00',
      updatedAt: isoMinus(120),
    });
  }),

  http.patch('/api/alert-monitor/setting', async ({ request }) => {
    await pause();
    const patch = await request.json().catch(() => ({}));
    return ok({ ...(patch as object), updatedAt: new Date().toISOString() });
  }),

  /* ---- Backend-style API aliases (real backend paths) ---- */
  http.get('/api/alert_rules', async ({ request }) => {
    await pause();
    const url = new URL(request.url);
    const kind = q(url, 'kind');
    const key = q(url, 'q')?.toLowerCase();
    const status = q(url, 'status');
    const { page, size } = parsePage(url);
    if (kind === 'event') {
      const items = [...db.events].filter((r) => key ? r.name.toLowerCase().includes(key) : true);
      return ok(paginate(items, page, size));
    }
    let items = [...db.metricRules].filter((r) => key ? r.name.toLowerCase().includes(key) : true);
    if (status) items = items.filter((r) => r.status === status);
    const pageData = paginate(items, page, size);
    return ok({ ...pageData, items: pageData.items.map(toApiAlertRule) });
  }),
  http.post('/api/alert_rules', async ({ request }) => {
    await pause();
    const body = (await request.json().catch(() => ({}))) as Partial<MetricRule>;
    const rule: MetricRule = {
      id: id('rule'), name: body.name ?? '未命名规则', metric: body.metric ?? 'extractor_pushed_rps_avg',
      operator: body.operator ?? '>', threshold: body.threshold ?? 100, level: body.level ?? 'major',
      status: 'enabled', periodMin: body.periodMin ?? 5, triggerCount: body.triggerCount ?? 1,
      recoveryThreshold: body.recoveryThreshold ?? 80, description: body.description ?? '',
    };
    db.metricRules.push(rule);
    return ok(toApiAlertRule(rule));
  }),
  http.get('/api/alert_rules/:id', async ({ params }) => {
    await pause();
    const rule = db.metricRules.find((r) => r.id === String(params.id));
    return rule ? ok(toApiAlertRule(rule)) : notFound();
  }),
  http.patch('/api/alert_rules/:id', async ({ params, request }) => {
    await pause();
    const patch = (await request.json().catch(() => ({}))) as Partial<MetricRule> & { enabled?: boolean };
    const idx = db.metricRules.findIndex((r) => r.id === String(params.id));
    if (idx < 0) return notFound();
    const { enabled, ...metricPatch } = patch;
    const nextPatch: Partial<MetricRule> = { ...metricPatch };
    if (typeof enabled === 'boolean') {
      nextPatch.status = enabled ? 'enabled' : 'disabled';
    }
    Object.assign(db.metricRules[idx], nextPatch);
    return ok(toApiAlertRule(db.metricRules[idx]));
  }),
  http.delete('/api/alert_rules/:id', async ({ params }) => {
    await pause();
    const idx = db.metricRules.findIndex((r) => r.id === String(params.id));
    if (idx < 0) return notFound();
    db.metricRules.splice(idx, 1);
    return ok({});
  }),

  http.get('/api/alarm_channels', async () => {
    await pause();
    return ok({ items: db.alarmChannels });
  }),
  http.post('/api/alarm_channels', async ({ request }) => {
    await pause();
    const body = (await request.json().catch(() => ({}))) as Partial<AlarmChannel>;
    const ch: AlarmChannel = {
      id: id('ch'), name: body.name ?? '新建通道', kind: body.kind ?? 'kafka', enabled: true,
      startAt: new Date().toISOString(), endAt: isoMinus(-365 * 86400_000),
      periodMin: body.periodMin ?? 1, kafka: body.kafka, snmp: body.snmp,
    };
    db.alarmChannels.push(ch);
    return ok(ch);
  }),
  http.get('/api/alarm_channels/:id', async ({ params }) => {
    await pause();
    const ch = db.alarmChannels.find((c) => c.id === String(params.id));
    return ch ? ok(ch) : notFound();
  }),
  http.patch('/api/alarm_channels/:id', async ({ params, request }) => {
    await pause();
    const patch = (await request.json().catch(() => ({}))) as Partial<AlarmChannel>;
    const idx = db.alarmChannels.findIndex((c) => c.id === String(params.id));
    if (idx < 0) return notFound();
    Object.assign(db.alarmChannels[idx], patch);
    return ok(db.alarmChannels[idx]);
  }),
  http.delete('/api/alarm_channels/:id', async ({ params }) => {
    await pause();
    const idx = db.alarmChannels.findIndex((c) => c.id === String(params.id));
    if (idx < 0) return notFound();
    db.alarmChannels.splice(idx, 1);
    return ok({});
  }),
  http.post('/api/alarm_channels/:id/test', async () => {
    await pause();
    return ok({ ok: true, message: 'Test alert dispatched successfully' });
  }),

  http.get('/api/alarm_templates', async () => {
    await pause();
    return ok({ items: db.alarmTemplates });
  }),
  http.post('/api/alarm_templates', async ({ request }) => {
    await pause();
    const body = (await request.json().catch(() => ({}))) as Partial<AlarmTemplate>;
    const tpl: AlarmTemplate = {
      id: id('tpl'), name: body.name ?? '新建模板', level: body.level ?? 'major',
      subject: '', body: body.body ?? '', updatedAt: new Date().toISOString(),
    };
    db.alarmTemplates.push(tpl);
    return ok(tpl);
  }),
  http.get('/api/alarm_templates/:id', async ({ params }) => {
    await pause();
    const tpl = db.alarmTemplates.find((t) => t.id === String(params.id));
    return tpl ? ok(tpl) : notFound();
  }),
  http.patch('/api/alarm_templates/:id', async ({ params, request }) => {
    await pause();
    const patch = (await request.json().catch(() => ({}))) as Partial<AlarmTemplate>;
    const idx = db.alarmTemplates.findIndex((t) => t.id === String(params.id));
    if (idx < 0) return notFound();
    Object.assign(db.alarmTemplates[idx], patch);
    db.alarmTemplates[idx].updatedAt = new Date().toISOString();
    return ok(db.alarmTemplates[idx]);
  }),
  http.delete('/api/alarm_templates/:id', async ({ params }) => {
    await pause();
    const idx = db.alarmTemplates.findIndex((t) => t.id === String(params.id));
    if (idx < 0) return notFound();
    db.alarmTemplates.splice(idx, 1);
    return ok({});
  }),
  http.post('/api/alarm_templates/preview', async ({ request }) => {
    await pause();
    const body = (await request.json().catch(() => ({})) as { body?: string });
    const rendered = (body.body ?? '')
      .replace(/\{\{task_id\}\}/g, 'tsk_sample_001')
      .replace(/\{\{taskName\}\}/g, 'Sample Task')
      .replace(/\{\{metric\}\}/g, 'extractor_rps_avg')
      .replace(/\{\{value\}\}/g, '150')
      .replace(/\{\{threshold\}\}/g, '100')
      .replace(/\{\{level\}\}/g, 'critical')
      .replace(/\{\{source\}\}/g, 'rps')
      .replace(/\{\{message\}\}/g, 'RPS exceeded threshold')
      .replace(/\{\{firstAt\}\}/g, new Date().toISOString())
      .replace(/\{\{lastAt\}\}/g, new Date().toISOString());
    return ok({ rendered });
  }),
];
