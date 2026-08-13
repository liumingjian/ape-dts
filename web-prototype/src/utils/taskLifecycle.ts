/**
 * Pause/resume gating, mirrored from the console backend contract (ADR 0004:
 * "Pause is a graceful stop with a resumable position").
 *
 * The rules encoded here are the ones the backend enforces, so the UI must not
 * offer a button the server will answer with 409:
 *
 * - **Pause** is only accepted for `snapshot` and `cdc` tasks, and not for the
 *   managed two-phase (`snapshot_and_cdc`) form — `check` and `struct` have no
 *   position to resume from, and the two-phase handover owns its own start
 *   marker. Rejected with `UNSUPPORTED_FOR_KIND`.
 * - **Pause** requires the active Run to be `running`; the Run then sits in
 *   `pausing` while the engine drains (~8s) before the supervisor writes the
 *   terminal `paused`.
 * - **Resume** requires the task's *latest* Run to be `paused`. A `failed` Run
 *   is not resumable — that path is `start`, not `resume`.
 *
 * See `dt-console-server/src/run_handlers.rs` (`pause_unsupported_reason`,
 * `pause_task`, `resume_task`).
 */
import type { ExtractType, RunStatus, TaskCategory, TaskStatus } from '@/types/domain';

/** The task fields the lifecycle gates read. `Task` satisfies this. */
export interface LifecycleTask {
  category: TaskCategory;
  extractType: ExtractType;
  status: TaskStatus;
}

/** Why pause is not on offer for this task, or `null` when it is. */
export type PauseUnsupportedReason = 'kind' | 'twoPhase' | null;

export function pauseUnsupportedReason(
  task: Pick<LifecycleTask, 'category' | 'extractType'>,
): PauseUnsupportedReason {
  if (task.category !== 'snapshot' && task.category !== 'cdc') return 'kind';
  if (task.extractType === 'snapshot_and_cdc') return 'twoPhase';
  return null;
}

/** Does this task kind have a resumable position at all? */
export function isPausableKind(
  task: Pick<LifecycleTask, 'category' | 'extractType'>,
): boolean {
  return pauseUnsupportedReason(task) === null;
}

/**
 * Can the pause button be offered right now?
 *
 * `runStatus` is the latest Run's status where the view has it (task detail);
 * views that only know the task (the list) omit it and fall back to the task
 * status, which the backend keeps in step with the Run except during the drain
 * window — see {@link displayStatus}.
 */
export function canPause(
  task: LifecycleTask,
  runStatus?: RunStatus | null,
): boolean {
  if (!isPausableKind(task)) return false;
  const effective = runStatus ?? task.status;
  return effective === 'running';
}

/**
 * Can the resume button be offered right now? Only a `paused` Run resumes;
 * `failed` and `stopped` start a fresh Run through `start`.
 */
export function canResume(
  task: Pick<LifecycleTask, 'status'>,
  runStatus?: RunStatus | null,
): boolean {
  return (runStatus ?? task.status) === 'paused';
}

/** Is the engine mid-drain — signalled, not yet confirmed paused? */
export function isPausing(runStatus?: RunStatus | string | null): boolean {
  return runStatus === 'pausing';
}

/**
 * The status to badge.
 *
 * `pausing` lives on the Run only: the `tasks.status` CHECK constraint has no
 * such value, so the task row still reads `running` for the ~8s the engine
 * spends draining. Views that load the latest Run surface the truer state.
 */
export function displayStatus(
  taskStatus: TaskStatus,
  runStatus?: RunStatus | null,
): TaskStatus {
  if (isPausing(runStatus)) return 'pausing';
  return taskStatus;
}
