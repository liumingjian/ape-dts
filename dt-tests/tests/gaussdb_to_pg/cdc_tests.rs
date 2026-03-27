#[cfg(test)]
mod test {

    use serial_test::serial;

    use crate::test_runner::test_base::TestBase;

    #[tokio::test]
    #[serial]
    async fn cdc_basic_test() {
        // GaussDB logical replication startup (slot create + START_REPLICATION) can be slow/flaky.
        // Give it enough headroom to avoid missing early DML.
        TestBase::run_cdc_test("gaussdb_to_pg/cdc/basic_test", 60000, 9000).await;
    }
}
