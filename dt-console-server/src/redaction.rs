//! Central secret redaction for every console read surface.
//!
//! One rule, one place: any JSON or INI the console hands back to a client goes
//! through here first. Task endpoints / extractor / sinker configs and alarm
//! channel configs all carry live database passwords, webhook tokens and SNMP
//! communities; before this module only `export?format=json` redacted anything,
//! and only the top-level `password` key of an endpoint.
//!
//! Two sentinels, because the two surfaces round-trip differently:
//! - [`REDACTED`] in JSON — writes accept it back and [`restore_secrets`]
//!   swaps the stored value in, so an edit that never touched the password
//!   cannot blank it.
//! - [`INI_REDACTED`] in rendered INI — a read-only preview surface, never
//!   parsed back in. The INI actually handed to `dt-main` is rendered by
//!   `ini_renderer` and written to disk unredacted; only the HTTP preview /
//!   export paths call [`redact_ini`].

/// Placeholder written over secret values in JSON responses.
pub const REDACTED: &str = "<redacted>";

/// Placeholder written over secret values in rendered INI previews.
pub const INI_REDACTED: &str = "******";

/// Normalise a config key for matching: lowercase, punctuation stripped, so
/// `sasl_password`, `saslPassword` and `SASL-PASSWORD` all compare equal.
fn normalize_key(key: &str) -> String {
    key.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// Keys whose value is a secret in its entirety and is replaced wholesale.
fn is_secret_key(key: &str) -> bool {
    let k = normalize_key(key);
    const NEEDLES: [&str; 10] = [
        "password",
        "passwd",
        "token",
        "secret",
        "apikey",
        "accesskey",
        "privatekey",
        "credential",
        "community",
        "webhook",
    ];
    NEEDLES.iter().any(|n| k.contains(n))
}

/// Keys holding a connection string, where only the `user:password@` userinfo
/// segment is a secret — the host/port/db part stays visible so a redacted
/// task is still identifiable.
fn is_url_key(key: &str) -> bool {
    let k = normalize_key(key);
    k.contains("url") || k.contains("uri") || k.contains("dsn")
}

/// Strip the credential segment from a connection URL.
///
/// `mysql://root:secret@host:3306/db` → `mysql://***@host:3306/db`.
/// URLs without a `scheme://user@` shape are returned unchanged.
pub fn redact_url(url: &str) -> String {
    if let Some(scheme_end) = url.find("://") {
        if let Some(at) = url[scheme_end + 3..].rfind('@') {
            let at = scheme_end + 3 + at;
            return format!("{}***{}", &url[..scheme_end + 3], &url[at..]);
        }
    }
    url.to_string()
}

/// Recursively redact every secret in a JSON value, in place.
///
/// Empty strings are left alone: `password=""` carries nothing and rendering it
/// as `<redacted>` would make "no password set" indistinguishable from one.
pub fn redact_secrets(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, child) in map.iter_mut() {
                match child {
                    serde_json::Value::String(s) if !s.is_empty() => {
                        if is_secret_key(key) {
                            *s = REDACTED.to_string();
                        } else if is_url_key(key) {
                            *s = redact_url(s);
                        }
                    }
                    other => redact_secrets(other),
                }
            }
        }
        serde_json::Value::Array(items) => {
            for item in items.iter_mut() {
                redact_secrets(item);
            }
        }
        _ => {}
    }
}

/// Put stored secrets back where an incoming write echoed a redaction.
///
/// A client that GETs a task, edits one unrelated field and PATCHes the whole
/// config back sends `<redacted>` for the password it never saw. Without this,
/// that write would persist the literal placeholder and break the task. Values
/// that differ from the placeholder are genuine user input and pass through —
/// that is how a password is actually changed.
pub fn restore_secrets(incoming: &mut serde_json::Value, stored: &serde_json::Value) {
    match (incoming, stored) {
        (serde_json::Value::Object(new_map), serde_json::Value::Object(old_map)) => {
            for (key, new_child) in new_map.iter_mut() {
                let Some(old_child) = old_map.get(key) else {
                    continue;
                };
                match (new_child, old_child) {
                    (serde_json::Value::String(new_s), serde_json::Value::String(old_s)) => {
                        if is_secret_key(key) && new_s == REDACTED {
                            *new_s = old_s.clone();
                        } else if is_url_key(key) && new_s.as_str() == redact_url(old_s) {
                            *new_s = old_s.clone();
                        }
                    }
                    (new_child, old_child) => restore_secrets(new_child, old_child),
                }
            }
        }
        (serde_json::Value::Array(new_items), serde_json::Value::Array(old_items)) => {
            for (new_item, old_item) in new_items.iter_mut().zip(old_items.iter()) {
                restore_secrets(new_item, old_item);
            }
        }
        _ => {}
    }
}

