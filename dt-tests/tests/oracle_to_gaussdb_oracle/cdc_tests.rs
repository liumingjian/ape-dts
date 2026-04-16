#[cfg(test)]
mod test {
    use serial_test::serial;

    use crate::test_runner::test_base::TestBase;

    #[tokio::test]
    #[serial]
    async fn cdc_basic_test() {
        // Oracle XE + sqlplus bootstrapping (trigger create) can be slow; give it a bit more time.
        TestBase::run_cdc_test("oracle_to_gaussdb_oracle/cdc/basic_test", 10_000, 10_000).await;
    }
}

