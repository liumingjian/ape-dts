import { describe, expect, it } from 'vitest';
import {
  mapApiAlert,
  mapApiAlertRule,
  type ApiAlert,
  type ApiAlertRule,
  type AlertLevel,
} from '@/types/domain';

describe('API field mapping — camelCase to frontend types', () => {
  describe('mapApiAlert', () => {
    const base: ApiAlert = {
      id: 'alert-001',
      taskId: 'task-001',
      runId: 'run-001',
      ruleId: 'rule-001',
      metricName: 'extractor_rps_avg',
      operator: '>',
      threshold: 100,
      severity: 'critical',
      value: 150,
      status: 'firing',
      silenced: false,
      firedAt: '2025-01-15T10:30:00Z',
      recoveredAt: null,
      clearedAt: null,
      deliveredAt: null,
      clearedBy: null,
      lastError: null,
      createdAt: '2025-01-15T10:30:00Z',
    };

    it('maps backend camelCase fields to frontend Alert', () => {
      const alert = mapApiAlert(base);
      expect(alert.id).toBe('alert-001');
      expect(alert.taskId).toBe('task-001');
      expect(alert.level).toBe('critical');
      expect(alert.status).toBe('active');
      expect(alert.firstAt).toBe('2025-01-15T10:30:00Z');
      expect(alert.source).toBe('extractor_rps_avg');
    });

    it('maps severity to AlertLevel', () => {
      const levels: [string, AlertLevel][] = [
        ['critical', 'critical'],
        ['major', 'major'],
        ['minor', 'minor'],
        ['info', 'info'],
      ];
      for (const [sev, expected] of levels) {
        const alert = mapApiAlert({ ...base, severity: sev });
        expect(alert.level).toBe(expected);
      }
    });

    it('maps firing status to active', () => {
      const alert = mapApiAlert({ ...base, status: 'firing' });
      expect(alert.status).toBe('active');
    });

    it('maps cleared status directly', () => {
      const alert = mapApiAlert({ ...base, status: 'cleared' });
      expect(alert.status).toBe('cleared');
    });

    it('handles null taskId', () => {
      const alert = mapApiAlert({ ...base, taskId: null });
      expect(alert.taskId).toBe('');
    });

    it('uses lastAt from recoveredAt when present', () => {
      const alert = mapApiAlert({ ...base, recoveredAt: '2025-01-15T11:00:00Z' });
      expect(alert.lastAt).toBe('2025-01-15T11:00:00Z');
    });

    it('falls back to firedAt for lastAt when no recoveredAt', () => {
      const alert = mapApiAlert(base);
      expect(alert.lastAt).toBe('2025-01-15T10:30:00Z');
    });

    it('maps clearedAt when present', () => {
      const alert = mapApiAlert({ ...base, clearedAt: '2025-01-15T12:00:00Z' });
      expect(alert.clearedAt).toBe('2025-01-15T12:00:00Z');
    });

    it('leaves clearedAt undefined when null', () => {
      const alert = mapApiAlert(base);
      expect(alert.clearedAt).toBeUndefined();
    });
  });

  describe('mapApiAlertRule', () => {
    const base: ApiAlertRule = {
      id: 'rule-001',
      name: 'High RPS',
      metricName: 'extractor_rps_avg',
      operator: '>',
      threshold: 1000,
      recoveryThreshold: 800,
      severity: 'major',
      dwellSecs: 300,
      channelIds: ['ch-001'],
      enabled: true,
      resourceGroupId: 'rg-001',
      createdAt: '2025-01-15T10:00:00Z',
      updatedAt: '2025-01-15T10:00:00Z',
    };

    it('maps backend camelCase fields to frontend MetricRule', () => {
      const rule = mapApiAlertRule(base);
      expect(rule.id).toBe('rule-001');
      expect(rule.name).toBe('High RPS');
      expect(rule.metric).toBe('extractor_rps_avg');
      expect(rule.operator).toBe('>');
      expect(rule.threshold).toBe(1000);
    });

    it('maps enabled=true to status="enabled"', () => {
      const rule = mapApiAlertRule({ ...base, enabled: true });
      expect(rule.status).toBe('enabled');
    });

    it('maps enabled=false to status="disabled"', () => {
      const rule = mapApiAlertRule({ ...base, enabled: false });
      expect(rule.status).toBe('disabled');
    });

    it('maps severity to AlertLevel', () => {
      const rule = mapApiAlertRule({ ...base, severity: 'critical' });
      expect(rule.level).toBe('critical');
    });

    it('maps dwellSecs to periodMin (seconds → minutes)', () => {
      const rule = mapApiAlertRule({ ...base, dwellSecs: 300 });
      expect(rule.periodMin).toBe(5);
    });

    it('maps recoveryThreshold correctly', () => {
      const rule = mapApiAlertRule({ ...base, recoveryThreshold: 800 });
      expect(rule.recoveryThreshold).toBe(800);
    });

    it('falls back to threshold when recoveryThreshold is undefined', () => {
      const rule = mapApiAlertRule({ ...base, recoveryThreshold: undefined });
      expect(rule.recoveryThreshold).toBe(1000);
    });

    it('maps metricName to metric', () => {
      const rule = mapApiAlertRule({ ...base, metricName: 'pipeline_buffer_size_avg' });
      expect(rule.metric).toBe('pipeline_buffer_size_avg');
    });

    it('clamps dwellSecs < 60 to periodMin=1', () => {
      const rule = mapApiAlertRule({ ...base, dwellSecs: 30 });
      expect(rule.periodMin).toBe(1);
    });
  });
});
