# Frontend Test Harness

Vitest + @vue/test-utils harness for the ape-dts Console SPA, scaffolded against
`web-prototype/docs/PRD-frontend-module.md` (Testing Decisions section).

## Run

```bash
pnpm install
pnpm test            # one-shot
pnpm test:watch      # watch mode
pnpm test:coverage   # with coverage (v8)
```

## Layout

```
tests/
  setup.ts                  Global Vitest setup: i18n, pinia, jsdom shims, MockEventSource
  helpers/
    mountWithProviders.ts   Wrapper around mount() that injects i18n + pinia
    fixtures.ts             Re-exports types from @/types/taskFixture; instance constants live here
  unit/                     Spec files
    wizardValidator.spec.ts → @/wizard/validator
    iniRenderer.spec.ts     → @/utils/iniRenderer
    authGuard.spec.ts       → @/auth/permissions
    alertStream.spec.ts     → @/composables/useAlertStream
    metricsSeries.spec.ts   → @/utils/metricsSeries
    apiClient.spec.ts       → @/api/client
    permissions.spec.ts     → @/auth/permissions
    wizardSteps.spec.ts     → @/composables/useWizardSteps
    i18nParity.spec.ts      → @/locales (zh-CN ↔ en-US)
    router.spec.ts          → router (deep-link redirect)
    taxonomy.spec.ts        → @/types/domain (legacyToCategory)
```

## High-leverage modules (PRD)

The PRD identifies five modules where one bug corrupts user trust. Each has a
spec file in `tests/unit/`. Three of the five are authored as Rust orchestrator
crates and only project into the frontend through composables / pure helpers;
the harness covers those projections, while the canonical Rust unit tests must
live in `dt-console-server` (out of scope for this task).

| Module           | Authoritative home  | Frontend projection here              |
|------------------|---------------------|---------------------------------------|
| WizardValidator  | SPA pure functions  | `src/wizard/validator.ts`             |
| IniRenderer      | Rust + SPA preview  | `src/utils/iniRenderer.ts` (preview)  |
| Authenticator    | Rust orchestrator   | `src/auth/permissions.ts` + `src/composables/useRbac.ts` |
| AlertEngine      | Rust orchestrator   | `src/composables/useAlertStream.ts`   |
| TimeSeriesStore  | Rust orchestrator   | `src/utils/metricsSeries.ts`          |

The historical `tests/stubs/` directory has been removed; specs now import
their subjects directly from `src/`. New `it.todo` lines remain the parking
lot for known-but-uncovered branches.

## Conventions

- Specs assert externally observable behaviour only — return values, emitted
  events, error shapes. No spying on internal helpers.
- Use real fixtures from `helpers/fixtures.ts`; do not inline ad-hoc Tasks in
  specs.
- `it.todo(...)` is the parking lot for known-but-uncovered branches; CI does
  not fail on todos but they remain visible.
- For `EventSource`/SSE behaviour, override `globalThis.EventSource` per spec
  with a deterministic fake; never hit the network.
- For component tests (none yet), use `mountWithProviders` so i18n + pinia are
  available and tests do not depend on global side effects.

## Out of scope

- Playwright e2e (PRD lists one happy-path; tracked separately).
- Rust unit tests for orchestrator modules (lives in `dt-console-server`).
- CI wiring (separate task; harness is authored to be CI-portable already).
