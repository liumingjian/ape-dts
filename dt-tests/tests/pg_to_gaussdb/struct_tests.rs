#[cfg(test)]
mod test {

    use serial_test::serial;

    use crate::test_runner::test_base::TestBase;

    #[tokio::test]
    #[serial]
    async fn struct_basic_test() {
        TestBase::run_pg_struct_test("pg_to_gaussdb/struct/basic_test").await;
    }

    #[tokio::test]
    #[serial]
    async fn struct_view_routine_test() {
        TestBase::run_pg_struct_test("pg_to_gaussdb/struct/view_routine_test").await;
    }
}
