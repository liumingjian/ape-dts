# Oracle 原生驱动选型调研(替换 sqlplus 文本管道)

> 调研日期:2026-08-11
>
> 文档定位:这是该日期的静态调研快照,不是当前能力或缺陷的权威清单;后续提交、crate 发版或 Oracle 许可条款变化都可能改变文中结论,落地前请复核关键事实
>
> 对应 wayfinder ticket:`.scratch/fix-map/issues/22-oracle-driver-research.md`
>
> 调研范围:`oracle` crate(rust-oracle/ODPI-C)现状与许可、Instant Client 分发与 distroless/musl 部署可行性、类型与批量 DML 覆盖、LogMiner 会话可行性、并发与连接池模型、备选方案对比,以及推荐方案与分批落地建议
>
> 调研方式:阅读本仓 Oracle 连接器代码(extractor/sinker/公共客户端)确定需求面,再对照一手来源(crates.io API、rust-oracle/ODPI-C 仓库与官方文档、docs.rs、Oracle 官网许可与下载页、Oracle Database Utilities 文档)逐条核实;未做实际编译与连库验证
>
> 说明:本报告未修改业务代码;所有版本号、日期、许可条款均以引用来源当日快照为准

---

## 1. 现状与需求面(为什么要换)

当前 Oracle 连接完全走 `sqlplus` 文本管道,单一入口是
`dt-connector/src/oracle/mod.rs` 的 `OracleSqlPlusClient`:

- 每次 `exec`/`query_lines` 都 spawn 一个新的 `sqlplus` 进程(本地或 `docker exec`),口令拼进登录串进入 argv(`build_login`,`{user}/{password}@//host:port/service`);
- 查询输出用 `SET COLSEP '|'` + `SET NULL '<NULL>'` 文本协议回传,行内出现 `|`、换行即损坏数据,`<NULL>` 与真实字符串 `'<NULL>'` 无法区分;
- `ORACLE_HOME` 硬编码 `/u01/app/oracle/product/11.2.0/xe`(11.2 XE 容器路径);
- 错误检测靠扫描 stdout 中的 `ORA-`/`SP2-` 前缀。

消费方(读它们可确认真实需求面):

| 消费方 | 文件 | 用法 | 对驱动的要求 |
|---|---|---|---|
| 快照抽取 | `dt-connector/src/extractor/oracle/oracle_snapshot_extractor.rs` | 全表 `SELECT ... ORDER BY`,一次性 `query_lines` 后按 `|` split,再按 `all_tab_columns` 的 data_type 文本解析 | 类型化 fetch、流式游标(现状是整表进内存)、分批 |
| Bootstrap 触发器 CDC | `dt-connector/src/extractor/oracle/oracle_cdc_extractor.rs` | 轮询自建 `APE_DTS_CDC_LOG` 表,行图像用 `<DT_SEP>` 拼串规避 `|` 冲突 | 普通查询 + 位点推进 |
| LogMiner CDC | `dt-connector/src/extractor/oracle/oracle_logminer_cdc_extractor/{extractor,logminer,sql_parser}.rs` | 因为每次调用都是新 sqlplus 进程,被迫在同一脚本里 `ADD_LOGFILE → START_LOGMNR → SELECT V$LOGMNR_CONTENTS → END_LOGMNR`(见 `logminer.rs` 注释),每轮 poll 重建整个 LogMiner 会话;`sql_redo`/`sql_undo` 再交给 `sql_parser` 文本反解 | **持久会话**、PL/SQL 调用、大文本列(sql_redo 可超长且含换行,文本管道下天然易碎) |
| DML 写入 | `dt-connector/src/sinker/oracle/oracle_sinker.rs` | 逐行拼 `INSERT/UPDATE/DELETE` 字面量 SQL,`';\n'.join` 后整批 `exec`,靠 `''` 转义 | 参数绑定、数组 DML(executemany)、二进制/LOB 绑定 |
| 结构迁移/校验 | `oracle_struct_extractor.rs`、`oracle_struct_sinker.rs`、`oracle_checker.rs` | 元数据查询 + DDL 执行 | 普通查询/执行即可 |

构建/发布链约束:

