#[cfg(test)]
mod test {

    use crate::test_runner::test_base::TestBase;
    use serial_test::serial;

    #[tokio::test]
    #[serial]
    async fn snapshot_basic_test() {
        TestBase::run_snapshot_test("gaussdb_to_mysql/snapshot/basic_test").await;
    }
}
