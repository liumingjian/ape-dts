# vendored 依赖(sqlx / rust-postgres)升级路径与 RUSTSEC 消除方案调研

> 调研日期：2026-08-11
>
> 对应 wayfinder ticket：`.scratch/fix-map/issues/23-vendor-upgrade-research.md`
>
> 文档定位：这是该日期的静态调研快照，不是当前能力或缺陷的权威清单；后续提交可能已经修复或改变文中的结论
>
> 调研范围：`vendor/sqlx`(0.6.2)与 `vendor/rust-postgres`(tokio-postgres 0.7.1)的来源考证与三层 diff 清单、sqlx 0.6→0.8/0.9 与 rust-postgres 0.7.1→0.7.18 的升级面评估、apecloud/reqwest fork 的必要性、RUSTSEC 三件套(rsa 0.6.1 / idna 0.2.3 / protobuf 2.28.0)的消除路径，以及分步实施建议
>
> 调研方式：本仓 git 历史与 `Cargo.lock` 考证；下载上游 release tag 与 apecloud fork 具体 commit 的源码 tarball 做逐文件 diff；核对上游 CHANGELOG、上游 master 源码与 RUSTSEC advisory 原文。每条结论均注明来源
>
> 说明：本报告为纯调研，未修改任何业务代码或 vendor 代码

---

## 1. 结论摘要(TL;DR)

**推荐「provenance + patch series 先行，升级后置、分阶段做」**，不推荐立即整体升级。核心理由:

1. vendor 实际上是**三层结构**(上游 tag → apecloud fork → 本仓本地 GaussDB 层),其中 rust-postgres 的 fork 层是一整套**上游至今(0.7.18, 2026-06-12)仍未合并的 CopyBoth/逻辑复制协议支持**(约 1600 行,PG/GaussDB CDC 的地基),升级 = 重新 rebase 这套补丁,风险高、安全收益为零(rust-postgres 依赖链上没有任何 RUSTSEC)。
2. RUSTSEC 三件套没有一个能靠「升级 vendor/sqlx 或 vendor/rust-postgres」立刻消除: rsa 的 advisory **无任何已修复版本**(sqlx 0.8 也只是换成同样被标记的 rsa 0.9);idna 0.2.3 **来自 mongodb 2.5.0,不来自 reqwest fork 链**(本次调研纠正了 ticket 中的假设);protobuf 2.28 来自 orc-format fork,需要单独动 fork 或换库。
3. 因此第一步价值最大的是把三层 diff 固化为 provenance 记录 + 可重放的 patch series(本文 §2 已给出完整逐文件清单,可直接生成),并用 cargo-deny ignore + 书面理由登记三个 advisory;随后按 §7 的阶段推进真正的升级。

各升级项的独立评估:

| 项目 | 结论 | 面积 |
|---|---|---|
| sqlx 0.6.2 → 0.8.6 | 可行且值得做(第二阶段);GaussDB patch 重做面积**小~中**;StarRocks 两处私改在 0.8 已被上游参数化,可直接删除 | vendor patch 小,业务侧迁移中 |
| sqlx → 0.9.0 | 暂缓:MSRV 1.94,本仓 toolchain 钉在 1.85.0;但 0.9 的 `mysql-rsa` 可选特性是彻底移除 rsa 的唯一路径 | 后置 |
| rust-postgres 0.7.1 → 0.7.13+/0.7.18 | 不建议主动升级:复制协议 backport 需整体 rebase,无安全收益 | 大,收益低 |
| reqwest fork → 上游 0.12 | fork 的能力(重定向保留敏感 header)上游 0.12 至今没有;但可用「手动处理 307」替代后弃用 fork,消掉 hyper 0.14 旧链路 | 中小,建议第一阶段做 |

---

## 2. 本仓事实考证(provenance)

### 2.1 vendor 引入历史

- vendor/ 目录在全部历史中只被一个提交触碰过:`f3f01b2f`("feat: support GaussDB bidirectional sync console E2E",作者日期 2026-05-22),一次性加入 734 个文件。(来源:`git log --all --oneline -- vendor/`、`git diff-tree --name-only -r f3f01b2f -- vendor/ | wc -l`)
- 该提交同时把 `Cargo.toml` 里的 git 依赖切成 path 依赖(来源:`git diff f3f01b2f~1..f3f01b2f -- Cargo.toml`):
  - `sqlx = {git = "https://github.com/apecloud/sqlx", ...}` → `{path = "vendor/sqlx", ...}`
  - `tokio-postgres/postgres-protocol/postgres-types = {git = "https://github.com/apecloud/rust-postgres"}` → `{path = "vendor/rust-postgres/..."}`,并新增 `postgres-openssl` path 依赖
