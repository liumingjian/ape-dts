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

期望：终端最后显示 `precheck passed`。如果失败，把终端输出贴回来即可。

关键前置条件：

- `dt-tests/tests/.env.local` 必须配置 `gaussdb_pg_candidate_hosts`，例如 `10.250.0.157:8000,10.250.0.223:8000`。
- `init` 会把这个变量传给 Console 和 `dt-main` 子进程，让 GaussDBOracle CDC 自动选择可写主库。不要只依赖页面里填写的单个 GaussDB IP；该 IP 可能是只读节点，快照能读但增量无法消费。
- 下文 Step 1 的连接信息按当前自检环境填写；如果 `dt-tests/tests/.env.local` 中的 `gaussdb_oracle_*` 或 `oracle_*` 值变化，手动填写值也要同步变化。

## 3. Init

```bash
bash scripts/e2e/gaussdb_oracle_console_self_check.sh init
```

这个步骤启动自检环境，并在隔离的 Console DB 中写入一条自检 License：

- Console 后端：`http://127.0.0.1:18082`
- Web UI：`http://localhost:5176`

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

## 5. 手动创建 GaussDBOracle -> Oracle 任务

打开：`http://localhost:5176/tasks/snapshot`

### Step 1 字段填写

源数据库信息：

| 页面字段 | 填写值 |
| --- | --- |
| 实例引擎类型 | `GaussDB` |
| GaussDB 子模式 | `oracle-mode (gaussdboracle)` |
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

## 6. 手动创建 Oracle -> GaussDBOracle 任务

回到：`http://localhost:5176/tasks/snapshot`

### Step 1 字段填写

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
| GaussDB 子模式 | `oracle-mode (gaussdboracle)` |
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

## 7. Apply CDC Changes

注意：必须同时存在并启动以下两条任务后再执行 mutate/verify：

- `GaussDBOracle -> Oracle`
- `Oracle -> GaussDBOracle`

如果只创建第一条任务，反向验证会必然出现 `oracle_to_gaussdb_oracle target=[]`，这是反向任务缺失，不是反向同步结果失败。

两条任务都已经进入增量阶段后，分别在两个源端手动执行下面 SQL。命令里的 tracer/payload 可以自行修改；验证时以你实际写入的值为准。

### GaussDBOracle -> Oracle：在 GaussDBOracle 源端写入变更

`GAUSSDB_RW_HOST` 填当前可写节点。若该节点超时或只读，改成候选节点里的另一个 IP 后重试。

```bash
export GAUSSDB_RW_HOST=10.250.0.157
export PGPASSWORD='Gauss_246'

psql "postgres://root@${GAUSSDB_RW_HOST}:8000/db_ora_mode?sslmode=require&protocolVersion=351" \
  -v ON_ERROR_STOP=1 <<'SQL'
DELETE FROM public.t_gaussdb_oracle_to_oracle WHERE id >= 2;
INSERT INTO public.t_gaussdb_oracle_to_oracle (id, tracer, payload)
  VALUES (2, 'cdc_insert_delete', 'to_delete');
UPDATE public.t_gaussdb_oracle_to_oracle
  SET tracer='cdc_update', payload='after_update'
  WHERE id=1;
DELETE FROM public.t_gaussdb_oracle_to_oracle WHERE id=2;
INSERT INTO public.t_gaussdb_oracle_to_oracle (id, tracer, payload)
  VALUES (3, 'cdc_insert', 'after_insert');
SQL
```

### Oracle -> GaussDBOracle：在 Oracle 源端写入变更

Oracle 容器里的 `sqlplus` 不一定在默认 `PATH`，所以命令显式设置 `ORACLE_HOME`。

```bash
docker exec -i oracle-xe-local bash -lc '
export ORACLE_HOME=/u01/app/oracle/product/11.2.0/xe
export PATH=$ORACLE_HOME/bin:$PATH
export LD_LIBRARY_PATH=$ORACLE_HOME/lib
sqlplus -s APE_DTS/ape_dts@//127.0.0.1:15211/XE
' <<'SQL'
WHENEVER SQLERROR EXIT SQL.SQLCODE
DELETE FROM APE_DTS.T_ORACLE_TO_GAUSSDB_ORACLE WHERE ID >= 2;
INSERT INTO APE_DTS.T_ORACLE_TO_GAUSSDB_ORACLE (ID, TRACER, PAYLOAD)
  VALUES (2, 'cdc_insert_delete', 'to_delete');
UPDATE APE_DTS.T_ORACLE_TO_GAUSSDB_ORACLE
  SET TRACER='cdc_update', PAYLOAD='after_update'
  WHERE ID=1;
DELETE FROM APE_DTS.T_ORACLE_TO_GAUSSDB_ORACLE WHERE ID=2;
INSERT INTO APE_DTS.T_ORACLE_TO_GAUSSDB_ORACLE (ID, TRACER, PAYLOAD)
  VALUES (3, 'cdc_insert', 'after_insert');
COMMIT;
EXIT
SQL
```

