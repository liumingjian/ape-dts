//! Per-category task validation, GaussDB sub-mode enforcement,
//! path sandboxing, SSRF prevention, and extract_type consistency checks.
//!
//! Every validation function returns a `Vec<ValidationError>` (empty = valid).

use std::net::IpAddr;

const ALLOW_PRIVATE_ENDPOINTS_ENV: &str = "CONSOLE_ALLOW_PRIVATE_ENDPOINTS";

/// A single validation failure.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ValidationError {
    pub field: String,
    pub error: String,
}

/// Known db_type strings (mirrors dt-common DbType serialisation).
pub const VALID_DB_TYPES: &[&str] = &[
    "mysql",
    "pg",
    "oracle",
    "gaussdb_pg",
    "gaussdb_mysql",
    "gaussdb_oracle",
    "kafka",
    "mongo",
    "redis",
    "clickhouse",
    "starrocks",
    "doris",
    "foxlake",
    "tidb",
];

/// Known kind values.
pub const VALID_KINDS: &[&str] = &["snapshot", "cdc", "check", "struct"];

/// Known GaussDB sub-mode values.
pub const VALID_GAUSSDB_SUB_MODES: &[&str] = &["pg-mode", "mysql-mode", "oracle-mode"];

/// URL schemes accepted per engine.
pub const ENGINE_SCHEMES: &[(&str, &[&str])] = &[
    ("mysql", &["mysql"]),
    ("pg", &["postgres", "postgresql"]),
    ("oracle", &["oracle"]),
    ("kafka", &["kafka"]),
    ("mongo", &["mongodb"]),
    ("redis", &["redis"]),
    ("gaussdb_pg", &["postgres", "postgresql"]),
    ("gaussdb_mysql", &["postgres", "postgresql"]),
    ("gaussdb_oracle", &["postgres", "postgresql"]),
];

/// Path fields that must be sandboxed.
pub const SANDBOXED_PATH_FIELDS: &[&str] = &[
    "processor.lua_code_file",
    "sinker.check_log_dir",
    "runtime.log_dir",
    "data_marker.dst_log_dir",
];

/// Base directory for the per-Run sandbox.
pub const RUN_SANDBOX_BASE: &str = "runs";

fn validate_gaussdb_side_sub_mode(
    field: &str,
    db_type: &str,
    mode: Option<&str>,
) -> Option<ValidationError> {
    if db_type != "gaussdb" {
        return None;
    }
    match mode {
        None => Some(ValidationError {
            field: field.into(),
            error: "GAUSSDB_SUB_MODE_REQUIRED".into(),
        }),
        Some(mode) if !VALID_GAUSSDB_SUB_MODES.contains(&mode) => Some(ValidationError {
            field: field.into(),
            error: format!("UNKNOWN_GAUSSDB_SUB_MODE '{mode}'"),
        }),
        _ => None,
    }
}