- **确切的 fork 基线 commit** 来自 `git show f3f01b2f~1:Cargo.lock`:
  - `apecloud/sqlx` @ `032ae40caf58b49598a22e1184346bf112d237ab`(sqlx 0.6.2)
  - `apecloud/rust-postgres` @ `39a35f10cb9dca1ef0fd988100b22716dfbd5c7d`(tokio-postgres 0.7.1 / postgres-protocol 0.6.1 / postgres-types 0.2.1 / postgres 0.19.1)
  - `apecloud/reqwest` @ `a9c1f4b9e613ae38439ee4ede06786c54aca87bd`(reqwest 0.11.22;至今仍是 git 依赖,未 vendor)
- 上游基线:`vendor/sqlx` 版本 0.6.2、CHANGELOG 顶部 `## 0.6.2 - 2022-09-14`,与 launchbadge/sqlx 的 `v0.6.2` tag 逐文件比对吻合;`vendor/rust-postgres` 与 sfackler/rust-postgres 的 `tokio-postgres-v0.7.1` tag(2021-04-03)吻合。(来源:下载两个 tag 的 tarball 与 vendor 逐文件 `diff -rq`)

### 2.2 三层结构与完整改动清单

把「上游 tag」「apecloud fork commit」「vendor 现状」三方 tarball 互相 diff,得到清晰的三层:

**vendor/sqlx = launchbadge/sqlx v0.6.2 + fork 层(3 个文件) + 本地层(9 个文件)**

fork 层(apecloud@032ae40 相对 v0.6.2;StarRocks 适配):

| 文件 | 改动 |
|---|---|
| `sqlx-core/src/mysql/options/connect.rs` | 注释掉连接初始化的 `SET sql_mode=(SELECT CONCAT(@@sql_mode,...))`(StarRocks 报 "Set statement only support constant expr"),仅保留 `SET time_zone='+00:00'` |
| `sqlx-core/src/query.rs` | 新增 `Query::disable_arguments()`:置空 arguments,让 SQL 走文本协议不 prepare(StarRocks 不支持 prepare) |
| `CHANGELOG.md` | 无关紧要的注释补充 |

本地层(vendor 相对 apecloud@032ae40,即 f3f01b2f 时新写的 GaussDB 认证支持):

| 文件 | 改动 |
|---|---|
| `sqlx-core/src/postgres/connection/gaussdb.rs` | **新增**,157 行:GaussDB SHA256(RFC5802 变体, PBKDF2-HMAC-SHA1 + HMAC-SHA256 proof)、MD5-SHA256、SHA256-MD5 三种口令算法 |
| `sqlx-core/src/postgres/connection/establish.rs` | 认证循环增加 `Authentication::GaussDbSha256 / GaussDbMd5Sha256` 两个分支 + `gaussdb_md5_salt()` |
| `sqlx-core/src/postgres/connection/mod.rs` | `mod gaussdb;` |
| `sqlx-core/src/postgres/message/authentication.rs` | auth code 10/11 的启发式分流(10 且 body 非 `SCRAM-SHA-256` 开头→GaussDbSha256;11 且 body 长 68→GaussDbMd5Sha256)+ 两个 body 解析结构 |
| `sqlx-core/src/postgres/message/password.rs` | 新增 `Password::Raw(&[u8])` 变体 |
| `sqlx-core/src/postgres/message/startup.rs` | Startup 增加 `protocol_version_minor`,版本字为 `(3<<16)\|minor` |
| `sqlx-core/src/postgres/options/{mod,parse}.rs` | `PgConnectOptions.protocol_version_minor` + URL 参数 `protocolVersion`(`351`→51、`350`→50) |
| `sqlx-core/src/sqlite/statement/unlock_notify.rs` | 纯 clippy 修饰(`let _ =` → `drop(...)`) |