/// Redact secrets in rendered INI text, line by line.
///
/// The INI grammar `ini_renderer` emits is flat `key=value` under `[section]`
/// headers, so a line scan is exact here — no continuation lines, no quoting.
pub fn redact_ini(ini: &str) -> String {
    let mut out = String::with_capacity(ini.len());
    for line in ini.split_inclusive('\n') {
        let (content, newline) = match line.strip_suffix('\n') {
            Some(c) => (c, "\n"),
            None => (line, ""),
        };
        match content.split_once('=') {
            Some((key, value)) if !key.trim_start().starts_with('#') => {
                let trimmed = key.trim();
                if value.is_empty() {
                    out.push_str(content);
                } else if is_secret_key(trimmed) {
                    out.push_str(key);
                    out.push('=');
                    out.push_str(INI_REDACTED);
                } else if is_url_key(trimmed) {
                    out.push_str(key);
                    out.push('=');
                    out.push_str(&redact_url(value));
                } else {
                    out.push_str(content);
                }
            }
            _ => out.push_str(content),
        }
        out.push_str(newline);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn secret_keys_matched_across_naming_styles() {
        for key in [
            "password",
            "Password",
            "sasl_password",
            "saslPassword",
            "auth-token",
            "apiKey",
            "client_secret",
            "community",
            "webhookUrl",
        ] {
            assert!(is_secret_key(key), "{key} should be a secret key");
        }
        for key in ["url", "username", "batch_size", "db_type", "parallel_size"] {
            assert!(!is_secret_key(key), "{key} should not be a secret key");
        }
    }

    #[test]
    fn redact_url_strips_credentials_only() {
        assert_eq!(
            redact_url("mysql://root:secret@127.0.0.1:3307/test"),
            "mysql://***@127.0.0.1:3307/test"
        );
        // Password containing '@' — the last '@' is the delimiter.
        assert_eq!(
            redact_url("postgres://u:p@ss@host:5432/db"),
            "postgres://***@host:5432/db"
        );
        assert_eq!(
            redact_url("mysql://127.0.0.1:3307/db"),
            "mysql://127.0.0.1:3307/db"
        );
        assert_eq!(redact_url(""), "");
    }

    #[test]
    fn redact_secrets_walks_nested_values() {
        let mut v = json!({
            "url": "mysql://root:secret@host:3306/db",
            "username": "root",
            "password": "hunter2",
            "empty": "",
            "conn_args": { "password": "nested", "timeout": 5 },
            "channels": [{ "webhook": "https://hooks.example/T/B/XXX" }],
        });
        redact_secrets(&mut v);
        assert_eq!(v["url"], "mysql://***@host:3306/db");
        assert_eq!(v["username"], "root");
        assert_eq!(v["password"], REDACTED);
        assert_eq!(v["empty"], "");
        assert_eq!(v["conn_args"]["password"], REDACTED);
        assert_eq!(v["conn_args"]["timeout"], 5);
        assert_eq!(v["channels"][0]["webhook"], REDACTED);
    }

    #[test]
    fn empty_password_is_not_redacted() {
        let mut v = json!({ "password": "" });
        redact_secrets(&mut v);
        assert_eq!(v["password"], "");
    }

    #[test]
    fn restore_secrets_puts_stored_values_back() {
        let stored = json!({
            "url": "mysql://root:secret@host:3306/db",
            "password": "hunter2",
            "conn_args": { "password": "nested" },
        });
        let mut incoming = stored.clone();
        redact_secrets(&mut incoming);
        restore_secrets(&mut incoming, &stored);
        assert_eq!(incoming, stored);
    }

    #[test]
    fn restore_secrets_keeps_genuine_new_values() {
        let stored = json!({ "password": "old", "url": "mysql://root:old@host/db" });
        let mut incoming = json!({ "password": "new", "url": "mysql://root:new@host/db" });
        restore_secrets(&mut incoming, &stored);
        assert_eq!(incoming["password"], "new");
        assert_eq!(incoming["url"], "mysql://root:new@host/db");
    }

    #[test]
    fn restore_secrets_ignores_unknown_paths() {
        let stored = json!({ "password": "old" });
        let mut incoming = json!({ "password": REDACTED, "extra": REDACTED });
        restore_secrets(&mut incoming, &stored);
        assert_eq!(incoming["password"], "old");
        assert_eq!(incoming["extra"], REDACTED);
    }

    #[test]
    fn redact_ini_masks_secret_and_url_lines() {
        let ini = "[extractor]\ndb_type=mysql\nurl=mysql://root:secret@host:3306/db\npassword=hunter2\nusername=root\nbatch_size=2\n";
        let out = redact_ini(ini);
        assert_eq!(
            out,
            "[extractor]\ndb_type=mysql\nurl=mysql://***@host:3306/db\npassword=******\nusername=root\nbatch_size=2\n"
        );
    }

    #[test]
    fn redact_ini_preserves_shape() {
        // Empty values, section headers, blank lines and a missing trailing
        // newline all survive untouched.
        let ini = "[sinker]\n\npassword=\nreplace=true\n[pipeline]\nbuffer_size=4";
        assert_eq!(redact_ini(ini), ini);
    }

    #[test]
    fn redact_ini_leaves_no_plaintext_secret() {
        let ini = "url=mysql://root:s3cr3t@host/db\nsasl_password=topsecret\n";
        let out = redact_ini(ini);
        assert!(!out.contains("s3cr3t"));
        assert!(!out.contains("topsecret"));
    }
}
