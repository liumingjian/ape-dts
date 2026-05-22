# Quickstart · 手动测试验证（MySQL → MySQL Snapshot）

本文给出 **经 e2e 回归验证过（regression #8：26 pass / 0 fail）** 的手动操作序列，用于验证 ape-dts Console 的 **Snapshot 全量迁移** 主链路。所有端口、账号、命令都与 `e2e/full-happy-path.spec.ts` 的常量一致，照抄即可。

验证范围：登录 → 7 步向导 → 启动 → 指标 → 停止 → 最终状态，以及 5 个关键横切场景（i18n、legacy 重定向、SSE 日志、停止流程、RBAC 入口）。

---

## 0 · 前置检查

```bash
# 1. 确认端口未占用
lsof -i :8080 -sTCP:LISTEN -P -n   # 期望：空 或 dt-console-server
lsof -i :5173 -sTCP:LISTEN -P -n   # 期望：空 或 vite
lsof -i :3307 -sTCP:LISTEN -P -n   # 期望：空 或 mysql-src-ci
lsof -i :3308 -sTCP:LISTEN -P -n   # 期望：空 或 mysql-dst-ci

# 2. 工具链
node --version   # >= 18
pnpm --version   # >= 8
cargo --version  # rustc >= 1.85
docker --version
```

如果 3307/3308/8080/5173 被其他进程占用，先停掉再继续。

---

## 1 · 启动 Docker 数据库栈

```bash
cd ape-dts/dt-tests
docker compose \
  -f docker-compose.ci.yml \
  -f docker-compose.override.local.yml \
  up -d mysql-src mysql-dst mysql-meta

# 等 30 s 让 MySQL 初始化完毕
sleep 30

# 健康检查：两条都应返回 1
docker exec mysql-src-ci mysql -uroot -p123456 -h127.0.0.1 -N -e "SELECT 1"
docker exec mysql-dst-ci mysql -uroot -p123456 -h127.0.0.1 -N -e "SELECT 1"
```

- `mysql-src-ci` → 主机端口 `3307`，root 密码 `123456`
- `mysql-dst-ci` → 主机端口 `3308`，root 密码 `123456`

### 1.1 准备源表 + 目标库

```bash
# 源端建表并插一行"tracer"数据（向导提交后应同步到目标端）
docker exec mysql-src-ci mysql -uroot -p123456 -h127.0.0.1 -e "
CREATE DATABASE IF NOT EXISTS test_db;
USE test_db;
CREATE TABLE IF NOT EXISTS manual_smoke (
  id INT PRIMARY KEY,
  tracer VARCHAR(128),
  payload VARCHAR(256)
);
INSERT INTO manual_smoke (id, tracer, payload)
  VALUES (1, 'manual-$(date +%s)', 'quickstart-row')
  ON DUPLICATE KEY UPDATE tracer=VALUES(tracer);
"

# 目标端只需建库——表结构由 snapshot 创建（勾选"同步表结构"时）
docker exec mysql-dst-ci mysql -uroot -p123456 -h127.0.0.1 -e "
CREATE DATABASE IF NOT EXISTS test_db;
"
```

---

## 2 · 启动后端 orchestrator

```bash
cd ape-dts  # 仓库根
cargo run --release -p dt-console-server 2>&1 | tee /tmp/console-server.log
```

> 首次 release 构建约 2 分 15 秒；启动成功的标志是日志出现 `listening on 0.0.0.0:8080` 和 `seeded default admin user`（仅第一次）。

保持这个终端不关。SQLite 库默认写在 `./data/console.db`，要干净重启时 `rm -rf ./data && pkill -f dt-console-server` 再跑。

---

## 3 · 启动前端

新开一个终端：

```bash
cd ape-dts/web-prototype
pnpm install
VITE_USE_MOCK=false pnpm dev
```

控制台输出 `Local: http://127.0.0.1:5173/` 后开浏览器。

> **注意**：`.env.development` 默认把 `VITE_USE_MOCK` 设为 `true`（MSW 接管 `/api/*`）。手动连真实后端一定要显式传 `VITE_USE_MOCK=false`，或写到 `.env.development.local`。

---

## 4 · 登录 & 快速健康检查

1. 浏览器访问 `http://127.0.0.1:5173/login`
2. 账号 `admin`，密码 `admin123`，点 **登录** / **Sign in**
3. 进入 `/dashboard`（侧边栏「工作台」），确认看到 4 块 KPI 卡和「最近任务」列表，无 500/Toast 报错
4. 顶栏右上角头像 → 账号显示 `Administrator / 管理员`

