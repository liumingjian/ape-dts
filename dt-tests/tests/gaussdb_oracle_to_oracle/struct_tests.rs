#[cfg(test)]
mod test {
    use serial_test::serial;

    use crate::test_runner::test_base::TestBase;

    #[tokio::test]
    #[serial]
    async fn struct_basic_test() {
        TestBase::run_oracle_dst_struct_test("gaussdb_oracle_to_oracle/struct/basic_test").await;
    }
}

