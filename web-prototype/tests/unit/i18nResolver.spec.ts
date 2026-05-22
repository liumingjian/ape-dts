import { describe, it, expect } from 'vitest';
import { i18n } from '@/locales';

const t = i18n.global.t;

describe('i18n nested-only message tree (post-normalize)', () => {
  it('resolves sidebar nav child keys (nav.tasks.*) to Chinese', () => {
    expect(t('nav.tasks.snapshot')).toBe('全量迁移');
    expect(t('nav.tasks.cdc')).toBe('增量同步');
    expect(t('nav.tasks.check')).toBe('数据校验');
    expect(t('nav.tasks.struct')).toBe('结构迁移');
  });

  it('resolves sidebar nav parent label via _label', () => {
    expect(t('nav.tasks._label')).toBe('任务管理');
    expect(t('nav.alerts._label')).toBe('告警管理');
    expect(t('nav.system._label')).toBe('系统管理');
    expect(t('nav.ops._label')).toBe('运维管理');
    expect(t('nav.alertMonitor._label')).toBe('告警监控');
  });

  it('resolves task list column / action / filter keys', () => {
    expect(t('taskList.col.name')).toBe('名称 / ID');
    expect(t('taskList.col.status')).toBe('状态');
    expect(t('taskList.action.export')).toBe('导出');
    expect(t('taskList.action.import')).toBe('批量导入任务');
    expect(t('taskList.filter.search')).toBe('请输入任务名称 / ID / 实例 IP');
  });

  it('resolves task status / action keys', () => {
    expect(t('task.status.running')).toBe('运行中');
    expect(t('task.action.view')).toBe('查看任务');
    expect(t('task.action.pause')).toBe('暂停');
  });

  it('resolves wizard step / source.section / mode keys (deepest path)', () => {
    expect(t('wizard.step.source')).toBe('实例来源');
    expect(t('wizard.source.section.source')).toBe('源数据库信息');
    expect(t('wizard.source.section.target')).toBe('目标数据库信息');
    expect(t('wizard.mode.snapshot.label')).toBe('全量');
  });

  it('returns the original key when no path matches', () => {
    const missing = '__definitely_missing_key__.nope';
    expect(t(missing)).toBe(missing);
  });

  it('resolves wizard basic-info form labels (regression: drsType / desc / *._label)', () => {
    expect(t('wizard.source.drsType._label')).toBe('实例形态');
    expect(t('wizard.source.desc._label')).toBe('描述');
    expect(t('wizard.source.subMode._label')).toBe('GaussDB 子模式');
    expect(t('wizard.objects.rate._label')).toBe('流速模式');
    expect(t('wizard.objects.fullType._label')).toBe('全量同步对象类型');
    expect(t('wizard.objects.conflict._label')).toBe('增量阶段冲突策略');
  });

  it('resolves wizard footer action keys (regression: prevents missing wizard.action.*)', () => {
    expect(t('wizard.action.back')).toBe('上一步');
    expect(t('wizard.action.next')).toBe('下一步');
    expect(t('wizard.action.testFirst')).toBe('请先通过连接测试');
    expect(t('wizard.action.precheckFirst')).toBe('请先完成预检查');
  });
});
