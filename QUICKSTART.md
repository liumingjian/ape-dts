# ape-dts Console 快速验证指南

## 1. 前置条件

```bash
# 确认 Docker DB 栈已启动（4 个容器 healthy）
cd dt-tests
docker compose -f docker-compose.ci.yml -f docker-compose.override.local.yml up -d
docker compose -f docker-compose.ci.yml -f docker-compose.override.local.yml ps
# 期望看到 mysql-src-ci:3307, mysql-dst-ci:3308, postgres-src-ci:5433, postgres-dst-ci:5434

# 回到项目根目录
cd ..
```

开发模式额外依赖：

| 工具 | 安装 | 用途 |
|------|------|------|
| cargo-watch | `cargo install cargo-watch --locked` | 后端 .rs 文件变更自动重编译+重启 |
| Node ≥18 | 已有 | 前端 |
| pnpm | 已有 | 前端 |

## 2. 开发模式（Hot-Reload）

修改代码后无需手动重编译或重启——后端和前端各自自动热加载。

### 2.1 启动后端（cargo-watch）

```bash
# 清除旧数据库（可选，干净测试时使用）
rm -f ./data/console.db

# 启动——任何 .rs 文件变更自动触发重编译 + 重启
cargo watch -x 'run -p dt-console-server'
```

首次编译约 10–30s（取决于机器），增量重编译约 1–5s。启动后验证：

```bash
curl -sf http://127.0.0.1:8080/api/healthz
# → {"status":"ok"}
```

首次启动自动创建 `./data/console.db`，执行 24 个迁移，种子管理员账户 `admin / admin123`。

### 2.2 启动前端（Vite HMR）

另开终端：

```bash
# 安装依赖（首次）
pnpm -C web-prototype install --frozen-lockfile

# 连接真实后端启动——Vite HMR 自动刷新浏览器
VITE_USE_MOCK=false pnpm -C web-prototype dev --host 127.0.0.1 --port 5173 --strictPort
```

Vite 代理 `/api` → `http://localhost:8080`，浏览器打开 http://127.0.0.1:5173/

`.vue` / `.ts` / `.css` 文件保存后浏览器即时更新，无需手动刷新。

### 2.3 验证端到端

1. 浏览器打开 http://127.0.0.1:5173/ → 自动跳转 /login
2. 登录 `admin` / `admin123` → Dashboard
3. 创建任务（见第 4 节详细步骤）→ 启动 → 状态 running → stopped

### 2.4 开发提示

| 提示 | 说明 |
|------|------|
| 首次编译慢 | cargo-watch 首次全量编译 10–30s，增量重编译 1–5s |
| cargo-watch 未检测变更（macOS 偶发） | Ctrl-C 退出后重新运行 `cargo watch -x 'run -p dt-console-server'` |
| 后端日志 | 在 cargo-watch 终端直接查看 |
| 前端 HMR 状态 | 浏览器 DevTools 控制台 + Vite 终端输出 |
| 修改后端端口 | 设置 `CONSOLE_BIND_ADDR` 环境变量，并同步修改 `web-prototype/vite.config.ts` 中 proxy target |

## 3. 生产构建

如需 release 优化构建（部署、性能测试等）：

```bash
# 构建后端（含 metrics 特性）和引擎二进制
cargo build --release --features metrics -p dt-console-server -p dt-main
# 首次约 3-5 分钟，后续增量秒级

# 启动
cargo run --release -p dt-console-server
# 或直接运行二进制：
# CONSOLE_BIND_ADDR=127.0.0.1:8080 target/release/dt-console-server
```

## 4. 手动验证流程

### 4.1 登录

| 步骤 | 操作 | 期望 |
|------|------|------|
| 1 | 浏览器打开 http://127.0.0.1:5173/ | 自动跳转到 /login |
| 2 | 输入 `admin` / `admin123`，点登录 | 跳转到 /dashboard，顶栏显示"管理员"角色 |

### 4.2 Dashboard

| 步骤 | 操作 | 期望 |
|------|------|------|
| 1 | 查看 KPI 卡片 | 显示 运行中/告警/吞吐/延迟 4 个指标 |
| 2 | 展开侧栏"任务管理" | 看到独立菜单项：**全量迁移** (`/tasks/snapshot`)、**增量同步** (`/tasks/cdc`)、数据校验 (`/tasks/check`)、结构迁移 (`/tasks/struct`) |
| 3 | 点击"全量迁移" | 跳转到 /tasks/snapshot |
| 4 | 切换右上角语言为 English | 侧栏变为英文 Dashboard/Tasks/Alerts 等 |

