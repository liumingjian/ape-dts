#[cfg(test)]
mod test {
    use serial_test::serial;

    use crate::test_runner::test_base::TestBase;

    #[tokio::test]
    #[serial]
    async fn cdc_basic_test() {
        // GaussDB logical replication startup can be slow/flaky; give it enough headroom.
        TestBase::run_cdc_test("gaussdb_oracle_to_oracle/cdc/basic_test", 60000, 30000).await;
    }
}

