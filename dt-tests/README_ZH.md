# [English](README.md) | 中文

# 运行测试用例
```rust
#[tokio::test]
#[serial]
async fn cdc_basic_test() {
    TestBase::run_cdc_test("mysql_to_mysql/cdc/basic_test", 3000, 2000).await;
}
```

```
cargo test --package dt-tests --test integration_test -- mysql_to_mysql::cdc_tests::test::cdc_basic_test --nocapture 
```

- 测试用例包括：
  - task_config.ini
  - src_prepare.sql
  - dst_prepare.sql
  - src_test.sql
  - dst_test.sql
  - `*.sql` 测试文件说明：
    - 多语句必须用 `;` 结束（runner 依赖 `;` 分割语句）。
    - 避免在字符串/JSON 字面量中出现 `--`（runner 会按行截断 `-- ...` 内联注释）。

- 一个典型测试用例的步骤：
  - 1，对源库执行 src_prepare.sql。
  - 2，对目标库执行 dst_prepare.sql。
  - 3，启动 数据同步 任务的线程。
  - 4，停顿若干毫秒（start_millis，根据测试环境的性能和网络状况，你可修改测试用例的预设值），等待任务初始化。
  - 5，对源库执行 src_test.sql。
  - 6，对目标库执行 dst_test.sql（如果有）。
  - 7，轮询收敛：每 200ms 比对一次源和目标，直到一致，或耗尽 parse_millis（超时后的不一致就是真失败，不会被重试洗白）。

# 环境守卫

集成测试需要真实的数据库。在没有环境的机器上，测试不再 panic，而是**打印原因后跳过**：

- `dt-tests/tests/.env` 与 `.env.local` 都不存在（复制 `.env.src` 填好即可）；
- 用例的 `task_config.ini` 引用了未定义的环境变量；
- 引用的地址 TCP 连不上。

所以 `cargo test -p dt-tests` 在任何机器上都可以跑：缺什么会说清楚，而不是挂掉。

环境本该就绪的地方（CI、compose 栈）设 `DT_TESTS_STRICT_ENV=1`，把跳过变成失败，避免坏环境冒充绿灯。

# 每日 E2E 矩阵与它的 compose 栈

`.github/workflows/e2e-tests.yml` 每晚跑 `mysql_to_mysql`、`pg_to_pg`、`mongo_to_mongo`、
`redis_to_redis` 四个套件（也可手动 *Run workflow*，输入逗号分隔的子集）。每个 job 只起
`docker-compose.ci.yml` 里对应的一个 profile，并把 `.env.ci` 复制成 `tests/.env`；本地要同一套
环境，两条命令即可：

```
docker compose -f dt-tests/docker-compose.ci.yml --profile mysql up --detach --wait
cp dt-tests/.env.ci dt-tests/tests/.env
```

profile 有 `mysql`、`pg`、`mongo`、`redis`、`clickhouse`。不指定 profile 什么都不会起——redis 的
job 因此不会顺带拉起五个它根本用不到的 MySQL。

工作流依赖的两条不变量，改它时请保留：

- `DT_TESTS_STRICT_ENV=1`——夜间任务里「跳过」和「通过」看起来一模一样，所以跳过必须是失败。
  反过来说，一个套件只有在 `docker-compose.ci.yml` 里备齐了整套依赖后，才够格进矩阵。
- `--test-threads 1`——套件共用数据库、靠 `#[serial]` 串行，而 `#[serial]` 只在进程内有效，
  nextest 给每个用例单独起进程。

`.env.ci` 与 `.env.src` 必须覆盖所有 `task_config.ini` 引用的 `{占位符}`；
`tests/test_runner/env_ci.rs` 不碰数据库就能断言这一点，工作流在起任何容器之前先跑它。所以新增
一个引用了新端点的用例时，两个 env 文件都要补 key；若还希望夜间矩阵覆盖它，`docker-compose.ci.yml`
里也要补上对应服务。

arm64 机器上 `docker-compose.override.local.yml` 把 MySQL 5.7 换成 8.0（5.7 没有 arm64 镜像）；
CI 跑 amd64，保持 5.7——`mysql_extractor_url` 与 `mysql_extractor_url_8_0` 本来就是两个不同版本的
服务端。redis 2.8 与 `redislabs/*` 模块镜像同样只有 amd64，所以完整的 redis profile 是 CI 专用栈。

依赖数据库之外的东西的用例标了 `#[ignore]`，需要显式运行：

```
cargo test -p dt-tests --test gaussdb_snapshot_cdc_e2e -- --ignored   # 需要 console + playwright + gaussdb
```

# 配置
- 所有数据库的 extractor url，sinker url 均配置在 ./tests/.env 文件，各测试用例的 task_config.ini 中引用。

```
[extractor]
url={mysql_extractor_url}

[sinker]
url={mysql_sinker_url}
```

# 测试环境搭建

- 本文均以 docker 搭建测试环境为例。[参考](/docs/en/tutorial/prerequisites.md)

# Postgres 环境搭建

[创建 Postgres](/docs/en/tutorial/pg_to_pg.md)

