//! Keeps the checked-in env files in step with what the tests actually reference.
//!
//! A `task_config.ini` refers to endpoints as `{placeholder}`, resolved from `tests/.env` at
//! run time. A placeholder nobody defines used to surface as a run-time failure deep inside a
//! suite — under the environment guards it is now a *skip*, which is worse: the nightly matrix
//! would go green while half the suite quietly did nothing.
//!
//! These tests turn that into a compile-fast unit failure instead. They need no databases, so
//! they run in the normal `cargo test -p dt-tests` pass and as the first step of the E2E
//! workflow, before a single container is started.

use std::{collections::BTreeSet, fs, path::Path};

use crate::{test_config_util::TestConfigUtil, test_runner::test_env::TestEnv};

/// `(placeholder, the config that references it)` for every task_config.ini under `tests/`.
fn referenced_placeholders() -> Vec<(String, String)> {
    let root = TestConfigUtil::get_absolute_path("");
    let mut configs = Vec::new();
    collect_task_configs(Path::new(&root), &mut configs);
    assert!(
        !configs.is_empty(),
        "no task_config.ini found under {} — the walk is broken, not the tests",
        root
    );

    let mut out = Vec::new();
    for config in configs {
        let Ok(body) = fs::read_to_string(&config) else {
            continue;
        };
        let relative = config.strip_prefix(&root).unwrap_or(&config).to_string();
        for name in TestEnv::placeholders(&body) {
            out.push((name, relative.clone()));
        }
    }
    out
}

fn collect_task_configs(dir: &Path, out: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_task_configs(&path, out);
        } else if path.file_name().and_then(|n| n.to_str()) == Some("task_config.ini") {
            out.push(path.to_string_lossy().to_string());
        }
    }
}

fn defined_keys(env_file: &str) -> BTreeSet<String> {
    let body =
        fs::read_to_string(env_file).unwrap_or_else(|e| panic!("cannot read {}: {}", env_file, e));

    body.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| line.split_once('='))
        .map(|(key, _)| key.trim().to_string())
        .collect()
}

/// Panics with every missing `key → first config that wants it` pair, not just the first.
fn assert_covers(env_file: &str) {
    let defined = defined_keys(env_file);

    let mut missing: Vec<(String, String)> = Vec::new();
    let mut seen = BTreeSet::new();
    for (name, config) in referenced_placeholders() {
        if defined.contains(&name) || !seen.insert(name.clone()) {
            continue;
        }
        missing.push((name, config));
    }

    assert!(
        missing.is_empty(),
        "{} does not define {} environment variable(s) referenced by task_config.ini files:\n{}",
        env_file,
        missing.len(),
        missing
            .iter()
            .map(|(name, config)| format!("  {} (e.g. {})", name, config))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The file the E2E workflow copies to `tests/.env`.
    #[test]
    fn env_ci_covers_every_referenced_placeholder() {
        assert_covers(&format!(
            "{}/dt-tests/.env.ci",
            TestConfigUtil::get_project_root()
        ));
    }

    /// The file a developer copies to `tests/.env.local`; same defect class, same guard.
    #[test]
    fn env_src_covers_every_referenced_placeholder() {
        assert_covers(&TestConfigUtil::get_absolute_path(".env.src"));
    }
}