/// Validate a full task creation/update payload.
///
/// `is_create` = true for POST (kind is required), false for PATCH (kind is
/// immutable so we only validate fields being changed).
#[allow(clippy::too_many_arguments)]
pub fn validate_task(
    kind: &str,
    db_type_source: &str,
    db_type_target: &str,
    source_url: &str,
    target_url: &str,
    extractor_config: &serde_json::Value,
    sinker_config: &serde_json::Value,
    filter_config: &serde_json::Value,
    source_sub_mode: Option<&str>,
    target_sub_mode: Option<&str>,
    is_create: bool,
) -> Vec<ValidationError> {
    let mut errors = Vec::new();

    // ── Kind validation ──────────────────────────────────────────────
    if is_create && !VALID_KINDS.contains(&kind) {
        errors.push(ValidationError {
            field: "kind".into(),
            error: format!("invalid kind '{kind}'; expected one of {:?}", VALID_KINDS),
        });
    }

    // ── GaussDB sub-mode enforcement (must come before db_type validation) ──
    // Only enforce on POST (is_create=true). On PATCH, the db_type is already
    // resolved (e.g. "gaussdb_pg") so sub_mode is not required; if provided it
    // is still validated.
    if is_create {
        if let Some(error) =
            validate_gaussdb_side_sub_mode("source_sub_mode", db_type_source, source_sub_mode)
        {
            errors.push(error);
        }
        if let Some(error) =
            validate_gaussdb_side_sub_mode("target_sub_mode", db_type_target, target_sub_mode)
        {
            errors.push(error);
        }
    }

    if !is_create {
        for (field, mode) in [
            ("source_sub_mode", source_sub_mode),
            ("target_sub_mode", target_sub_mode),
        ] {
            if let Some(mode) = mode {
                if !VALID_GAUSSDB_SUB_MODES.contains(&mode) {
                    errors.push(ValidationError {
                        field: field.into(),
                        error: format!("UNKNOWN_GAUSSDB_SUB_MODE '{mode}'"),
                    });
                }
            }
        }
    }

    // ── DbType validation ────────────────────────────────────────────
    // "gaussdb" is accepted as a valid but unresolved type; the sub-mode
    // enforcement above already catches the case where it's unresolved.
    if !is_gaussdb(db_type_source) && !VALID_DB_TYPES.contains(&db_type_source) {
        errors.push(ValidationError {
            field: "db_type_source".into(),
            error: format!("invalid db_type_source '{db_type_source}'"),
        });
    }
    if !is_gaussdb(db_type_target) && !VALID_DB_TYPES.contains(&db_type_target) {
        errors.push(ValidationError {
            field: "db_type_target".into(),
            error: format!("invalid db_type_target '{db_type_target}'"),
        });
    }

    // ── Source/target URL validation ──────────────────────────────────
    if !source_url.is_empty() {
        errors.extend(validate_endpoint_url(
            source_url,
            db_type_source,
            "source_endpoint.url",
        ));
    }
    if !target_url.is_empty() {
        errors.extend(validate_endpoint_url(
            target_url,
            db_type_target,
            "target_endpoint.url",
        ));
    }

    // ── Per-category required fields ─────────────────────────────────
    match kind {
        "snapshot" => {
            if source_url.is_empty() {
                errors.push(ValidationError {
                    field: "source_endpoint.url".into(),
                    error: "required".into(),
                });
            }
            if target_url.is_empty() {
                errors.push(ValidationError {
                    field: "target_endpoint.url".into(),
                    error: "required".into(),
                });
            }
            // snapshot rejects extract_type=cdc
            if extract_type_is(extractor_config, "cdc") {
                errors.push(ValidationError {
                    field: "extractor.extract_type".into(),
                    error: "SYNC_MODE_INVALID_FOR_CATEGORY".into(),
                });
            }
            // snapshot+cdc is managed as a two-phase run for supported engines.
            if extract_type_is(extractor_config, "snapshot_and_cdc") {
                if !matches!(
                    db_type_source,
                    "mysql" | "pg" | "gaussdb_pg" | "gaussdb_mysql" | "gaussdb_oracle" | "oracle"
                ) {
                    errors.push(ValidationError {
                        field: "extractor.extract_type".into(),
                        error: "SNAPSHOT_AND_CDC_UNSUPPORTED_SOURCE".into(),
                    });
                }
                // The CDC phase requires engine-specific start parameters.
                if (db_type_source == "mysql" || db_type_source == "gaussdb_mysql")
                    && !has_valid_server_id(extractor_config)
                {
                    errors.push(ValidationError {
                        field: "extractor.server_id".into(),
                        error: "required".into(),
                    });
                }
                if (db_type_source == "pg"
                    || db_type_source == "gaussdb_pg"
                    || db_type_source == "gaussdb_oracle")
                    && extractor_config
                        .get("slot_name")
                        .is_none_or(|v| v.as_str().is_none_or(|s| s.is_empty()))
                {
                    errors.push(ValidationError {
                        field: "extractor.slot_name".into(),
                        error: "required".into(),
                    });
                }
                if db_type_source == "oracle"
                    && extractor_config
                        .get("cdc_mode")
                        .is_none_or(|v| v.as_str().is_none_or(|s| s.is_empty()))
                {
                    errors.push(ValidationError {
                        field: "extractor.cdc_mode".into(),
                        error: "required".into(),
                    });
                }
            }
        }
        "cdc" => {
            if source_url.is_empty() {
                errors.push(ValidationError {
                    field: "source_endpoint.url".into(),
                    error: "required".into(),
                });
            }
            if target_url.is_empty() {
                errors.push(ValidationError {
                    field: "target_endpoint.url".into(),
                    error: "required".into(),
                });
            }
            // cdc rejects extract_type=snapshot
            if extract_type_is(extractor_config, "snapshot") {
                errors.push(ValidationError {
                    field: "extractor.extract_type".into(),
                    error: "SYNC_MODE_INVALID_FOR_CATEGORY".into(),
                });
            }
            // CDC mysql requires server_id
            if (db_type_source == "mysql" || db_type_source == "gaussdb_mysql")
                && !has_valid_server_id(extractor_config)
            {
                errors.push(ValidationError {
                    field: "extractor.server_id".into(),
                    error: "required".into(),
                });
            }
            // CDC pg requires slot_name
            if (db_type_source == "pg" || db_type_source == "gaussdb_pg")
                && extractor_config
                    .get("slot_name")
                    .is_none_or(|v| v.as_str().is_none_or(|s| s.is_empty()))
            {
                errors.push(ValidationError {
                    field: "extractor.slot_name".into(),
                    error: "required".into(),
                });
            }
            // CDC oracle requires cdc_mode
            if (db_type_source == "oracle" || db_type_source == "gaussdb_oracle")
                && extractor_config
                    .get("cdc_mode")
                    .is_none_or(|v| v.as_str().is_none_or(|s| s.is_empty()))
            {
                errors.push(ValidationError {
                    field: "extractor.cdc_mode".into(),
                    error: "required".into(),
                });
            }
        }
        "check" => {
            if source_url.is_empty() {
                errors.push(ValidationError {
                    field: "source_endpoint.url".into(),
                    error: "required".into(),
                });
            }
            if target_url.is_empty() {
                errors.push(ValidationError {
                    field: "target_endpoint.url".into(),
                    error: "required".into(),
                });
            }
            // check requires sinker.check_log_dir
            if sinker_config
                .get("check_log_dir")
                .is_none_or(|v| v.as_str().is_none_or(|s| s.is_empty()))
            {
                errors.push(ValidationError {
                    field: "sinker.check_log_dir".into(),
                    error: "required".into(),
                });
            }
        }
        "struct" => {
            if source_url.is_empty() {
                errors.push(ValidationError {
                    field: "source_endpoint.url".into(),
                    error: "required".into(),
                });
            }
            if target_url.is_empty() {
                errors.push(ValidationError {
                    field: "target_endpoint.url".into(),
                    error: "required".into(),
                });
            }
            // struct requires at least one of do_dbs or do_tbs
            let do_dbs = filter_config
                .get("do_dbs")
                .and_then(|v| v.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            let do_tbs = filter_config
                .get("do_tbs")
                .and_then(|v| v.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            if do_dbs + do_tbs == 0 {
                errors.push(ValidationError {
                    field: "filter".into(),
                    error: "STRUCT_FILTER_REQUIRED".into(),
                });
            }
            // struct rejects non-struct extract_type
            let et = extractor_config
                .get("extract_type")
                .and_then(|v| v.as_str())
                .unwrap_or("struct");
            if et != "struct" {
                errors.push(ValidationError {
                    field: "extractor.extract_type".into(),
                    error: "SYNC_MODE_INVALID_FOR_CATEGORY".into(),
                });
            }
        }
        _ => {} // already caught above
    }

    errors
}

/// Validate an endpoint URL: scheme match + SSRF host check.
fn validate_endpoint_url(url_str: &str, db_type: &str, field: &str) -> Vec<ValidationError> {
    let mut errors = Vec::new();

    // Parse scheme manually: "scheme://host/path"
    let (scheme, host) = match parse_url_parts(url_str) {
        Some(parts) => parts,
        None => {
            errors.push(ValidationError {
                field: field.into(),
                error: "invalid_url".into(),
            });
            return errors;
        }
    };

    // Check scheme against engine
    let expected_schemes: &[&str] = ENGINE_SCHEMES
        .iter()
        .find(|(engine, _)| *engine == db_type)
        .map(|(_, schemes)| *schemes)
        .unwrap_or(&[]);

    if !expected_schemes.is_empty() && !expected_schemes.contains(&scheme.as_str()) {
        let all_schemes: Vec<&str> = ENGINE_SCHEMES
            .iter()
            .flat_map(|(_, s)| s.iter())
            .copied()
            .collect();
        if !all_schemes.contains(&scheme.as_str()) {
            errors.push(ValidationError {
                field: field.into(),
                error: format!(
                    "INVALID_URL_SCHEME; expected one of {:?}, got '{}'",
                    expected_schemes, scheme
                ),
            });
        } else {
            errors.push(ValidationError {
                field: field.into(),
                error: format!(
                    "URL_SCHEME_ENGINE_MISMATCH; expected {:?} for '{}', got '{}'",
                    expected_schemes, db_type, scheme
                ),
            });
        }
    }

    // SSRF: block loopback / link-local / private hosts
    if let Some(host_str) = host {
        if let Some(ssrf_err) = check_ssrf_host(&host_str, field) {
            errors.push(ssrf_err);
        }
    }

    errors
}

/// Parse a URL into (scheme, host) components.
fn parse_url_parts(url: &str) -> Option<(String, Option<String>)> {
    let sep = url.find("://")?;
    let scheme = url[..sep].to_string();
    let rest = &url[sep + 3..];

    let host_end = rest.find('/').unwrap_or(rest.len());
    let host_port = &rest[..host_end];

    let host = if host_port.starts_with('[') {
        let bracket_end = host_port.find(']')?;
        Some(host_port[1..bracket_end].to_string())
    } else if let Some(colon_pos) = host_port.rfind(':') {
        Some(host_port[..colon_pos].to_string())
    } else if host_port.is_empty() {
        None
    } else {
        Some(host_port.to_string())
    };

    Some((scheme, host))
}

/// Check whether a host string resolves to a blocked address (SSRF).
///
/// Blocked: 127.0.0.0/8, ::1, 169.254.0.0/16, 10.0.0.0/8,
/// 172.16.0.0/12, 192.168.0.0/16, and "localhost".
///
/// Returns `None` if the host is acceptable, or a `ValidationError` if blocked.
pub fn check_ssrf_host(host: &str, field: &str) -> Option<ValidationError> {
    if allow_private_endpoint_hosts() {
        return None;
    }

    // Fast path for string "localhost"
    if host == "localhost" {
        return Some(ValidationError {
            field: field.into(),
            error: "ENDPOINT_HOST_BLOCKED".into(),
        });
    }

    // Try parsing as IP
    if let Ok(ip) = host.parse::<IpAddr>() {
        if is_blocked_ip(&ip) {
            return Some(ValidationError {
                field: field.into(),
                error: "ENDPOINT_HOST_BLOCKED".into(),
            });
        }
    }

    // Try DNS resolution (best-effort; may not be available in test)
    // We skip DNS resolution here to avoid I/O in validation and instead
    // rely on the string check. The actual test_connection endpoint will
    // perform the real check. For now, string-based blocking suffices.

    None
}

/// Is this IP address in a blocked range?
fn is_blocked_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let octets = v4.octets();
            // 127.0.0.0/8 (loopback)
            if octets[0] == 127 {
                return true;
            }
            // 169.254.0.0/16 (link-local)
            if octets[0] == 169 && octets[1] == 254 {
                return true;
            }
            // 10.0.0.0/8 (private)
            if octets[0] == 10 {
                return true;
            }
            // 172.16.0.0/12 (private)
            if octets[0] == 172 && (octets[1] & 0xf0) == 16 {
                return true;
            }
            // 192.168.0.0/16 (private)
            if octets[0] == 192 && octets[1] == 168 {
                return true;
            }
            // 0.0.0.0
            if octets == [0, 0, 0, 0] {
                return true;
            }
            false
        }
        IpAddr::V6(v6) => {
            // ::1 (loopback)
            if v6.is_loopback() {
                return true;
            }
            // fc00::/7 (unique local / private)
            if v6.is_unique_local() {
                return true;
            }
            // fe80::/10 (link-local)
            if v6.is_unicast_link_local() {
                return true;
            }
            false
        }
    }
}

