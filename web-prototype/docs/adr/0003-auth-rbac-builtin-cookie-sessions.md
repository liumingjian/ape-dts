# 0003 — Built-in users with cookie sessions and three roles

For the on-prem MVP we use a built-in user store (table in the SQLite metadata DB, bcrypt-hashed passwords) with server-side cookie sessions, and three roles `admin | operator | viewer` matching the prototype's existing `User.role` type. Authorization is enforced both at the orchestrator API boundary (axum/actix middleware checking session → role → required permission) and in the UI (route `meta.roles` and per-button visibility). OIDC / LDAP integration is explicitly deferred to v2 as a pluggable adapter.

## Considered options

- **A. Built-in users + cookie sessions (chosen)** — simplest, easy revocation, no IDP dependency.
- **B. Built-in users + JWT** — rejected: stateless tokens make logout / role-change-takes-effect harder.
- **C. OIDC/SAML SSO only** — rejected: forces every customer to provision an IDP integration before first use.
- **D. Both built-in and OIDC from day one** — rejected: scope explosion for MVP.

## Consequences

- Sessions table lives in SQLite; rotation and idle-timeout configurable in `[security]` global params.
- The prototype's auth store is naive (any non-empty creds pass) — must be rewritten to talk to the real orchestrator.
- v2 OIDC adapter must keep the same role mapping; we do not add new roles to satisfy enterprise IDP claims.
