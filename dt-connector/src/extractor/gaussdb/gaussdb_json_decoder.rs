use std::collections::HashMap;

use anyhow::{bail, Context};
use serde_json::Value;

use dt_common::meta::{col_value::ColValue, dt_data::DtData, row_data::RowData, row_type::RowType};

#[derive(Default, Debug, Clone)]
pub struct GaussDBJsonDecoder {}

impl GaussDBJsonDecoder {
    pub fn decode_message(&self, raw: &str) -> anyhow::Result<Vec<DtData>> {
        let raw = raw.trim();
        if raw.is_empty() {
            return Ok(vec![]);
        }

        // Some output plugins produce plain text BEGIN/COMMIT messages.
        let upper = raw.to_ascii_uppercase();
        if upper.starts_with("BEGIN") {
            return Ok(vec![DtData::Begin {}]);
        }
        if upper.starts_with("COMMIT") {
            return Ok(vec![DtData::Commit { xid: String::new() }]);
        }

        let v: Value = serde_json::from_str(raw).context("failed to parse mppdb_decoding json")?;
        let op_type =
            Self::get_str(&v, &["op_type", "opType", "op"]).context("missing field 'op_type'")?;
        let op_upper = op_type.to_ascii_uppercase();

        if op_upper == "BEGIN" {
            return Ok(vec![DtData::Begin {}]);
        }
        if op_upper == "COMMIT" {
            return Ok(vec![DtData::Commit { xid: String::new() }]);
        }

        let row_type = match op_upper.as_str() {
            "INSERT" => RowType::Insert,
            "UPDATE" => RowType::Update,
            "DELETE" => RowType::Delete,
            _ => {
                if Self::is_ddl_like(&op_upper, &v) {
                    bail!(
                        "unsupported op_type: {} (likely DDL/object event). MVP supports only DML events (INSERT/UPDATE/DELETE). Suggested: run struct sync for DDL objects, avoid online DDL during CDC, or provide raw sample to extend decoder compatibility.",
                        op_type
                    );
                }
                bail!(
                    "unsupported op_type: {}. MVP supports only DML events (INSERT/UPDATE/DELETE). Please provide raw sample to extend decoder compatibility.",
                    op_type
                )
            }
        };

        let table = Self::get_str(&v, &["table", "table_name", "relation", "rel"])
            .context("missing field 'table'/'table_name'")?;
        let (schema, tb) = table
            .split_once('.')
            .with_context(|| format!("invalid table format: {}", table))?;

        let after = match row_type {
            RowType::Insert | RowType::Update => Some(Self::build_col_map(
                &v,
                "columns_name",
                "columns_type",
                "columns_val",
            )?),
            RowType::Delete => None,
        };

        let before = match row_type {
            RowType::Update => {
                match Self::build_col_map(&v, "old_keys_name", "old_keys_type", "old_keys_val") {
                    Ok(map) => Some(map),
                    Err(old_keys_err) => {
                        // Some GaussDB output plugins may omit `old_keys_*` for UPDATE. Fall back to
                        // using the `after` row as the WHERE key (works when primary keys do not change).
                        if let Some(after) = after.as_ref() {
                            Some(after.clone())
                        } else {
                            return Err(old_keys_err);
                        }
                    }
                }
            }
            RowType::Delete => {
                match Self::build_col_map(&v, "old_keys_name", "old_keys_type", "old_keys_val") {
                    Ok(map) => Some(map),
                    Err(old_keys_err) => {
                        // For DELETE, prefer old_keys_*, but fall back to columns_* if needed.
                        match Self::build_col_map(&v, "columns_name", "columns_type", "columns_val")
                        {
                            Ok(map) => Some(map),
                            Err(_) => return Err(old_keys_err),
                        }
                    }
                }
            }
            RowType::Insert => None,
        };

        Ok(vec![DtData::Dml {
            row_data: RowData::new(schema.to_string(), tb.to_string(), row_type, before, after),
        }])
    }

    fn is_ddl_like(op_upper: &str, v: &Value) -> bool {
        if op_upper.contains("DDL") || op_upper == "QUERY" {
            return true;
        }
        // Some plugins emit DDL/query payload in these fields.
        let ddlish_keys = [
            "sql",
            "query",
            "ddl",
            "statement",
            "object_type",
            "objectType",
        ];
        ddlish_keys.iter().any(|k| v.get(*k).is_some())
    }

