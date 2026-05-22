use std::{env, fs, path::Path, time::Duration};

use anyhow::{bail, Context};
use dt_common::config::connection_auth_config::ConnectionAuthConfig;
use dt_task::task_util::TaskUtil;
use openssl::ssl::{SslConnector, SslMethod, SslVerifyMode};
use postgres_openssl::MakeTlsConnector;
use sqlx::{Pool, Postgres, Row};
use tokio::time::timeout;
use tokio_postgres::NoTls;
use url::Url;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    load_env_override("dt-tests/tests/.env")?;
    load_env_override("dt-tests/tests/.env.local")?;

    let prefix =
        env::var("GAUSSDB_PROBE_PREFIX").unwrap_or_else(|_| "gaussdb_pg_extractor".to_string());
    let url = env::var(format!("{prefix}_without_auth_url"))?;
    let auth = ConnectionAuthConfig::Basic {
        username: env::var(format!("{prefix}_username"))?,
        password: Some(env::var(format!("{prefix}_password"))?),
    };

    println!("prefix={prefix}");
    println!("base_url={}", redact_url(&url));
    println!("username={}", username(&auth));
    println!(
        "candidate_hosts={}",
        env::var("gaussdb_pg_candidate_hosts").unwrap_or_default()
    );

    let mut ok = false;
    for candidate in ordered_candidates(&url)? {
        for url in with_ssl_variants(&candidate) {
            match probe(&url, &auth).await {
                Ok(info) => {
                    ok = true;
                    println!("OK {} {}", redact_url(&url), info);
                }
                Err(e) => println!("ERR {} {e:#}", redact_url(&url)),
            }
        }

        for ssl_mode in [SslMode::Require, SslMode::Disable] {
            match probe_tokio_postgres(&candidate, &auth, ssl_mode).await {
                Ok(info) => {
                    ok = true;
                    println!(
                        "OK tokio-postgres {} sslmode={} {}",
                        redact_url(candidate.as_str()),
                        ssl_mode.as_str(),
                        info
                    );
                }
                Err(e) => println!(
                    "ERR tokio-postgres {} sslmode={} {e:#}",
                    redact_url(candidate.as_str()),
                    ssl_mode.as_str()
                ),
            }
        }
    }

    if !ok {
        bail!("all GaussDB probe attempts failed");
    }
    Ok(())
}

fn load_env_override(path: &str) -> anyhow::Result<()> {
    let path = Path::new(path);
    if !path.exists() {
        return Ok(());
    }
    for line in fs::read_to_string(path)?.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        env::set_var(key.trim(), value.trim());
    }
    Ok(())
}

fn username(auth: &ConnectionAuthConfig) -> &str {
    match auth {
        ConnectionAuthConfig::Basic { username, .. } => username,
        ConnectionAuthConfig::NoAuth => "",
    }
}

fn ordered_candidates(base_url: &str) -> anyhow::Result<Vec<Url>> {
    let base = Url::parse(base_url)?;
    let hosts = env::var("gaussdb_pg_candidate_hosts").unwrap_or_default();
    if hosts.trim().is_empty() {
        return Ok(vec![base]);
    }

    let mut out = Vec::new();
    for raw in hosts.split(',').map(str::trim).filter(|v| !v.is_empty()) {
        out.push(rewrite_host_port(&base, raw)?);
    }
    Ok(out)
}

fn rewrite_host_port(base: &Url, raw: &str) -> anyhow::Result<Url> {
    let mut url = base.clone();
    let (host, port) = match raw.rsplit_once(':') {
        Some((host, port)) if !host.is_empty() && port.chars().all(|c| c.is_ascii_digit()) => {
            (host, Some(port.parse::<u16>()?))
        }
        _ => (raw, base.port()),
    };
    url.set_host(Some(host))
        .map_err(|_| anyhow::anyhow!("invalid host {host}"))?;
    if let Some(port) = port {
        url.set_port(Some(port))
            .map_err(|_| anyhow::anyhow!("invalid port {port}"))?;
    }
    Ok(url)
}

fn with_ssl_variants(url: &Url) -> Vec<String> {
    let mut urls = vec![url.to_string()];
    if url
        .query_pairs()
        .any(|(key, value)| key == "sslmode" && value == "disable")
    {
        return urls;
    }

    let mut no_ssl = url.clone();
    let pairs: Vec<_> = no_ssl
        .query_pairs()
        .into_owned()
        .filter(|(key, _)| key != "sslmode")
        .collect();
    no_ssl.query_pairs_mut().clear();
    for (key, value) in pairs {
        no_ssl.query_pairs_mut().append_pair(&key, &value);
    }
    no_ssl.query_pairs_mut().append_pair("sslmode", "disable");
    urls.push(no_ssl.to_string());
    urls
}

