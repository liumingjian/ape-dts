use dt_common::config::task_config::TaskConfig;

use crate::{
    builder::prechecker_builder::PrecheckerBuilder, config::task_config::PrecheckTaskConfig,
};

pub mod builder;
pub mod config;
pub mod fetcher;
pub mod meta;
pub mod prechecker;

pub fn load_precheck_configs(config: &str) -> anyhow::Result<(PrecheckTaskConfig, TaskConfig)> {
    let task_config = TaskConfig::new(config)?;
    let precheck_config = PrecheckTaskConfig::new(config)?;
    Ok((precheck_config, task_config))
}

pub async fn do_precheck(
    precheck_config: PrecheckTaskConfig,
    task_config: TaskConfig,
) -> anyhow::Result<()> {
    let checker_connector = PrecheckerBuilder::build(precheck_config.precheck, task_config);
    checker_connector.verify_check_result().await?;

    println!("precheck passed.");
    Ok(())
}
