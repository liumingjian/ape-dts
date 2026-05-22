# Spec: Oracle <-> GaussDBOracle Bootstrap E2E Script

## Goal

提供一键回归脚本，将 `Oracle <-> GaussDBOracle（bootstrap）` 的 `snapshot/check/cdc/precheck` 回归用例收口为单个入口，用于日常开发与环境联调。

## Constraints / Assumptions

- 依赖 `dt-tests/tests/.env.local`（如存在）与 `dt-tests/tests/.env` 注入连接信息。
- Oracle XE 使用 `dt-tests/docker-compose.oracle_xe.yml`（镜像 `wnameless/oracle-xe-11g-r2:latest`）。

## Deliverables

- `scripts/e2e/oracle_gaussdboracle_bootstrap.sh`
- docs：`docs/agent-summary/gaussdb-e2e-test-plan.md` + `docs/agent-summary/gaussdb-progress-tracker.md` 入口更新

## Acceptance / Validation

```bash
bash scripts/e2e/oracle_gaussdboracle_bootstrap.sh
```

