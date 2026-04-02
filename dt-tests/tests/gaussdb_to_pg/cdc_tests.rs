#[cfg(test)]
mod test {

    use serial_test::serial;

    use crate::test_runner::test_base::TestBase;

    #[tokio::test]
    #[serial]
    async fn cdc_basic_test() {
        // GaussDB logical replication startup (slot create + START_REPLICATION) can be slow/flaky.
        // Give it enough headroom to avoid missing early DML and to tolerate transient HA hiccups.
        TestBase::run_cdc_test("gaussdb_to_pg/cdc/basic_test", 60000, 30000).await;
    }

    #[tokio::test]
    #[serial]
    async fn cdc_resume_test() {
        TestBase::run_cdc_resume_test("gaussdb_to_pg/cdc/resume_test", 60000, 30000).await;
    }

    #[tokio::test]
    #[serial]
    async fn cdc_failover_test() {
        TestBase::run_cdc_failover_test("gaussdb_to_pg/cdc/failover_test", 60000, 30000).await;
    }

    #[tokio::test]
    #[serial]
    async fn cdc_type_matrix_test() {
        TestBase::run_cdc_test("gaussdb_to_pg/cdc/type_matrix_test", 60000, 30000).await;
    }
}
