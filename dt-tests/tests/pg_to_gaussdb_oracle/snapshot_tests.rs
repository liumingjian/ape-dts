#[cfg(test)]
mod test {

    use serial_test::serial;

    use crate::test_runner::test_base::TestBase;

    #[tokio::test]
    #[serial]
    async fn smoke_test() {
        TestBase::run_snapshot_test("pg_to_gaussdb_oracle/snapshot/smoke_test").await;
    }
}

