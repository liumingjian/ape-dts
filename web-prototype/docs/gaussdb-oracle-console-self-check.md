# GaussDBOracle <-> Oracle Console 手动自检

这个自检用于确认 Console 前端可以手动驱动 GaussDB Oracle mode 与 Oracle 的全量 + 增量同步链路，并用独立验证命令证明两端数据最终一致。

原则：

- 脚本只负责环境、License 前置激活、测试数据初始化，以及可选的自动回归。
- Console 里的登录、创建任务、测试连接、预检查、启动、查看详情、停止任务必须手动完成。
- 增删改变更必须用源端 SQL 手动执行，验证必须用目标端 SQL 手动查询。
- 自检结论必须同时满足前端可见正常、源端与目标端数据一致。

## 1. Teardown

```bash
bash scripts/e2e/gaussdb_oracle_console_self_check.sh teardown
```

期望：终端显示 self-check Web/Console/运行进程是否被停止，并以 `teardown complete` 结束。重复执行安全。

## 2. Precheck

```bash
bash scripts/e2e/gaussdb_oracle_console_self_check.sh precheck
```

期望：终端最后显示 `precheck passed`。如果 GaussDB Oracle mode 实验环境不可用，终端会打印 `WARN` 提示；这表示外部实验环境当前不达标，但本地初始化仍可继续。如果 Docker、Oracle、本地依赖或端口失败，把终端输出贴回来即可。

关键前置条件：

- `dt-tests/tests/.env.local` 必须配置 `gaussdb_pg_candidate_hosts`，例如 `10.250.0.157:8000,10.250.0.223:8000`。
- `precheck` 会检查 Docker daemon，启动本地 Oracle Docker 容器，验证 Oracle `APE_DTS` 用户连通性、Oracle LogMiner CDC 前置条件，并探测 GaussDB Oracle mode 实验环境。
- GaussDB Oracle mode 检查不是只看端口连通；脚本会在候选节点上创建/写入/查询/删除临时探测表。全部候选节点都不可写时，`precheck` 会打印 `WARN` 和候选端点错误，用来提示实验环境当前不满足 CDC 自检条件。
- `init` 会把这个变量传给 Console 和 `dt-main` 子进程，让 GaussDBOracle CDC 自动选择可写主库。不要只依赖页面里填写的单个 GaussDB IP；该 IP 可能是只读节点，快照能读但增量无法消费。
- `init` 只操作本地环境：停止上一轮 self-check 进程、启动本地 Oracle XE Docker 容器、验证 Oracle CDC 前置条件、启动 Console 和 Web UI。GaussDB Oracle mode 是外部实验环境，`init` 不会创建、重启或阻塞等待它。
- 下文 Step 1 的连接信息按当前自检环境填写；如果 `dt-tests/tests/.env.local` 中的 `gaussdb_oracle_*` 或 `oracle_*` 值变化，手动填写值也要同步变化。

## 3. Init

```bash
bash scripts/e2e/gaussdb_oracle_console_self_check.sh init
```

这个步骤启动自检环境，并在隔离的 Console DB 中写入一条自检 License：

- Console 后端：`http://127.0.0.1:18082`
- Web UI：`http://localhost:5176`

脚本会先确保本地 Oracle XE 容器已经启动并完成初始化，再验证 Oracle CDC 前置条件；这些本地检查通过后才会启动 Console 和 Web UI。`init` 不会操作外部 GaussDB Oracle mode 实验环境。
脚本会以 `VITE_USE_MOCK=false` 启动 Web UI，确保 `/api/*` 走真实 Console 后端。
脚本还会显示 `self-check license activated in isolated Console DB`，表示本轮自检不会卡在 `/license` 页面。

打开：`http://localhost:5176/login`

可见检查：

- 页面是 ape-dts Console 登录页。
- 使用 `admin` / `admin123` 登录后进入控制台。
- 如果浏览器仍跳转到 `/license`，执行 `bash scripts/e2e/gaussdb_oracle_console_self_check.sh license` 后刷新页面。
- 进入 `http://localhost:5176/tasks/snapshot`，页面不是未登录状态。

注意：请使用 `localhost`，不要用 `127.0.0.1`。浏览器 cookie 按域名共享，不按端口隔离；使用 `localhost` 可以避开旧 Console 在 `127.0.0.1` 上留下的 session cookie。

## 4. Prepare Normal

```bash
bash scripts/e2e/gaussdb_oracle_console_self_check.sh test normal
```

这个步骤只准备数据，不创建 Console 任务：

- 重置 `gaussdb_oracle_to_oracle` 源端和目标端表。
- 重置 `oracle_to_gaussdb_oracle` 源端和目标端表。
- 在两个源端写入全量初始行：`id=1, tracer=snapshot, payload=before`。

