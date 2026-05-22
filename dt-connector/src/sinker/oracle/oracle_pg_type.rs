use anyhow::bail;

pub(crate) fn pg_type_to_oracle(pg_type: &str) -> anyhow::Result<String> {
    let t = pg_type.trim().to_lowercase();
    if t.contains("[]") {
        bail!("oracle struct: array types are not supported: {}", pg_type);
    }

    match t.as_str() {
        "integer" | "int" | "int4" => return Ok("NUMBER(10)".to_string()),
        "bigint" | "int8" => return Ok("NUMBER(19)".to_string()),
        "smallint" | "int2" => return Ok("NUMBER(5)".to_string()),
        "boolean" | "bool" => return Ok("NUMBER(1)".to_string()),
        "real" | "float4" => return Ok("BINARY_FLOAT".to_string()),
        "double precision" | "float8" => return Ok("BINARY_DOUBLE".to_string()),
        "text" => return Ok("CLOB".to_string()),
        "date" => return Ok("DATE".to_string()),
        "bytea" => return Ok("BLOB".to_string()),
        "json" | "jsonb" => return Ok("CLOB".to_string()),
        "timestamp without time zone" => return Ok("TIMESTAMP".to_string()),
        "timestamp with time zone" => return Ok("TIMESTAMP WITH TIME ZONE".to_string()),
        _ => {}
    };

    if let Some((prec, scale)) =
        parse_numeric(&t, "numeric").or_else(|| parse_numeric(&t, "decimal"))
    {
        return Ok(match (prec, scale) {
            (Some(p), Some(s)) => format!("NUMBER({},{})", p, s),
            (Some(p), None) => format!("NUMBER({})", p),
            _ => "NUMBER".to_string(),
        });
    }

    if let Some(len) = parse_len_type(&t, &["character varying", "varchar", "character"]) {
        return Ok(format!("VARCHAR2({})", len));
    }

    bail!("oracle struct: unsupported pg column type: {}", pg_type);
}

fn parse_numeric(s: &str, prefix: &str) -> Option<(Option<u32>, Option<u32>)> {
    let s = s.trim();
    if !s.starts_with(prefix) {
        return None;
    }
    if s == prefix {
        return Some((None, None));
    }
    let open = s.find('(')?;
    let close = s.rfind(')')?;
    if close <= open + 1 {
        return Some((None, None));
    }
    let inner = &s[open + 1..close];
    let mut parts = inner.split(',').map(|p| p.trim());
    let p = parts.next()?.parse::<u32>().ok();
    let sc = parts.next().and_then(|v| v.parse::<u32>().ok());
    Some((p, sc))
}

fn parse_len_type(s: &str, prefixes: &[&str]) -> Option<u32> {
    let s = s.trim();
    let prefix = prefixes.iter().find(|p| s.starts_with(**p))?;
    let rest = s.strip_prefix(prefix)?.trim();
    if !rest.starts_with('(') {
        return None;
    }
    let close = rest.find(')')?;
    rest[1..close].trim().parse::<u32>().ok()
}
