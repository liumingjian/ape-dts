# Progress

- 新增用例：
  - `gaussdb_oracle_to_oracle::struct_tests::test::struct_basic_test`
  - `gaussdb_oracle_to_oracle::check_tests::test::check_basic_test`（parallelizer 使用 `serial`）
- 验证：
  - `cargo test -p dt-tests --test integration_test -- gaussdb_oracle_to_oracle::struct_tests::test::struct_basic_test gaussdb_oracle_to_oracle::check_tests::test::check_basic_test --nocapture` PASS
