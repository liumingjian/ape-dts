import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useLogStream, type LogLine } from '@/composables/useLogStream';

type Listener = (event: MessageEvent) => void;

class FakeEventSource {
  static CONNECTING = 0;
  static OPEN = 1;
  static CLOSED = 2;
  static instances: FakeEventSource[] = [];

  readonly url: string;
  readyState = FakeEventSource.CONNECTING;
  onopen: ((event: Event) => void) | null = null;
  onmessage: ((event: MessageEvent) => void) | null = null;
  onerror: ((event: Event) => void) | null = null;
  private listeners = new Map<string, Set<Listener>>();

  constructor(url: string) {
    this.url = url;
    FakeEventSource.instances.push(this);
  }

  addEventListener(type: string, listener: EventListenerOrEventListenerObject) {
    const callback = listener as Listener;
    const listeners = this.listeners.get(type) ?? new Set<Listener>();
    listeners.add(callback);
    this.listeners.set(type, listeners);
  }

  removeEventListener(type: string, listener: EventListenerOrEventListenerObject) {
    this.listeners.get(type)?.delete(listener as Listener);
  }

  dispatchEvent() {
    return false;
  }

  open() {
    this.readyState = FakeEventSource.OPEN;
    this.onopen?.(new Event('open'));
  }

  emit(type: string, data: unknown) {
    const event = new MessageEvent(type, { data: JSON.stringify(data) });
    for (const listener of this.listeners.get(type) ?? []) listener(event);
  }

  fail() {
    this.onerror?.(new Event('error'));
  }

  close() {
    this.readyState = FakeEventSource.CLOSED;
  }
}

beforeEach(() => {
  FakeEventSource.instances.length = 0;
  vi.useFakeTimers();
  // @ts-expect-error deterministic EventSource for the public composable seam
  globalThis.EventSource = FakeEventSource;
});

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe('useLogStream', () => {
  it('consumes named log events and confirms connection only after open plus delivery', () => {
    const onLine = vi.fn();
    const handle = useLogStream({ runId: 'run 17', file: 'default', onLine });
    const source = FakeEventSource.instances[0];
    const line: LogLine = {
      timestamp: '2026-07-18T12:00:00Z',
      level: 'INFO',
      source: 'dt-main',
      file: 'default.log',
      message: 'snapshot started',
    };

    expect(source.url).toBe('/api/runs/run%2017/logs/stream?file=default');
    expect(handle.state.value).toBe('connecting');
    source.open();
    expect(handle.state.value).toBe('connecting');

    source.emit('log', line);

    expect(handle.state.value).toBe('connected');
    expect(handle.lines.value).toEqual([line]);
    expect(handle.lastEventAt.value).not.toBeNull();
    expect(onLine).toHaveBeenCalledWith(line);
    handle.close();
  });

  it('does not treat a default message event as a log event', () => {
    const handle = useLogStream({ runId: 'run-17' });
    const source = FakeEventSource.instances[0];
    source.open();
    source.onmessage?.(new MessageEvent('message', { data: JSON.stringify({ message: 'wrong channel' }) }));
    expect(handle.lines.value).toEqual([]);
    expect(handle.state.value).toBe('connecting');
    handle.close();
  });

  it('reports no protocol delivery and transport exhaustion for persisted fallback', () => {
    const onUnavailable = vi.fn();
    const handle = useLogStream({
      runId: 'run-17',
      confirmationTimeoutMs: 1000,
      maxReconnects: 0,
      onUnavailable,
    });
    const source = FakeEventSource.instances[0];
    source.open();

    vi.advanceTimersByTime(1000);
    expect(onUnavailable).toHaveBeenCalledWith('no-event');

    source.fail();
    expect(handle.state.value).toBe('disconnected');
    expect(onUnavailable).toHaveBeenCalledWith('error');
    handle.close();
  });
});