- `rust-toolchain.toml` 固定 `1.85.0`;
- `Cross.toml` 目标为 `x86_64/aarch64-unknown-linux-gnu`(glibc),显式注释放弃了 `crt-static`;
- `Dockerfile` 最终镜像 `gcr.io/distroless/cc`(Debian 基线、glibc),`LIBC=musl` 仅是切 alpine 时的备选参数。

---

## 2. 候选一:`oracle` crate(rust-oracle,基于 ODPI-C)

### 2.1 版本、维护、MSRV、许可

| 项 | 结论 | 来源 |
|---|---|---|
| 最新稳定版 | 0.6.3(2025-01-02 发布);crates 总下载 ~245 万,近端 ~95 万 | crates.io API:<https://crates.io/api/v1/crates/oracle> |
| 维护活跃度 | 仓库最后 push 2025-03-23(升级 ODPI-C 到 5.5.0、迁 CI 到 gvenzl/oracle-free),master 为 0.7.0-dev;open issues 26,单一维护者 @kubo,228 stars。**调研时点已约 17 个月无发版**,活跃但节奏偏慢 | GitHub API `repos/kubo/rust-oracle`(pushed_at=2025-03-23)、<https://github.com/kubo/rust-oracle> |
| MSRV | 0.6.3 标注 `rust-version = "1.60.0"`;master(0.7.0-dev)为 1.68.0。**均远低于本仓 1.85.0,兼容** | <https://github.com/kubo/rust-oracle/blob/v0.6.3/Cargo.toml>、<https://raw.githubusercontent.com/kubo/rust-oracle/master/Cargo.toml> |
| License | `UPL-1.0/Apache-2.0` 双许可(crate 与 ODPI-C 一致),对商用/再分发无障碍 | 同上 Cargo.toml;ODPI-C README:<https://github.com/oracle/odpi> |
| ODPI-C 绑定方式 | 0.6.3 依赖 `odpic-sys = 0.1.1`(对应 ODPI-C 5.4.1),master 用 `odpic-sys = 0.2.0`(ODPI-C 5.5.0);ODPI-C C 源码由 `cc` 在构建期本地编译,**构建期不需要任何 Oracle 库** | v0.6.3 Cargo.toml(同上);ODPI-C 安装文档(下节) |
| ODPI-C 上游 | Oracle 官方项目,持续活跃:v5.6.3(2025-10)、v5.6.4(2025-11)、v6.0.0(2026-05)。注意 rust-oracle 尚停在 5.5.0 线 | GitHub API `repos/oracle/odpi/releases` |

### 2.2 运行时模型:ODPI-C 动态加载 Instant Client

