use std::{
    collections::HashMap,
    env, fs,
    sync::{Mutex, OnceLock},
    time::Duration,
};

use tokio::{net::TcpStream, time::timeout};
use url::Url;

use crate::test_config_util::TestConfigUtil;

/// Set to `1`/`true` to turn every "environment not available" skip into a hard failure.
/// CI uses it to prove the suite really ran against live databases instead of skipping green.
const STRICT_ENV_VAR: &str = "DT_TESTS_STRICT_ENV";

const PROBE_TIMEOUT: Duration = Duration::from_millis(2_000);

/// Guards integration tests that need a live environment (databases, brokers, console).
///
/// Every `TestBase::run_*` entry point asks this first: when `.env` is absent, when the test's
/// `task_config.ini` references an env var nobody defined, or when the endpoint it points at
/// refuses a TCP connection, the test prints why and returns instead of panicking. That makes
/// `cargo test -p dt-tests` meaningful on a machine that never provisioned the compose stack.
pub struct TestEnv {}

#[allow(dead_code)]
impl TestEnv {
    /// Returns true when the caller should return early. Prints the reason (or panics in strict mode).
    pub async fn skip(relative_test_dir: &str) -> bool {
        let Some(reason) = Self::skip_reason(relative_test_dir).await else {
            return false;
        };

        if Self::strict() {
            panic!(
                "{}=1 but the test environment is unusable for {}: {}",
                STRICT_ENV_VAR, relative_test_dir, reason
            );
        }

        println!(
            "SKIP {}: {} (see dt-tests/README.md; set {}=1 to make this a failure)",
            relative_test_dir, reason, STRICT_ENV_VAR
        );
        true
    }

    pub fn strict() -> bool {
        matches!(
            env::var(STRICT_ENV_VAR).unwrap_or_default().as_str(),
            "1" | "true" | "TRUE"
        )
    }

    /// `None` means the environment looks usable; `Some(reason)` is a human-readable skip reason.
    pub async fn skip_reason(relative_test_dir: &str) -> Option<String> {
        if !Self::load_env_files() {
            return Some(
                "no dt-tests/tests/.env or .env.local (copy .env.src and fill it in)".into(),
            );
        }

        let Some(config_file) = Self::find_task_config(relative_test_dir) else {
            // No task_config.ini anywhere: nothing to probe, let the test itself decide.
            return None;
        };
        let Ok(ini) = fs::read_to_string(&config_file) else {
            return None;
        };

        for (key, raw) in Self::parse_url_values(&ini) {
            let resolved = match Self::resolve_placeholders(&raw, |k| env::var(k).ok()) {
                Ok(v) => v,
                Err(missing) => {
                    return Some(format!(
                        "env var `{}` is not set (needed by `{}`)",
                        missing, key
                    ))
                }
            };

            let Some((host, port)) = Self::host_port(&resolved) else {
                continue;
            };
            if !Self::probe(&host, port).await {
                return Some(format!("{}:{} is not reachable (`{}`)", host, port, key));
            }
        }

        None
    }

    /// The test's own `task_config.ini`, or — for suites like the cycle tests whose directory
    /// only holds one sub-directory per node — the first sub-directory's config. Every config
    /// under such a suite points at the same environment, so one is enough to judge it.
    fn find_task_config(relative_test_dir: &str) -> Option<String> {
        let dir = TestConfigUtil::get_absolute_path(relative_test_dir);

        let own = format!("{}/task_config.ini", dir);
        if fs::metadata(&own).is_ok() {
            return Some(own);
        }

        let mut sub_dirs: Vec<String> = fs::read_dir(&dir)
            .ok()?
            .flatten()
            .filter(|e| e.path().is_dir())
            .map(|e| e.path().to_string_lossy().to_string())
            .collect();
        sub_dirs.sort();

        sub_dirs
            .into_iter()
            .map(|d| format!("{}/task_config.ini", d))
            .find(|f| fs::metadata(f).is_ok())
    }

