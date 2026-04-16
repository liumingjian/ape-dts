use std::collections::HashMap;

use anyhow::{bail, Context};
use dt_common::meta::col_value::ColValue;
use dt_common::meta::row_type::RowType;

pub(crate) fn row_images_from_logminer(
    row_type: &RowType,
    sql_redo: &str,
    sql_undo: &str,
) -> anyhow::Result<(Option<HashMap<String, ColValue>>, Option<HashMap<String, ColValue>>)> {
    match row_type {
        RowType::Insert => Ok((None, Some(parse_insert_values(sql_redo)?))),
        RowType::Delete => Ok((Some(parse_insert_values(sql_undo)?), None)),
        RowType::Update => {
            let (redo_set, redo_where) = parse_update_set_and_where(sql_redo)?;
            let (undo_set, undo_where) = parse_update_set_and_where(sql_undo)?;

            let mut before = undo_where;
            before.extend(undo_set);
            let mut after = redo_where;
            after.extend(redo_set);

            Ok((Some(before), Some(after)))
        }
    }
}

fn parse_insert_values(sql: &str) -> anyhow::Result<HashMap<String, ColValue>> {
    let trimmed = strip_trailing_semicolon(sql);
    let lower = trimmed.to_lowercase();
    let values_pos = lower
        .find("values")
        .with_context(|| format!("logminer insert missing VALUES keyword: {}", trimmed))?;

    let cols_open = trimmed
        .find('(')
        .with_context(|| format!("logminer insert missing columns '(': {}", trimmed))?;
    let cols_close = find_matching_paren(trimmed, cols_open)?;

    let vals_open = trimmed[values_pos..]
        .find('(')
        .map(|i| values_pos + i)
        .with_context(|| format!("logminer insert missing values '(': {}", trimmed))?;
    let vals_close = find_matching_paren(trimmed, vals_open)?;

    let cols_raw = &trimmed[cols_open + 1..cols_close];
    let vals_raw = &trimmed[vals_open + 1..vals_close];

    let cols = split_csv(cols_raw);
    let vals = split_csv(vals_raw);
    if cols.len() != vals.len() {
        bail!(
            "logminer insert cols/vals mismatch: cols={}, vals={}, sql={}",
            cols.len(),
            vals.len(),
            trimmed
        );
    }

    let mut out = HashMap::with_capacity(cols.len());
    for (idx, col) in cols.iter().enumerate() {
        out.insert(normalize_ident(col), parse_literal(vals[idx])?);
    }
    Ok(out)
}

fn parse_update_set_and_where(
    sql: &str,
) -> anyhow::Result<(HashMap<String, ColValue>, HashMap<String, ColValue>)> {
    let trimmed = strip_trailing_semicolon(sql);
    let lower = trimmed.to_lowercase();

    let set_pos = lower
        .find(" set ")
        .with_context(|| format!("logminer update missing SET keyword: {}", trimmed))?;
    let where_pos = lower
        .find(" where ")
        .with_context(|| format!("logminer update missing WHERE keyword: {}", trimmed))?;
    if where_pos <= set_pos {
        bail!("logminer update malformed (WHERE before SET): {}", trimmed);
    }

    let set_clause = trimmed[set_pos + 5..where_pos].trim();
    let where_clause = trimmed[where_pos + 7..].trim();
    Ok((parse_eq_assignments(set_clause)?, parse_where_eqs(where_clause)?))
}

fn parse_eq_assignments(clause: &str) -> anyhow::Result<HashMap<String, ColValue>> {
    let mut out = HashMap::new();
    for item in clause.split(',') {
        let trimmed = item.trim();
        if trimmed.is_empty() {
            continue;
        }
        let (lhs, rhs) = trimmed
            .split_once('=')
            .with_context(|| format!("logminer assignment missing '=': {}", trimmed))?;
        out.insert(normalize_ident(lhs), parse_literal(rhs)?);
    }
    Ok(out)
}

fn parse_where_eqs(where_clause: &str) -> anyhow::Result<HashMap<String, ColValue>> {
    let mut out = HashMap::new();
    let re = regex::Regex::new(r"(?i)\s+AND\s+").context("build AND splitter regex")?;
    for cond in re.split(where_clause) {
        let trimmed = cond.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Some((lhs, rhs)) = trimmed.split_once('=') else {
            continue;
        };
        out.insert(normalize_ident(lhs), parse_literal(rhs)?);
    }
    Ok(out)
}

fn strip_trailing_semicolon(sql: &str) -> &str {
    sql.trim().trim_end_matches(';').trim()
}

fn normalize_ident(raw: &str) -> String {
    let last = raw.trim().split('.').last().unwrap_or(raw.trim());
    last.trim().trim_matches('"').to_uppercase()
}

fn split_csv(raw: &str) -> Vec<&str> {
    raw.split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect()
}

fn parse_literal(raw: &str) -> anyhow::Result<ColValue> {
    let trimmed = raw.trim().trim_end_matches(')').trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("NULL") || trimmed == "<NULL>" {
        return Ok(ColValue::None);
    }

    let inner = if trimmed.starts_with('\'') && trimmed.ends_with('\'') && trimmed.len() >= 2 {
        trimmed[1..trimmed.len() - 1].replace("''", "'")
    } else {
        trimmed.to_string()
    };

    if inner.contains('.') {
        return Ok(ColValue::Decimal(inner));
    }
    if let Ok(v) = inner.parse::<i64>() {
        return Ok(ColValue::LongLong(v));
    }
    Ok(ColValue::String(inner))
}

fn find_matching_paren(s: &str, open_pos: usize) -> anyhow::Result<usize> {
    let bytes = s.as_bytes();
    let mut depth = 0i32;
    let mut in_single = false;
    let mut in_double = false;

    let mut i = open_pos;
    while i < bytes.len() {
        let c = bytes[i] as char;

        if in_single {
            if c == '\'' {
                if i + 1 < bytes.len() && bytes[i + 1] as char == '\'' {
                    i += 2;
                    continue;
                }
                in_single = false;
            }
            i += 1;
            continue;
        }
        if in_double {
            if c == '"' {
                if i + 1 < bytes.len() && bytes[i + 1] as char == '"' {
                    i += 2;
                    continue;
                }
                in_double = false;
            }
            i += 1;
            continue;
        }

        match c {
            '\'' => in_single = true,
            '"' => in_double = true,
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Ok(i);
                }
            }
            _ => {}
        }
        i += 1;
    }

    bail!("matching ')' not found in sql: {}", s)
}
