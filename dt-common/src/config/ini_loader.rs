use std::{any::type_name, str::FromStr};

use anyhow::{bail, Context};
use configparser::ini::Ini;

use crate::error::Error;

pub struct IniLoader {
    pub ini: Ini,
}

impl IniLoader {
    pub fn new(ini_file: &str) -> anyhow::Result<Self> {
        let config_str = std::fs::read_to_string(ini_file)
            .with_context(|| format!("failed to open or read ini file: {ini_file}"))?;
        let mut ini = Ini::new();
        // allow using comment symbols(; and #) in value
        // E.g. do_dbs=`a;`,`bcd`
        ini.set_inline_comment_symbols(Some(&Vec::new()));
        ini.read(config_str)
            .map_err(|error| anyhow::anyhow!("failed to parse ini file {ini_file}: {error}"))?;
        Ok(Self { ini })
    }

    pub fn get_required<T>(&self, section: &str, key: &str) -> anyhow::Result<T>
    where
        T: FromStr,
    {
        if let Some(value) = self.ini.get(section, key) {
            if !value.is_empty() {
                return Self::parse_value(section, key, &value);
            }
        }
        bail!(Error::ConfigError(format!(
            "config [{}].{} does not exist or is empty",
            section, key
        )))
    }

    pub fn get_optional<T>(&self, section: &str, key: &str) -> anyhow::Result<T>
    where
        T: Default + FromStr,
    {
        self.get_with_default(section, key, T::default())
    }

    pub fn get_with_default<T>(&self, section: &str, key: &str, default: T) -> anyhow::Result<T>
    where
        T: FromStr,
    {
        if let Some(value) = self.ini.get(section, key) {
            if !value.is_empty() {
                return Self::parse_value(section, key, &value);
            }
        }
        Ok(default)
    }

    pub fn contains(&self, section: &str, key: &str) -> bool {
        self.ini.get(section, key).is_some()
    }

    fn parse_value<T>(section: &str, key: &str, value: &str) -> anyhow::Result<T>
    where
        T: FromStr,
    {
        match value.parse::<T>() {
            Ok(v) => Ok(v),
            Err(_) => bail! {Error::ConfigError(format!(
                "config [{}].{}={}, can not be parsed as {}",
                section,
                key,
                value,
                type_name::<T>(),
            ))},
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicUsize, Ordering},
    };

    use super::IniLoader;

    static CONFIG_ID: AtomicUsize = AtomicUsize::new(0);

    fn write_config(content: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "ape-dts-ini-loader-{}-{}.ini",
            std::process::id(),
            CONFIG_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn invalid_value_returns_error() {
        let path = write_config("[runtime]\ntb_parallel_size=not-a-number\n");
        let loader = IniLoader::new(path.to_str().unwrap()).unwrap();

        let error = loader
            .get_required::<usize>("runtime", "tb_parallel_size")
            .err()
            .unwrap();

        fs::remove_file(path).unwrap();
        assert!(
            error
                .to_string()
                .contains("config [runtime].tb_parallel_size=not-a-number, can not be parsed"),
            "unexpected error: {error:#}"
        );
    }

    #[test]
    fn missing_required_value_returns_error() {
        let path = write_config("[runtime]\nlog_level=info\n");
        let loader = IniLoader::new(path.to_str().unwrap()).unwrap();

        let error = loader
            .get_required::<usize>("runtime", "tb_parallel_size")
            .err()
            .unwrap();

        fs::remove_file(path).unwrap();
        assert!(
            error
                .to_string()
                .contains("config [runtime].tb_parallel_size does not exist or is empty"),
            "unexpected error: {error:#}"
        );
    }

    #[test]
    fn malformed_ini_returns_error() {
        let path = write_config("[runtime\ntb_parallel_size=1\n");

        let error = IniLoader::new(path.to_str().unwrap()).err().unwrap();

        fs::remove_file(path).unwrap();
        assert!(
            error.to_string().contains("failed to parse ini file"),
            "unexpected error: {error:#}"
        );
    }

    #[test]
    fn missing_file_returns_error() {
        let error = IniLoader::new("/path/that/does/not/exist.ini")
            .err()
            .unwrap();

        assert!(
            error
                .to_string()
                .contains("failed to open or read ini file"),
            "unexpected error: {error:#}"
        );
    }
}
