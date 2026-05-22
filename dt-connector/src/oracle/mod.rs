use std::process::Stdio;

use anyhow::{bail, Context};
use dt_common::config::connection_auth_config::ConnectionAuthConfig;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use url::Url;

const DEFAULT_ORACLE_PORT: u16 = 1521;
const ORACLE_HOME: &str = "/u01/app/oracle/product/11.2.0/xe";

fn shell_quote_single(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

fn has_sqlplus_error(text: &str) -> bool {
    text.lines().any(|line| {
        let trimmed = line.trim_start();
        trimmed.starts_with("ORA-") || trimmed.starts_with("SP2-")
    })
}

#[derive(Clone, Debug)]
pub struct OracleSqlPlusClient {
    pub url: String,
    pub connection_auth: ConnectionAuthConfig,
}

impl OracleSqlPlusClient {
    pub fn new(url: String, connection_auth: ConnectionAuthConfig) -> Self {
        Self {
            url,
            connection_auth,
        }
    }

    fn docker_container() -> Option<String> {
        std::env::var("ORACLE_SQLPLUS_DOCKER_CONTAINER")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }

    fn parse_url(&self) -> anyhow::Result<(String, u16, String)> {
        let parsed =
            Url::parse(&self.url).with_context(|| format!("invalid oracle url: {}", self.url))?;
        let host = parsed
            .host_str()
            .context("oracle url host missing")?
            .to_string();
        let port = parsed.port().unwrap_or(DEFAULT_ORACLE_PORT);
        let service = parsed.path().trim_start_matches('/').to_string();
        if service.is_empty() {
            bail!("oracle url service/SID missing: {}", self.url);
        }
        Ok((host, port, service))
    }

    fn get_basic_auth(&self) -> anyhow::Result<(String, String)> {
        match &self.connection_auth {
            ConnectionAuthConfig::Basic { username, password } => {
                Ok((username.clone(), password.clone().unwrap_or_default()))
            }
            ConnectionAuthConfig::NoAuth => bail!("oracle connection_auth requires username"),
        }
    }

    fn build_login(&self) -> anyhow::Result<String> {
        let (username, password) = self.get_basic_auth()?;
        let (_host, _port, service) = self.parse_url()?;

        // For local dt-tests we run `sqlplus` inside the Oracle XE container. In that mode,
        // connect to the in-container listener (`127.0.0.1:1521`) and reuse the service name.
        let (host, port) = if Self::docker_container().is_some() {
            ("127.0.0.1".to_string(), DEFAULT_ORACLE_PORT)
        } else {
            let (host, port, _service) = self.parse_url()?;
            (host, port)
        };

        Ok(format!(
            "{}/{}@//{}:{}/{}",
            username, password, host, port, service
        ))
    }

    fn build_sqlplus_script(sql: &str, with_query_format: bool) -> String {
        let mut out = String::new();
        out.push_str("WHENEVER SQLERROR EXIT SQL.SQLCODE;\n");
        out.push_str("SET PAGESIZE 0;\n");
        out.push_str("SET FEEDBACK OFF;\n");
        out.push_str("SET VERIFY OFF;\n");
        out.push_str("SET HEADING OFF;\n");
        out.push_str("SET ECHO OFF;\n");
        out.push_str("SET TRIMSPOOL ON;\n");
        out.push_str("SET TRIMOUT ON;\n");
        out.push_str("SET LINESIZE 32767;\n");
        if with_query_format {
            out.push_str("SET COLSEP '|';\n");
            out.push_str("SET NULL '<NULL>';\n");
        }
        out.push_str(sql);
        let trimmed = sql.trim_end();
        // Allow callers to pass fully-terminated multi-statement scripts (including PL/SQL blocks
        // that end with a trailing `/`).
        if trimmed.ends_with(';') || trimmed.ends_with('/') {
            out.push('\n');
        } else {
            out.push_str(";\n");
        }
        out.push_str("EXIT;\n");
        out
    }

    async fn run_sqlplus(&self, script: &str) -> anyhow::Result<(String, String)> {
        let login = self.build_login()?;

        let docker_container = Self::docker_container();
        let mut uses_docker = false;
        let mut cmd = if let Some(container) = docker_container {
            uses_docker = true;
            let quoted_login = shell_quote_single(&login);
            let quoted_script = shell_quote_single(script);
            let command = format!(
                "export ORACLE_HOME={}; export PATH=$ORACLE_HOME/bin:$PATH; export LD_LIBRARY_PATH=$ORACLE_HOME/lib; tmp=$(mktemp /tmp/ape-dts-sql.XXXXXX); printf %s {} > \"$tmp\"; sqlplus -s {} @\"$tmp\"; rc=$?; rm -f \"$tmp\"; exit $rc",
                ORACLE_HOME, quoted_script, quoted_login
            );
            let mut c = Command::new("docker");
            c.arg("exec")
                .arg(container)
                .arg("bash")
                .arg("-lc")
                .arg(command);
            c
        } else {
            let mut c = Command::new("sqlplus");
            c.arg("-s").arg(login);
            c
        };

        cmd.stdin(if uses_docker {
            Stdio::null()
        } else {
            Stdio::piped()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

        let mut child = cmd.spawn().context("failed to spawn sqlplus")?;
        if !uses_docker {
            if let Some(mut stdin) = child.stdin.take() {
                stdin
                    .write_all(script.as_bytes())
                    .await
                    .context("failed to write sqlplus script")?;
                stdin
                    .shutdown()
                    .await
                    .context("failed to close sqlplus stdin")?;
            }
        }

        let output = child.wait_with_output().await?;
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        if !output.status.success() || has_sqlplus_error(&stdout) || has_sqlplus_error(&stderr) {
            bail!(
                "sqlplus failed (exit={:?}). stderr: {}\nstdout: {}",
                output.status.code(),
                stderr.trim(),
                stdout.trim()
            );
        }
        Ok((stdout, stderr))
    }

    pub async fn exec(&self, sql: &str) -> anyhow::Result<()> {
        let script = Self::build_sqlplus_script(sql, false);
        let _ = self.run_sqlplus(&script).await?;
        Ok(())
    }

    pub async fn query_lines(&self, sql: &str) -> anyhow::Result<Vec<String>> {
        let script = Self::build_sqlplus_script(sql, true);
        let (stdout, _stderr) = self.run_sqlplus(&script).await?;
        Ok(stdout
            .lines()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::has_sqlplus_error;

    #[test]
    fn detects_oracle_error_in_sqlplus_stdout() {
        let stdout = "ERROR:\nORA-12514: TNS:listener does not currently know of service requested";
        assert!(has_sqlplus_error(stdout));
    }

    #[test]
    fn detects_sqlplus_error_in_sqlplus_stdout() {
        let stdout = "SP2-0306: Invalid option.";
        assert!(has_sqlplus_error(stdout));
    }

    #[test]
    fn allows_normal_sqlplus_query_output() {
        let stdout = "APE_DTS\n11.2.0.2.0\n";
        assert!(!has_sqlplus_error(stdout));
    }
}
