# MySQL → GaussDBMySQL snapshot basic

目标：在已确认的“GaussDB MySQL-compatible database may use `postgres://` wire protocol”前提下，打通最小 `snapshot basic` 路径，并形成自动化与真实环境证据。

范围：

- 源端：本机 Docker MySQL 8（`ape-dts-mysql8`，3311）
- 目标端：GaussDB MySQL-compatible database，通过 `postgres://.../jyp_test_m` 接入
- 本 child 优先解决 `snapshot` 的最小数据写入与对账

约束：

- 不在本 child 内解决 `struct/check` 全量能力
- 若 `replace=true` 在 M 模式下不兼容，可在测试配置中先收敛为 `replace=false`，并把限制写入 PROGRESS

成功标准：

- `mysql_to_gaussdb_mysql::snapshot_tests::test::snapshot_basic_test` 可以编译并开始执行
- 至少一轮真实环境 `smoke/basic` 能完成源端写数、目标端落数与对账，或明确暴露下一个可复现 blocker