期望：终端显示 `PASS: normal scenario prepared`。

## 5. 手动创建两条同步任务

必须先创建并启动以下两条任务，再执行批量 CDC mock 数据写入和 SQL 校验：

- `GaussDBOracle -> Oracle`
- `Oracle -> GaussDBOracle`

如果只创建其中一条任务，另一个方向的目标端必然不会变化，这不是同步结果失败。

### 5.1 手动创建 GaussDBOracle -> Oracle 任务

打开：`http://localhost:5176/tasks/snapshot`

#### Step 1 字段填写

源数据库信息：

| 页面字段 | 填写值 |
| --- | --- |
| 实例引擎类型 | `GaussDB` |
| GaussDB 子模式 | 页面选择 `Oracle mode`；API/配置值为 `oracle-mode`，后端任务类型为 `gaussdb_oracle` |
| IP 地址 / 域名 | `10.250.0.157,10.250.0.223` |
| 端口 | `8000` |
| PDB 名称 | 关闭 |
| 数据库用户名 | `root` |
| 数据库密码 | `Gauss_246` |
| 数据库 | `db_ora_mode` |
| SSL 安全连接 | 开启 |

目标数据库信息：

| 页面字段 | 填写值 |
| --- | --- |
| 实例引擎类型 | `Oracle` |
| IP 地址 / 域名 | `127.0.0.1` |
| 端口 | `15211` |
| 数据库用户名 | `APE_DTS` |
| 数据库密码 | `ape_dts` |
| 数据库 | `XE` |
| SSL 安全连接 | 关闭 |

任务信息：

| 页面字段 | 填写值 |
| --- | --- |
| 同步模式 | `全量+增量` |
| 任务名称 | `manual-gaussdb-oracle-to-oracle` |
| 资源组 | 默认资源组 |

说明：GaussDB 的 IP 用英文逗号填写所有可用节点。页面会生成带 `sslmode=require&protocolVersion=351` 的 oracle-mode 连接，并由后端自动选择可写主节点。

手动路径：

1. 点击 **创建任务**。
2. 在向导中选择源端 **GaussDB**，子模式选择 **Oracle mode**。
3. 目标端选择 **Oracle**。
4. 同步模式选择 **全量+增量**。
5. 任务名建议：`manual-gaussdb-oracle-to-oracle`。
6. 源端表选择或填写：`public.t_gaussdb_oracle_to_oracle`。
7. 目标端表映射到：`APE_DTS.T_GAUSSDB_ORACLE_TO_ORACLE`。
8. 手动点击源端和目标端 **测试连接**，确认都成功。
9. 手动执行 **预检查**，确认没有阻塞错误。
10. 在确认页检查 INI 预览，确认 `[extractor] extract_type=snapshot_and_cdc`。
11. 提交并启动任务。

可见检查：

- 页面跳转到任务详情页。
- 顶部状态从 `ready` 进入 `running`，或小数据集短暂运行后留下运行记录。
- 同步对象区域能看到 `public.t_gaussdb_oracle_to_oracle`。
- 运行日志 tab 有内容，不是空白或错误页。
- 运行日志里能看到 `gaussdb cdc endpoint selection: prefer_candidates=true`，并且最终选中了可写节点。
- 监控 tab 有图表区域。
- 运行历史 tab 至少有 1 条 run 记录。

### 5.2 手动创建 Oracle -> GaussDBOracle 任务

回到：`http://localhost:5176/tasks/snapshot`

#### Step 1 字段填写

源数据库信息：

| 页面字段 | 填写值 |
| --- | --- |
| 实例引擎类型 | `Oracle` |
| IP 地址 / 域名 | `127.0.0.1` |
| 端口 | `15211` |
| 数据库用户名 | `APE_DTS` |
| 数据库密码 | `ape_dts` |
| 数据库 | `XE` |
| SSL 安全连接 | 关闭 |

目标数据库信息：

| 页面字段 | 填写值 |
| --- | --- |
| 实例引擎类型 | `GaussDB` |
| GaussDB 子模式 | 页面选择 `Oracle mode`；API/配置值为 `oracle-mode`，后端任务类型为 `gaussdb_oracle` |
| IP 地址 / 域名 | `10.250.0.157,10.250.0.223` |
| 端口 | `8000` |
| PDB 名称 | 关闭 |
| 数据库用户名 | `root` |
| 数据库密码 | `Gauss_246` |
| 数据库 | `db_ora_mode` |
| SSL 安全连接 | 开启 |

任务信息：

| 页面字段 | 填写值 |
| --- | --- |
| 同步模式 | `全量+增量` |
| 任务名称 | `manual-oracle-to-gaussdb-oracle` |
| 资源组 | 默认资源组 |

