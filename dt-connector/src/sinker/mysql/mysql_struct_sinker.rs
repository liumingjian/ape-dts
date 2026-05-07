use std::sync::Arc;

use async_trait::async_trait;
use sqlx::{MySql, Pool};

use std::collections::{HashMap, HashSet};

use crate::{
    rdb_router::RdbRouter,
    sinker::base_struct_sinker::{BaseStructSinker, DBConnPool},
    Sinker,
};
use dt_common::meta::struct_meta::{
    statement::{
        mysql_create_database_statement::MysqlCreateDatabaseStatement,
        mysql_create_table_statement::MysqlCreateTableStatement, struct_statement::StructStatement,
    },
    structure::{
        column::{Column, ColumnDefault},
        database::Database,
        index::{Index, IndexColumn, IndexKind, IndexType},
        table::Table,
    },
};
use dt_common::{
    config::config_enums::ConflictPolicyEnum, meta::struct_meta::struct_data::StructData,
    monitor::monitor::Monitor, rdb_filter::RdbFilter,
};

use regex::Regex;

#[derive(Clone)]
pub struct MysqlStructSinker {
    pub conn_pool: Pool<MySql>,
    pub conflict_policy: ConflictPolicyEnum,
    pub filter: RdbFilter,
    pub router: RdbRouter,
    pub monitor: Arc<Monitor>,
    pub monitor_interval: u64,
}

#[async_trait]
impl Sinker for MysqlStructSinker {
    async fn sink_struct(&mut self, data: Vec<StructData>) -> anyhow::Result<()> {
        // `PgStructExtractor` produces Postgres-flavored struct statements. For MySQL as the
        // destination, translate the subset we can support into MySQL-flavored statements
        // before executing. Unsupported statement kinds are skipped.
        let mut converted = Vec::with_capacity(data.len());
        for struct_data in data.into_iter() {
            match struct_data.statement {
                StructStatement::MysqlCreateDatabase(_) | StructStatement::MysqlCreateTable(_) => {
                    converted.push(struct_data)
                }

                StructStatement::PgCreateSchema(s) => {
                    converted.push(StructData {
                        schema: struct_data.schema,
                        statement: StructStatement::MysqlCreateDatabase(
                            MysqlCreateDatabaseStatement {
                                database: Database {
                                    name: s.schema.name,
                                    ..Default::default()
                                },
                            },
                        ),
                    });
                }

                StructStatement::PgCreateTable(s) => {
                    if let Some(mysql_stmt) = Self::try_convert_pg_table_to_mysql(s)? {
                        converted.push(StructData {
                            schema: struct_data.schema,
                            statement: StructStatement::MysqlCreateTable(mysql_stmt),
                        });
                    }
                }

                // MySQL doesn't support PG routines/views/RBAC in our struct pipeline today.
                // Keep the migration resilient by skipping them explicitly.
                _ => {}
            }
        }

        BaseStructSinker::sink_structs(
            &DBConnPool::MySQL(self.conn_pool.clone()),
            &self.conflict_policy,
            converted,
            &self.filter,
            &self.monitor,
            self.monitor_interval,
        )
        .await
    }

