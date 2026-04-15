#[cfg(test)]
mod test {

    use serial_test::serial;

    use crate::test_runner::test_base::TestBase;

    #[tokio::test]
    #[serial]
    async fn struct_basic_test() {
        TestBase::run_pg_struct_test("pg_to_gaussdb_oracle/struct/basic_test").await;
    }
}

