#[cfg(test)]
mod test {
    use std::collections::{HashMap, HashSet};

    use serial_test::serial;

    use crate::test_runner::test_base::TestBase;

    #[tokio::test]
    #[serial]
    async fn struct_supported_basic_test() {
        let test_dir = "oracle_to_gaussdb_oracle/precheck/struct_supported_basic_test";
        TestBase::run_precheck_test(test_dir, &HashSet::new(), &HashMap::new(), &HashMap::new())
            .await
    }
}