    /// Loads `.env` / `.env.local` the same way the task config rewriter does.
    /// Returns false when neither file exists.
    fn load_env_files() -> bool {
        let env_local_file = TestConfigUtil::get_absolute_path(".env.local");
        let env_file = TestConfigUtil::get_absolute_path(".env");

        let mut loaded = false;
        // .env.local wins, so it is loaded first (dotenv never overwrites an existing var).
        if fs::metadata(&env_local_file).is_ok() {
            let _ = dotenv::from_path(&env_local_file);
            loaded = true;
        }
        if fs::metadata(&env_file).is_ok() {
            let _ = dotenv::from_path(&env_file);
            loaded = true;
        }
        loaded
    }

    /// Collects `url=...` entries from a raw ini body, keyed by `<section>.url`.
    fn parse_url_values(ini: &str) -> Vec<(String, String)> {
        let mut section = String::new();
        let mut out = Vec::new();

        for line in ini.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                continue;
            }
            if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                section = name.trim().to_string();
                continue;
            }
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            if k.trim() != "url" {
                continue;
            }
            let v = v.trim();
            if v.is_empty() {
                continue;
            }
            out.push((format!("{}.url", section), v.to_string()));
        }
        out
    }

    /// Replaces every `{VAR}` with its env value. `Err(var)` names the first undefined one.
    fn resolve_placeholders<F>(raw: &str, lookup: F) -> Result<String, String>
    where
        F: Fn(&str) -> Option<String>,
    {
        let mut out = String::with_capacity(raw.len());
        let mut rest = raw;

        while let Some(start) = rest.find('{') {
            let Some(end_offset) = rest[start..].find('}') else {
                break;
            };
            let end = start + end_offset;
            let name = &rest[start + 1..end];
            // `{}` or a brace that is part of the value itself: copy it through untouched.
            if !Self::is_placeholder_name(name) {
                out.push_str(&rest[..end + 1]);
                rest = &rest[end + 1..];
                continue;
            }
            match lookup(name) {
                Some(value) => {
                    out.push_str(&rest[..start]);
                    out.push_str(&value);
                }
                None => return Err(name.to_string()),
            }
            rest = &rest[end + 1..];
        }
        out.push_str(rest);
        Ok(out)
    }

    /// Every `{VAR}` in `raw`, in order of appearance, duplicates included.
    ///
    /// Shares `is_placeholder_name` with `resolve_placeholders`, so what the coverage tests
    /// demand and what the guard resolves can never drift apart.
    pub fn placeholders(raw: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut rest = raw;

        while let Some(start) = rest.find('{') {
            let Some(end_offset) = rest[start..].find('}') else {
                break;
            };
            let end = start + end_offset;
            let name = &rest[start + 1..end];
            if Self::is_placeholder_name(name) {
                out.push(name.to_string());
            }
            rest = &rest[end + 1..];
        }
        out
    }

    /// `{}` and braces that belong to the value itself (json, url options) are not placeholders.
    fn is_placeholder_name(name: &str) -> bool {
        !name.is_empty() && !name.contains(['/', ':', '@', ' '])
    }

    /// Best-effort `host:port` extraction. `None` means "not probeable", never "unreachable".
    fn host_port(endpoint: &str) -> Option<(String, u16)> {
        if let Ok(url) = Url::parse(endpoint) {
            if let Some(host) = url.host_str() {
                let port = url.port().or_else(|| Self::default_port(url.scheme()))?;
                return Some((host.to_string(), port));
            }
        }

        // Schemeless endpoints (kafka brokers are written as `host:port`).
        let (host, port) = endpoint.rsplit_once(':')?;
        if host.is_empty() || host.contains('/') {
            return None;
        }
        let port: u16 = port.split(['/', '?']).next()?.parse().ok()?;
        Some((host.to_string(), port))
    }

    fn default_port(scheme: &str) -> Option<u16> {
        match scheme {
            "mysql" => Some(3306),
            "postgres" | "postgresql" => Some(5432),
            "mongodb" => Some(27017),
            "redis" => Some(6379),
            "oracle" => Some(1521),
            "http" => Some(80),
            "https" => Some(443),
            _ => None,
        }
    }

    /// TCP-connects once per `host:port` and caches the verdict — hundreds of tests share
    /// the same handful of endpoints, and an unreachable one costs a full timeout each time.
    async fn probe(host: &str, port: u16) -> bool {
        static CACHE: OnceLock<Mutex<HashMap<String, bool>>> = OnceLock::new();
        let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
        let key = format!("{}:{}", host, port);

        if let Some(cached) = cache.lock().unwrap().get(&key).copied() {
            return cached;
        }

        let reachable = matches!(
            timeout(PROBE_TIMEOUT, TcpStream::connect(&key)).await,
            Ok(Ok(_))
        );
        cache.lock().unwrap().insert(key, reachable);
        reachable
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_of<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |k| {
            pairs
                .iter()
                .find(|(name, _)| *name == k)
                .map(|(_, v)| v.to_string())
        }
    }

    #[test]
    fn parse_url_values_keys_urls_by_section() {
        let ini = "\
[extractor]
db_type=mysql
url={mysql_extractor_url}

[sinker]
url=postgres://127.0.0.1:5433/postgres

[router]
tb_map=
";
        assert_eq!(
            TestEnv::parse_url_values(ini),
            vec![
                (
                    "extractor.url".to_string(),
                    "{mysql_extractor_url}".to_string()
                ),
                (
                    "sinker.url".to_string(),
                    "postgres://127.0.0.1:5433/postgres".to_string()
                ),
            ]
        );
    }

    #[test]
    fn parse_url_values_ignores_comments_and_empty_urls() {
        let ini = "[extractor]\n# url=commented\nurl=\n";
        assert!(TestEnv::parse_url_values(ini).is_empty());
    }

    #[test]
    fn resolve_placeholders_substitutes_env() {
        let resolved = TestEnv::resolve_placeholders(
            "{a}/x/{b}",
            env_of(&[("a", "mysql://h:1"), ("b", "tail")]),
        )
        .unwrap();
        assert_eq!(resolved, "mysql://h:1/x/tail");
    }

    #[test]
    fn resolve_placeholders_names_the_missing_var() {
        let err = TestEnv::resolve_placeholders("{missing_url}", env_of(&[])).unwrap_err();
        assert_eq!(err, "missing_url");
    }

    #[test]
    fn resolve_placeholders_leaves_literal_urls_alone() {
        let raw = "postgres://127.0.0.1:5433/postgres?options[statement_timeout]=10s";
        assert_eq!(
            TestEnv::resolve_placeholders(raw, env_of(&[])).unwrap(),
            raw
        );
    }

    #[test]
    fn host_port_reads_explicit_and_default_ports() {
        assert_eq!(
            TestEnv::host_port("mysql://root:123456@127.0.0.1:3307?ssl-mode=disabled"),
            Some(("127.0.0.1".to_string(), 3307))
        );
        assert_eq!(
            TestEnv::host_port("mongodb://127.0.0.1"),
            Some(("127.0.0.1".to_string(), 27017))
        );
        assert_eq!(
            TestEnv::host_port("oracle://127.0.0.1:15211/XE"),
            Some(("127.0.0.1".to_string(), 15211))
        );
    }

    #[test]
    fn host_port_reads_schemeless_brokers() {
        assert_eq!(
            TestEnv::host_port("127.0.0.1:9093"),
            Some(("127.0.0.1".to_string(), 9093))
        );
    }

    #[test]
    fn host_port_gives_up_instead_of_guessing() {
        assert_eq!(TestEnv::host_port(""), None);
        assert_eq!(TestEnv::host_port("s3c://ln-test"), None);
        assert_eq!(TestEnv::host_port("not a url"), None);
    }
}
