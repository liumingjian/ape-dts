# Task Specification

## Task Shape

- **Shape**: `single-full`

## Goals

- 对 `DbType::GaussDBPg`（source + do_cdc）增强 precheck：
  - 候选优先选主：读取 `gaussdb_pg_candidate_hosts`，按顺序探测，必须找到 RW 主，否则直接 fail
  - 权限检查：`rolsuper`/`rolreplication` 必须至少一个为 true，否则 fail
  - slot active 检查：若配置含 `slot_name`，发现 active=true 直接 fail
  - HA 端口可达性：对选中的主库检查 `sql_port+1` TCP 可达，否则 fail

## Done-When

- [ ] 相关单测/集成用例通过（至少本地可复现验证一轮）

