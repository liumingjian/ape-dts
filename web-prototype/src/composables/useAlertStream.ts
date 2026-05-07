import { getCurrentInstance, onBeforeUnmount, ref, type Ref } from 'vue';
import { registerSse, unregisterSse } from '@/composables/useSseRegistry';

export interface AlertEvent {
  id: string;
  taskId: string;
  level: 'critical' | 'major' | 'minor' | 'info';
  message: string;
  ts: number;
}

export interface AlertStreamHandle {
  close: () => void;
  isOpen: () => boolean;
  reconnectCount: () => number;
  events: Ref<AlertEvent[]>;
}

export interface AlertStreamOptions {
  url: string;
  onAlert?: (e: AlertEvent) => void;
  onError?: (err: unknown) => void;
  reconnectDelayMs?: number;
  bufferLimit?: number;
}

export function useAlertStream(opts: AlertStreamOptions): AlertStreamHandle {
  const events = ref<AlertEvent[]>([]) as Ref<AlertEvent[]>;
  const limit = opts.bufferLimit ?? 200;

  let source: EventSource | null = registerSse(new EventSource(opts.url));
  let reconnects = 0;
  let closed = false;

  const wire = () => {
    if (!source) return;
    source.onmessage = (ev: MessageEvent) => {
      try {
        const parsed = JSON.parse(ev.data) as AlertEvent;
        events.value = [...events.value.slice(-(limit - 1)), parsed];
        opts.onAlert?.(parsed);
      } catch (err) {
        opts.onError?.(err);
      }
    };
    source.onerror = (err) => {
      opts.onError?.(err);
      if (closed) return;
      reconnects += 1;
      source?.close();
      unregisterSse(source!);
      source = registerSse(new EventSource(opts.url));
      wire();
    };
  };
  wire();

  const close = () => {
    closed = true;
    if (source) {
      source.close();
      unregisterSse(source);
    }
    source = null;
  };

  if (getCurrentInstance()) onBeforeUnmount(close);

  return {
    close,
    isOpen: () => source !== null && !closed,
    reconnectCount: () => reconnects,
    events,
  };
}
