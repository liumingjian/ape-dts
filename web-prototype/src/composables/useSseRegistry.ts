/**
 * Global registry of active SSE EventSource connections.
 * Used to close all streams when the user logs out,
 * preventing stale connections that bypass session invalidation.
 */

const activeStreams: Set<EventSource> = new Set();

/** Register an EventSource — returns the same reference for convenience. */
export function registerSse(es: EventSource): EventSource {
  activeStreams.add(es);
  es.addEventListener('error', () => {
    // Auto-remove when the connection closes permanently
    if (es.readyState === EventSource.CLOSED) {
      activeStreams.delete(es);
    }
  });
  return es;
}

/** Unregister an EventSource (e.g. on deliberate close). */
export function unregisterSse(es: EventSource): void {
  activeStreams.delete(es);
}

/** Close ALL registered SSE connections. Called on logout. */
export function closeAllSseStreams(): void {
  for (const es of activeStreams) {
    es.close();
  }
  activeStreams.clear();
}
