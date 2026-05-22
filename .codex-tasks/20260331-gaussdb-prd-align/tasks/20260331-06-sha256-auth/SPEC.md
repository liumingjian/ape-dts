# SHA256 Auth Support (BLOCKED)

## Background

GaussDB (HCS) can be configured to use Huawei non-standard **SHA256** authentication
(not SCRAM-SHA-256). Upstream `tokio-postgres` / `sqlx` do not support this handshake.

PRD requirement: `docs/agent-summary/gaussdb-prd.md` “认证方式”章节。

## Goal / Deliverables

- Implement SHA256 handshake support in the chosen `rust-postgres` fork (per PRD: `apecloud/rust-postgres`).
- Wire the fork into this repo so GaussDB connections work with SHA256.
- Add tests:
  - SHA256-only GaussDB instance can be connected.
  - Mixed scenario: one side MD5, the other side SHA256 (no regression).

## Status

- **BLOCKED**: awaiting a stable GaussDB environment that enforces SHA256 auth (and related access/credentials).

## Success Criteria

- `dt-tests` (or a dedicated minimal harness) can connect to SHA256-enabled GaussDB reliably.
- Existing PG and GaussDB(MD5) paths remain green.

