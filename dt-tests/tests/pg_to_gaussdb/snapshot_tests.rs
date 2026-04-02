#[cfg(test)]
mod test {

    use serial_test::serial;

    use crate::test_runner::test_base::TestBase;

    #[tokio::test]
    #[serial]
    async fn snapshot_basic_test() {
        TestBase::run_snapshot_test("pg_to_gaussdb/snapshot/basic_test").await;
    }

    #[tokio::test]
    #[serial]
    async fn type_matrix_test() {
        TestBase::run_snapshot_test("pg_to_gaussdb/snapshot/type_matrix_test").await;
    }
}