如果登录后立刻被踢回 `/login`，九成是 `VITE_USE_MOCK=false` 没生效，检查浏览器 Network 里 `/api/auth/login` 是不是命中真实 8080。

---

## 5 · 7 步向导 · MySQL → MySQL Snapshot（主链路）

侧边栏点 **任务管理 → 全量迁移**，进入 `/tasks/snapshot`，右上角 **创建任务**。

### Step 1 · 实例来源

| 字段 | 填值 |
| --- | --- |
| 源端引擎 | **MySQL**（点 `engine-chip-source-mysql` chip） |
| 源端 Host | `127.0.0.1` |
| 源端 Port | `3307` |
| 源端 Username | `root` |
| 源端 Password | `123456` |
| 源端 Database | `test_db` |
| 同步模式 | **仅全量（snapshot）** |
| 目标端引擎 | **MySQL** |
| 目标端 Host | `127.0.0.1` |
| 目标端 Port | `3308` |
| 目标端 Username | `root` |
| 目标端 Password | `123456` |
| 任务名称 | `manual_snapshot_` + 随机后缀（例如时间戳） |
| 资源组 | 下拉第 1 项（默认 `default`） |

点 **下一步**。

### Step 2 · 测试连接

- 源端面板点 **测试连接**，等待出现绿色 `conn-status-ok`（约 1–5 s）
- 目标端同理
- 两边都绿后点 **下一步**

> 如果任一侧红色 `conn-status-fail`：先去第 0 步 docker 健康检查；不要用 Bypass（admin 虽然能跳，但会掩盖真实问题）。

### Step 3 · 选择迁移对象 & 设置迁移

| 字段 | 填值 |
| --- | --- |
| 迁移库（do_dbs） | `*` |
| 迁移表（do_tbs） | `*.*` |
| 其余 | 默认（留空） |

点 **下一步**。

> 上面两个字段不填，**下一步** 按钮会保持 disabled——这是精确校验，不是 bug。

### Step 4 · 数据加工

默认即可，直接点 **下一步**。

### Step 5 · 高级设置

默认即可（并行度 4、缓冲区 16000、断点 10 s、续传 from_log、不限速、Prometheus 启用），直接点 **下一步**。

### Step 6 · 预检查

- 进入此步会自动触发 precheck，进度条从 0 滚到 100%（约 3–8 s）
- 出现 warn（黄色）可以直接跳过；出现 `wizard__check-result--fail`（红色）需要解决才能继续
- 100% 后点 **下一步**

### Step 7 · 任务确认

- 页面顶部默认选中 **创建并立即启动**
- 展开 INI 预览检查一眼（`[extractor]` url 应包含 `mysql://root:***@127.0.0.1:3307/test_db`，`[sinker]` 应是 3308）
- 点 **创建并启动**

**期望行为**：

1. 右下角 toast `任务已创建并启动`
2. URL 跳转到 `/tasks/snapshot/<taskId>`
3. 顶部状态徽章在 ~5 s 内从 `ready` → `running`
4. "抽取速率 / 同步延迟 / 同步进度" 卡片开始有数

---

## 6 · 运行中观测

在任务详情页（`/tasks/snapshot/<taskId>`）依次验证：