**vendor/rust-postgres = sfackler/rust-postgres tokio-postgres-v0.7.1 + fork 层(复制协议 backport,~1600 行) + 本地层(GaussDB 认证)**

fork 层(apecloud@39a35f1 相对 tag;即上游**至今未合并**的 CopyBoth/逻辑复制支持,与社区流传的 replication patch 同源):

- 新增 `tokio-postgres/src/copy_both.rs`(249 行 CopyBothDuplex)、`tokio-postgres/src/replication.rs`、`tokio-postgres/tests/test/replication.rs`
- `postgres-protocol/src/message/backend.rs`:`CopyBothResponse`、`ReplicationMessage`(XLogData/PrimaryKeepAlive)、`LogicalReplicationMessage`(Begin/Commit/Origin/Relation/Type/Insert/Update/Delete/Truncate 全套解析,~650 行)
- `tokio-postgres/src/client.rs`:`copy_in_simple` / `copy_out_simple` / `copy_both_simple`
- `tokio-postgres/src/{connection,copy_in,copy_out,simple_query,transaction,generic_client,lib,config}.rs`:RequestMessages::CopyBoth 管道、`ReplicationMode`(`replication=true|database` 连接参数)等配套
- docker 测试环境调整(`wal_level = logical` 等)

本地层(vendor 相对 apecloud@39a35f1;GaussDB 认证,与 sqlx 本地层同构):

- 新增 `postgres-protocol/src/authentication/gaussdb.rs`(159 行,算法与 sqlx 版一致)
- `postgres-protocol/src/message/backend.rs`:auth code 10/11 同样的启发式分流 + 两个 body 类型
- `postgres-protocol/src/message/frontend.rs`:`startup_message_with_version()`
- `postgres-protocol/Cargo.toml`:加 `sha1`(`sha-1` 0.9)依赖
- `tokio-postgres/src/config.rs`:`protocol_version_minor` + `protocolVersion` 参数解析(351→51)
- `tokio-postgres/src/connect_raw.rs`:GaussDbSha256 / GaussDbMd5Sha256 认证分支

(以上全部来源:三方 tarball `diff -ruN`,完整 diff 存档见调研工作目录 `sqlx.diff` 530 行、`rpg.diff` 2347 行;文件清单可据此直接生成 `vendor/patches/{sqlx,rust-postgres}/{01-fork-layer,02-gaussdb-layer}.patch`。)

### 2.3 业务侧对私有 API 的依赖点(升级时的兼容清单)

- `disable_arguments()`:约 20 处调用,集中在 `dt-common/src/meta/{mysql,pg}/*`、`dt-connector/src/meta_fetcher/*`、`dt-connector/src/sinker/mysql/mysql_sinker.rs`、`starrocks_struct_sinker.rs`
- `copy_both_simple`:`dt-connector/src/extractor/pg/pg_cdc_client.rs:188`、`dt-connector/src/extractor/gaussdb/gaussdb_cdc_client.rs:751`
- `protocolVersion=351` 连接串:仅 tokio-postgres 路径(`gaussdb_cdc_client.rs` 4 处、`dt-tests` 2 处);sqlx 侧的 GaussDB 认证靠消息启发式自动生效,不依赖该参数
- `LogicalReplicationMessage`/`ReplicationMessage` 解析:pg/gaussdb CDC extractor 全线使用

---

## 3. sqlx 0.6 → 0.8 升级面

### 3.1 runtime 特性模型变化(来源:sqlx CHANGELOG 0.7.0/0.8.0 段)