说明：目标 GaussDB 同样填写全部候选节点，避免主备切换或单节点不可用时任务绑定到错误节点。

手动路径：

1. 点击 **创建任务**。
2. 源端选择 **Oracle**。
3. 目标端选择 **GaussDB**，子模式选择 **Oracle mode**。
4. 同步模式选择 **全量+增量**。
5. 任务名建议：`manual-oracle-to-gaussdb-oracle`。
6. 源端表选择或填写：`APE_DTS.T_ORACLE_TO_GAUSSDB_ORACLE`。
7. 目标端表映射到：`public.t_oracle_to_gaussdb_oracle`。
8. 手动点击源端和目标端 **测试连接**，确认都成功。
9. 手动执行 **预检查**，确认没有阻塞错误。
10. 在确认页检查 INI 预览，确认 `[extractor] extract_type=snapshot_and_cdc`。
11. 提交并启动任务。

可见检查：

- 页面跳转到任务详情页。
- 顶部状态从 `ready` 进入 `running`，或小数据集短暂运行后留下运行记录。
- 同步对象区域能看到 `APE_DTS.T_ORACLE_TO_GAUSSDB_ORACLE`。
- 运行日志 tab 有内容，不是空白或错误页。
- 监控 tab 有图表区域。
- 运行历史 tab 至少有 1 条 run 记录。

## 6. Apply Bulk CDC Mock Data

两条任务都已经进入增量阶段后，执行批量 mock 脚本。脚本会分别在两个方向的源端写入相同规模的 CDC 变更：

- 更新初始行：`id=1` -> `tracer=bulk_update, payload=bulk_after_update`
- 插入保留行：默认 `id=1000..3999`，共 `3000` 行，按 `200` 行一组提交
- 插入后删除探针行：默认 `id=900000..900199`，共 `200` 行，最终不应存在

`GAUSSDB_RW_HOST` 必须填当前可写节点，使用第 2 步 `precheck` 或自动回归输出里的 `GaussDBOracle writable candidate verified` / `selected GaussDB RW URL for E2E` 结果。不要直接照抄示例 IP；GaussDB Oracle mode 是实验环境，主备可能切换，写到只读节点会导致批量验证无效。默认批量行数是几千条级别；如需调整，用 `BULK_ROW_COUNT` 覆盖。脚本默认 `BULK_COMMIT_EVERY=200`，用于避免单个超大事务在 CDC 重连后从事务起点重放，导致目标端已写入部分行但 checkpoint 长时间停在事务开始 LSN。
脚本默认从 `dt-tests/tests/.env.local` 读取 GaussDBOracle 和 Oracle 的测试密码；也可以用 `GAUSSDB_PASSWORD` / `ORACLE_PASSWORD` 环境变量覆盖。

```bash
export GAUSSDB_RW_HOST=<precheck 输出的当前可写 GaussDB 节点 IP>
export BULK_ROW_COUNT=3000
export BULK_COMMIT_EVERY=200

bash scripts/e2e/gaussdb_oracle_bulk_cdc_mock.sh apply
```

脚本执行成功后会输出期望结果：

- 每张表最终行数：`3001`
- 保留批量行范围：`1000..3999`
- 已删除探针范围：`900000..900199`
- 源端事务分组大小：`200` 行

可见检查：

- 回到两个任务详情页。
- 运行日志继续追加。
- 监控图表或运行指标有变化。
- 任务没有因为增量变更进入失败状态。

## 7. Verify Bulk Result

先用脚本等待增量同步收敛。脚本会同时检查两个方向的源端和目标端：总数、范围、删除探针、抽样行，以及源/目标状态一致。

```bash
export GAUSSDB_RW_HOST=<precheck 输出的当前可写 GaussDB 节点 IP>
export BULK_ROW_COUNT=3000

bash scripts/e2e/gaussdb_oracle_bulk_cdc_verify.sh
```

如果脚本超时，会打印最后一次看到的四端状态。此时再用下面 SQL 在 DBeaver 里分别切换到对应数据库连接排查。

默认期望：

- `expected_total = 3001`
- `bulk_start_id = 1000`
- `bulk_end_id = 3999`
- `delete_start_id = 900000`
- `delete_end_id = 900199`

### 查询 GaussDBOracle -> Oracle 方向

源端 GaussDBOracle：

```sql
SELECT COUNT(*) AS total_rows FROM public.t_gaussdb_oracle_to_oracle;
SELECT MIN(id) AS min_id, MAX(id) AS max_id FROM public.t_gaussdb_oracle_to_oracle;
SELECT COUNT(*) AS deleted_probe_rows
FROM public.t_gaussdb_oracle_to_oracle
WHERE id BETWEEN 900000 AND 900199;
SELECT id, tracer, payload
FROM public.t_gaussdb_oracle_to_oracle
WHERE id IN (1, 1000, 1001, 2500, 3999)
ORDER BY id;
```