    fn get_str<'a>(v: &'a Value, keys: &[&str]) -> Option<&'a str> {
        for key in keys {
            if let Some(s) = v.get(*key).and_then(|v| v.as_str()) {
                if !s.is_empty() {
                    return Some(s);
                }
            }
        }
        None
    }

    fn get_vec_str(v: &Value, key: &str) -> Option<Vec<String>> {
        let arr = v.get(key)?.as_array()?;
        let mut out = Vec::with_capacity(arr.len());
        for item in arr {
            if let Some(s) = item.as_str() {
                out.push(s.to_string());
            } else if let Some(n) = item.as_i64() {
                out.push(n.to_string());
            } else if let Some(n) = item.as_u64() {
                out.push(n.to_string());
            } else if let Some(n) = item.as_f64() {
                out.push(n.to_string());
            } else if let Some(b) = item.as_bool() {
                out.push(if b { "true" } else { "false" }.to_string());
            } else if item.is_null() {
                out.push(String::new());
            } else {
                out.push(item.to_string());
            }
        }
        Some(out)
    }

    fn build_col_map(
        v: &Value,
        name_key: &str,
        type_key: &str,
        val_key: &str,
    ) -> anyhow::Result<HashMap<String, ColValue>> {
        let names = Self::get_vec_str(v, name_key)
            .with_context(|| format!("missing field '{}'", name_key))?;
        let vals = Self::get_vec_str(v, val_key)
            .with_context(|| format!("missing field '{}'", val_key))?;
        let types = Self::get_vec_str(v, type_key).unwrap_or_default();

        if names.is_empty() {
            bail!("'{}' is empty", name_key);
        }

        let len = names.len().min(vals.len());
        if len == 0 {
            bail!(
                "invalid column arrays: {} len={}, {} len={}",
                name_key,
                names.len(),
                val_key,
                vals.len()
            );
        }

        let mut map = HashMap::new();
        for i in 0..len {
            let name = names[i].clone();
            let ty = types.get(i).map(|s| s.as_str());
            let val = vals.get(i).map(|s| s.as_str());
            map.insert(name, Self::parse_col_value(ty, val));
        }
        Ok(map)
    }

    fn parse_col_value(ty: Option<&str>, raw_val: Option<&str>) -> ColValue {
        let Some(raw_val) = raw_val else {
            return ColValue::None;
        };

        let raw_val = raw_val.trim();
        if raw_val.is_empty() || raw_val.eq_ignore_ascii_case("null") {
            return ColValue::None;
        }

        let mut val = Self::unquote_sql_literal(raw_val);

        let Some(ty) = ty else {
            return ColValue::String(val);
        };

        let ty = ty.trim().to_ascii_lowercase();
        if ty.is_empty() {
            return ColValue::String(val);
        }

        // Arrays and unknown types: keep as string to avoid over-parsing.
        if ty.starts_with('_') || ty.ends_with("[]") {
            return ColValue::String(val);
        }

        match ty.as_str() {
            "bool" | "boolean" => match val.to_ascii_lowercase().as_str() {
                "t" | "true" | "1" => ColValue::Bool(true),
                "f" | "false" | "0" => ColValue::Bool(false),
                _ => ColValue::String(val),
            },

            "int1" | "int2" | "smallint" | "serial2" | "tinyint" => val
                .parse::<i16>()
                .map(ColValue::Short)
                .unwrap_or_else(|_| ColValue::String(val)),
            "int4" | "int" | "integer" | "serial4" => val
                .parse::<i32>()
                .map(ColValue::Long)
                .unwrap_or_else(|_| ColValue::String(val)),
            "int8" | "bigint" | "serial8" => val
                .parse::<i64>()
                .map(ColValue::LongLong)
                .unwrap_or_else(|_| ColValue::String(val)),

            "float4" | "real" => val
                .parse::<f32>()
                .map(ColValue::Float)
                .unwrap_or_else(|_| ColValue::String(val)),
            "float8" | "double" | "double precision" => val
                .parse::<f64>()
                .map(ColValue::Double)
                .unwrap_or_else(|_| ColValue::String(val)),

            "numeric" | "decimal" => ColValue::Decimal(val),

            "bytea" | "blob" => {
                let bytes = if val.starts_with("0x") {
                    hex::decode(val.trim_start_matches("0x")).ok()
                } else if val.starts_with("\\") {
                    // Some plugins double-escape bytea values and may produce `\\x...` or even
                    // `\\\\x...`. Collapse any number of leading `\\` and then parse `x...`.
                    let trimmed = val.trim_start_matches('\\');
                    if let Some(hex) = trimmed.strip_prefix('x') {
                        hex::decode(hex).ok()
                    } else {
                        hex::decode(trimmed).ok()
                    }
                } else if let Some(hex) = val.strip_prefix('x') {
                    hex::decode(hex).ok()
                } else {
                    hex::decode(val.as_str()).ok()
                };
                bytes
                    .map(ColValue::Blob)
                    .unwrap_or_else(|| ColValue::String(val))
            }

            "timestamptz" => ColValue::Timestamp(val),
            "timestamp" | "smalldatetime" => ColValue::DateTime(val),
            "time" => ColValue::Time(val),
            "date" => ColValue::String(val),

            "json" | "jsonb" => ColValue::Json2(val),

            // bpchar: fixed-length, blank-padded
            "bpchar" => {
                val = val.trim_end().into();
                ColValue::String(val)
            }

            "nvarchar2" | "clob" => ColValue::String(val),

            _ => ColValue::String(val),
        }
    }

    fn unquote_sql_literal(raw: &str) -> String {
        let raw = raw.trim();
        if raw.len() < 2 {
            return raw.to_string();
        }

        // E'...' / e'...'
        if (raw.starts_with("E'") || raw.starts_with("e'")) && raw.ends_with('\'') && raw.len() >= 3
        {
            let inner = &raw[2..raw.len() - 1];
            return inner.replace("''", "'");
        }

        if raw.starts_with('\'') && raw.ends_with('\'') && raw.len() >= 2 {
            let inner = &raw[1..raw.len() - 1];
            return inner.replace("''", "'");
        }

        raw.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_insert() {
        let d = GaussDBJsonDecoder::default();
        let res = d
            .decode_message(r#"{"table":"public.t1","op_type":"INSERT","columns_name":["id","name"],"columns_type":["int4","text"],"columns_val":["1","'hello'"]}"#)
            .unwrap();
        assert_eq!(res.len(), 1);
        let DtData::Dml { row_data } = &res[0] else {
            panic!("expected dml");
        };
        assert_eq!(row_data.schema, "public");
        assert_eq!(row_data.tb, "t1");
        assert_eq!(row_data.row_type, RowType::Insert);
        let after = row_data.after.as_ref().unwrap();
        assert_eq!(after.get("id").unwrap(), &ColValue::Long(1));
        assert_eq!(
            after.get("name").unwrap(),
            &ColValue::String("hello".to_string())
        );
    }

    #[test]
    fn test_decode_insert_table_name() {
        let d = GaussDBJsonDecoder::default();
        let res = d
            .decode_message(r#"{"table_name":"public.t1","op_type":"INSERT","columns_name":["id","name"],"columns_type":["int4","text"],"columns_val":["1","'hello'"]}"#)
            .unwrap();
        assert_eq!(res.len(), 1);
        let DtData::Dml { row_data } = &res[0] else {
            panic!("expected dml");
        };
        assert_eq!(row_data.schema, "public");
        assert_eq!(row_data.tb, "t1");
        assert_eq!(row_data.row_type, RowType::Insert);
    }

    #[test]
    fn test_decode_update() {
        let d = GaussDBJsonDecoder::default();
        let res = d
            .decode_message(r#"{"table":"public.t1","op_type":"UPDATE","columns_name":["id","name"],"columns_type":["int4","text"],"columns_val":["1","'new'"],"old_keys_name":["id"],"old_keys_type":["int4"],"old_keys_val":["1"]}"#)
            .unwrap();
        assert_eq!(res.len(), 1);
        let DtData::Dml { row_data } = &res[0] else {
            panic!("expected dml");
        };
        assert_eq!(row_data.row_type, RowType::Update);
        let before = row_data.before.as_ref().unwrap();
        let after = row_data.after.as_ref().unwrap();
        assert_eq!(before.get("id").unwrap(), &ColValue::Long(1));
        assert_eq!(
            after.get("name").unwrap(),
            &ColValue::String("new".to_string())
        );
    }

    #[test]
    fn test_decode_delete() {
        let d = GaussDBJsonDecoder::default();
        let res = d
            .decode_message(r#"{"table":"public.t1","op_type":"DELETE","old_keys_name":["id"],"old_keys_type":["int4"],"old_keys_val":["1"]}"#)
            .unwrap();
        assert_eq!(res.len(), 1);
        let DtData::Dml { row_data } = &res[0] else {
            panic!("expected dml");
        };
        assert_eq!(row_data.row_type, RowType::Delete);
        let before = row_data.before.as_ref().unwrap();
        assert_eq!(before.get("id").unwrap(), &ColValue::Long(1));
        assert!(row_data.after.is_none());
    }

    #[test]
    fn test_decode_delete_fallback_to_columns() {
        let d = GaussDBJsonDecoder::default();
        let res = d
            .decode_message(r#"{"table":"public.t1","op_type":"DELETE","columns_name":["id"],"columns_type":["int4"],"columns_val":["1"]}"#)
            .unwrap();
        assert_eq!(res.len(), 1);
        let DtData::Dml { row_data } = &res[0] else {
            panic!("expected dml");
        };
        assert_eq!(row_data.row_type, RowType::Delete);
        let before = row_data.before.as_ref().unwrap();
        assert_eq!(before.get("id").unwrap(), &ColValue::Long(1));
        assert!(row_data.after.is_none());
    }

    #[test]
    fn test_decode_update_fallback_to_after_as_before() {
        let d = GaussDBJsonDecoder::default();
        let res = d
            .decode_message(r#"{"table":"public.t1","op_type":"UPDATE","columns_name":["id","name"],"columns_type":["int4","text"],"columns_val":["1","'new'"]}"#)
            .unwrap();
        assert_eq!(res.len(), 1);
        let DtData::Dml { row_data } = &res[0] else {
            panic!("expected dml");
        };
        assert_eq!(row_data.row_type, RowType::Update);
        let before = row_data.before.as_ref().unwrap();
        let after = row_data.after.as_ref().unwrap();
        assert_eq!(before.get("id").unwrap(), &ColValue::Long(1));
        assert_eq!(after.get("id").unwrap(), &ColValue::Long(1));
    }

    #[test]
    fn test_decode_begin_commit_plain_text() {
        let d = GaussDBJsonDecoder::default();
        let begin = d.decode_message("BEGIN").unwrap();
        assert!(matches!(begin[0], DtData::Begin {}));

        let commit = d.decode_message("COMMIT").unwrap();
        assert!(matches!(commit[0], DtData::Commit { .. }));
    }

    #[test]
    fn test_unquote_sql_literal() {
        assert_eq!(
            GaussDBJsonDecoder::unquote_sql_literal("'O''Reilly'"),
            "O'Reilly"
        );
    }

    #[test]
    fn test_decode_bytea_prefixes() {
        assert_eq!(
            GaussDBJsonDecoder::parse_col_value(Some("bytea"), Some("0x0102")),
            ColValue::Blob(vec![1, 2])
        );
        assert_eq!(
            GaussDBJsonDecoder::parse_col_value(Some("bytea"), Some("'\\\\x0102'")),
            ColValue::Blob(vec![1, 2])
        );
        assert_eq!(
            GaussDBJsonDecoder::parse_col_value(Some("bytea"), Some("'\\\\\\\\x0102'")),
            ColValue::Blob(vec![1, 2])
        );
    }

    #[test]
    fn test_decode_unsupported_op_type_ddl_like() {
        let d = GaussDBJsonDecoder::default();
        let err = d
            .decode_message(r#"{"op_type":"DDL","sql":"CREATE TABLE t1(id int)"}"#)
            .unwrap_err();
        assert!(err.to_string().contains("likely DDL"));
        assert!(err.to_string().contains("MVP supports only DML"));
    }

    #[test]
    fn test_decode_unsupported_unknown_op_type_has_raw_sample_hint() {
        let d = GaussDBJsonDecoder::default();
        let err = d
            .decode_message(r#"{"table":"public.t1","op_type":"TRUNCATE"}"#)
            .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("unsupported op_type: TRUNCATE"));
        assert!(msg.contains("MVP supports only DML"));
        assert!(msg.contains("provide raw sample"));
    }

    #[test]
    fn test_decode_gaussdb_alias_types_for_cdc_values() {
        assert_eq!(
            GaussDBJsonDecoder::parse_col_value(Some("tinyint"), Some("8")),
            ColValue::Short(8)
        );
        assert_eq!(
            GaussDBJsonDecoder::parse_col_value(Some("smalldatetime"), Some("'2026-04-02 16:21:00'")),
            ColValue::DateTime("2026-04-02 16:21:00".to_string())
        );
        assert_eq!(
            GaussDBJsonDecoder::parse_col_value(Some("nvarchar2"), Some("'alpha'")),
            ColValue::String("alpha".to_string())
        );
        assert_eq!(
            GaussDBJsonDecoder::parse_col_value(Some("clob"), Some("'updated clob text'")),
            ColValue::String("updated clob text".to_string())
        );
        assert_eq!(
            GaussDBJsonDecoder::parse_col_value(Some("blob"), Some("'00A1FF'")),
            ColValue::Blob(vec![0, 161, 255])
        );
    }
}
