# 0002 — On-prem single-tenant deployment, no SaaS multi-tenancy

The console is shipped as private-deployment software. The prototype already encodes this assumption (License module with activation/expiry/sku, a System-Monitor host inventory page, an Operate-Log audit page), all of which are SaaS anti-patterns. We accept that posture: there is no organisation/workspace concept, no cross-tenant isolation, no billing. Multi-team installations colocate Tasks under **Resource Group**s within the same orchestrator instance.

## Consequences

- The **License** module stays and is enforced at Task-creation time (cap on concurrent Task definitions).
- Permissions live entirely inside the three-role model from ADR-0003; we never add an "org" axis.
- Customers wanting team isolation use multiple orchestrator deployments, not a multi-tenant feature.
- If a SaaS variant is needed later it will be a separate product surface, not a configuration switch.