- 0.7.0 起 driver 拆分为独立 crate(`sqlx-postgres`/`sqlx-mysql`/`sqlx-sqlite`),`sqlx-rt` 消失;runtime 与 TLS 特性解耦:`runtime-async-std-rustls` 这类组合特性在 0.7 被拆成 `runtime-tokio`/`runtime-async-std` + `tls-rustls`/`tls-native-tls`(0.8.0 #3399 起组合特性仅作为兼容别名保留,0.9.0 已删除)。
- **借升级切到 `runtime-tokio` 完全可行且应当做**:本仓 `dt-main`/`dt-console-server` 及全部 sinker/extractor 都跑在 tokio 上,现状是 sqlx 的 I/O 独自跑 async-std 的双运行时共存。切换后 sqlx 侧的 async-std/futures-rustls 链路消失。注意 async-std 仍是本仓直接依赖(`dt-connector` redis TcpStream、`dt-pipeline` 的 `async_std::sync::Mutex` 等),消除 sqlx 绑定不等于移除 async-std 本身,需另行迁移(独立小任务)。

### 3.2 Postgres 连接建立/认证代码在 0.8 的位置与形状(来源:v0.8.6 tarball 实测比对)

- 位置:`sqlx-core/src/postgres/*` → **`sqlx-postgres/src/*`**,文件一一对应:`connection/establish.rs`、`connection/mod.rs`、`message/{authentication,password,startup}.rs`、`options/{mod,parse}.rs` 全部还在。
- 形状:`establish()` 的认证 `loop { match message.decode()? }` 结构原样保留,仅改名(`MessageFormat`→`BackendMessageFormat`、`stream.send(...)`→`stream.write(...)+flush()`、连接字段挪进 `PgConnectionInner`)。实测 0.6.2→0.8.6 的 `message/authentication.rs` diff 仅 41 行、`startup.rs` 仅 23 行。
- **GaussDB patch 重做面积:小~中**。理由:9 个文件的落点在 0.8.6 中全部存在且角色不变,`gaussdb.rs`(157 行纯算法,零框架耦合)可原样拷贝;需要适配的只是消息编解码 trait 的签名变化(`Encode`/`FrontendMessage` 接口、`err_protocol!` 位置)与 options 结构体字段迁移,估计 1~2 天含 GaussDB 真库回归。
- **StarRocks 两处 fork 私改在 0.8 可以直接删掉**:
  - 0.8.6 的 `sqlx-mysql/src/options/connect.rs` 已把连接初始化参数化:`MySqlConnectOptions::pipes_as_concat(false).no_engine_substitution(false).timezone(...)/set_names(...)`(实测 0.8.6 源码,`sql_mode` 为空时不发 `SET sql_mode`),与 fork 注释掉的行为等价;
  - `disable_arguments()` 可被上游 `sqlx::raw_sql()`(0.7.4 引入)替代,~20 处调用点机械替换。
- 业务侧迁移成本(中):51 个文件、209 处 `sqlx::` 调用;主要破坏点是 0.7.0 的「`Transaction`/`PoolConnection` 不再直接 impl `Executor`,需 `&mut *conn` 解引用」、`ConnectOptions` builder 所有权风格、0.8 的 `Encode` 返回 `Result` 等(来源:CHANGELOG 0.7.0 #2039、0.8.0 Breaking 段)。
- 0.9.0(2026-05-06 发布)暂缓:MSRV 1.94(CHANGELOG 0.9.0 段),本仓 `rust-toolchain` 钉 1.85.0;且 `query*()` 改为 `SqlSafeStr`(#3723)对本仓大量动态拼 SQL 的代码是一次额外的全量改动。但 0.9 的 `mysql-rsa` 可选特性(#4142)与 rsa 消除直接相关,见 §6.1。

## 4. rust-postgres 0.7.1 → 0.7.13+/0.7.18 升级面

- 上游 tokio-postgres 已到 **0.7.18(2026-06-12)**;0.7.2~0.7.18 的 CHANGELOG 全文中没有 CopyBoth/replication 相关条目,并实测上游 master 的 `tokio-postgres/src/client.rs` 无 `copy_both_simple`、`postgres-protocol/src/message/backend.rs` 无 `XLogData/LogicalReplicationMessage`——**上游至今没有复制协议支持**。(来源:上游 CHANGELOG + master 源码 grep)
- 因此升级到 0.7.13+ 不是「重做 gaussdb.rs + connect_raw 两个分支」这么简单,而是**连同 ~1600 行复制协议 backport 一起 rebase**(backend.rs 解析层、copy_both.rs 管道、connection.rs 状态机都要跟着上游 0.7.x 的内部演进对齐,如 0.7.9 的 hostaddr/load_balance_hosts、0.7.11 的 Config setter 签名变化、0.7.13 的 direct TLS)。面积:**大**。
- 收益核对:tokio-postgres 0.7.1 依赖链(base64 0.13/rand 0.8/sha2 0.9 等)当前没有任何 RUSTSEC advisory 指向;0.7.2~0.7.18 的修复以功能与边角 bug 为主(唯一偏安全的是 0.7.18 "Error instead of panicking on DataRow field/column count mismatch",属健壮性)。
- **结论:暂不升级 rust-postgres**,以 patch series 固化现状;只有当出现真实需求(如需要 0.7.13 的 direct TLS negotiation)时再专项 rebase。替代观察项:gaussdb 官方 rust 驱动生态(HuaweiCloudDeveloper 的 gaussdb-rust 即 rust-postgres 系 fork)若日后携带复制支持成熟,可整体换基。

## 5. reqwest fork(redirect-with-sensitive-headers)是否仍必要

- fork 内容实测(GitHub compare `seanmonstar/reqwest v0.11.22...apecloud@a9c1f4b`):在上游 0.11.22 + 6 个上游小提交之上,只有一个真实改动——新增 cargo feature `redirect-with-sensitive-headers`,把 `src/async_impl/client.rs` 重定向路径里的 `remove_sensitive_headers(...)` 用 `#[cfg(not(feature))]` 包起来。**本质是 3 行补丁**。
- 上游 0.12 现状(实测 master `src/redirect.rs` + `async_impl/client.rs` + 全量 CHANGELOG grep):`remove_sensitive_headers` 在跨 host/port/scheme 重定向时无条件剥掉 `Authorization/Cookie/cookie2/Proxy-Authorization/WWW-Authenticate`,tower 化后的 `TowerRedirectPolicy::on_request` 同样无条件调用;**没有任何 builder 选项或 Policy hook 能保留敏感 header**。CHANGELOG 中也从无此类 opt-out(只有反方向的加固,如 "strip sensitive headers when the scheme changes")。→ **fork 提供的能力上游 0.12 至今没有**。
- 本仓调用点:`dt-task/src/sinker_util.rs`(StarRocks/Doris 与 ClickHouse sinker 各建一个 `Policy::custom(|a| a.follow())` 的 client)、`dt-connector/src/sinker/starrocks/starrocks_sinker.rs`(stream load PUT 带 `basic_auth`,FE 会 307 到另一 host:port 的 BE,正是需要跨 host 保留 Authorization 的场景)、`clickhouse_sinker.rs` 同构、`dt-console-server/src/metrics_scraper.rs`(本机 metrics 抓取,无重定向需求)。
- **替代方案(推荐)**:弃用 fork,统一到 workspace reqwest 0.12(dt-tests 已在用 0.12),对 StarRocks/Doris stream load 改为 `Policy::none()` + 手动处理 307:读 `Location` 后带认证重发 PUT(StarRocks/Doris 官方文档本就推荐处理 redirect 或直连 BE;两个 sinker 约 50 行改动)。**过渡方案**:把 fork 的 3 行补丁 rebase 到 0.12.x(补丁点变成 client.rs + redirect.rs `on_request` 两处,仍是个位数行),登记进 patch series。
- 收益:锁文件中 `reqwest 0.11.22(git)` 与 `reqwest 0.12.24` 双栈、`hyper 0.14.32` 与 `hyper 1.6.0` 双栈可归一;hyper 0.14 链(含 hyper-tls 0.5)整条消失。**注意:idna 0.2.3 不在此链上,消不掉它**(见 §6.2,ticket 中「经 reqwest fork 链引入 idna 0.2.3」的假设与锁文件不符——fork 的 url 依赖解析到 url 2.5.4 → idna 1.0.3)。

## 6. RUSTSEC 三件套消除路径

### 6.1 rsa 0.6.1 — RUSTSEC-2023-0071(Marvin timing attack)

- 引入链:`sqlx-core 0.6.2 → rsa 0.6`(MySQL `caching_sha2_password`/`sha256_password` 在非 TLS 连接下用服务器公钥加密口令)。(来源:0.6.2 tarball `sqlx-core/Cargo.toml`、本仓 Cargo.lock)
- advisory 原文:`patched = []`——**至今没有任何已修复版本**;workaround 是"避免在攻击者可观测时延的环境使用"。(来源:rustsec/advisory-db `crates/rsa/RUSTSEC-2023-0071.md`)
- 路径:
  - 随 sqlx 0.8 升级:rsa 0.6.1→0.9(0.8.6 实测 `sqlx-mysql` 依赖 `rsa = "0.9"`),依赖树更健康但 **advisory 依旧命中**;
  - **彻底消除只有 sqlx 0.9 的 `mysql-rsa` 可选特性**(#4142,默认不启用则完全不编译 rsa;代价是非 TLS + caching_sha2 的连接会运行时报错,需运维上保证 TLS 或 mysql_native_password);
  - 暂时缓解:cargo-deny `[advisories] ignore` 登记,理由书面化——sqlx 只用 rsa 做**客户端一次性公钥加密**,Marvin 攻击针对的是私钥持有方的解密/签名时延,客户端场景不成立。
- 附带事实:sqlx 0.6.2 的 mysql 认证同样依赖它,和 GaussDB patch 无关,vendor 不动它也不会更糟。

### 6.2 idna 0.2.3 — RUSTSEC-2024-0421(Punycode 混淆)

- **引入链纠正:`mongodb 2.5.0 → trust-dns-resolver 0.21.2 → trust-dns-proto 0.21.2 → idna 0.2.3`**,与 reqwest fork 无关(锁文件反查:唯一依赖 idna 0.2.3 的是 trust-dns-proto,唯一依赖 trust-dns 的是 mongodb)。(来源:本仓 Cargo.lock)
- advisory:`patched = [">= 1.0.0"]`。(来源:rustsec/advisory-db `crates/idna/RUSTSEC-2024-0421.md`)
- 消除路径:**升级 mongodb 2.5 → 3.x**(实测 crates.io:mongodb 3.2.5 依赖 hickory-proto/resolver ^0.24.2,hickory-proto 0.24.2 依赖 idna ^1.0)。mongodb 3.x 是 driver 大版本(API 有破坏性变化,dt-connector 的 mongo extractor/sinker 需要一轮适配),是独立于 vendor 的任务。
- 暂时缓解:deny ignore,理由——idna 0.2.3 只在 `mongodb+srv://` 的 DNS SRV 解析路径被触达,连接目标是运维配置的可信域名,不处理攻击者可控的国际化域名。

### 6.3 protobuf 2.28.0 — RUSTSEC-2024-0437(解析未知字段栈溢出)

- 引入链:`orc-format(git apecloud/orc-format@4fb40f9)→ protobuf 2.28.0`(以及 build 期 protobuf-codegen/protoc-rust 2.x)。advisory `patched = [">= 3.7.2"]`,**2.x 无修复版本**。(来源:Cargo.lock、rustsec/advisory-db `crates/protobuf/RUSTSEC-2024-0437.md`)
- 使用面:仅 `dt-connector/src/sinker/foxlake/foxlake_pusher.rs`(Foxlake sink,**只写 ORC**:`orc_format::writer`),不解析外部输入的 ORC/protobuf。
- 消除路径(三选一,按代价升序):
  1. 暂时缓解:deny ignore,理由——漏洞在 `CodedInputStream::skip_group` 的**解析**路径,本仓只做序列化输出,不接触不可信输入;
  2. 给 apecloud/orc-format 提 PR/自持分支,把 protobuf 2.x 升到 3.7.2+(codegen API 变化,中等工作量);
  3. 换库:orc-rust(datafusion-orc)等,需评估写路径特性对齐,面积最大。

## 7. 分步方案(推荐路线)

**阶段 0:provenance + patch series(立即,纯记录性,零行为风险)**

1. 新增 `vendor/PROVENANCE.md`:记录 §2 的三层来源(上游 tag、fork commit hash、本地层引入提交 f3f01b2f)。
2. 生成 patch series:`vendor/patches/sqlx/{0001-starrocks-fork-layer,0002-gaussdb-auth}.patch`、`vendor/patches/rust-postgres/{0001-replication-fork-layer,0002-gaussdb-auth}.patch`(按 §2.2 清单从三方 tarball diff 直接导出),并加 `make vendor-refresh`:下载上游 tag tarball → 依次打 patch → 与 vendor/ 比对(或重建),保证 vendor 可重放、patch 不腐化。
3. 引入 `deny.toml`(仓库当前没有):登记 RUSTSEC-2023-0071 / 2024-0421 / 2024-0437 三条 ignore,每条附 §6 的书面理由与解除条件。

**阶段 1:reqwest 归一(小,独立)**

4. StarRocks/Doris/ClickHouse sinker 改手动 307 处理(`Policy::none()` + Location 重发,保留 Authorization),workspace reqwest 切上游 0.12;删除 fork 依赖 → hyper 0.14/reqwest 0.11 双栈消失。验证:StarRocks stream load e2e。

**阶段 2:sqlx 0.6.2 → 0.8.6 vendored 升级(中,一个专门迭代)**

5. 重新 vendor 上游 v0.8.6;丢弃 StarRocks 两个 fork patch(改用 `pipes_as_concat(false)/no_engine_substitution(false)/timezone` 选项 + `sqlx::raw_sql` 替换 ~20 处 `disable_arguments`);把 GaussDB patch(§2.2 本地层 9 文件)移植到 `sqlx-postgres`(§3.2:落点一一对应,面积小~中)。
6. 特性切 `runtime-tokio` + `tls-rustls`(或 native-tls 对齐现状),消除 sqlx 侧双运行时;业务侧按 0.7/0.8 Breaking 清单迁移(51 文件)。
7. 验收红线:`bash scripts/e2e/mysql_to_postgresql_redline.sh` 全绿 + GaussDB 双向 e2e(`dt-tests` gaussdb 套件)+ StarRocks e2e。
8. (后置可选)toolchain ≥1.94 后升 sqlx 0.9,评估关闭 `mysql-rsa` 以彻底摘除 rsa。

**阶段 3:非 vendor 的 RUSTSEC 收尾(独立小任务,可并行)**

9. mongodb 2.5 → 3.x(消 idna 0.2.3;顺带消 trust-dns 0.21 链)。
10. orc-format 的 protobuf 3.7.2+ 升级或换库(消 protobuf 2.28)。

**rust-postgres:维持 0.7.1 三层现状**,靠阶段 0 的 patch series 管住;仅在出现明确功能需求时专项 rebase(预算按「复制协议 backport 全量重排」估,不是按 gaussdb 两个文件估)。

---

## 附:关键证据索引

| 结论 | 来源 |
|---|---|
| vendor 引入提交与 fork commit | `git log --all -- vendor/`;`git show f3f01b2f~1:Cargo.lock`(apecloud/sqlx@032ae40、apecloud/rust-postgres@39a35f1、apecloud/reqwest@a9c1f4b) |
| 三层 diff 清单 | launchbadge/sqlx v0.6.2、sfackler/rust-postgres tokio-postgres-v0.7.1、apecloud 两 fork commit 的 tarball 与 vendor 逐文件 `diff -ruN` |
| sqlx 0.7/0.8/0.9 变更 | launchbadge/sqlx `CHANGELOG.md`(0.7.0 #2039 driver 拆分、0.8.0 #3399 rustls/特性拆分、0.9.0 MSRV 1.94 与 #4142 mysql-rsa) |
| sqlx 0.8.6 落点形状 | v0.8.6 tarball:`sqlx-postgres/src/connection/establish.rs` 等与 0.6.2 对应文件 diff(authentication.rs 仅 41 行差异);`sqlx-mysql/src/options/{connect,mod}.rs` 的 pipes_as_concat/no_engine_substitution/timezone/set_names |
| tokio-postgres 上游无复制支持 | 上游 `tokio-postgres/CHANGELOG.md`(0.7.2~0.7.18 无相关条目);master `client.rs`/`backend.rs` grep 无 copy_both/XLogData |
| reqwest fork 内容与上游现状 | GitHub compare API(fork 仅 `redirect-with-sensitive-headers` cfg 补丁);上游 master `src/redirect.rs` `remove_sensitive_headers` 无条件调用;上游 CHANGELOG 无 opt-out |
| RUSTSEC 原文 | rustsec/advisory-db:`rsa/RUSTSEC-2023-0071.md`(patched=[])、`idna/RUSTSEC-2024-0421.md`(patched>=1.0.0)、`protobuf/RUSTSEC-2024-0437.md`(patched>=3.7.2) |
| idna 归属 mongodb 链 | 本仓 `Cargo.lock` 反查(trust-dns-proto 0.21.2 ← mongodb 2.5.0);crates.io API(mongodb 3.2.5 → hickory ^0.24.2 → idna ^1.0) |
| 调用点清单 | 本仓 grep:`disable_arguments`(~20 处)、`copy_both_simple`(2 处)、`protocolVersion=351`(6 处)、reqwest sinker 调用点 |