目标端 Oracle：

```sql
SELECT COUNT(*) AS TOTAL_ROWS
FROM APE_DTS.T_GAUSSDB_ORACLE_TO_ORACLE;
SELECT MIN(ID) AS MIN_ID, MAX(ID) AS MAX_ID
FROM APE_DTS.T_GAUSSDB_ORACLE_TO_ORACLE;
SELECT COUNT(*) AS DELETED_PROBE_ROWS
FROM APE_DTS.T_GAUSSDB_ORACLE_TO_ORACLE
WHERE ID BETWEEN 900000 AND 900199;
SELECT ID, TRACER, PAYLOAD
FROM APE_DTS.T_GAUSSDB_ORACLE_TO_ORACLE
WHERE ID IN (1, 1000, 1001, 2500, 3999)
ORDER BY ID;
```

### 查询 Oracle -> GaussDBOracle 方向

源端 Oracle：

```sql
SELECT COUNT(*) AS TOTAL_ROWS
FROM APE_DTS.T_ORACLE_TO_GAUSSDB_ORACLE;
SELECT MIN(ID) AS MIN_ID, MAX(ID) AS MAX_ID
FROM APE_DTS.T_ORACLE_TO_GAUSSDB_ORACLE;
SELECT COUNT(*) AS DELETED_PROBE_ROWS
FROM APE_DTS.T_ORACLE_TO_GAUSSDB_ORACLE
WHERE ID BETWEEN 900000 AND 900199;
SELECT ID, TRACER, PAYLOAD
FROM APE_DTS.T_ORACLE_TO_GAUSSDB_ORACLE
WHERE ID IN (1, 1000, 1001, 2500, 3999)
ORDER BY ID;
```

目标端 GaussDBOracle：

```sql
SELECT COUNT(*) AS total_rows FROM public.t_oracle_to_gaussdb_oracle;
SELECT MIN(id) AS min_id, MAX(id) AS max_id FROM public.t_oracle_to_gaussdb_oracle;
SELECT COUNT(*) AS deleted_probe_rows
FROM public.t_oracle_to_gaussdb_oracle
WHERE id BETWEEN 900000 AND 900199;
SELECT id, tracer, payload
FROM public.t_oracle_to_gaussdb_oracle
WHERE id IN (1, 1000, 1001, 2500, 3999)
ORDER BY id;
```

最终一致性标准：

- 两个方向都必须通过。
- 源端与目标端 `COUNT(*)` 都必须是 `3001`。
- `MIN(id)` 必须是 `1`，`MAX(id)` 必须是 `3999`。
- `deleted_probe_rows` 必须是 `0`，证明 delete 增量生效。
- 抽样行必须一致：
  - `1 | bulk_update | bulk_after_update`
  - `1000 | bulk_insert_000001 | payload_000001`
  - `1001 | bulk_insert_000002 | payload_000002`
  - `2500 | bulk_insert_001501 | payload_001501`
  - `3999 | bulk_insert_003000 | payload_003000`

## 8. 手动停止任务

在两个任务详情页手动点击停止或终止。

可见检查：

- 操作按钮可点击，确认弹窗正常。
- 任务状态进入非运行态。
- 运行历史保留 run 记录。
- 失败时详情页能展示失败信息和日志。

## 9. Destroy

```bash
bash scripts/e2e/gaussdb_oracle_console_self_check.sh destroy
```

期望：终端显示 `destroy complete`。刷新 `http://localhost:5176/login` 后页面无法继续访问，说明本自检启动的服务已清理。

## 边界

- 脚本使用独立端口：Console `18082`，Web UI `5176`。
- 脚本使用独立本地状态目录：`.local/self-check/gaussdb-oracle-console`。
- 脚本只清理 self-check 状态目录下的运行进程，不清理仓库里已有的其他 Console、Vite 或 `dt-main` 进程。
- Oracle sqlplus 默认使用 Docker 容器 `oracle-xe-local`；`init` 会通过 `dt-tests/docker-compose.oracle_xe.yml` 启动它，`precheck` 会验证它已运行且 CDC 前置条件可用。
- GaussDB Oracle mode 默认使用 `dt-tests/tests/.env.local` 里的外部候选节点；只有 `precheck` 会做真实写探测并给出 `WARN`，`init` 不会操作这个外部实验环境。
- 全自动回归仍可用：`bash scripts/e2e/gaussdb_oracle_console_self_check.sh e2e normal`，但它不是手动自检主路径。
- `bash scripts/e2e/gaussdb_oracle_console_self_check.sh verify normal` 仍保留小样本自动验证语义，不适用于第 6 步批量 mock 数据。