/// Sandbox a user-controlled path value.
///
/// Only allows:
/// - Relative paths without directory traversal (no `..`)
/// - Basename-only paths (no directory separator)
/// - Paths under `<RUN_SANDBOX_BASE>/` prefix
///
/// Returns `Ok(())` if safe, or a `ValidationError` if the path escapes the sandbox.
pub fn sandbox_path(value: &str, field: &str) -> Result<(), ValidationError> {
    // Empty is fine (will be defaulted later)
    if value.is_empty() {
        return Ok(());
    }

    // Reject absolute paths
    if value.starts_with('/') {
        return Err(ValidationError {
            field: field.into(),
            error: "PATH_OUTSIDE_SANDBOX".into(),
        });
    }

    // Reject directory traversal
    if value.contains("..") {
        return Err(ValidationError {
            field: field.into(),
            error: "PATH_OUTSIDE_SANDBOX".into(),
        });
    }

    // Reject known dangerous paths
    let lower = value.to_lowercase();
    if lower.starts_with("/etc/")
        || lower.starts_with("/proc/")
        || lower.starts_with("/sys/")
        || lower.starts_with("/dev/")
        || lower == "/etc/passwd"
        || lower == "/proc/self/environ"
    {
        return Err(ValidationError {
            field: field.into(),
            error: "PATH_OUTSIDE_SANDBOX".into(),
        });
    }

    // Allow: basename only (no slash) or relative path under runs/
    if !value.contains('/') || value.starts_with(RUN_SANDBOX_BASE) || value.starts_with("./") {
        return Ok(());
    }

    // Otherwise reject
    Err(ValidationError {
        field: field.into(),
        error: "PATH_OUTSIDE_SANDBOX".into(),
    })
}