### 4.3 创建任务（Wizard）

任务向导第 1 步会显示 3 种迁移模式（仅在 `/tasks/snapshot` 类目下可见）：

- **全量迁移 (snapshot)** — 一次性导出全部历史数据，所有源端引擎都支持。
- **增量同步 (cdc)** — 仅订阅 binlog/WAL/SCN，跑长任务。
- **全量 + 增量 (snapshot_and_cdc)** — 先跑一次全量，再无缝切到增量，零数据丢失的热切方案。
  - **当前仅支持 MySQL 源端**（含 GaussDB-MySQL）；选其他源端时此卡片会自动置灰并提示「Snapshot+CDC mode is currently only supported for MySQL sources」。
  - 后端实现为「单个 Run，串行两个 dt-main 子进程」：console-server 在启动 Run 之前抓取一次 `start_time_utc`，先跑 `extract_type=snapshot` 子进程，结束后自动启动 `extract_type=cdc` 子进程并把 `start_time_utc` 注入 INI，CDC 阶段会扫到全量过程中产生的所有 binlog 变更（sinker 用 upsert 保证幂等）。
  - 在控制日志（control_log）里能看到 `phase_transition: snapshot_to_cdc` 的事件；Run 的 PID 会在切换瞬间更新；同一个 `run_id` 贯穿两个阶段。

| 步骤 | 操作 | 期望 |
|------|------|------|
| 1 | 点击 /tasks/snapshot 页面的"新建任务" | 进入 7 步向导 |
| 2 | Step 1: 选 MySQL→MySQL，填 source `127.0.0.1:3307` root/123456/src_db，target `127.0.0.1:3308` root/123456/dst_db，填任务名；选「全量+增量」模式 | 表单校验通过；非 MySQL 源端时全量+增量卡片置灰 |
| 3 | Step 2: 点"测试连接" | source 和 target 均 ✅ OK |
| 4 | Step 3: 填 do_dbs=* | 下一步可用 |
| 5 | Step 4-5: 保持默认 | 下一步 |
| 6 | Step 6: 点"预检查" | 检查结果出现（可能有 warning，可继续） |
| 7 | Step 7: 确认提交 | 跳转到任务详情页 |

### 4.4 启动/停止任务

| 步骤 | 操作 | 期望 |
|------|------|------|
| 1 | 任务详情页点"启动" | 状态变为 running，出现 PID |
| 2 | 等待几秒，观察状态 | 小数据集约 2-5 秒完成，状态变为 stopped |
| 3 | 点"运行日志" Tab | 看到日志内容和文件选择器 |
| 4 | 点"监控" Tab | 显示 1h/6h/24h 图表区域 |
| 5 | 点"历史" Tab | 显示 Run 记录，含 exit_code=0 |

### 4.5 RBAC 验证

```bash
# 创建 operator 用户
curl -s -b cookies.txt -X POST http://127.0.0.1:8080/api/users \
  -H 'Content-Type: application/json' \
  -H 'X-XSRF-TOKEN: <从 cookies 提取>' \
  -d '{"username":"op1","password":"op123456","role":"operator"}'

# 创建 viewer 用户
curl -s -b cookies.txt -X POST http://127.0.0.1:8080/api/users \
  -H 'Content-Type: application/json' \
  -H 'X-XSRF-TOKEN: <从 cookies 提取>' \
  -d '{"username":"viewer1","password":"vw123456","role":"viewer"}'
```

| 角色 | 期望可见页面 | 期望不可见 |
|------|-------------|-----------|
| admin | 全部（含用户管理、操作日志、License） | — |
| operator | Dashboard、Tasks、Alerts、License（只读）、运行管理 | 用户管理、操作日志、License 激活 |
| viewer | Dashboard、Tasks（只读）、Alerts（只读） | 所有写操作、用户管理、License |

### 4.6 License

| 步骤 | 操作 | 期望 |
|------|------|------|
| 1 | 侧栏点 License | 显示当前许可证状态（未激活/已激活） |
| 2 | 点"激活"，输入许可证码 | 状态变为 active，显示 maxTasks 和到期日 |

### 4.7 告警规则

