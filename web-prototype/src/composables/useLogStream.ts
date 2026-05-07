import { getCurrentInstance, onBeforeUnmount, ref, type Ref } from 'vue';

export type LogLevel = 'debug' | 'info' | 'warn' | 'error';

export interface LogLine {
  ts: number;
  level: LogLevel;
  source: string;
  message: string;
}

export interface LogStreamHandle {
  close: () => void;
  isOpen: () => boolean;
  reconnectCount: () => number;
  lines: Ref<LogLine[]>;
}

export interface LogStreamOptions {
  taskId: string;
  level?: LogLevel;
  onLine?: (l: LogLine) => void;
  onError?: (err: unknown) => void;
  bufferLimit?: number;
  baseUrl?: string;
}

export function useLogStream(opts: LogStreamOptions): LogStreamHandle {
  const lines = ref<LogLine[]>([]) as Ref<LogLine[]>;
  const limit = opts.bufferLimit ?? 500;

  const params = new URLSearchParams();
  if (opts.level) params.set('level', opts.level);
  const url = `${opts.baseUrl ?? '/api'}/tasks/${encodeURIComponent(opts.taskId)}/logs/stream${
    params.toString() ? `?${params.toString()}` : ''
  }`;

  let source: EventSource | null = new EventSource(url);
  let reconnects = 0;
  let closed = false;

  const wire = () => {
    if (!source) return;
    source.onmessage = (ev: MessageEvent) => {
      try {
        const parsed = JSON.parse(ev.data) as LogLine;
        lines.value = [...lines.value.slice(-(limit - 1)), parsed];
        opts.onLine?.(parsed);
      } catch (err) {
        opts.onError?.(err);
      }
    };
    source.onerror = (err) => {
      opts.onError?.(err);
      if (closed) return;
      reconnects += 1;
      source?.close();
      source = new EventSource(url);
      wire();
    };
  };
  wire();

  const close = () => {
    closed = true;
    source?.close();
    source = null;
  };

  if (getCurrentInstance()) onBeforeUnmount(close);

  return { close, isOpen: () => source !== null && !closed, reconnectCount: () => reconnects, lines };
}
