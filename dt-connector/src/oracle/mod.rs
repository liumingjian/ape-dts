use std::process::Stdio;

use anyhow::{bail, Context};
use dt_common::config::connection_auth_config::ConnectionAuthConfig;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use url::Url;

const DEFAULT_ORACLE_PORT: u16 = 1521;
const ORACLE_HOME: &str = "/u01/app/oracle/product/11.2.0/xe";
const CONNECTED_BANNER: &str = "Connected.";

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

    /// SQL*Plus `CONNECT` parses the password itself, so wrap it in double quotes to keep
    /// separators (`/`, `@`, spaces) inside the password. `"` has no escape form there, and a
    /// newline would split the CONNECT command, so both are rejected instead of silently
    /// producing a different connect string.
    fn quote_connect_password(password: &str) -> anyhow::Result<String> {
        if password.is_empty() {
            return Ok(String::new());
        }
        if password.contains('"') || password.contains('\n') || password.contains('\r') {
            bail!("oracle password contains characters sqlplus CONNECT cannot express: '\"' or newline. Use a password without them, or connect through a wallet.");
        }
        Ok(format!("\"{}\"", password))
    }

    /// The connect identifier, fed to sqlplus through the script (stdin) rather than argv so the
    /// password never shows up in the process list (`ps`, and `docker exec bash -lc` alike).
    fn build_connect_command(&self) -> anyhow::Result<String> {
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
            "CONNECT {}/{}@//{}:{}/{}",
            username,
            Self::quote_connect_password(&password)?,
            host,
            port,
            service
        ))
    }

    fn build_sqlplus_script(connect: &str, sql: &str, with_query_format: bool) -> String {
        let mut out = String::new();
        out.push_str("WHENEVER SQLERROR EXIT SQL.SQLCODE;\n");
        // Must precede CONNECT: without it SQL*Plus treats `&` in *any* text - including the
        // password and string literals such as 'A&B' - as a substitution variable, silently
        // rewriting the value or swallowing the rest of the script while it waits for input.
        out.push_str("SET DEFINE OFF;\n");
        // Blank lines inside a statement (multi-line text values) would otherwise end it early,
        // truncating the value. Every statement we emit is terminated with `;` or `/`.
        out.push_str("SET SQLBLANKLINES ON;\n");
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
        out.push_str(connect);
        out.push('\n');
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

    /// `sqlplus -s` normally stays quiet about CONNECT, but some releases still echo
    /// `Connected.` before the query output. Drop it when it is the first line so it cannot be
    /// mistaken for a result row.
    fn strip_connected_banner(lines: Vec<String>) -> Vec<String> {
        let mut lines = lines;
        if lines.first().map(|l| l.as_str()) == Some(CONNECTED_BANNER) {
            lines.remove(0);
        }
        lines
    }

    async fn run_sqlplus(&self, script: &str) -> anyhow::Result<(String, String)> {
        let docker_container = Self::docker_container();
        let mut cmd = if let Some(container) = docker_container {
            // The script (carrying CONNECT, hence the password) arrives on stdin and lands in a
            // `mktemp` file, which is created 0600 - never on the command line.
            let command = format!(
                "export ORACLE_HOME={}; export PATH=$ORACLE_HOME/bin:$PATH; export LD_LIBRARY_PATH=$ORACLE_HOME/lib; umask 077; tmp=$(mktemp /tmp/ape-dts-sql.XXXXXX) || exit 1; cat > \"$tmp\"; sqlplus -s /nolog @\"$tmp\"; rc=$?; rm -f \"$tmp\"; exit $rc",
                ORACLE_HOME
            );
            let mut c = Command::new("docker");
            c.arg("exec")
                .arg("-i")
                .arg(container)
                .arg("bash")
                .arg("-lc")
                .arg(command);
            c
        } else {
            let mut c = Command::new("sqlplus");
            c.arg("-s").arg("/nolog");
            c
        };

        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = cmd.spawn().context("failed to spawn sqlplus")?;
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
        let script = Self::build_sqlplus_script(&self.build_connect_command()?, sql, false);
        let _ = self.run_sqlplus(&script).await?;
        Ok(())
    }

    pub async fn query_lines(&self, sql: &str) -> anyhow::Result<Vec<String>> {
        let script = Self::build_sqlplus_script(&self.build_connect_command()?, sql, true);
        let (stdout, _stderr) = self.run_sqlplus(&script).await?;
        Ok(Self::strip_connected_banner(
            stdout
                .lines()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dt_common::config::connection_auth_config::ConnectionAuthConfig;

    fn client(password: Option<&str>) -> OracleSqlPlusClient {
        OracleSqlPlusClient::new(
            "oracle://db.example.com:1522/XE".to_string(),
            ConnectionAuthConfig::Basic {
                username: "ape_dts".to_string(),
                password: password.map(|p| p.to_string()),
            },
        )
    }

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

    #[test]
    fn disables_substitution_variables_before_connecting() {
        let script = OracleSqlPlusClient::build_sqlplus_script(
            "CONNECT ape_dts/\"pa&ss\"@//db:1521/XE",
            "INSERT INTO t VALUES ('A&B')",
            false,
        );
        let define_off = script.find("SET DEFINE OFF;").expect("SET DEFINE OFF missing");
        let connect = script.find("CONNECT ").expect("CONNECT missing");
        assert!(
            define_off < connect,
            "SET DEFINE OFF must precede CONNECT, script was:\n{}",
            script
        );
        assert!(script.contains("'A&B'"));
    }

    #[test]
    fn keeps_blank_lines_inside_statements() {
        let script = OracleSqlPlusClient::build_sqlplus_script(
            "CONNECT ape_dts@//db:1521/XE",
            "INSERT INTO t VALUES ('line1\n\nline3')",
            false,
        );
        assert!(script.contains("SET SQLBLANKLINES ON;"));
        assert!(script.contains("'line1\n\nline3')"));
    }

    #[test]
    fn terminates_script_after_connect_and_sql() {
        let script = OracleSqlPlusClient::build_sqlplus_script(
            "CONNECT ape_dts@//db:1521/XE",
            "SELECT 1 FROM DUAL",
            true,
        );
        assert!(script.contains("SET COLSEP '|';"));
        assert!(script.ends_with("SELECT 1 FROM DUAL;\nEXIT;\n"));
    }

    #[test]
    fn does_not_double_terminate_plsql_blocks() {
        let script = OracleSqlPlusClient::build_sqlplus_script(
            "CONNECT ape_dts@//db:1521/XE",
            "BEGIN NULL; END;\n/",
            false,
        );
        assert!(script.ends_with("BEGIN NULL; END;\n/\nEXIT;\n"));
    }

    #[test]
    fn quotes_password_in_connect_command() {
        let connect = client(Some("p@ss/word 1")).build_connect_command().unwrap();
        assert_eq!(
            connect,
            "CONNECT ape_dts/\"p@ss/word 1\"@//db.example.com:1522/XE"
        );
    }

    #[test]
    fn keeps_empty_password_unquoted() {
        let connect = client(None).build_connect_command().unwrap();
        assert_eq!(connect, "CONNECT ape_dts/@//db.example.com:1522/XE");
    }

    #[test]
    fn rejects_passwords_sqlplus_connect_cannot_express() {
        for password in ["pa\"ss", "pa\nss", "pa\rss"] {
            let err = client(Some(password)).build_connect_command().unwrap_err();
            assert!(
                err.to_string().contains("sqlplus CONNECT cannot express"),
                "unexpected error for {:?}: {}",
                password,
                err
            );
        }
    }

    #[test]
    fn strips_leading_connected_banner_only() {
        let lines = vec![
            "Connected.".to_string(),
            "APE_DTS".to_string(),
            "Connected.".to_string(),
        ];
        assert_eq!(
            OracleSqlPlusClient::strip_connected_banner(lines),
            vec!["APE_DTS".to_string(), "Connected.".to_string()]
        );
        assert_eq!(
            OracleSqlPlusClient::strip_connected_banner(vec!["APE_DTS".to_string()]),
            vec!["APE_DTS".to_string()]
        );
        assert!(OracleSqlPlusClient::strip_connected_banner(vec![]).is_empty());
    }
}
