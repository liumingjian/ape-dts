#[cfg(test)]
mod test {

    use serial_test::serial;

    use crate::test_runner::test_base::TestBase;

    #[tokio::test]
    #[serial]
    async fn check_basic_test() {
        TestBase::run_check_test("gaussdb_to_pg/check/basic_test").await;
    }

    #[tokio::test]
    #[serial]
    async fn type_matrix_test() {
        TestBase::run_check_test("gaussdb_to_pg/check/type_matrix_test").await;
    }
}