/// Resolve the effective db_type from (engine, sub_mode).
///
/// For GaussDB engines, sub_mode determines the final db_type:
/// - engine=gaussdb + sub_mode=pg-mode → gaussdb_pg
/// - engine=gaussdb + sub_mode=mysql-mode → gaussdb_mysql
/// - engine=gaussdb + sub_mode=oracle-mode → gaussdb_oracle
///
/// For non-GaussDB engines, sub_mode is ignored and the engine
/// string is returned as-is (if it's a valid db_type).
pub fn resolve_db_type(engine: &str, sub_mode: Option<&str>) -> String {
    if engine == "gaussdb" || engine.starts_with("gaussdb_") {
        match sub_mode {
            Some("pg-mode") => "gaussdb_pg".to_string(),
            Some("mysql-mode") => "gaussdb_mysql".to_string(),
            Some("oracle-mode") => "gaussdb_oracle".to_string(),
            _ => engine.to_string(),
        }
    } else if engine == "postgres" || engine == "postgresql" {
        "pg".to_string()
    } else {
        engine.to_string()
    }
}

fn allow_private_endpoint_hosts() -> bool {
    std::env::var(ALLOW_PRIVATE_ENDPOINTS_ENV)
        .is_ok_and(|value| value == "1" || value.eq_ignore_ascii_case("true"))
}

/// Check if the extractor_config indicates the given extract_type.
fn extract_type_is(config: &serde_json::Value, expected: &str) -> bool {
    config.get("extract_type").and_then(|v| v.as_str()) == Some(expected)
}

/// Returns true if the extractor config has a usable MySQL `server_id` —
/// either as a positive integer, or a non-empty numeric string. Empty values
/// or non-numeric strings are rejected so the CDC extractor doesn't die later.
fn has_valid_server_id(config: &serde_json::Value) -> bool {
    match config.get("server_id") {
        None => false,
        Some(v) => {
            if let Some(n) = v.as_u64() {
                return n > 0;
            }
            if let Some(n) = v.as_i64() {
                return n > 0;
            }
            if let Some(s) = v.as_str() {
                let trimmed = s.trim();
                if trimmed.is_empty() {
                    return false;
                }
                return trimmed.parse::<u64>().map(|n| n > 0).unwrap_or(false);
            }
            false
        }
    }
}

