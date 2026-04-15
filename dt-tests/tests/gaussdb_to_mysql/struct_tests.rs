#[cfg(test)]
mod test {
    use serial_test::serial;

    use crate::test_runner::test_base::TestBase;

    #[tokio::test]
    #[serial]
    async fn struct_basic_test() {
        TestBase::run_mysql_struct_test("gaussdb_to_mysql/struct/basic_test").await;
    }

    #[tokio::test]
    #[serial]
    async fn struct_advanced_test() {
        TestBase::run_mysql_struct_test("gaussdb_to_mysql/struct/advanced_test").await;
    }
}