    async fn close(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

impl MysqlStructSinker {
    fn try_convert_pg_table_to_mysql(
        pg: dt_common::meta::struct_meta::statement::pg_create_table_statement::PgCreateTableStatement,
    ) -> anyhow::Result<Option<MysqlCreateTableStatement>> {
        // Only translate the core "table" structure for now; ignore sequences, indexes,
        // constraints (except primary key), and comments. This is sufficient for a basic
        // GaussDBPg -> MySQL bootstrap and can be expanded later.

        let pk_cols = Self::extract_pg_primary_key_cols(&pg.constraints);

        let dst_db = pg.table.schema_name.clone();
        let dst_tb = pg.table.table_name.clone();

        let mut table = Table {
            database_name: dst_db.clone(),
            table_name: dst_tb.clone(),
            ..Default::default()
        };

        let mut mysql_col_types: HashMap<String, String> = HashMap::new();
        table.columns = pg
            .table
            .columns
            .iter()
            .map(|c| {
                let mut out = Column::default();
                out.column_name = c.column_name.clone();
                out.ordinal_position = c.ordinal_position;
                out.is_nullable = c.is_nullable;
                out.extra = String::new();
                out.column_comment = String::new();
                out.character_set_name = String::new();
                out.collation_name = String::new();

                out.column_type = Self::map_pg_col_type_to_mysql(&c.column_type);
                out.column_default =
                    Self::map_pg_col_default_to_mysql(&c.column_default, &out.column_type);

                if Self::should_map_pg_col_to_auto_increment(c, &out.column_type) {
                    out.extra = "auto_increment".to_string();
                    out.is_nullable = false;
                    out.column_default = None;
                }

                if pk_cols.contains(&c.column_name) {
                    out.column_key = "PRI".to_string();
                    // In MySQL, primary key columns are implicitly NOT NULL.
                    out.is_nullable = false;
                }

                mysql_col_types.insert(out.column_name.clone(), out.column_type.clone());
                out
            })
            .collect();

        let indexes = Self::try_convert_pg_indexes_to_mysql(
            &pg.indexes,
            &dst_db,
            &dst_tb,
            &pk_cols,
            &mysql_col_types,
        );

        Ok(Some(MysqlCreateTableStatement {
            table,
            constraints: Vec::new(),
            indexes,
        }))
    }

    fn map_pg_col_type_to_mysql(pg_type: &str) -> String {
        let t = pg_type.trim().to_ascii_lowercase();

        // Common PG spellings.
        if t == "integer" || t == "int4" {
            return "int".to_string();
        }
        if t == "bigint" || t == "int8" {
            return "bigint".to_string();
        }
        if t == "smallint" || t == "int2" {
            return "smallint".to_string();
        }
        if t == "boolean" || t == "bool" {
            return "boolean".to_string();
        }
        if t == "text" {
            return "text".to_string();
        }
        if t == "bytea" {
            return "blob".to_string();
        }

        // varchar/character varying(n)
        if let Some(size) = Self::extract_pg_varchar_size(&t) {
            return format!("varchar({})", size);
        }

        // varchar/character varying (no length)
        if t == "character varying" || t == "varchar" {
            return "varchar(255)".to_string();
        }

        // char/character(n)
        if let Some(size) = Self::extract_pg_char_size(&t) {
            return format!("char({})", size);
        }
        if t == "character" || t == "char" {
            return "char(1)".to_string();
        }

        // numeric(p,s)
        if let Some((p, s)) = Self::extract_pg_numeric_ps(&t) {
            if let Some(s) = s {
                return format!("decimal({},{})", p, s);
            }
            return format!("decimal({})", p);
        }

        if t == "numeric" || t == "decimal" {
            return "decimal(65,0)".to_string();
        }

        if t == "double precision" {
            return "double".to_string();
        }
        if t == "real" {
            return "float".to_string();
        }
        if t == "date" {
            return "date".to_string();
        }
        if t == "timestamp without time zone" || t == "timestamp" {
            return "datetime".to_string();
        }
        if t.starts_with("timestamp(") {
            // e.g. timestamp(6) without time zone
            return "datetime".to_string();
        }
        if t == "timestamp with time zone" {
            return "timestamp".to_string();
        }

        if t == "time without time zone" || t == "time" {
            return "time".to_string();
        }
        if t.starts_with("time(") {
            return "time".to_string();
        }

        if t == "uuid" {
            return "char(36)".to_string();
        }
        if t == "json" || t == "jsonb" {
            return "json".to_string();
        }

        // Fallback: keep the raw type string; MySQL may reject it but this keeps
        // the behavior explicit and debuggable.
        pg_type.trim().to_string()
    }

    fn extract_pg_varchar_size(t: &str) -> Option<u32> {
        // Examples:
        // - "character varying(10)"
        // - "varchar(10)"
        // - "character varying"
        static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
        let re =
            RE.get_or_init(|| Regex::new(r"^(?:character varying|varchar)\((\d+)\)$").unwrap());
        if let Some(caps) = re.captures(t) {
            return caps.get(1)?.as_str().parse::<u32>().ok();
        }
        None
    }

    fn extract_pg_char_size(t: &str) -> Option<u32> {
        static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
        let re = RE.get_or_init(|| Regex::new(r"^(?:character|char)\((\d+)\)$").unwrap());
        let caps = re.captures(t)?;
        let p = caps.get(1)?.as_str().parse::<u32>().ok()?;
        Some(p)
    }

    fn extract_pg_numeric_ps(t: &str) -> Option<(u32, Option<u32>)> {
        static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
        let re = RE.get_or_init(|| Regex::new(r"^numeric\((\d+)(?:,\s*(\d+))?\)$").unwrap());
        let caps = re.captures(t)?;
        let p = caps.get(1)?.as_str().parse::<u32>().ok()?;
        let s = caps.get(2).and_then(|m| m.as_str().parse::<u32>().ok());
        Some((p, s))
    }

    fn should_map_pg_col_to_auto_increment(src_col: &Column, mysql_type: &str) -> bool {
        if !Self::mysql_type_supports_auto_increment(mysql_type) {
            return false;
        }

        if src_col.generated.is_some() {
            return true;
        }

        let Some(def) = &src_col.column_default else {
            return false;
        };

        let raw = match def {
            ColumnDefault::Literal(v) | ColumnDefault::Expression(v) => v,
        };
        raw.trim().to_ascii_lowercase().contains("nextval(")
    }

    fn mysql_type_supports_auto_increment(mysql_type: &str) -> bool {
        let t = mysql_type.trim().to_ascii_lowercase();
        t.starts_with("tinyint")
            || t.starts_with("smallint")
            || t.starts_with("mediumint")
            || t.starts_with("int")
            || t.starts_with("bigint")
    }

    fn map_pg_col_default_to_mysql(
        pg_default: &Option<ColumnDefault>,
        mysql_col_type: &str,
    ) -> Option<ColumnDefault> {
        let raw = match pg_default {
            Some(ColumnDefault::Literal(v)) | Some(ColumnDefault::Expression(v)) => v.trim(),
            None => return None,
        };
        let expr = Self::strip_wrapping_parens(raw);
        if expr.is_empty() {
            return None;
        }

        let lower = expr.to_ascii_lowercase();
        if lower == "null" {
            return None;
        }

        // Sequences/identity defaults are mapped to AUTO_INCREMENT elsewhere.
        if lower.contains("nextval(") {
            return None;
        }

        // Timestamp defaults.
        if lower.contains("now()") || lower.starts_with("current_timestamp") {
            return Some(ColumnDefault::Expression("CURRENT_TIMESTAMP".to_string()));
        }

        // Parse '...'::type string literals (and plain '...').
        if let Some(v) = Self::parse_pg_single_quoted_literal(expr) {
            return Some(ColumnDefault::Literal(v));
        }

        // Strip simple "::type" casts for primitive literals (avoid touching complex expressions).
        let expr_no_cast = if expr.contains("::")
            && !expr.contains('(')
            && !expr.contains('\'')
            && !expr.contains('\"')
        {
            expr.split_once("::").map(|(l, _)| l.trim()).unwrap_or(expr)
        } else {
            expr
        };
        let lower_no_cast = expr_no_cast.to_ascii_lowercase();

        // Booleans: MySQL `SHOW CREATE TABLE` normalizes to 0/1 literals.
        if lower_no_cast == "true" {
            return Some(ColumnDefault::Literal("1".to_string()));
        }
        if lower_no_cast == "false" {
            return Some(ColumnDefault::Literal("0".to_string()));
        }

        // Numeric literals: MySQL `SHOW CREATE TABLE` prints them quoted.
        if Self::looks_like_numeric_literal(expr_no_cast) {
            return Some(ColumnDefault::Literal(expr_no_cast.to_string()));
        }

        // Best-effort: for other expressions, only accept if they look safe for MySQL.
        if Self::looks_like_mysql_safe_default_expr(expr_no_cast, mysql_col_type) {
            return Some(ColumnDefault::Expression(expr_no_cast.to_string()));
        }

        None
    }

    fn strip_wrapping_parens(mut s: &str) -> &str {
        loop {
            let trimmed = s.trim();
            if !trimmed.starts_with('(') || !trimmed.ends_with(')') {
                return trimmed;
            }
            if !Self::is_wrapped_by_single_pair_of_parens(trimmed) {
                return trimmed;
            }
            s = &trimmed[1..trimmed.len() - 1];
        }
    }

    fn is_wrapped_by_single_pair_of_parens(s: &str) -> bool {
        // s starts with '(' and ends with ')'
        let mut depth: i32 = 0;
        for (idx, ch) in s.char_indices() {
            match ch {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 && idx != s.len().saturating_sub(1) {
                        // Outer '(' closes before the end; not a full wrapper.
                        return false;
                    }
                    if depth < 0 {
                        return false;
                    }
                }
                _ => {}
            }
        }
        depth == 0
    }

    fn parse_pg_single_quoted_literal(expr: &str) -> Option<String> {
        let s = expr.trim();
        if !s.starts_with('\'') {
            return None;
        }

        let bytes = s.as_bytes();
        let mut i = 1usize;
        let mut out = String::new();
        while i < bytes.len() {
            let b = bytes[i];
            if b == b'\'' {
                // Escaped quote: '' => '
                if i + 1 < bytes.len() && bytes[i + 1] == b'\'' {
                    out.push('\'');
                    i += 2;
                    continue;
                }
                // Closing quote.
                i += 1;
                break;
            }
            out.push(b as char);
            i += 1;
        }
        if i == 1 {
            return None;
        }

        let rest = s.get(i..)?.trim_start();
        if rest.is_empty() || rest.starts_with("::") {
            return Some(out);
        }
        None
    }

    fn looks_like_numeric_literal(s: &str) -> bool {
        let s = s.trim();
        if s.is_empty() {
            return false;
        }
        let mut seen_dot = false;
        for (idx, ch) in s.char_indices() {
            if idx == 0 && (ch == '-' || ch == '+') {
                continue;
            }
            if ch == '.' {
                if seen_dot {
                    return false;
                }
                seen_dot = true;
                continue;
            }
            if !ch.is_ascii_digit() {
                return false;
            }
        }
        true
    }

    fn looks_like_mysql_safe_default_expr(expr: &str, mysql_col_type: &str) -> bool {
        // Keep this conservative; most defaults should already be handled above.
        let t = mysql_col_type.trim().to_ascii_lowercase();
        let e = expr.trim().to_ascii_lowercase();
        if (t == "datetime" || t.starts_with("timestamp")) && e == "current_timestamp" {
            return true;
        }
        false
    }

    fn try_convert_pg_indexes_to_mysql(
        pg_indexes: &[Index],
        dst_db: &str,
        dst_tb: &str,
        pk_cols: &HashSet<String>,
        mysql_col_types: &HashMap<String, String>,
    ) -> Vec<Index> {
        let mut out = Vec::new();
        for idx in pg_indexes.iter() {
            let def = idx.definition.trim();
            if def.is_empty() {
                continue;
            }

            let lower = def.to_ascii_lowercase();
            // Skip partial indexes and non-btree indexes for now.
            if lower.contains(" where ") {
                continue;
            }
            if !Self::pg_indexdef_is_btree(def) {
                continue;
            }

            let Some(cols) = Self::extract_pg_index_cols(def) else {
                continue;
            };
            if cols.is_empty() {
                continue;
            }

            let cols_set: HashSet<String> = cols.iter().cloned().collect();
            // Primary key is already represented via `PRIMARY KEY (...)` in CREATE TABLE.
            if !pk_cols.is_empty() && idx.index_kind == IndexKind::Unique && cols_set == *pk_cols {
                continue;
            }

            let mut columns = Vec::with_capacity(cols.len());
            let mut unsupported = false;
            for (i, col) in cols.iter().enumerate() {
                if col.is_empty() {
                    unsupported = true;
                    break;
                }
                let mysql_ty = mysql_col_types
                    .get(col)
                    .map(|s| s.to_ascii_lowercase())
                    .unwrap_or_default();
                // MySQL JSON can't be indexed directly (without generated columns). Skip.
                if mysql_ty == "json" {
                    unsupported = true;
                    break;
                }
                let prefix_length = if mysql_ty.starts_with("text") || mysql_ty.starts_with("blob")
                {
                    Some(255)
                } else {
                    None
                };
                columns.push(IndexColumn {
                    column_name: col.clone(),
                    seq_in_index: (i as u32) + 1,
                    prefix_length,
                });
            }
            if unsupported || columns.is_empty() {
                continue;
            }

            out.push(Index {
                database_name: dst_db.to_string(),
                schema_name: String::new(),
                table_name: dst_tb.to_string(),
                index_name: idx.index_name.clone(),
                index_kind: idx.index_kind.clone(),
                index_type: IndexType::Btree,
                comment: String::new(),
                table_space: String::new(),
                definition: String::new(),
                columns,
            });
        }
        out
    }

    fn pg_indexdef_is_btree(def: &str) -> bool {
        let lower = def.to_ascii_lowercase();
        let Some(pos) = lower.find(" using ") else {
            // No USING clause -> default access method is btree.
            return true;
        };
        let after = lower[pos + " using ".len()..].trim_start();
        let method = after
            .split_whitespace()
            .next()
            .unwrap_or("")
            .trim_end_matches('(');
        // GaussDB may use `ubtree` access method; treat it as btree-equivalent for MySQL.
        method.is_empty() || method == "btree" || method == "ubtree"
    }

    fn extract_pg_index_cols(def: &str) -> Option<Vec<String>> {
        let start = def.find('(')?;
        let mut depth: i32 = 0;
        let mut open_idx: Option<usize> = None;
        let mut close_idx: Option<usize> = None;
        for (idx, ch) in def.char_indices().skip(start) {
            match ch {
                '(' => {
                    depth += 1;
                    if depth == 1 {
                        open_idx = Some(idx + 1);
                    }
                }
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        close_idx = Some(idx);
                        break;
                    }
                    if depth < 0 {
                        return None;
                    }
                }
                _ => {}
            }
        }
        let (open, close) = (open_idx?, close_idx?);
        if open > close || close > def.len() {
            return None;
        }
        let inner = &def[open..close];