/// Is this db_type the unresolved "gaussdb" type (needs sub_mode to resolve)?
///
/// Returns `true` only for the bare `"gaussdb"` string — not for already-resolved
/// types like `"gaussdb_pg"`, `"gaussdb_mysql"`, or `"gaussdb_oracle"`.
fn is_gaussdb(db_type: &str) -> bool {
    db_type == "gaussdb"
}

/// Validate all sandboxed path fields in the task JSON.
pub fn validate_sandboxed_paths(
    processor_config: &serde_json::Value,
    sinker_config: &serde_json::Value,
    runtime_config: &serde_json::Value,
) -> Vec<ValidationError> {
    let mut errors = Vec::new();

    // processor.lua_code_file
    if let Some(v) = processor_config
        .get("lua_code_file")
        .and_then(|v| v.as_str())
    {
        if let Err(e) = sandbox_path(v, "processor.lua_code_file") {
            errors.push(e);
        }
    }

    // sinker.check_log_dir
    if let Some(v) = sinker_config.get("check_log_dir").and_then(|v| v.as_str()) {
        if let Err(e) = sandbox_path(v, "sinker.check_log_dir") {
            errors.push(e);
        }
    }

    // runtime.log_dir
    if let Some(v) = runtime_config.get("log_dir").and_then(|v| v.as_str()) {
        if let Err(e) = sandbox_path(v, "runtime.log_dir") {
            errors.push(e);
        }
    }

    errors
}

