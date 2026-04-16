# Spec

## Goal

更新本机 `dt-tests/docker-compose.oracle_xe.yml` 的初始化逻辑，补齐 LogMiner 所需权限与 supplemental logging（幂等）。

## Acceptance

- `docker compose -f dt-tests/docker-compose.oracle_xe.yml up -d` 可运行并完成初始化
- APE_DTS 具备 LogMiner 相关能力（能 `SELECT CURRENT_SCN FROM V$DATABASE`、查询 `V$LOGFILE`、查询 `V$LOGMNR_CONTENTS`，并可执行 `DBMS_LOGMNR.*`）
- DB supplemental logging（MIN + PRIMARY KEY）被启用