        let mut cols = Vec::new();
        let mut buf = String::new();
        let mut depth: i32 = 0;
        for ch in inner.chars() {
            match ch {
                '(' => {
                    depth += 1;
                    buf.push(ch);
                }
                ')' => {
                    depth -= 1;
                    if depth < 0 {
                        return None;
                    }
                    buf.push(ch);
                }
                ',' if depth == 0 => {
                    let part = buf.trim();
                    if let Some(col) = Self::parse_pg_index_column_spec(part) {
                        cols.push(col);
                    } else {
                        return None;
                    }
                    buf.clear();
                }
                _ => buf.push(ch),
            }
        }
        let part = buf.trim();
        if !part.is_empty() {
            if let Some(col) = Self::parse_pg_index_column_spec(part) {
                cols.push(col);
            } else {
                return None;
            }
        }
        Some(cols)
    }

    fn parse_pg_index_column_spec(spec: &str) -> Option<String> {
        let mut s = spec.trim();
        if s.is_empty() {
            return None;
        }
        s = Self::strip_wrapping_parens(s);

        // Expressions are not supported in this bootstrap path.
        if s.contains('(') || s.contains(')') {
            return None;
        }

        let first = if s.starts_with('\"') {
            // Parse a quoted identifier, handling "" escapes.
            let bytes = s.as_bytes();
            let mut i = 1usize;
            let mut out = String::new();
            while i < bytes.len() {
                let b = bytes[i];
                if b == b'\"' {
                    if i + 1 < bytes.len() && bytes[i + 1] == b'\"' {
                        out.push('\"');
                        i += 2;
                        continue;
                    }
                    break;
                }
                out.push(b as char);
                i += 1;
            }
            out
        } else if let Some(stripped) = s.strip_prefix('`') {
            let end = stripped.find('`')?;
            stripped[..end].to_string()
        } else {
            s.split_whitespace().next()?.to_string()
        };

        if first.is_empty() {
            return None;
        }
        let last = first.rsplit_once('.').map(|(_, v)| v).unwrap_or(&first);
        let col = last.trim().trim_matches('\"').trim_matches('`');
        if col.is_empty() {
            return None;
        }
        // Keep it conservative: only accept simple identifiers.
        if !col.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return None;
        }
        Some(col.to_string())
    }

    fn extract_pg_primary_key_cols(
        constraints: &[dt_common::meta::struct_meta::structure::constraint::Constraint],
    ) -> std::collections::HashSet<String> {
        use dt_common::meta::struct_meta::structure::constraint::ConstraintType;

        static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
        let re = RE.get_or_init(|| Regex::new(r"(?i)primary\s+key\s*\(([^)]+)\)").unwrap());

        let mut out = std::collections::HashSet::new();
        for c in constraints.iter() {
            if c.constraint_type != ConstraintType::Primary {
                continue;
            }
            if let Some(caps) = re.captures(&c.definition) {
                if let Some(cols) = caps.get(1).map(|m| m.as_str()) {
                    for col in cols.split(',') {
                        let col = col.trim().trim_matches('"').trim_matches('`');
                        if !col.is_empty() {
                            out.insert(col.to_string());
                        }
                    }
                }
            }
        }
        out
    }
}
