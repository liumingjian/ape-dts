#[cfg(test)]
mod test {

    use serial_test::serial;

    use crate::test_runner::test_base::TestBase;

    #[tokio::test]
    #[serial]
    async fn smoke_test() {
        TestBase::run_snapshot_test("mysql_to_gaussdb_mysql/snapshot/smoke_test").await;
    }

    #[tokio::test]
    #[serial]
    async fn snapshot_basic_test() {
        TestBase::run_snapshot_test("mysql_to_gaussdb_mysql/snapshot/smoke_test").await;
    }
}
