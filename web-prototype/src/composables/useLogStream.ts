import { getCurrentInstance, onBeforeUnmount, ref, type Ref } from 'vue';

export type LogLevel = 'debug' | 'info' | 'warn' | 'error';

export interface LogLine {
  ts: number;
  level: LogLevel;
  source: string;
  message: string;
}

export type SseState = 'connected' | 'reconnecting' | 'disconnected';

export interface LogStreamHandle {
  close: () => void;
  isOpen: () => boolean;
  reconnectCount: () => number;
  lines: Ref<LogLine[]>;
  state: Ref<SseState>;
}

export interface LogStreamOptions {
  /** Task ID (for task-level log endpoint) */
  taskId?: string;
  /** Run ID (for run-level log endpoint — preferred) */
  runId?: string;
  /** Log file name (default, position, monitor, etc.) */
  file?: string;
  level?: LogLevel;
  onLine?: (l: LogLine) => void;
  onError?: (err: unknown) => void;
  bufferLimit?: number;
  baseUrl?: string;
  /** Max reconnect attempts before giving up */
  maxReconnects?: number;
}

const MAX_RECONNECTS_DEFAULT = 10;
const RECONNECT_DELAY_MS = 2000;

export function useLogStream(opts: LogStreamOptions): LogStreamHandle {
  const lines = ref<LogLine[]>([]) as Ref<LogLine[]>;
  const state = ref<SseState>('connected') as Ref<SseState>;
  const limit = opts.bufferLimit ?? 500;

  const params = new URLSearchParams();
  if (opts.file) params.set('file', opts.file);
  if (opts.level) params.set('level', opts.level);

  // Prefer run-level endpoint (/api/runs/:id/logs/stream) over task-level
  const baseUrl = opts.baseUrl ?? '/api';
  let streamPath: string;
  if (opts.runId) {
    streamPath = `/runs/${encodeURIComponent(opts.runId)}/logs/stream`;
  } else if (opts.taskId) {
    streamPath = `/tasks/${encodeURIComponent(opts.taskId)}/logs/stream`;
  } else {
    streamPath = '/tasks/unknown/logs/stream';
  }
  const url = `${baseUrl}${streamPath}${params.toString() ? `?${params.toString()}` : ''}`;

  let source: EventSource | null = new EventSource(url);
  let reconnects = 0;
  let closed = false;
  const maxReconnects = opts.maxReconnects ?? MAX_RECONNECTS_DEFAULT;

  const wire = () => {
    if (!source) return;
    state.value = 'connected';
    source.onmessage = (ev: MessageEvent) => {
      try {
        const parsed = JSON.parse(ev.data) as LogLine;
        lines.value = [...lines.value.slice(-(limit - 1)), parsed];
        opts.onLine?.(parsed);
      } catch (err) {
        opts.onError?.(err);
      }
    };
    source.onerror = () => {
      state.value = 'reconnecting';
      if (closed) return;
      reconnects += 1;
      if (reconnects > maxReconnects) {
        state.value = 'disconnected';
        source?.close();
        source = null;
        return;
      }
      source?.close();
      setTimeout(() => {
        if (closed) return;
        source = new EventSource(url);
        wire();
      }, RECONNECT_DELAY_MS);
    };
  };
  wire();

  const close = () => {
    closed = true;
    source?.close();
    source = null;
  };

  if (getCurrentInstance()) onBeforeUnmount(close);

  return { close, isOpen: () => source !== null && !closed, reconnectCount: () => reconnects, lines, state };
}