async fn probe(url: &str, auth: &ConnectionAuthConfig) -> anyhow::Result<String> {
    let pool = timeout(
        Duration::from_secs(20),
        TaskUtil::create_pg_conn_pool(url, auth, 1, false, false),
    )
    .await
    .context("connect timed out")??;

    let info = timeout(Duration::from_secs(10), read_info(&pool))
        .await
        .context("query timed out")??;
    pool.close().await;
    Ok(info)
}

async fn read_info(pool: &Pool<Postgres>) -> anyhow::Result<String> {
    let row = sqlx::query(
        "SELECT \
            current_user::text, \
            current_database()::text, \
            inet_server_addr()::text, \
            current_setting('wal_level')::text, \
            pg_is_in_recovery()::text, \
            current_setting('sql_compatibility')::text",
    )
    .fetch_one(pool)
    .await?;
    Ok(format!(
        "current_user={} current_database={} server_addr={} wal_level={} in_recovery={} sql_compatibility={}",
        row.get::<String, _>(0),
        row.get::<String, _>(1),
        row.get::<String, _>(2),
        row.get::<String, _>(3),
        row.get::<String, _>(4),
        row.get::<String, _>(5)
    ))
}

#[derive(Clone, Copy)]
enum SslMode {
    Require,
    Disable,
}

impl SslMode {
    fn as_str(self) -> &'static str {
        match self {
            SslMode::Require => "require",
            SslMode::Disable => "disable",
        }
    }
}

async fn probe_tokio_postgres(
    url: &Url,
    auth: &ConnectionAuthConfig,
    ssl_mode: SslMode,
) -> anyhow::Result<String> {
    let client = match ssl_mode {
        SslMode::Disable => connect_tokio_no_tls(url, auth).await?,
        SslMode::Require => connect_tokio_tls(url, auth).await?,
    };

    let row = timeout(
        Duration::from_secs(10),
        client.query_one(
            "SELECT \
                current_user::text, \
                current_database()::text, \
                inet_server_addr()::text, \
                current_setting('wal_level')::text, \
                pg_is_in_recovery()::text, \
                current_setting('sql_compatibility')::text",
            &[],
        ),
    )
    .await
    .context("tokio-postgres query timed out")??;
    Ok(format!(
        "current_user={} current_database={} server_addr={} wal_level={} in_recovery={} sql_compatibility={}",
        row.get::<_, String>(0),
        row.get::<_, String>(1),
        row.get::<_, String>(2),
        row.get::<_, String>(3),
        row.get::<_, String>(4),
        row.get::<_, String>(5)
    ))
}

async fn connect_tokio_no_tls(
    url: &Url,
    auth: &ConnectionAuthConfig,
) -> anyhow::Result<tokio_postgres::Client> {
    let conn_info = tokio_conn_info(url, auth, SslMode::Disable)?;
    let (client, connection) = timeout(
        Duration::from_secs(20),
        tokio_postgres::connect(&conn_info, NoTls),
    )
    .await
    .context("tokio-postgres connect timed out")??;
    tokio::spawn(async move {
        let _ = connection.await;
    });
    Ok(client)
}

async fn connect_tokio_tls(
    url: &Url,
    auth: &ConnectionAuthConfig,
) -> anyhow::Result<tokio_postgres::Client> {
    let conn_info = tokio_conn_info(url, auth, SslMode::Require)?;
    let mut builder = SslConnector::builder(SslMethod::tls())?;
    builder.set_verify(SslVerifyMode::NONE);
    let (client, connection) = timeout(
        Duration::from_secs(20),
        tokio_postgres::connect(&conn_info, MakeTlsConnector::new(builder.build())),
    )
    .await
    .context("tokio-postgres connect timed out")??;
    tokio::spawn(async move {
        let _ = connection.await;
    });
    Ok(client)
}

fn tokio_conn_info(
    url: &Url,
    auth: &ConnectionAuthConfig,
    ssl_mode: SslMode,
) -> anyhow::Result<String> {
    let ConnectionAuthConfig::Basic { username, password } = auth else {
        bail!("GaussDB probe requires basic auth");
    };
    Ok(format!(
        "host={} port={} dbname={} user={} password={} sslmode={} protocolVersion=351",
        url.host_str().context("missing host")?,
        url.port().unwrap_or(5432),
        url.path().trim_start_matches('/'),
        username,
        password.as_deref().unwrap_or_default(),
        ssl_mode.as_str()
    ))
}

fn redact_url(url: &str) -> String {
    match Url::parse(url) {
        Ok(mut parsed) => {
            let _ = parsed.set_username("***");
            let _ = parsed.set_password(Some("***"));
            parsed.to_string()
        }
        Err(_) => url.to_string(),
    }
}
