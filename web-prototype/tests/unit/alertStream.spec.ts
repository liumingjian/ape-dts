import { afterEach, describe, expect, it, vi } from 'vitest';
import { useAlertStream, type AlertEvent } from '@/composables/useAlertStream';

interface MockEs {
  url: string;
  readyState: number;
  close: ReturnType<typeof vi.fn>;
  onmessage: ((ev: MessageEvent) => void) | null;
  onerror: ((ev: Event) => void) | null;
  onopen: ((ev: Event) => void) | null;
}

const liveSources: MockEs[] = [];

class FakeEventSource implements MockEs {
  static CONNECTING = 0;
  static OPEN = 1;
  static CLOSED = 2;
  url: string;
  readyState = FakeEventSource.OPEN;
  close = vi.fn(() => {
    this.readyState = FakeEventSource.CLOSED;
  });
  onmessage: ((ev: MessageEvent) => void) | null = null;
  onerror: ((ev: Event) => void) | null = null;
  onopen: ((ev: Event) => void) | null = null;
  addEventListener() {}
  removeEventListener() {}
  dispatchEvent() {
    return false;
  }
  constructor(url: string) {
    this.url = url;
    liveSources.push(this);
  }
}

beforeEach(() => {
  liveSources.length = 0;
  // @ts-expect-error overriding for tests
  globalThis.EventSource = FakeEventSource;
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe('useAlertStream', () => {
  it('opens an EventSource against the given URL', () => {
    const handle = useAlertStream({ url: '/api/alerts/stream' });
    expect(liveSources).toHaveLength(1);
    expect(liveSources[0].url).toBe('/api/alerts/stream');
    expect(handle.isOpen()).toBe(true);
    handle.close();
  });

  it('parses and forwards alert events', () => {
    const onAlert = vi.fn();
    const handle = useAlertStream({ url: '/api/alerts/stream', onAlert });
    const payload: AlertEvent = {
      id: 'a1',
      taskId: 't1',
      level: 'major',
      message: 'lag exceeded',
      ts: 1700000000000,
    };
    liveSources[0].onmessage?.(new MessageEvent('message', { data: JSON.stringify(payload) }));
    expect(onAlert).toHaveBeenCalledWith(payload);
    handle.close();
  });

  it('reports parse errors via onError without crashing', () => {
    const onError = vi.fn();
    const handle = useAlertStream({ url: '/api/alerts/stream', onError });
    liveSources[0].onmessage?.(new MessageEvent('message', { data: 'not-json' }));
    expect(onError).toHaveBeenCalled();
    handle.close();
  });

  it('reconnects on transport error and increments counter', () => {
    const handle = useAlertStream({ url: '/api/alerts/stream' });
    liveSources[0].onerror?.(new Event('error'));
    expect(handle.reconnectCount()).toBe(1);
    expect(liveSources.length).toBeGreaterThanOrEqual(2);
    handle.close();
  });

  it('close() stops further reconnect attempts', () => {
    const handle = useAlertStream({ url: '/api/alerts/stream' });
    handle.close();
    expect(handle.isOpen()).toBe(false);
  });

  it('back-pressure: drops oldest events when subscriber is slow', () => {
    const handle = useAlertStream({ url: '/api/alerts/stream', bufferLimit: 3 });
    const fire = (id: string) => {
      const ev: AlertEvent = { id, taskId: 't', level: 'info', message: id, ts: 0 };
      liveSources[0].onmessage?.(new MessageEvent('message', { data: JSON.stringify(ev) }));
    };
    for (const id of ['a', 'b', 'c', 'd', 'e']) fire(id);
    expect(handle.events.value.map((e) => e.id)).toEqual(['c', 'd', 'e']);
    handle.close();
  });

  it.todo('topic teardown unsubscribes only the matching channel');
});
