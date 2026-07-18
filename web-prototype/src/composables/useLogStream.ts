import { getCurrentInstance, onBeforeUnmount, ref, type Ref } from 'vue';
import { registerSse, unregisterSse } from '@/composables/useSseRegistry';

export type LogLevel = 'debug' | 'info' | 'warn' | 'error';

export interface LogLine {
  timestamp: string;
  level: LogLevel;
  source: string;
  file: string;
  message: string;
}

export type SseState = 'connecting' | 'connected' | 'reconnecting' | 'disconnected';
export type LogStreamUnavailableReason = 'error' | 'no-event';

export interface LogStreamHandle {
  close: () => void;
  isOpen: () => boolean;
  reconnectCount: () => number;
  lines: Ref<LogLine[]>;
  state: Ref<SseState>;
  lastEventAt: Ref<number | null>;
}

export interface LogStreamOptions {
  taskId?: string;
  runId?: string;
  file?: string;
  level?: LogLevel;
  onLine?: (line: LogLine) => void;
  onError?: (error: unknown) => void;
  onUnavailable?: (reason: LogStreamUnavailableReason) => void;
  bufferLimit?: number;
  baseUrl?: string;
  maxReconnects?: number;
  confirmationTimeoutMs?: number;
}

const MAX_RECONNECTS_DEFAULT = 10;
const RECONNECT_DELAY_MS = 2000;
const CONFIRMATION_TIMEOUT_MS = 5000;

function isLogLine(value: unknown): value is LogLine {
  if (!value || typeof value !== 'object') return false;
  const line = value as Record<string, unknown>;
  return typeof line.timestamp === 'string'
    && ['debug', 'info', 'warn', 'error'].includes(String(line.level))
    && typeof line.source === 'string'
    && typeof line.file === 'string'
    && typeof line.message === 'string';
}

export function useLogStream(opts: LogStreamOptions): LogStreamHandle {
  const lines = ref<LogLine[]>([]) as Ref<LogLine[]>;
  const state = ref<SseState>('connecting') as Ref<SseState>;
  const lastEventAt = ref<number | null>(null);
  const limit = opts.bufferLimit ?? 500;

  const params = new URLSearchParams();
  if (opts.file) params.set('file', opts.file);
  if (opts.level) params.set('level', opts.level);

  const baseUrl = opts.baseUrl ?? '/api';
  const streamPath = opts.runId
    ? `/runs/${encodeURIComponent(opts.runId)}/logs/stream`
    : opts.taskId
      ? `/tasks/${encodeURIComponent(opts.taskId)}/logs/stream`
      : '/tasks/unknown/logs/stream';
  const url = `${baseUrl}${streamPath}${params.toString() ? `?${params.toString()}` : ''}`;

  let source: EventSource | null = null;
  let reconnects = 0;
  let closed = false;
  let transportOpen = false;
  let confirmationTimer: ReturnType<typeof setTimeout> | null = null;
  let reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  const maxReconnects = opts.maxReconnects ?? MAX_RECONNECTS_DEFAULT;

  const clearConfirmationTimer = () => {
    if (confirmationTimer) clearTimeout(confirmationTimer);
    confirmationTimer = null;
  };

  const detach = () => {
    if (!source) return;
    source.close();
    unregisterSse(source);
    source = null;
  };

  const connect = () => {
    if (closed) return;
    transportOpen = false;
    state.value = reconnects > 0 ? 'reconnecting' : 'connecting';
    source = registerSse(new EventSource(url));
    const currentSource = source;

    currentSource.onopen = () => {
      transportOpen = true;
      clearConfirmationTimer();
      confirmationTimer = setTimeout(() => {
        if (!closed && state.value !== 'connected') opts.onUnavailable?.('no-event');
      }, opts.confirmationTimeoutMs ?? CONFIRMATION_TIMEOUT_MS);
    };

    currentSource.addEventListener('log', (event) => {
      try {
        const parsed = JSON.parse((event as MessageEvent).data) as unknown;
        if (!isLogLine(parsed)) throw new Error('Invalid structured log event');
        if (!transportOpen) return;
        clearConfirmationTimer();
        state.value = 'connected';
        lastEventAt.value = Date.now();
        lines.value = [...lines.value.slice(-(limit - 1)), parsed];
        opts.onLine?.(parsed);
      } catch (error) {
        opts.onError?.(error);
      }
    });

    currentSource.onerror = () => {
      if (closed || currentSource !== source) return;
      clearConfirmationTimer();
      reconnects += 1;
      detach();
      if (reconnects > maxReconnects) {
        state.value = 'disconnected';
        opts.onUnavailable?.('error');
        return;
      }
      state.value = 'reconnecting';
      reconnectTimer = setTimeout(connect, RECONNECT_DELAY_MS);
    };
  };

  connect();

  const close = () => {
    closed = true;
    clearConfirmationTimer();
    if (reconnectTimer) clearTimeout(reconnectTimer);
    reconnectTimer = null;
    detach();
    state.value = 'disconnected';
  };

  if (getCurrentInstance()) onBeforeUnmount(close);

  return {
    close,
    isOpen: () => source !== null && !closed,
    reconnectCount: () => reconnects,
    lines,
    state,
    lastEventAt,
  };
}
