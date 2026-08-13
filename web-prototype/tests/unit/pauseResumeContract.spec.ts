import { describe, it, expect } from 'vitest';
import {
  canPause,
  canResume,
  displayStatus,
  isPausableKind,
  isPausing,
  pauseUnsupportedReason,
  type LifecycleTask,
} from '@/utils/taskLifecycle';
import zh from '@/locales/zh-CN.json';
import en from '@/locales/en-US.json';

function task(over: Partial<LifecycleTask> = {}): LifecycleTask {
  return {
    category: 'cdc',
    extractType: 'cdc',
    status: 'running',
    source: { engine: 'mysql' },
    ...over,
  };
}

describe('pause kind gate — mirrors pause_unsupported_reason in the backend', () => {
  it('offers pause for snapshot and cdc tasks', () => {
    expect(isPausableKind(task({ category: 'snapshot', extractType: 'snapshot' }))).toBe(true);
    expect(isPausableKind(task({ category: 'cdc', extractType: 'cdc' }))).toBe(true);
  });

  it('refuses check and struct — they have no resumable position', () => {
    expect(pauseUnsupportedReason(task({ category: 'check', extractType: 'scan' }))).toBe('kind');
    expect(pauseUnsupportedReason(task({ category: 'struct', extractType: 'struct' }))).toBe('kind');
  });

  it('refuses the managed two-phase form, whose handover owns the start position', () => {
    for (const engine of ['mysql', 'pg', 'gaussdb_pg', 'gaussdb_oracle', 'oracle']) {
      expect(
        pauseUnsupportedReason(
          task({ category: 'snapshot', extractType: 'snapshot_and_cdc', source: { engine } }),
        ),
        engine,
      ).toBe('twoPhase');
    }
  });

  it('allows snapshot_and_cdc on engines the two-phase orchestration does not manage', () => {
    // `is_two_phase_task` returns false for these, so the backend accepts the
    // pause and hiding the button would be the UI inventing a rule.
    for (const engine of ['mongo', 'redis', 'kafka', 'starrocks']) {
      expect(
        isPausableKind(
          task({ category: 'snapshot', extractType: 'snapshot_and_cdc', source: { engine } }),
        ),
        engine,
      ).toBe(true);
    }
  });

  it('hides pause on an unsupported kind even while it is running', () => {
    expect(canPause(task({ category: 'check', extractType: 'scan', status: 'running' }))).toBe(false);
  });
});

describe('pause availability', () => {
  it('needs a running task', () => {
    expect(canPause(task({ status: 'running' }))).toBe(true);
    for (const status of ['paused', 'stopped', 'failed', 'draft', 'ready', 'completed'] as const) {
      expect(canPause(task({ status }))).toBe(false);
    }
  });

  it('is withdrawn once the Run is already pausing — a second pause is a 409', () => {
    expect(canPause(task({ status: 'running' }), 'pausing')).toBe(false);
  });

  it('prefers the Run status over the stale task status during the drain window', () => {
    // The task row still reads `running`: `tasks.status` has no `pausing`.
    expect(canPause(task({ status: 'running' }), 'running')).toBe(true);
  });
});

describe('resume availability', () => {
  it('only a paused Run resumes', () => {
    expect(canResume(task({ status: 'paused' }))).toBe(true);
    expect(canResume(task({ status: 'paused' }), 'paused')).toBe(true);
  });

  it('a failed Run is not resumable — that path is start, not resume', () => {
    expect(canResume(task({ status: 'failed' }))).toBe(false);
    expect(canResume(task({ status: 'failed' }), 'failed')).toBe(false);
  });

  it('a pausing Run is not resumable yet — the exit code has not landed', () => {
    expect(canResume(task({ status: 'running' }), 'pausing')).toBe(false);
  });

  it('a stopped or running Run is not resumable', () => {
    expect(canResume(task({ status: 'stopped' }))).toBe(false);
    expect(canResume(task({ status: 'running' }))).toBe(false);
  });
});

describe('displayed status', () => {
  it('surfaces pausing from the Run while the task row still says running', () => {
    expect(displayStatus('running', 'pausing')).toBe('pausing');
    expect(isPausing('pausing')).toBe(true);
  });

  it('otherwise reports the task status unchanged', () => {
    expect(displayStatus('running', 'running')).toBe('running');
    expect(displayStatus('paused', 'paused')).toBe('paused');
    expect(displayStatus('failed', null)).toBe('failed');
  });
});

describe('i18n', () => {
  it('labels every Run status the backend can return', () => {
    const runStatuses = ['pending', 'running', 'pausing', 'paused', 'stopping', 'stopped', 'failed', 'orphaned'];
    for (const s of runStatuses) {
      expect((zh.task.status as Record<string, string>)[s], `zh ${s}`).toBeTruthy();
      expect((en.task.status as Record<string, string>)[s], `en ${s}`).toBeTruthy();
    }
  });

  it('keeps start copy free of resume language — start is not a resume', () => {
    expect(zh.taskList.toast.action.started).not.toContain('位点');
    expect(zh.taskList.toast.action.started).not.toContain('续传');
    expect(en.taskList.toast.action.started.toLowerCase()).not.toContain('resum');
  });

  it('says pause stops the engine and keeps the position, in both locales', () => {
    expect(zh.taskDetail.confirm.pause).toContain('位点');
    expect(zh.taskDetail.confirm.pause).toContain('续传');
    expect(zh.taskList.confirm.pause).toContain('位点');
    expect(en.taskDetail.confirm.pause.toLowerCase()).toContain('position');
    expect(en.taskDetail.confirm.pause.toLowerCase()).toContain('resumed');
  });

  it('reports a pause as in-flight rather than done', () => {
    expect(zh.taskDetail.toast.pausing).toContain('暂停中');
    expect(zh.taskList.toast.action.pausing).toContain('暂停');
    expect(en.taskList.toast.action.pausing.toLowerCase()).toContain('requested');
  });
});