这组变更覆盖三类 CDC：

- 更新初始行：`id=1` -> `tracer=cdc_update, payload=after_update`
- 插入临时行：`id=2`，随后删除
- 插入最终新增行：`id=3, tracer=cdc_insert, payload=after_insert`

可见检查：

- 回到两个任务详情页。
- 运行日志继续追加。
- 监控图表或运行指标有变化。
- 任务没有因为增量变更进入失败状态。

## 8. Verify Normal

等待几秒让增量同步收敛，然后用 SQL 分别查询两个方向的源端和目标端。最终结论以这些查询结果为准；脚本验证只作为辅助。

### 查询 GaussDBOracle -> Oracle 方向

源端 GaussDBOracle：

```bash
export GAUSSDB_RW_HOST=10.250.0.157
export PGPASSWORD='Gauss_246'

psql "postgres://root@${GAUSSDB_RW_HOST}:8000/db_ora_mode?sslmode=require&protocolVersion=351" \
  -c "SELECT id, tracer, payload FROM public.t_gaussdb_oracle_to_oracle ORDER BY id;"
```

目标端 Oracle：

```bash
docker exec -i oracle-xe-local bash -lc '
export ORACLE_HOME=/u01/app/oracle/product/11.2.0/xe
export PATH=$ORACLE_HOME/bin:$PATH
export LD_LIBRARY_PATH=$ORACLE_HOME/lib
sqlplus -s APE_DTS/ape_dts@//127.0.0.1:15211/XE
' <<'SQL'
SET PAGESIZE 100 LINESIZE 200 FEEDBACK OFF VERIFY OFF
COLUMN TRACER FORMAT A24
COLUMN PAYLOAD FORMAT A32
SELECT ID, TRACER, PAYLOAD
FROM APE_DTS.T_GAUSSDB_ORACLE_TO_ORACLE
ORDER BY ID;
EXIT
SQL
```

### 查询 Oracle -> GaussDBOracle 方向

源端 Oracle：

```bash
docker exec -i oracle-xe-local bash -lc '
export ORACLE_HOME=/u01/app/oracle/product/11.2.0/xe
export PATH=$ORACLE_HOME/bin:$PATH
export LD_LIBRARY_PATH=$ORACLE_HOME/lib
sqlplus -s APE_DTS/ape_dts@//127.0.0.1:15211/XE
' <<'SQL'
SET PAGESIZE 100 LINESIZE 200 FEEDBACK OFF VERIFY OFF
COLUMN TRACER FORMAT A24
COLUMN PAYLOAD FORMAT A32
SELECT ID, TRACER, PAYLOAD
FROM APE_DTS.T_ORACLE_TO_GAUSSDB_ORACLE
ORDER BY ID;
EXIT
SQL
```

目标端 GaussDBOracle：

```bash
export GAUSSDB_RW_HOST=10.250.0.157
export PGPASSWORD='Gauss_246'

psql "postgres://root@${GAUSSDB_RW_HOST}:8000/db_ora_mode?sslmode=require&protocolVersion=351" \
  -c "SELECT id, tracer, payload FROM public.t_oracle_to_gaussdb_oracle ORDER BY id;"
```

可选自动验证：

```bash
bash scripts/e2e/gaussdb_oracle_console_self_check.sh verify normal
```

可选验证通过时会输出：

```text
PASS: gaussdb_oracle_to_oracle source rows match target rows
PASS: oracle_to_gaussdb_oracle source rows match target rows
PASS: GaussDBOracle <-> Oracle snapshot+cdc data is consistent.
```

最终一致性标准：

- 两个方向都必须通过。
- 源端与目标端最终行集必须一致。
- 最终行集必须是：
  - `1 | cdc_update | after_update`
  - `3 | cdc_insert | after_insert`
- `id=2` 不应存在，证明 delete 增量也生效。

## 9. 手动停止任务

在两个任务详情页手动点击停止或终止。

可见检查：

- 操作按钮可点击，确认弹窗正常。
- 任务状态进入非运行态。
- 运行历史保留 run 记录。
- 失败时详情页能展示失败信息和日志。

## 10. Destroy

```bash
bash scripts/e2e/gaussdb_oracle_console_self_check.sh destroy
```

期望：终端显示 `destroy complete`。刷新 `http://localhost:5176/login` 后页面无法继续访问，说明本自检启动的服务已清理。

## 边界

- 脚本使用独立端口：Console `18082`，Web UI `5176`。
- 脚本使用独立本地状态目录：`.local/self-check/gaussdb-oracle-console`。
- 脚本只清理 self-check 状态目录下的运行进程，不清理仓库里已有的其他 Console、Vite 或 `dt-main` 进程。
- Oracle sqlplus 默认使用 Docker 容器 `oracle-xe-local`。
- 全自动回归仍可用：`bash scripts/e2e/gaussdb_oracle_console_self_check.sh e2e normal`，但它不是手动自检主路径。
