use regex::Regex;

use crate::rdb_filter::RdbFilter;

use crate::meta::struct_meta::structure::structure_type::StructureType;

#[derive(Debug, Clone, PartialEq)]
pub enum PgRoutineKind {
    Function,
    Procedure,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PgCreateRoutineStatement {
    pub schema_name: String,
    pub routine_name: String,
    pub kind: PgRoutineKind,
    /// `pg_get_functiondef(oid)` output.
    pub create_sql: String,
}

impl PgCreateRoutineStatement {
    pub fn route(&mut self, dst_schema: &str, dst_routine: &str) {
        self.schema_name = dst_schema.to_string();
        self.routine_name = dst_routine.to_string();
        self.create_sql = Self::rewrite_header_qualified_name(
            &self.create_sql,
            &self.kind,
            &self.schema_name,
            &self.routine_name,
        );
    }

    pub fn to_sqls(&self, filter: &RdbFilter) -> anyhow::Result<Vec<(String, String)>> {
        let mut sqls = Vec::new();
        if filter.filter_structure(&StructureType::Routine) {
            return Ok(sqls);
        }

        // Routines are schema-bound objects; reuse table filter semantics for allow/deny lists.
        if filter.filter_tb(&self.schema_name, &self.routine_name) {
            return Ok(sqls);
        }

        let key = format!("routine.{}.{}", self.schema_name, self.routine_name);
        let sql = match self.kind {
            PgRoutineKind::Function => Self::normalize_function_create_sql(&self.create_sql),
            // GaussDB and Postgres have incompatible CREATE PROCEDURE syntax.
            // Emit a portable DO block that tries Postgres syntax first, then falls back to
            // GaussDB syntax if the first attempt fails.
            PgRoutineKind::Procedure => Self::build_portable_procedure_create_sql(&self.create_sql),
        };
        sqls.push((key, sql));
        Ok(sqls)
    }

    fn normalize_function_create_sql(create_sql: &str) -> String {
        let sql = Self::strip_script_delimiter(create_sql);

        // `pg_get_functiondef` on GaussDB can include extra clauses that Postgres does not accept,
        // for example: `NOT FENCED NOT SHIPPABLE`.
        //
        // We only attempt to strip them from the header, and keep the routine body unchanged.
        let as_re = Regex::new(r"(?is)\bAS\b").unwrap();
        let Some(m) = as_re.find(&sql) else {
            return sql.trim().to_string();
        };
        let (header, body) = sql.split_at(m.start());

        let mut header = header.to_string();
        for pattern in [
            r"(?is)\s+NOT\s+FENCED\b",
            r"(?is)\s+FENCED\b",
            r"(?is)\s+NOT\s+SHIPPABLE\b",
            r"(?is)\s+SHIPPABLE\b",
        ] {
            let re = Regex::new(pattern).unwrap();
            header = re.replace_all(&header, "").to_string();
        }

        let header = header.trim_end_matches(|c: char| c.is_whitespace());
        let sep = if header.is_empty() { "" } else { "\n" };
        format!("{}{}{}", header, sep, body.trim_start())
            .trim()
            .to_string()
    }

    fn build_portable_procedure_create_sql(create_sql: &str) -> String {
        let sql = Self::strip_script_delimiter(create_sql);
        let (pg_sql, gauss_sql) = match Self::derive_procedure_variants(&sql) {
            Some(v) => v,
            None => return sql.trim().to_string(),
        };

        // Use unique dollar tags to avoid clashing with `$procedure$` / `$function$` inside bodies.
        let do_tag = "$ape_dts_proc_do$";
        let pg_tag = "$ape_dts_proc_pg$";
        let gauss_tag = "$ape_dts_proc_gauss$";

        // We intentionally do not attempt to pattern-match specific SQLSTATEs here: the
        // incompatible syntax errors differ between Postgres/GaussDB versions.
        // If both attempts fail, rethrow the last error to preserve fail-fast semantics.
        format!(
            "DO {do_tag}\nBEGIN\n  BEGIN\n    EXECUTE {pg_tag}\n{pg_sql}\n{pg_tag};\n  EXCEPTION WHEN OTHERS THEN\n    EXECUTE {gauss_tag}\n{gauss_sql}\n{gauss_tag};\n  END;\nEND\n{do_tag};",
            do_tag = do_tag,
            pg_tag = pg_tag,
            gauss_tag = gauss_tag,
            pg_sql = pg_sql.trim(),
            gauss_sql = gauss_sql.trim(),
        )
        .trim()
        .to_string()
    }

