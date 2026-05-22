# 全量迁移模块（Snapshot Migration, MySQL→MySQL）E2E 验证报告

**日期**: 2026-05-11
**范围**: 侧边栏「全量迁移」模块 = `/tasks/snapshot` list + `/tasks/create/snapshot` wizard + `/tasks/snapshot/:id` detail + KPI/Dashboard 链接
**引擎对**: MySQL 8.0 → MySQL 8.0 (docker mysql-src-ci:3307 → mysql-dst-ci:3308)
**方法**: C 方案（baseline `full-happy-path.spec.ts` + `/playwright-cli` 探索性测试，三批 P0/P1/P2 盲区）
**产物**: 19 条 BUG（其中 BUG-015 降级为 not-a-bug）

## 严重度汇总

| severity | 数量 | BUG IDs |
|---|---|---|
| **blocker** | 3 | BUG-002, BUG-004, BUG-016 |
| **major** | 13 | BUG-000, BUG-001, BUG-005, BUG-006, BUG-007, BUG-009, BUG-010, BUG-011, BUG-012, BUG-014, BUG-017, BUG-018 + BUG-003 (dup of 000) |
| **minor** | 2 | BUG-008, BUG-013 |
| **not-a-bug** | 1 | BUG-015 |

## 按模块分组

### 核心数据迁移（后端 + Rust engine）
- **BUG-002** [blocker]: Snapshot 任务 target 表不存在时 panic (`dt-main/src/main.rs:29`)，空目标库迁移 100% 失败。**全量迁移核心功能不可用**。
- **BUG-016** [blocker]: `extract_type=snapshot_and_cdc` INI 渲染器漏掉 `server_id` → CDC 阶段无法启动。
- **BUG-013** [minor]: `/api/tasks` validation 要求 `server_id` 为字符串，数字传入报 `required`，错误信息误导。

### 认证 / 会话
- **BUG-004** [blocker]: `POST /auth/logout` 后旧 cookie 仍能访问受保护 API（**安全问题**：session 未在 DB 失效）。
- **BUG-005** [major]: Logout 后刷新页面不跳 login（是 BUG-004 下游症状）。

### 任务详情页
- **BUG-001** [major]: `stop while running` 状态翻转逻辑依赖 test 对 `stopped|stopping` 的硬断言，实际终态常是 `finished`（空表 snapshot 秒退）。部分 test 缺陷。
- **BUG-006** [major]: `GET /api/runs/:id` 返回 camelCase `stoppedAt/exitCode`，test 读 snake_case → 永远 null。**test 缺陷**。
- **BUG-014** [major]: Detail 页「同步配置」tab 的 parallel_size / buffer_size / checkpoint_interval 展示**默认值**（1 / 4 rows / 10s），而后端存储正确（2 / 4000 / 1s）。展示层 bug。

### Wizard
- **BUG-000/003** [major, 合并]: wizard step 1 测试 selector 依赖 placeholder，但 wizard 默认填充 value → Element Plus 自动抑制 placeholder。**test 设计缺陷**（修 testid）。
- **BUG-011** [major]: `/tasks/create/snapshot` wizard 暴露「增量」(cdc) sub-mode 按钮，违反 ADR-0006 taxonomy；后端 validation 直接 reject `kind=snapshot + extract_type=cdc`。UI 与 backend 不一致。
- **BUG-012** [major]: 任务名重复校验不随输入更新，改名后红字不清，下一步永远 disabled，**用户无法创建任务**（需刷新丢 state）。

### 列表页工具栏
- **BUG-007** [major]: 「按状态/吞吐/源排序」按钮点击不发 `?sort=` 参数，列表不变，前端无本地 sort 兜底。
- **BUG-009** [major]: 「导出」按钮点击完全无响应（不发请求、无 UI 变化）。handler 未实现或被 Icon 渲染断链。
- **BUG-010** [major]: `TaskListView.vue` 引用 15+ 个 `IconXxx` 未注册组件，模板 Vue runtime warning 海量刷屏，所有工具栏按钮无图标。
- **BUG-008** [minor]: el-table 行 checkbox a11y 路径点击无效，批量操作无法通过自动化测试触发。
- **BUG-018** [major]: 工具栏「创建任务」「批量导入」「下载模板」按钮对 `viewer_e2e` 可见可点击，**越权风险**（行级 RBAC 正确但工具栏 RBAC 缺失）。

### Dashboard
- **BUG-017** [major]: Dashboard 的 4 张 KPI 卡全是纯 StaticText，没有深链；`page-map.md:72-74` 承诺的 `KPI → /tasks/snapshot?status=running` 等全部未实现。

### 既有 spec 缺陷（归类在 test 侧）
- BUG-000/003/006 分类属测试代码问题，不是产品 bug。但仍需修，否则回归 gate 永远不绿。

## 分类结论

| 类别 | 数量 | 下一步动作 |
|---|---|---|
| 产品 bug（后端） | 4 | BUG-002, BUG-004, BUG-013, BUG-016 需 Rust 侧改 |
| 产品 bug（前端） | 9 | BUG-007/009/010/011/012/014/017/018 + 共享 BUG-005 |
| 测试代码 bug | 3 | BUG-000/003/006 修 spec selector 和字段名 |
| 文档 drift | 1 | `page-map.md` Dashboard 链接章节需和 BUG-017 修复同步更新 |
| not-a-bug | 1 | BUG-015（intentional engine constraint） |

## CONTEXT / ADR 更新建议

1. **CONTEXT.md** — 在 "Flagged ambiguities" 段下追加：
   > "sync mode" / "syncMode" — 原型内部变量名，现仅作为 wizard form 字段映射到 `extract_type`。不要与 **Engine Sub-Mode**（GaussDB pg-mode/mysql-mode/oracle-mode）混淆。UI 代码里出现 "sub-mode" 这个词时必须指明上下文。

2. **ADR-0006** — 不需新建 ADR。已有约束明确；BUG-011 是代码未遵循既定决策。

3. **page-map.md** — 修复 BUG-017 后同步更新 Cross-page links 章节，把 Dashboard KPI → snapshot list 的链接标"实现"或标"TODO"。

## 下一阶段：修复优先级

按 severity + 依赖顺序：

1. **BUG-002**（blocker）：snapshot 空目标库 panic → 修 `dt-main` 或加 Wizard "迁移结构" 预选项
2. **BUG-004**（blocker）：logout 不失效 session → 修 `auth_handlers::logout`
3. **BUG-016**（blocker）：server_id 渲染漏洞 → 修 `ini_renderer.rs` extractor 段
4. **BUG-010**（major，阻塞 UX）：Icon 组件未注册 → 修 `TaskListView.vue` + auto-import 配置
5. **BUG-012**（major，阻塞用户建任务）：任务名重复校验不更新
6. **BUG-011**（major，taxonomy）：wizard 过滤 "增量" sub-mode
7. **BUG-018**（major，安全）：工具栏 RBAC
8. **BUG-014**（major，信任）：detail 展示真实值
9. **BUG-009**（major，UX）：导出按钮 handler
10. **BUG-007**（major，UX）：排序参数
11. **BUG-017**（major，UX）：Dashboard KPI 深链
12. **BUG-005**（伴随 BUG-004 一并修好）
13. **BUG-000/003/006**（test 代码）：修 selector + 字段 camelCase
14. **BUG-001**（test，扩断言）
15. **BUG-013, BUG-008**（minor）：时间允许再修
