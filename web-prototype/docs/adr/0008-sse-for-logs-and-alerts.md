# 0008 — Polling-first with SSE for logs and alerts (no WebSocket)

The console fetches list/detail data via plain HTTP polling (5–10s for lists, 3s for run-detail KPIs). Two streams that demand low-latency push — task **Log Stream** tail and live **Alert** firing — use Server-Sent Events. We deliberately reject WebSocket: SSE is one-way (matches both use cases), reuses the cookie session from ADR-0003 without any extra auth, has built-in browser auto-reconnect, and traverses HTTP reverse proxies without sticky-session gymnastics. WS would add bidirectional protocol surface, ping/pong handling, and proxy/load-balancer constraints we do not need at this scale.

## Considered options

- **A. All-HTTP polling** — rejected: log tailing UX is poor.
- **B. Polling + SSE (chosen)** — minimal complexity, fits one-way streams.
- **C. Full WebSocket** — rejected: complexity-without-benefit for on-prem console scale.
- **D. NATS / Redis Streams + WS gateway** — rejected: a message-bus dependency for ~tens of concurrent sessions is over-engineering.

## Consequences

- Frontend uses `EventSource` for `/api/runs/:id/logs/stream` and `/api/alerts/stream`; everything else is polled `fetch`.
- The orchestrator caps per-stream throughput and applies coalescing on the server side so a chatty Run does not flood the browser.
- If a customer needs >100 simultaneous viewers, we revisit and consider WS — not before.
