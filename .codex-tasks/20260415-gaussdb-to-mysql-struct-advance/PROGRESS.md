# Progress Log

## Context Recovery Block

- **Task**: `GaussDBPg -> MySQL struct advanced`
- **Shape**: `single-full`
- **Truth file**: `.codex-tasks/20260415-gaussdb-to-mysql-struct-advance/TODO.csv`
- **Current status**: `DONE`

## 验证记录

### 1) 真实环境运行（PASS）

- 命令（为减少 RW host 探测可临时收敛候选）：
```bash
set -a; source dt-tests/tests/.env.local; set +a
export gaussdb_pg_candidate_hosts=10.250.0.51:8000
cargo test -p dt-tests --test integration_test -- \
  gaussdb_to_mysql::struct_tests::test::struct_advanced_test --nocapture
```

- 结果：PASS
- 关键点：
  - GaussDB `pg_indexes.indexdef` 使用 `USING ubtree ... WITH (storage_type=USTORE)`，在 MySQL 侧按 btree 等价处理后成功创建普通/unique 索引。
  - 默认值映射在 MySQL `SHOW CREATE TABLE` 中落为：
    - `decimal(10,2) DEFAULT '0.00'`
    - `tinyint(1) DEFAULT '1'`
    - `datetime DEFAULT CURRENT_TIMESTAMP`