| 步骤 | 操作 | 期望 |
|------|------|------|
| 1 | 侧栏"告警配置"→"指标规则" | 显示规则列表 |
| 2 | 点"新建规则" | 抽屉打开，填 metric/运算符/阈值/持续时间 |
| 3 | 保存 | 规则出现在列表中，可启用/禁用 |

## 5. 自动化验证（Playwright E2E）

```bash
cd web-prototype

# 确保后端和前端都在运行
E2E_REAL_BACKEND=1 \
E2E_DB_SOURCE_DSN='mysql://root:123456@127.0.0.1:3307/src_db' \
E2E_DB_TARGET_DSN='mysql://root:123456@127.0.0.1:3308/dst_db' \
pnpm exec playwright test e2e/full-happy-path.spec.ts --timeout=300000

# 期望：27 tests passed
```

## 6. 单独后端 API 验证（curl）

```bash
# 1. 登录拿 cookie
curl -s -c cookies.txt http://127.0.0.1:8080/api/auth/login \
  -H 'Content-Type: application/json' \
  -d '{"username":"admin","password":"admin123"}'

# 2. 获取 CSRF token
XSRF=$(grep XSRF-TOKEN cookies.txt | awk '{print $NF}')

# 3. 创建任务
curl -s -b cookies.txt http://127.0.0.1:8080/api/tasks \
  -H 'Content-Type: application/json' \
  -H "X-XSRF-TOKEN: $XSRF" \
  -d '{
    "name": "test-snap",
    "kind": "snapshot",
    "engineSource": "mysql",
    "engineTarget": "mysql",
    "sourceEndpoint": {"url":"mysql://root:123456@127.0.0.1:3307/src_db"},
    "targetEndpoint": {"url":"mysql://root:123456@127.0.0.1:3308/dst_db"},
    "extractor": {"extract_type":"snapshot","do_dbs":"*"},
    "resource_group_id": "default"
  }'

# 4. 启动任务
TASK_ID=<从上一步结果获取>
curl -s -b cookies.txt -X POST "http://127.0.0.1:8080/api/tasks/$TASK_ID/start" \
  -H "X-XSRF-TOKEN: $XSRF"
# → 202 {"runId":"..."}

# 5. 查看 Run 状态
RUN_ID=<从上一步获取>
curl -s -b cookies.txt "http://127.0.0.1:8080/api/runs/$RUN_ID"
# → status: "running" 或 "stopped", exit_code: 0

# 6. 预览 INI
curl -s -b cookies.txt "http://127.0.0.1:8080/api/tasks/$TASK_ID/preview_ini"
# → 完整 INI 文本

# 7. 健康检查
curl -sf http://127.0.0.1:8080/api/healthz   # liveness
curl -sf http://127.0.0.1:8080/api/readyz     # readiness (DB + scraper)
```

## 7. Rust 测试套件

```bash
# 全量测试（约 30s）
cargo nextest run --workspace --exclude dt-tests --lib --bins

# 只跑 dt-console-server
cargo nextest run -p dt-console-server --lib --bins

# Lint
cargo clippy --workspace --all-targets --all-features --exclude dt-tests -- -D warnings
cargo fmt --all --check
```

## 8. 前端测试套件

```bash
cd web-prototype

# 单元测试
pnpm test --run          # 291 tests

# 类型检查
pnpm exec vue-tsc --noEmit

# Lint
pnpm lint                # --max-warnings 0

# 构建
pnpm build
```

## 9. 常见问题

| 问题 | 原因 | 解决 |
|------|------|------|
| `cargo build` 失败 | dt-connector clippy 警告 | 用 `--exclude dt-tests` 构建 |
| 启动任务 exit_code=101 | INI 渲染问题 | 确认 `do_dbs` 非空（检查 preview_ini） |
| 测试连接全部失败 | Docker DB 未启动 | `docker compose up -d` |
| 前端 MSW 拦截请求 | 未设 `VITE_USE_MOCK=false` | 设置环境变量 |
| License 激活失败 | 许可证码格式不对 | 需 HMAC-SHA256 签名的码 |
| SSE 日志无数据 | Run 未运行或无日志文件 | 确认 Run 状态为 running |
| cargo-watch 编译失败 | Rust 版本 <1.88 | `cargo install cargo-watch --locked` 使用锁定依赖 |
| 前端代理 502 | 后端未启动或端口不对 | 先启动后端，确认 8080 端口；检查 vite.config.ts proxy target |