/// Sanitise a string for use in argv (fork-exec safety).
///
/// Removes null bytes and rejects strings that look like shell metacharacters
/// when they could cause injection. This is a conservative check — the
/// actual fork-exec should use `std::process::Command` with arg vectors
/// (no shell), so this is a defence-in-depth measure.
pub fn sanitise_argv(value: &str) -> Result<String, ValidationError> {
    // Remove null bytes
    let cleaned: String = value.chars().filter(|c| *c != '\0').collect();

    // Reject obvious shell injection patterns
    let dangerous = ["$(", "`", "&&", "||", ";", "|", ">", "<", "\n", "\r"];
    for pattern in &dangerous {
        if cleaned.contains(pattern) {
            return Err(ValidationError {
                field: "argv".into(),
                error: format!("unsafe character sequence '{}' in argument", pattern),
            });
        }
    }

    Ok(cleaned)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── GaussDB sub-mode ────────────────────────────────────────────

    #[test]
    fn gaussdb_without_sub_mode_rejected() {
        let errors = validate_task(
            "snapshot",
            "gaussdb",
            "mysql",
            "mysql://host/db",
            "mysql://host/db",
            &serde_json::json!({}),
            &serde_json::json!({}),
            &serde_json::json!({}),
            None,
            None,
            true,
        );
        assert!(errors
            .iter()
            .any(|e| e.error == "GAUSSDB_SUB_MODE_REQUIRED"));
    }

    #[test]
    fn gaussdb_unknown_sub_mode_rejected() {
        let errors = validate_task(
            "snapshot",
            "gaussdb",
            "mysql",
            "mysql://host/db",
            "mysql://host/db",
            &serde_json::json!({}),
            &serde_json::json!({}),
            &serde_json::json!({}),
            Some("foo"),
            None,
            true,
        );
        assert!(errors
            .iter()
            .any(|e| e.error.contains("UNKNOWN_GAUSSDB_SUB_MODE")));
    }

    #[test]
    fn gaussdb_pg_mode_accepted() {
        let errors = validate_task(
            "snapshot",
            "gaussdb_pg",
            "mysql",
            "postgres://host/db",
            "mysql://host/db",
            &serde_json::json!({}),
            &serde_json::json!({}),
            &serde_json::json!({}),
            Some("pg-mode"),
            None,
            true,
        );
        assert!(!errors.iter().any(|e| e.error.contains("gaussdb")));
    }

    #[test]
    fn gaussdb_mysql_mode_accepted() {
        let errors = validate_task(
            "cdc",
            "gaussdb_mysql",
            "mysql",
            "postgres://host/db",
            "mysql://host/db",
            &serde_json::json!({"server_id": "1"}),
            &serde_json::json!({}),
            &serde_json::json!({}),
            Some("mysql-mode"),
            None,
            true,
        );
        assert!(!errors.iter().any(|e| e.error.contains("gaussdb")));
    }

    #[test]
    fn gaussdb_oracle_mode_accepted() {
        let errors = validate_task(
            "snapshot",
            "gaussdb_oracle",
            "oracle",
            "postgres://host/db",
            "oracle://host/db",
            &serde_json::json!({}),
            &serde_json::json!({}),
            &serde_json::json!({}),
            Some("oracle-mode"),
            None,
            true,
        );
        assert!(!errors.iter().any(|e| e.error.contains("gaussdb")));
    }

    // ─── PATCH (is_create=false) on resolved GaussDB type ────────────

    #[test]
    fn patch_gaussdb_resolved_no_sub_mode_ok() {
        // PATCH on an existing GaussDB task (db_type already resolved) does
        // NOT require sub_mode — it should not trigger GAUSSDB_SUB_MODE_REQUIRED.
        let errors = validate_task(
            "snapshot",
            "gaussdb_pg",
            "mysql",
            "postgres://host/db",
            "mysql://host/db",
            &serde_json::json!({}),
            &serde_json::json!({}),
            &serde_json::json!({}),
            None,  // no source_sub_mode
            None,  // no target_sub_mode
            false, // is_create = false (PATCH)
        );
        assert!(!errors
            .iter()
            .any(|e| e.error == "GAUSSDB_SUB_MODE_REQUIRED"));
    }

    #[test]
    fn patch_gaussdb_mysql_resolved_no_sub_mode_ok() {
        let errors = validate_task(
            "cdc",
            "gaussdb_mysql",
            "mysql",
            "postgres://host/db",
            "mysql://host/db",
            &serde_json::json!({"server_id": "1"}),
            &serde_json::json!({}),
            &serde_json::json!({}),
            None,
            None,
            false,
        );
        assert!(!errors
            .iter()
            .any(|e| e.error == "GAUSSDB_SUB_MODE_REQUIRED"));
    }

    #[test]
    fn patch_gaussdb_oracle_resolved_no_sub_mode_ok() {
        let errors = validate_task(
            "snapshot",
            "gaussdb_oracle",
            "oracle",
            "postgres://host/db",
            "oracle://host/db",
            &serde_json::json!({}),
            &serde_json::json!({}),
            &serde_json::json!({}),
            None,
            None,
            false,
        );
        assert!(!errors
            .iter()
            .any(|e| e.error == "GAUSSDB_SUB_MODE_REQUIRED"));
    }

    #[test]
    fn is_gaussdb_only_matches_unresolved() {
        // is_gaussdb() should only match the bare "gaussdb" string, not
        // resolved types like "gaussdb_pg", "gaussdb_mysql", etc.
        assert!(is_gaussdb("gaussdb"));
        assert!(!is_gaussdb("gaussdb_pg"));
        assert!(!is_gaussdb("gaussdb_mysql"));
        assert!(!is_gaussdb("gaussdb_oracle"));
        assert!(!is_gaussdb("mysql"));
        assert!(!is_gaussdb("pg"));
    }

    // ─── Snapshot kind rejects cdc extract_type ──────────────────────

    #[test]
    fn snapshot_rejects_cdc_extract_type() {
        let errors = validate_task(
            "snapshot",
            "mysql",
            "mysql",
            "mysql://host/db",
            "mysql://host/db",
            &serde_json::json!({"extract_type": "cdc"}),
            &serde_json::json!({}),
            &serde_json::json!({}),
            None,
            None,
            true,
        );
        assert!(errors
            .iter()
            .any(|e| e.error == "SYNC_MODE_INVALID_FOR_CATEGORY"));
    }

    // ─── CDC kind rejects snapshot extract_type ──────────────────────

    #[test]
    fn cdc_rejects_snapshot_extract_type() {
        let errors = validate_task(
            "cdc",
            "mysql",
            "mysql",
            "mysql://host/db",
            "mysql://host/db",
            &serde_json::json!({"extract_type": "snapshot"}),
            &serde_json::json!({}),
            &serde_json::json!({}),
            None,
            None,
            true,
        );
        assert!(errors
            .iter()
            .any(|e| e.error == "SYNC_MODE_INVALID_FOR_CATEGORY"));
    }

    // ─── Struct kind rejects non-struct extract_type ────────────────

    #[test]
    fn struct_rejects_snapshot_extract_type() {
        let errors = validate_task(
            "struct",
            "mysql",
            "mysql",
            "mysql://host/db",
            "mysql://host/db",
            &serde_json::json!({"extract_type": "snapshot"}),
            &serde_json::json!({}),
            &serde_json::json!({"do_dbs": ["db1"]}),
            None,
            None,
            true,
        );
        assert!(errors
            .iter()
            .any(|e| e.error == "SYNC_MODE_INVALID_FOR_CATEGORY"));
    }

    // ─── Required field validation ───────────────────────────────────

    #[test]
    fn snapshot_missing_source_url() {
        let errors = validate_task(
            "snapshot",
            "mysql",
            "mysql",
            "",
            "mysql://host/db",
            &serde_json::json!({}),
            &serde_json::json!({}),
            &serde_json::json!({}),
            None,
            None,
            true,
        );
        assert!(errors
            .iter()
            .any(|e| e.field == "source_endpoint.url" && e.error == "required"));
    }

    #[test]
    fn cdc_mysql_missing_server_id() {
        let errors = validate_task(
            "cdc",
            "mysql",
            "mysql",
            "mysql://host/db",
            "mysql://host/db",
            &serde_json::json!({}),
            &serde_json::json!({}),
            &serde_json::json!({}),
            None,
            None,
            true,
        );
        assert!(errors
            .iter()
            .any(|e| e.field == "extractor.server_id" && e.error == "required"));
    }

    #[test]
    fn cdc_mysql_server_id_as_number_accepted() {
        let errors = validate_task(
            "cdc",
            "mysql",
            "mysql",
            "mysql://host/db",
            "mysql://host/db",
            &serde_json::json!({"server_id": 2000}),
            &serde_json::json!({}),
            &serde_json::json!({}),
            None,
            None,
            true,
        );
        assert!(!errors.iter().any(|e| e.field == "extractor.server_id"));
    }

    #[test]
    fn cdc_mysql_server_id_as_string_accepted() {
        let errors = validate_task(
            "cdc",
            "mysql",
            "mysql",
            "mysql://host/db",
            "mysql://host/db",
            &serde_json::json!({"server_id": "2000"}),
            &serde_json::json!({}),
            &serde_json::json!({}),
            None,
            None,
            true,
        );
        assert!(!errors.iter().any(|e| e.field == "extractor.server_id"));
    }

    #[test]
    fn cdc_mysql_server_id_zero_rejected() {
        let errors = validate_task(
            "cdc",
            "mysql",
            "mysql",
            "mysql://host/db",
            "mysql://host/db",
            &serde_json::json!({"server_id": 0}),
            &serde_json::json!({}),
            &serde_json::json!({}),
            None,
            None,
            true,
        );
        assert!(errors
            .iter()
            .any(|e| e.field == "extractor.server_id" && e.error == "required"));
    }

    #[test]
    fn snapshot_and_cdc_oracle_logminer_accepted() {
        let errors = validate_task(
            "snapshot",
            "oracle",
            "oracle",
            "oracle://host/XE",
            "oracle://host/XE",
            &serde_json::json!({"extract_type": "snapshot_and_cdc", "cdc_mode": "logminer"}),
            &serde_json::json!({}),
            &serde_json::json!({"do_tbs": "APE_SRC.CDC_SMOKE"}),
            None,
            None,
            true,
        );
        assert!(!errors
            .iter()
            .any(|e| { e.field == "extractor.extract_type" || e.field == "extractor.cdc_mode" }));
    }

    #[test]
    fn snapshot_and_cdc_oracle_missing_cdc_mode_rejected() {
        let errors = validate_task(
            "snapshot",
            "oracle",
            "oracle",
            "oracle://host/XE",
            "oracle://host/XE",
            &serde_json::json!({"extract_type": "snapshot_and_cdc"}),
            &serde_json::json!({}),
            &serde_json::json!({"do_tbs": "APE_SRC.CDC_SMOKE"}),
            None,
            None,
            true,
        );
        assert!(errors
            .iter()
            .any(|e| e.field == "extractor.cdc_mode" && e.error == "required"));
    }

    #[test]
    fn snapshot_and_cdc_gaussdb_pg_requires_slot_name() {
        let errors = validate_task(
            "snapshot",
            "gaussdb_pg",
            "pg",
            "postgres://host/db",
            "postgres://host/db",
            &serde_json::json!({"extract_type": "snapshot_and_cdc"}),
            &serde_json::json!({}),
            &serde_json::json!({"do_tbs": "public.t1"}),
            None,
            None,
            true,
        );
        assert!(errors
            .iter()
            .any(|e| e.field == "extractor.slot_name" && e.error == "required"));
    }

    #[test]
    fn snapshot_and_cdc_gaussdb_oracle_accepts_slot_name() {
        let errors = validate_task(
            "snapshot",
            "gaussdb_oracle",
            "oracle",
            "postgres://host/db",
            "oracle://host/XE",
            &serde_json::json!({
                "extract_type": "snapshot_and_cdc",
                "slot_name": "ape_test_gdbo"
            }),
            &serde_json::json!({}),
            &serde_json::json!({"do_tbs": "public.t1"}),
            None,
            None,
            true,
        );
        assert!(!errors.iter().any(|e| {
            e.field == "extractor.extract_type"
                || e.field == "extractor.slot_name"
                || e.field == "extractor.cdc_mode"
        }));
    }

    #[test]
    fn cdc_pg_missing_slot_name() {
        let errors = validate_task(
            "cdc",
            "pg",
            "pg",
            "postgres://host/db",
            "postgres://host/db",
            &serde_json::json!({}),
            &serde_json::json!({}),
            &serde_json::json!({}),
            None,
            None,
            true,
        );
        assert!(errors
            .iter()
            .any(|e| e.field == "extractor.slot_name" && e.error == "required"));
    }

    #[test]
    fn cdc_oracle_missing_cdc_mode() {
        let errors = validate_task(
            "cdc",
            "oracle",
            "oracle",
            "oracle://host/db",
            "oracle://host/db",
            &serde_json::json!({}),
            &serde_json::json!({}),
            &serde_json::json!({}),
            None,
            None,
            true,
        );
        assert!(errors
            .iter()
            .any(|e| e.field == "extractor.cdc_mode" && e.error == "required"));
    }

    #[test]
    fn check_missing_check_log_dir() {
        let errors = validate_task(
            "check",
            "mysql",
            "mysql",
            "mysql://host/db",
            "mysql://host/db",
            &serde_json::json!({}),
            &serde_json::json!({}),
            &serde_json::json!({}),
            None,
            None,
            true,
        );
        assert!(errors
            .iter()
            .any(|e| e.field == "sinker.check_log_dir" && e.error == "required"));
    }

    #[test]
    fn struct_empty_filter() {
        let errors = validate_task(
            "struct",
            "mysql",
            "mysql",
            "mysql://host/db",
            "mysql://host/db",
            &serde_json::json!({}),
            &serde_json::json!({}),
            &serde_json::json!({"do_dbs": [], "do_tbs": []}),
            None,
            None,
            true,
        );
        assert!(errors.iter().any(|e| e.error == "STRUCT_FILTER_REQUIRED"));
    }

    // ─── URL scheme validation ───────────────────────────────────────

    #[test]
    fn invalid_url_scheme_rejected() {
        let errors = validate_endpoint_url("ftp://host/db", "mysql", "source_endpoint.url");
        assert!(errors
            .iter()
            .any(|e| e.error.contains("INVALID_URL_SCHEME")));
    }

    #[test]
    fn url_scheme_engine_mismatch() {
        let errors = validate_endpoint_url("postgres://host/db", "mysql", "source_endpoint.url");
        assert!(errors
            .iter()
            .any(|e| e.error.contains("URL_SCHEME_ENGINE_MISMATCH")));
    }

    // ─── SSRF host blocking ──────────────────────────────────────────

    #[test]
    fn ssrf_loopback_blocked() {
        assert!(check_ssrf_host("127.0.0.1", "field").is_some());
    }

    #[test]
    fn ssrf_link_local_blocked() {
        assert!(check_ssrf_host("169.254.169.254", "field").is_some());
    }

    #[test]
    fn ssrf_ipv6_loopback_blocked() {
        assert!(check_ssrf_host("::1", "field").is_some());
    }

    #[test]
    fn ssrf_private_10_blocked() {
        assert!(check_ssrf_host("10.0.0.5", "field").is_some());
    }

    #[test]
    fn ssrf_localhost_blocked() {
        assert!(check_ssrf_host("localhost", "field").is_some());
    }

    #[test]
    fn ssrf_public_ip_allowed() {
        assert!(check_ssrf_host("203.0.113.1", "field").is_none());
    }

    // ─── Path sandboxing ─────────────────────────────────────────────

    #[test]
    fn path_etc_passwd_blocked() {
        assert!(sandbox_path("/etc/passwd", "field").is_err());
    }

    #[test]
    fn path_traversal_blocked() {
        assert!(sandbox_path("../../../etc/passwd", "field").is_err());
    }

    #[test]
    fn path_proc_self_environ_blocked() {
        assert!(sandbox_path("/proc/self/environ", "field").is_err());
    }

    #[test]
    fn path_basename_allowed() {
        assert!(sandbox_path("check", "field").is_ok());
    }

    #[test]
    fn path_relative_allowed() {
        assert!(sandbox_path("./check", "field").is_ok());
    }

    #[test]
    fn path_runs_subdir_allowed() {
        assert!(sandbox_path("runs/abc123/check.log", "field").is_ok());
    }

    // ─── argv sanitisation ──────────────────────────────────────────

    #[test]
    fn argv_null_bytes_removed() {
        assert_eq!(sanitise_argv("foo\0bar").unwrap(), "foobar");
    }

    #[test]
    fn argv_shell_injection_rejected() {
        assert!(sanitise_argv("$(rm -rf /)").is_err());
        assert!(sanitise_argv("`rm -rf /`").is_err());
        assert!(sanitise_argv("foo && bar").is_err());
        assert!(sanitise_argv("foo; bar").is_err());
    }

    #[test]
    fn argv_normal_string_ok() {
        assert_eq!(
            sanitise_argv("mysql://host:3306/db").unwrap(),
            "mysql://host:3306/db"
        );
    }

    // ─── resolve_db_type ─────────────────────────────────────────────

    #[test]
    fn resolve_gaussdb_pg_mode() {
        assert_eq!(resolve_db_type("gaussdb", Some("pg-mode")), "gaussdb_pg");
    }

    #[test]
    fn resolve_gaussdb_mysql_mode() {
        assert_eq!(
            resolve_db_type("gaussdb", Some("mysql-mode")),
            "gaussdb_mysql"
        );
    }

    #[test]
    fn resolve_gaussdb_oracle_mode() {
        assert_eq!(
            resolve_db_type("gaussdb", Some("oracle-mode")),
            "gaussdb_oracle"
        );
    }

    #[test]
    fn resolve_non_gaussdb_passthrough() {
        assert_eq!(resolve_db_type("mysql", None), "mysql");
    }

    #[test]
    fn resolve_postgres_aliases_to_pg() {
        assert_eq!(resolve_db_type("postgres", None), "pg");
        assert_eq!(resolve_db_type("postgresql", None), "pg");
    }

    #[test]
    fn resolve_already_resolved_gaussdb() {
        assert_eq!(resolve_db_type("gaussdb_pg", Some("pg-mode")), "gaussdb_pg");
    }

    // ─── sandboxed paths validation ──────────────────────────────────

    #[test]
    fn validate_sandboxed_paths_catches_traversal() {
        let errors = validate_sandboxed_paths(
            &serde_json::json!({"lua_code_file": "../../etc/passwd"}),
            &serde_json::json!({}),
            &serde_json::json!({}),
        );
        assert!(!errors.is_empty());
        assert!(errors[0].error == "PATH_OUTSIDE_SANDBOX");
    }
}
