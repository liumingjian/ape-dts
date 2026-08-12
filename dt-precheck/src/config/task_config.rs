use anyhow::bail;
use dt_common::{config::ini_loader::IniLoader, error::Error};

use super::precheck_config::PrecheckConfig;

const PRECHECK: &str = "precheck";

pub struct PrecheckTaskConfig {
    pub precheck: PrecheckConfig,
}

impl PrecheckTaskConfig {
    pub fn is_precheck(task_config_file: &str) -> anyhow::Result<bool> {
        let loader = IniLoader::new(task_config_file)?;
        Ok(loader.ini.sections().contains(&PRECHECK.to_string()))
    }

    pub fn new(task_config_file: &str) -> anyhow::Result<Self> {
        let loader = IniLoader::new(task_config_file)?;
        let precheck_config = Self::load_precheck_config(&loader)?;
        Ok(Self {
            precheck: precheck_config,
        })
    }

    fn load_precheck_config(loader: &IniLoader) -> anyhow::Result<PrecheckConfig> {
        if loader.contains(PRECHECK, "do_struct_init") && loader.contains(PRECHECK, "do_cdc") {
            Ok(PrecheckConfig {
                do_struct_init: loader.get_required(PRECHECK, "do_struct_init")?,
                do_cdc: loader.get_required(PRECHECK, "do_cdc")?,
            })
        } else {
            bail! {Error::ConfigError(
                "config is not valid for precheck.".into(),
            )}
        }
    }
}
