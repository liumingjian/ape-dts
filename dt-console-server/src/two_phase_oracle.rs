use dt_common::config::connection_auth_config::ConnectionAuthConfig;

use crate::models::Task;

pub async fn capture_current_scn(task: &Task) -> std::io::Result<u64> {
    let source = parse_json_object(&task.source_endpoint)?;
    let url = source_string(&source, "url")?;
    let username = source_string(&source, "username").ok();
    let password = source_string(&source, "password").ok();
    let client = dt_connector::oracle::OracleSqlPlusClient::new(
        url,
        ConnectionAuthConfig::Basic {
            username: username.unwrap_or_default(),
            password,
        },
    );
    let lines = client
        .query_lines("SELECT CURRENT_SCN FROM V$DATABASE")
        .await
        .map_err(std::io::Error::other)?;
    parse_current_scn(&lines)
}

fn parse_current_scn(lines: &[String]) -> std::io::Result<u64> {
    let first = lines.first().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "oracle current_scn query returned no rows",
        )
    })?;
    first.trim().parse::<u64>().map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("invalid oracle CURRENT_SCN '{}': {e}", first.trim()),
        )
    })
}

fn parse_json_object(raw: &str) -> std::io::Result<serde_json::Map<String, serde_json::Value>> {
    match serde_json::from_str::<serde_json::Value>(raw) {
        Ok(serde_json::Value::Object(map)) => Ok(map),
        Ok(_) => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "endpoint config must be a JSON object",
        )),
        Err(e) => Err(std::io::Error::new(std::io::ErrorKind::InvalidData, e)),
    }
}

fn source_string(
    map: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> std::io::Result<String> {
    map.get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("source endpoint missing {key}"),
            )
        })
}