1. **同步配置** tab：各字段与 Step 1/5 所填一致（密码脱敏成 `***`）
2. **同步对象** tab：展示 `test_db.manual_smoke`（snapshot 探到的表清单）
3. **运行日志** tab：点进后看到 SSE 日志流实时追加，出现 `[INFO]` / `Pipeline` 字样；点 **暂停**/**继续** 验证控制按钮
4. **监控** tab：3 个 ECharts 折线有数据点（`extractor_rps_avg`、`sinker_record_count_avg_by_sec`、`pipeline_buffer_size_avg`）
5. **运行历史** tab：此刻应恰有 1 行，`状态=running`，`开始时间` 非空

打开浏览器 DevTools Network，随手找几个请求确认 Cookie + `X-XSRF-TOKEN` 双 header 都带上了。

### 6.1 目标库数据抽样（关键收敛点）

再开一个终端：

```bash
docker exec mysql-dst-ci mysql -uroot -p123456 -h127.0.0.1 test_db \
  -e "SELECT id, tracer, payload FROM manual_smoke"
```

预期 1 行 `1 | manual-<timestamp> | quickstart-row`。这是 snapshot 链路确实写通的唯一硬证据。

---

## 7 · 停止任务

- 详情页顶部点 **终止**
- 弹出 `ElMessageBox` 确认框 → 点 **确定**
- 状态徽章 30 s 内变成 `stopped`
- 运行历史新增一行（或该行 `结束时间` 非空、`exit_code=0`）

> 不要用 `window.confirm` 拦 dialog 的思路手工验证自动化——这里是 Element Plus 的 DOM 模态框，需要用鼠标点按钮。

---

## 8 · 横切验证（任选几个回归）

| # | 场景 | 操作 | 期望 |
| --- | --- | --- | --- |
| 8.1 | **Legacy URL 重定向** | 地址栏访问 `/tasks/sync` | 301 到 `/tasks/snapshot` |
| 8.2 | 同上 | `/tasks/replay` → | `/tasks/snapshot` |
| 8.3 | 同上 | `/tasks/verify` → | `/tasks/check` |
| 8.4 | 同上 | `/tasks/create/sync` → | `/tasks/create/snapshot` |
| 8.5 | **i18n 持久化** | 顶栏语言切到 `English`，刷新 | 语言保持英文 |
| 8.6 | **Deep-link 刷新** | 详情页切到 `监控` tab，复制地址刷新 | 直接停在 `?tab=monitor` |
| 8.7 | **RBAC · viewer** | `/users` 开一个 viewer 账号 → 登出 → 用 viewer 登录 | 侧边栏 **License/系统管理** 不可见；列表页 **创建/删除/启动** 按钮消失 |
| 8.8 | **停止 → 再启动** | 终止后点 **启动** | `运行历史` 出现第 2 行 |
| 8.9 | **SSE 登出失效** | 开详情日志 tab 看流；另一 tab 退登 | 原 tab 下一次 API 401，重定向到 `/login` |
| 8.10 | **License cap** | 进 License → 注入 `currentTasks=max` | 列表 **创建** 按钮被 license banner 锁死 |

### 8.7 viewer 账号一键建

```bash
# 登录 admin 后（浏览器 Cookie 已就绪）——或改用 curl + 先登录拿 token
# 省事起见，直接在前端 /users 页面 → 新增用户 → role=viewer
```

---

## 9 · 清理（测完收尾）

```bash
# 停前端
# Ctrl-C vite 终端

# 停后端
pkill -f 'dt-console-server'

# 清数据（可选——保留 console.db 能复现任务列表）
rm -rf ape-dts/data/console.db

# 停 docker
cd ape-dts/dt-tests
docker compose -f docker-compose.ci.yml -f docker-compose.override.local.yml down

# 清掉手动测试表（可选）
docker exec mysql-src-ci mysql -uroot -p123456 -h127.0.0.1 \
  -e "DROP TABLE IF EXISTS test_db.manual_smoke"
docker exec mysql-dst-ci mysql -uroot -p123456 -h127.0.0.1 \
  -e "DROP TABLE IF EXISTS test_db.manual_smoke"
```

---

## 10 · 常见失败对照

| 症状 | 根因 | 处置 |
| --- | --- | --- |
| 登录后立刻回 `/login` | `VITE_USE_MOCK` 仍为 `true` | 重启 vite 时显式 `VITE_USE_MOCK=false pnpm dev` |
| Step 2 测试连接一直红 | docker mysql 还在初始化 / 端口占用 | `docker compose ps` + `lsof -i :3307` |
| Step 3 下一步禁用 | `do_dbs` / `do_tbs` 为空 | 填 `*` / `*.*` |
| Step 7 提交后状态卡 `ready` 不变 | `POST /tasks/{id}/start` 失败 | 看 `/tmp/console-server.log`，多半是 `dt-main` 可执行文件路径 / 权限 |
| 详情页监控全是 `暂无数据` | 任务刚起 < 10 s，还没有采样 | 等 10–15 s 刷新；仍空则检查 INI 里 `[metrics]` |
| 终止按钮点了无反应 | `ElMessageBox` 确认框被 iframe 挡住 / XSRF-TOKEN 缺失 | 刷新页面后重试；DevTools Network 确认 `POST /tasks/{id}/stop` 返回 200 |
| 目标库查不到 `manual_smoke` | snapshot 未完成 / 表结构勾选未选 | 回详情页看 `同步进度=100%`；Step 5 勾选 `同步表结构` |

---

## 11 · 自动化回归兜底

手动测完如果想再跑全量回归对比：

```bash
cd ape-dts/web-prototype
E2E_REAL_BACKEND=1 pnpm exec playwright test e2e/full-happy-path.spec.ts \
  --reporter=line --workers=1
```

基线（2026-05-11 regression #8）：**26 passed / 0 failed / 1 skipped（2.3 min）**。任何与此偏离的 fail 都值得定位。