## 如要执行 [双向同步](/docs/zh/cdc/two_way.md) 相关测试
- pg_to_pg::cdc_tests::test::cycle_

- 总共需要创建 3 个 Postgres 示例，并按照 [创建 Postgres](/docs/en/tutorial/pg_to_pg.md) 为每个实例都设置 wal_level = logical。

## 如要执行 [charset 相关测试](../dt-tests/tests/pg_to_pg/snapshot/charset_euc_cn_test)
- 在源和目标分别预建数据库 postgres_euc_cn。

```
CREATE DATABASE postgres_euc_cn
  ENCODING 'EUC_CN'
  LC_COLLATE='C'
  LC_CTYPE='C'
  TEMPLATE template0;
```

# GaussDB（PG 兼容模式）

- 测试用例目录：
  - `pg_to_gaussdb/*`（snapshot/struct/check）
  - `gaussdb_to_pg/*`（snapshot/cdc/check）
- 配置说明：
  - 测试用例 `task_config.ini` 中使用 `db_type=gaussdb_pg`
  - 推荐通过 `dt-tests/tests/.env.local` 覆盖 URL（而非直接改仓库内 `.env`）：
    - `gaussdb_pg_extractor_without_auth_url`、`gaussdb_pg_extractor_username`、`gaussdb_pg_extractor_password`
    - `gaussdb_pg_sinker_without_auth_url`、`gaussdb_pg_sinker_username`、`gaussdb_pg_sinker_password`
- 注意事项：
  - MVP 需要 GaussDB 启用 MD5 认证（例如 `password_encryption_type=1`）
  - CDC 需要 `wal_level=logical` 且 `mppdb_decoding` 可用

# MySQL 环境搭建
[创建 MySQL](/docs/en/tutorial/mysql_to_mysql.md)

## 如要执行 [双向同步](/docs/zh/cdc/two_way.md) 相关测试
- mysql_to_mysql::cdc_tests::test::cycle_

- 总共需要创建 3 个 MySQL 示例

# Mongo
[创建 Mongo](/docs/en/tutorial/mongo_to_mongo.md)

# Kafka
[创建 Kafka](/docs/en/tutorial/mysql_to_kafka_consumer.md)

# StarRocks
[创建 StarRocks](/docs/en/tutorial/mysql_to_starrocks.md)

创建老版本 StarRocks: 2.5.4

```
docker run -itd --name some-starrocks-2.5.4 \
-p 9031:9030 \
-p 8031:8030 \
-p 8041:8040 \
starrocks/allin1-ubuntu:2.5.4
```

# Doris
[创建 Doris](/docs/en/tutorial/mysql_to_doris.md)

# Redis
[创建 Redis](/docs/en/tutorial/redis_to_redis.md)

## 更多版本

- redis 不同版本的数据格式差距较大，我们支持 2.8 - 7.*，rebloom，rejson。
- redis:7.0
- redis:6.0
- redis:6.2
- redis:5.0
- redis:4.0
- redis:2.8.22
- redislabs/rebloom:2.6.3
- redislabs/rejson:2.6.4
- mac 上无法部署 2.8，rebloom，rejson 镜像，可在 EKS(amazon)/AKS(azure)/ACK(alibaba) 上部署，参考目录：dt-tests/k8s/redis。

### 源

```
docker run --name src-redis-7-0 \
    -p 6380:6379 \
    -d redis:7.0 redis-server \
    --requirepass 123456 \
    --save 60 1 \
    --loglevel warning

docker run --name src-redis-6-2 \
    -p 6381:6379 \
    -d redis:6.2 redis-server \
    --requirepass 123456 \
    --save 60 1 \
    --loglevel warning

docker run --name src-redis-6-0 \
    -p 6382:6379 \
    -d redis:6.0 redis-server \
    --requirepass 123456 \
    --save 60 1 \
    --loglevel warning

docker run --name src-redis-5-0 \
    -p 6383:6379 \
    -d redis:5.0 redis-server \
    --requirepass 123456 \
    --save 60 1 \
    --loglevel warning

docker run --name src-redis-4-0 \
    -p 6384:6379 \
    -d redis:4.0 redis-server \
    --requirepass 123456 \
    --save 60 1 \
    --loglevel warning
```

### 目标

```
docker run --name dst-redis-7-0 \
    -p 6390:6379 \
    -d redis:7.0 redis-server \
    --requirepass 123456 \
    --save 60 1 \
    --loglevel warning

docker run --name dst-redis-6-2 \
    -p 6391:6379 \
    -d redis:6.2 redis-server \
    --requirepass 123456 \
    --save 60 1 \
    --loglevel warning

docker run --name dst-redis-6-0 \
    -p 6392:6379 \
    -d redis:6.0 redis-server \
    --requirepass 123456 \
    --save 60 1 \
    --loglevel warning

docker run --name dst-redis-5-0 \
    -p 6393:6379 \
    -d redis:5.0 redis-server \
    --requirepass 123456 \
    --save 60 1 \
    --loglevel warning

docker run --name dst-redis-4-0 \
    -p 6394:6379 \
    -d redis:4.0 redis-server \
    --requirepass 123456 \
    --save 60 1 \
    --loglevel warning
```
