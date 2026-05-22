#[cfg(test)]
mod test {

    use serial_test::serial;

    use crate::test_runner::test_base::TestBase;

    #[tokio::test]
    #[serial]
    async fn struct_view_routine_test() {
        TestBase::run_pg_struct_test("gaussdb_to_pg/struct/view_routine_test").await;
    }
}