    fn derive_procedure_variants(create_sql: &str) -> Option<(String, String)> {
        let signature = Self::extract_signature(create_sql, &PgRoutineKind::Procedure)?;
        let body = Self::extract_body_after_as(create_sql)?;

        let lang = Self::extract_language(create_sql).unwrap_or_else(|| "plpgsql".to_string());
        let pg_sql = format!(
            "{signature}\n LANGUAGE {lang}\nAS $ape_dts_proc_body$\n{body}\n$ape_dts_proc_body$",
            signature = signature.trim(),
            lang = lang.trim(),
            body = body.trim(),
        );

        // GaussDB `CREATE PROCEDURE` does not support dollar-quoted bodies and does not accept
        // `LANGUAGE <lang>` in the header. It expects the body directly after `AS`.
        let gauss_sql = format!(
            "{signature} AS\n{body}",
            signature = signature.trim(),
            body = body.trim()
        );

        Some((pg_sql.trim().to_string(), gauss_sql.trim().to_string()))
    }

    fn extract_signature(create_sql: &str, kind: &PgRoutineKind) -> Option<String> {
        let keyword = match kind {
            PgRoutineKind::Function => "FUNCTION",
            PgRoutineKind::Procedure => "PROCEDURE",
        };
        let header_re = Regex::new(&format!(
            r"(?is)^\s*CREATE\s+(?:OR\s+REPLACE\s+)?{}\b",
            keyword
        ))
        .unwrap();
        let m = header_re.find(create_sql)?;

        // Find the matching `)` of the argument list.
        let open_rel = create_sql[m.end()..].find('(')?;
        let open_idx = m.end() + open_rel;
        let mut depth = 0i32;
        let mut close_idx: Option<usize> = None;
        for (i, ch) in create_sql[open_idx..].char_indices() {
            match ch {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        close_idx = Some(open_idx + i);
                        break;
                    }
                }
                _ => {}
            }
        }
        let close_idx = close_idx?;
        Some(create_sql[..=close_idx].trim().to_string())
    }

    fn extract_body_after_as(create_sql: &str) -> Option<String> {
        // Prefer `AS $tag$ ... $tag$` (Postgres) when present.
        let as_dollar_re = Regex::new(r"(?is)\bAS\s+\$([A-Za-z0-9_]*)\$").unwrap();
        if let Some(m) = as_dollar_re.find(create_sql) {
            let cap = as_dollar_re.captures(m.as_str())?;
            let tag = cap.get(1).map(|m| m.as_str()).unwrap_or_default();
            let delim = format!("${}$", tag);
            let body_start = m.end();
            let close_rel = create_sql[body_start..].rfind(&delim)?;
            let close_idx = body_start + close_rel;
            return Some(create_sql[body_start..close_idx].trim().to_string());
        }

        // GaussDB procedure output uses `AS ...` without dollar-quoting.
        let as_re = Regex::new(r"(?is)\bAS\b").unwrap();
        let m = as_re.find(create_sql)?;
        Some(create_sql[m.end()..].trim().to_string())
    }

    fn extract_language(create_sql: &str) -> Option<String> {
        let re = Regex::new(r"(?is)\bLANGUAGE\s+([a-z0-9_]+)\b").unwrap();
        let caps = re.captures(create_sql)?;
        let lang = caps.get(1)?.as_str().to_string();
        if lang.eq_ignore_ascii_case("plpgsql") || lang.eq_ignore_ascii_case("sql") {
            return Some(lang);
        }
        None
    }

    fn strip_script_delimiter(sql: &str) -> String {
        // GaussDB `pg_get_functiondef` may append an Oracle-style trailing delimiter (`/`)
        // on its own line for procedures. It must be removed when executing via drivers.
        let re = Regex::new(r"(?m)^\s*/\s*$").unwrap();
        re.replace_all(sql, "").trim().to_string()
    }

    fn rewrite_header_qualified_name(
        create_sql: &str,
        kind: &PgRoutineKind,
        dst_schema: &str,
        dst_routine: &str,
    ) -> String {
        // We only rewrite the qualified name in the routine header.
        // The routine body (inside $$ ... $$) must not be modified.
        let keyword = match kind {
            PgRoutineKind::Function => "FUNCTION",
            PgRoutineKind::Procedure => "PROCEDURE",
        };
        let pattern = format!(r"(?is)^\s*CREATE\s+(?:OR\s+REPLACE\s+)?{}\s+", keyword);
        let re = Regex::new(&pattern).unwrap();
        let Some(m) = re.find(create_sql) else {
            return create_sql.to_string();
        };

        let name_start = m.end();
        let Some(paren_rel) = create_sql[name_start..].find('(') else {
            return create_sql.to_string();
        };
        let paren_idx = name_start + paren_rel;

        let prefix = &create_sql[..name_start];
        let suffix = &create_sql[paren_idx..];
        let schema_escaped = dst_schema.replace('"', "\"\"");
        let routine_escaped = dst_routine.replace('"', "\"\"");
        let qualified = format!("\"{}\".\"{}\"", schema_escaped, routine_escaped);
        format!("{}{}{}", prefix, qualified, suffix)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrite_header_qualified_name_only_touches_header() {
        let sql = r#"CREATE OR REPLACE FUNCTION public.f1()
RETURNS integer
LANGUAGE plpgsql
AS $function$
BEGIN
  -- keep `public.f1` inside body
  RAISE NOTICE 'public.f1';
  RETURN 1;
END;
$function$;
"#;
        let rewritten = PgCreateRoutineStatement::rewrite_header_qualified_name(
            sql,
            &PgRoutineKind::Function,
            "dst_schema",
            "dst_name",
        );
        assert!(rewritten.starts_with("CREATE OR REPLACE FUNCTION \"dst_schema\".\"dst_name\"("));
        // Body should remain unchanged.
        assert!(rewritten.contains("RAISE NOTICE 'public.f1'"));
    }

    #[test]
    fn normalize_function_strips_gaussdb_only_clauses() {
        let sql = r#"CREATE OR REPLACE FUNCTION public.f1()
RETURNS integer
LANGUAGE sql
NOT FENCED NOT SHIPPABLE
AS $function$SELECT 1$function$;
"#;
        let normalized = PgCreateRoutineStatement::normalize_function_create_sql(sql);
        assert!(!normalized.to_uppercase().contains("FENCED"));
        assert!(!normalized.to_uppercase().contains("SHIPPABLE"));
        assert!(normalized.contains("LANGUAGE sql"));
        assert!(normalized.contains("AS $function$SELECT 1$function$"));
    }

    #[test]
    fn build_portable_procedure_contains_do_and_both_variants() {
        let sql = r#"CREATE OR REPLACE PROCEDURE public.p1()
LANGUAGE plpgsql
AS $procedure$
BEGIN
  PERFORM 1;
END;
$procedure$;
"#;
        let portable = PgCreateRoutineStatement::build_portable_procedure_create_sql(sql);
        assert!(portable.to_uppercase().starts_with("DO $APE_DTS_PROC_DO$"));
        assert!(portable.contains("EXECUTE $ape_dts_proc_pg$"));
        assert!(portable.contains("CREATE OR REPLACE PROCEDURE"));
        assert!(portable.contains("EXECUTE $ape_dts_proc_gauss$"));
        // Gauss variant must not include LANGUAGE/dollar-quoted body.
        assert!(!portable
            .to_uppercase()
            .contains("LANGUAGE PLPGSQL\nAS $APE_DTS_PROC_GAUSS$"));
    }
}