ODPI-C 官方安装文档(<https://odpi-c.readthedocs.io/en/latest/user_guide/installation.html>):

- "ODPI-C dynamically loads available Oracle Client libraries at runtime. This allows code using ODPI-C to be built only once, and then run using any available Oracle Client 21, 19, 18, 12, or 11.2 libraries." —— 即 **编译期零 Oracle 依赖,运行期 dlopen `libclntsh`**;
- Linux 查找顺序:`oracleClientLibDir` 参数 → 可执行文件同目录 → `LD_LIBRARY_PATH` 等系统路径 → `$ORACLE_HOME/lib`;用 Instant Client 时其目录必须在系统库搜索路径中;
- 只需 Instant Client **Basic 或 Basic Light** 包。

对本仓的直接含义:`cross` 的 gnu 目标构建**完全不受影响**(只多一个 C 编译步骤,cross 镜像自带 gcc;现有 pre-build 已装 cmake/libclang,无需新增 Oracle 相关步骤);CI 的 `cargo check/clippy/test` 不需要 Instant Client,只有真正连库的 e2e 需要。

### 2.3 Instant Client 许可与镜像分发约束

Instant Client 的许可是 **OTN Development and Distribution License Terms for Instant Client**(<https://www.oracle.com/downloads/licenses/instant-client-lic.html>,以下为该页原文摘录):

> "License. We grant you a non-exclusive right and license to use the Programs solely for your business purposes and development and testing purposes …"
>
> "Distribution License. We grant you a non-exclusive right and license to distribute the Programs, provided that you do not charge your end users for use of the Programs. Your distribution of such Programs shall at a minimum include the following terms in an executed license agreement between you and the end user …"(后接限制转让、限制第三方使用、Oracle 保留所有权等一系列必须传导给终端用户的条款)

解读(非法律意见):

- **允许免费再分发**(不得就 Instant Client 本身向最终用户收费),但要求分发者与终端用户之间有一份"executed license agreement"承载 Oracle 规定的最低条款;
- 对一个公开拉取的开源 Docker 镜像来说,"与每个拉取者签署协议"不现实,**默认发布镜像不应预装 Instant Client**;
- 合规的替代路径:①文档指导用户在自己的构建/部署环节从 Oracle 官网永久链接自取(下载页提供 Basic/Basic Light zip 的 permanent links);②volume 挂载宿主机已装的 IC;③提供一个 opt-in 的 `Dockerfile` target,由用户本地 build 时下载(用户成为被许可人);④基于 Oracle 官方 container-registry 的 instantclient 镜像做变体。Oracle 同时在 yum.oracle.com 提供免点击许可页的 RPM("Instant Client RPMs are also available without click-through from yum.oracle.com",下载页原文),但那是面向 Oracle Linux 的获取渠道,不改变我们二次分发的义务。

### 2.4 distroless / musl 部署可行性

一手事实(Instant Client Linux x86-64 下载页,<https://www.oracle.com/database/technologies/instant-client/linux-x86-64-downloads.html>,2026-08 快照):

- 23.26.3 要求 **glibc 2.28**;21.23 / 19.32 要求 **glibc 2.14**;所有版本只发 glibc 二进制,**没有 musl 构建**;
- 安装说明要求 OS 装 `libaio`("Install the operating system libaio package. This is called libaio1 on some Linux distributions. On Oracle Linux 8 prior to Instant Client 21 you also need the libnsl package");node-oracledb 官方安装文档对 19/21/23ai 一致要求 libaio(<https://node-oracledb.readthedocs.io/en/latest/user_guide/installation.html>);
- 客户端-服务端互操作:"Oracle Call Interface 19.3 can connect to Oracle Database 11.2 or later"(下载页援引 MOS Doc ID 207303.1)。本仓 dt-tests 用 11.2 XE,**IC 19 是同时覆盖 11.2 老库与新库的安全选择**;
- musl/Alpine:Oracle 从未发布 musl 版客户端库;社区在 Alpine 上加载 glibc 版 IC 报符号错误(如 node-oracledb Alpine issue:<https://github.com/oracle/node-oracledb/issues/476>,`getcontext` relocation error)。**结论:任何 OCI 系驱动(rust-oracle、sibyl、自研 FFI)都与 musl 目标不兼容。**

对本仓发布链的影响:

- 现有默认路径(cross gnu 目标 + `gcr.io/distroless/cc`,Debian 基线 glibc)**可行**:distroless/cc 自带 glibc/libstdc++(当前 Debian 12 基线 glibc 2.36,满足 2.28/2.14 要求;落地时以镜像实际版本复核),需要额外 COPY 进 `libaio.so.1` 和 Instant Client 目录,并设 `LD_LIBRARY_PATH`(或把 IC 放在二进制同目录,利用 ODPI-C 的同目录查找);
- `Dockerfile` 的 `LIBC=musl`/alpine 备选路线在启用 Oracle 驱动的构建里**必须放弃或把 Oracle 特性做成 feature 开关**;
- 二进制本身不链接 Oracle 库,所以**没有 Instant Client 的环境仍可运行非 Oracle 任务**——只有真正建 Oracle 连接时 dlopen 失败才报错,不影响主发布镜像保持现状。

### 2.5 类型覆盖

来源:docs.rs `oracle::sql_type`(<https://docs.rs/oracle/latest/oracle/sql_type/index.html>、<https://docs.rs/oracle/latest/oracle/sql_type/enum.OracleType.html>):

| 需求类型 | 支持 | 说明 |
|---|---|---|
| CLOB/BLOB/NCLOB | ✅ | `sql_type::Clob/Blob/Nclob` 句柄 + `io` 模块流式读写;也可直接按 `String`/`Vec<u8>` 整值 fetch/bind |
| RAW / LONG RAW | ✅ | `OracleType::Raw(size)` / `LongRaw`,Rust 侧 `Vec<u8>` |
| TIMESTAMP [WITH TIME ZONE / LOCAL TIME ZONE] | ✅ | `OracleType::Timestamp/TimestampTZ/TimestampLTZ(fsprec)`,`sql_type::Timestamp` 携带时区偏移;可开 `chrono` feature 与 `chrono` 类型互转 |
| INTERVAL | ✅ | `sql_type::IntervalDS`/`IntervalYM`(`OracleType::IntervalDS(lfprec, fsprec)`/`IntervalYM`) |
| NUMBER 精度 | ✅ | `OracleType::Number(prec, scale)`,可按 `String` fetch 保精度(替代现 `ColValue::Decimal` 文本路径) |
| 未覆盖 | ⚠️ | Object/collection 类型支持不完整、无 scrollable cursor(README 自述);对本仓需求面无影响 |

**批量 DML(替代拼 SQL 文本的 sinker)**:`Connection::batch(sql, batch_size)` 返回 `BatchBuilder`,`append_row(&[&dyn ToSql])`/`append_row_named` + `execute()`,即 executemany/数组绑定;`with_batch_errors()`(要求 Oracle 12.1+)可拿到逐行错误与行号(来源:<https://docs.rs/oracle/latest/oracle/struct.Batch.html>,文档示例即 1000 行一批的 insert)。据此 sinker 可改为「预编译 3 类语句 + 数组绑定」,彻底消除字面量拼接、`''` 转义和 hextoraw 文本化,BLOB 直接按字节绑定。注意 11.2 目标库上不能用 `with_batch_errors`,退化为整批失败后逐行重放的策略即可(与现有 MySQL/PG sinker 的 batch→serial 退化一致)。

### 2.6 LogMiner 可行性

Oracle Database Utilities 19c LogMiner 章(<https://docs.oracle.com/en/database/oracle/oracle-database/19/sutil/oracle-logminer-utility.html>)关键事实:

- "you must call `DBMS_LOGMNR.START_LOGMNR` before querying the `V$LOGMNR_CONTENTS` view" —— LogMiner 状态是**会话级**的,`V$LOGMNR_CONTENTS` 只在启动了 LogMiner 的那个会话内可查(现有 `logminer.rs` 注释也正是为此被迫单脚本打包);
- 权限:"You must have the `EXECUTE_CATALOG_ROLE` role and the `LOGMINING` privilege to query the `V$LOGMNR_CONTENTS` view and to use the LogMiner PL/SQL packages";
- 需先开 supplemental logging(`ALTER DATABASE ADD SUPPLEMENTAL LOG DATA`);字典用 redo 承载时需 ARCHIVELOG;
- `CONTINUOUS_MINE` 在 19c 已 desupport —— 现有"显式 ADD_LOGFILE + SCN 窗口"的设计方向本来就是对的,不需要改。

用 rust-oracle 实现的映射关系:

1. `Connection` 就是一条持久 OCI 会话(dedicated 连接;不要对 LogMiner 连接启用任何池化/DRCP),`conn.execute("BEGIN DBMS_LOGMNR.ADD_LOGFILE(...); END;", ...)` / `START_LOGMNR` / 随后在**同一个 `Connection`** 上 `query` `V$LOGMNR_CONTENTS`,天然满足同会话要求——这正是 Debezium 等 JDBC 实现的同构做法;
2. 流式读取:`Statement::query` 返回逐行迭代的 `ResultSet`(底层按 fetch array size 分批取),`sql_redo` 含换行/`|` 不再是问题,超长 redo 也可靠(必要时 `PRINT_PRETTY_SQL` 都不需要);
3. 会话生命周期:可以保持连接常驻、每个 SCN 窗口 `START_LOGMNR` → 读完 → `END_LOGMNR`,省掉每轮 poll 的进程 spawn 与全量重建;连接断开时 LogMiner 状态随会话消失,重连后按 checkpoint 的 `(scn, rs_id, ssn)` cursor 重新 START 即可,与现有 resume 语义兼容;
4. 阻塞长查询的取消:`Connection` 提供 `break_execution`(ODPI-C `dpiConn_breakExecution`),可在 shutdown 时打断正在 fetch 的 V$LOGMNR_CONTENTS 查询(落地时验证其在目标 client 版本的行为)。

**结论:LogMiner 场景完全可行,且是收益最大的一块**(去掉"每轮重建 LogMiner 会话 + 文本反解 sql_redo 的分隔符风险"两大脆弱点)。

### 2.7 并发模型与连接池

- rust-oracle 是**纯阻塞**驱动(README/docs 无任何 async 支持);`Connection` 实现 `Send + Sync`(docs.rs Connection 页 Auto Trait Implementations:<https://docs.rs/oracle/latest/oracle/struct.Connection.html>),可跨线程移动/共享;
- 与 tokio 协作的两种成熟形态:
  1. **`tokio::task::spawn_blocking`**:适合快照抽取、sinker、元数据查询这类"一次调用一段阻塞工作"的模式;本仓 extractor/sinker trait 都是 `async fn`,在实现内部把持有 `Connection` 的闭包丢进 spawn_blocking 即可,改动局部;
  2. **专用 OS 线程 + channel**:适合 LogMiner 常驻会话——单独一条线程独占 `Connection` 顺序执行 START/fetch/END,经 mpsc 把行推回 async 侧,天然保证"同会话 + 顺序性",也避免 spawn_blocking 线程池被长 fetch 占满;
- 连接池:`r2d2-oracle` 0.7.0(2024-05-30,MIT/Apache-2.0)与 `bb8-oracle` 0.3.0(2025-03-13,MIT/Apache-2.0)都存在(crates.io API)。但本仓形态(每个任务固定少量长连接:快照 N 路、sinker N 路、LogMiner 1 路)用不上通用池,**建议自持 `Vec<Connection>`/每并行度一连接,不引入池依赖**;LogMiner 绝对不能走池。

---

## 3. 候选二(备选):sibyl 与直接 OCI FFI

- **sibyl** 0.7.1(2026-06-26,MIT,<https://github.com/quietboil/sibyl>):直接绑 OCI,同时提供 blocking 与 nonblocking(tokio/actix)API,更新比 rust-oracle 勤,但生态小(总下载 ~5.7 万,约为 oracle crate 的 1/40)、同样依赖 glibc 版 Oracle 客户端库,async 模式建立在 OCI 非阻塞模式上、路径更少人踩——作为 rust-oracle 停更时的 Plan B,不作首选。
- **自研 OCI FFI**:OCI 面积巨大(句柄体系、descriptor、LOB locator、批量绑定、错误链),ODPI-C 存在的意义就是替你写掉这层;自研仅在上面两者都不可用时才值得,一句话:不做。
- **纯 Rust 线协议(TNS)实现**:调研时点无生产级 crate,Oracle 线协议无公开规范,不可行。

---

## 4. 推荐方案

**采用 `oracle` crate 0.6.3(ODPI-C)+ Instant Client 19(Basic Light),阻塞调用统一经 spawn_blocking/专用线程封装;发布镜像默认不打包 Instant Client,提供 opt-in 的 Oracle-enabled 镜像构建路径。**

理由:license 干净(UPL/Apache 双许可)、MSRV 1.60 ≪ 1.85、编译期零 Oracle 依赖不动 cross 发布链、类型/批量绑定/LogMiner 三块需求全覆盖、是 Rust 生态事实标准(245 万下载);唯一实质短板是发版节奏慢,已用 sibyl 兜底。

建议封装形态:新建 `OracleNativeClient`(内部持 `oracle::Connection` + spawn_blocking 封装,暴露与现有 `exec`/`query_lines` 对齐的 async API 外加类型化 `query_rows`/`batch_dml`),让各消费方逐个迁移,`OracleSqlPlusClient` 保留到全部切换完成后删除。

### 4.1 迁移风险清单

| # | 风险 | 等级 | 缓解 |
|---|---|---|---|
| R1 | Instant Client 再分发合规:OTN 条款要求向终端用户传导许可,公开镜像不宜预装 | 高 | 默认镜像不含 IC;文档 + opt-in Dockerfile target(构建者自行从 Oracle permalink 下载);运行时挂载路径支持 `LD_LIBRARY_PATH` |
| R2 | musl/alpine 路线与 OCI 驱动根本不兼容 | 中 | 把 Oracle 连接器做成 cargo feature(默认开,musl 构建关)或明确宣布 musl 镜像不支持 Oracle |
| R3 | rust-oracle 发版停滞(最近 0.6.3 为 2025-01,master 未跟进 ODPI-C 6) | 中 | 锁定 0.6.3;关注上游;必要时切 sibyl 或 fork 升级 odpic-sys |
| R4 | dt-tests 现走 `docker exec sqlplus`,驱动化后 CI/e2e 机器需装 IC + libaio | 中 | e2e runner 增加 IC 安装步骤(zip 解压 + `LD_LIBRARY_PATH`);单测/`cargo check` 不受影响(运行期才 dlopen) |
| R5 | 类型语义迁移:NUMBER 精度、DATE vs TIMESTAMP、TZ 归一化与现文本解析结果可能有细微差异 | 中 | 迁移时逐类型写对拍用例(sqlplus 输出 vs 驱动输出),checker 链路先行验证 |
| R6 | 11.2 XE 目标库限制:`with_batch_errors` 需 12.1+;IC 23 不能连 11.2 | 低 | 选 IC 19;batch 失败退化为逐行重放 |
| R7 | 阻塞调用占用 tokio blocking 线程池;LogMiner 长 fetch 尤甚 | 低 | LogMiner 用专用 OS 线程;必要时调 `max_blocking_threads`;shutdown 用 `break_execution` |
| R8 | distroless 镜像需补 `libaio.so.1` 与 IC 目录、体积 +~80MB(Basic)/更小(Basic Light) | 低 | 仅 Oracle-enabled 镜像变体承担;选 Basic Light |

### 4.2 分批落地建议(先 snapshot/sinker,后 LogMiner)

1. **第 1 批:公共客户端 + snapshot extractor + sinker**(风险最低、消除面最大)
   - 引入 `oracle` crate 与 `OracleNativeClient`;快照抽取改为类型化流式游标(顺带解决现"整表 `query_lines` 进内存"的问题);sinker 改为预编译语句 + `Batch` 数组绑定;struct extractor/sinker、checker 的元数据查询同步切换(纯查询,搭车即可)。
   - 该批不改位点/会话语义,回归面清晰:用现有 Oracle e2e/checker 对拍即可。
2. **第 2 批:bootstrap 触发器 CDC**
   - 查询路径直接换客户端,删除 `<DT_SEP>` 拼串协议;位点语义不变。
3. **第 3 批:LogMiner CDC**(收益最大、语义最重,放最后)
   - 专用线程 + 常驻 `Connection`,窗口式 START/END;`V$LOGMNR_CONTENTS` 流式 fetch;保留 `(scn, rs_id, ssn)` cursor 与 resume 语义;之后再评估是否用列级 redo 信息替代 `sql_parser` 文本反解(独立后续票)。
4. **收尾**:删除 `OracleSqlPlusClient` 与 `ORACLE_SQLPLUS_DOCKER_CONTAINER` 路径,更新部署文档(IC 安装/挂载指引)与 Dockerfile 变体。

每批完成后按仓库红线跑 `bash scripts/e2e/mysql_to_postgresql_redline.sh`(改动触及 extractor/sinker 公共面),Oracle 侧用 dt-tests 的 Oracle 用例回归。

---

## 5. 引用来源汇总

- 本仓代码:`dt-connector/src/oracle/mod.rs`、`dt-connector/src/extractor/oracle/**`、`dt-connector/src/sinker/oracle/**`、`Cross.toml`、`Dockerfile`、`rust-toolchain.toml`
- crates.io API:`/api/v1/crates/oracle`、`/sibyl`、`/r2d2-oracle`、`/bb8-oracle`
- rust-oracle:<https://github.com/kubo/rust-oracle>(v0.6.3 与 master Cargo.toml、README)、GitHub API(pushed_at、commits、tags)
- docs.rs:`oracle::sql_type`、`oracle::sql_type::OracleType`、`oracle::Connection`、`oracle::Batch`
- ODPI-C:<https://github.com/oracle/odpi>(README/license)、<https://odpi-c.readthedocs.io/en/latest/user_guide/installation.html>(运行时加载/查找路径)、GitHub API releases
- Oracle 官网:OTN Instant Client 许可 <https://www.oracle.com/downloads/licenses/instant-client-lic.html>;Instant Client Linux x86-64 下载页(glibc/libaio/互操作)<https://www.oracle.com/database/technologies/instant-client/linux-x86-64-downloads.html>
- Oracle Database Utilities 19c LogMiner:<https://docs.oracle.com/en/database/oracle/oracle-database/19/sutil/oracle-logminer-utility.html>
- 佐证(Oracle 第一方项目文档):node-oracledb 安装文档(glibc/libaio)<https://node-oracledb.readthedocs.io/en/latest/user_guide/installation.html>;Alpine 不兼容实例 <https://github.com/oracle/node-oracledb/issues/476>
