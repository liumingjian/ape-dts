use std::collections::{HashMap, HashSet};

use anyhow::{bail, Context};
use dt_common::meta::{
    adaptor::{
        pg_col_value_convertor::PgColValueConvertor,
        sqlx_ext::{SqlxMysqlExt, SqlxPgExt},
    },
    col_value::ColValue,
    mysql::{mysql_col_type::MysqlColType, mysql_tb_meta::MysqlTbMeta},
    pg::pg_tb_meta::PgTbMeta,
    rdb_tb_meta::RdbTbMeta,
    row_data::RowData,
    row_type::RowType,
};
use dt_common::{config::config_enums::DbType, error::Error, utils::sql_util::SqlUtil};
use sqlx::{mysql::MySqlArguments, postgres::PgArguments, query::Query, MySql, Postgres};

pub struct RdbQueryInfo<'a> {
    pub sql: String,
    pub cols: Vec<String>,
    pub binds: Vec<Option<&'a ColValue>>,
}

pub struct RdbQueryBuilder<'a> {
    rdb_tb_meta: &'a RdbTbMeta,
    db_type: DbType,
    ignore_cols: Option<&'a HashSet<String>>,
    pg_tb_meta: Option<&'a PgTbMeta>,
    mysql_tb_meta: Option<&'a MysqlTbMeta>,
}

impl RdbQueryBuilder<'_> {
    #[inline(always)]
    pub fn new_for_mysql<'a>(
        tb_meta: &'a MysqlTbMeta,
        ignore_cols: Option<&'a HashSet<String>>,
    ) -> RdbQueryBuilder<'a> {
        RdbQueryBuilder {
            rdb_tb_meta: &tb_meta.basic,
            pg_tb_meta: None,
            mysql_tb_meta: Some(tb_meta),
            db_type: DbType::Mysql,
            ignore_cols,
        }
    }

    #[inline(always)]
    pub fn new_for_pg<'a>(
        tb_meta: &'a PgTbMeta,
        ignore_cols: Option<&'a HashSet<String>>,
    ) -> RdbQueryBuilder<'a> {
        Self::new_for_pg_compatible(tb_meta, ignore_cols, DbType::Pg)
    }

    #[inline(always)]
    pub fn new_for_pg_compatible<'a>(
        tb_meta: &'a PgTbMeta,
        ignore_cols: Option<&'a HashSet<String>>,
        db_type: DbType,
    ) -> RdbQueryBuilder<'a> {
        RdbQueryBuilder {
            rdb_tb_meta: &tb_meta.basic,
            pg_tb_meta: Some(tb_meta),
            mysql_tb_meta: None,
            db_type,
            ignore_cols,
        }
    }

    #[inline(always)]
    pub fn create_mysql_query<'a>(
        &self,
        query_info: &'a RdbQueryInfo,
    ) -> anyhow::Result<Query<'a, MySql, MySqlArguments>> {
        let mut query: Query<MySql, MySqlArguments> = sqlx::query(&query_info.sql);
        let tb_meta = self
            .mysql_tb_meta
            .as_ref()
            .context("mysql table meta missing when creating mysql query")?;
        for i in 0..query_info.binds.len() {
            let col_type = tb_meta.get_col_type(&query_info.cols[i])?;
            query = query.bind_col_value(query_info.binds[i], col_type);
        }
        Ok(query)
    }

    #[inline(always)]
    pub fn create_pg_query<'a>(
        &self,
        query_info: &'a RdbQueryInfo,
    ) -> anyhow::Result<Query<'a, Postgres, PgArguments>> {
        let mut query: Query<Postgres, PgArguments> = sqlx::query(&query_info.sql);
        let tb_meta = self
            .pg_tb_meta
            .as_ref()
            .context("postgres table meta missing when creating pg query")?;
        for i in 0..query_info.binds.len() {
            let col_type = tb_meta.get_col_type(&query_info.cols[i])?;
            query = query.bind_col_value(query_info.binds[i], col_type);
        }
        Ok(query)
    }

    pub fn get_query_info<'a>(
        &self,
        row_data: &'a RowData,
        replace: bool,
    ) -> anyhow::Result<RdbQueryInfo<'a>> {
        self.get_query_info_internal(row_data, replace, true)
    }

    pub fn get_query_sql(&self, row_data: &RowData, replace: bool) -> anyhow::Result<String> {
        let query_info = self.get_query_info_internal(row_data, replace, false)?;
        Ok(query_info.sql + ";")
    }

    fn get_query_info_internal<'a>(
        &self,
        row_data: &'a RowData,
        replace: bool,
        placeholder: bool,
    ) -> anyhow::Result<RdbQueryInfo<'a>> {
        match row_data.row_type {
            RowType::Insert => {
                if replace {
                    self.get_replace_query(row_data, placeholder)
                } else {
                    self.get_insert_query(row_data, placeholder)
                }
            }
            RowType::Update => self.get_update_query(row_data, placeholder),
            RowType::Delete => self.get_delete_query(row_data, placeholder),
        }
    }

    pub fn get_batch_delete_query<'a>(
        &self,
        data: &'a [RowData],
        start_index: usize,
        batch_size: usize,
    ) -> anyhow::Result<(RdbQueryInfo<'a>, usize)> {
        let mut data_size = 0;
        let mut all_placeholders = Vec::with_capacity(batch_size);
        let mut placeholder_index = 1;
        for _ in 0..batch_size {
            let mut placeholders = Vec::with_capacity(self.rdb_tb_meta.id_cols.len());
            for col in self.rdb_tb_meta.id_cols.iter() {
                placeholders.push(self.get_placeholder(placeholder_index, col)?);
                placeholder_index += 1;
            }
            all_placeholders.push(format!("({})", placeholders.join(",")));
        }

        let sql = format!(
            "DELETE FROM {}.{} WHERE ({}) IN ({})",
            self.escape(&self.rdb_tb_meta.schema),
            self.escape(&self.rdb_tb_meta.tb),
            self.escape_cols(&self.rdb_tb_meta.id_cols).join(","),
            all_placeholders.join(",")
        );

        let cap = batch_size.saturating_mul(self.rdb_tb_meta.id_cols.len());
        let mut cols = Vec::with_capacity(cap);
        let mut binds = Vec::with_capacity(cap);
        for row_data in data.iter().skip(start_index).take(batch_size) {
            data_size += row_data.data_size;
            let before = row_data.require_before()?;
            for col in self.rdb_tb_meta.id_cols.iter() {
                cols.push(col.clone());
                let col_value = Self::get_col_value(before, col)?;
                if col_value.is_none() || matches!(col_value, Some(ColValue::None)) {
                    bail! {
                        "where col: {} is NULL, which should not happen in batch delete, sql: {}",
                        col, sql
                    }
                }
                binds.push(col_value);
            }
        }
        Ok((RdbQueryInfo { sql, cols, binds }, data_size))
    }

    pub fn get_batch_insert_query<'a>(
        &self,
        data: &'a [RowData],
        start_index: usize,
        batch_size: usize,
        replace: bool,
    ) -> anyhow::Result<(RdbQueryInfo<'a>, usize)> {
        let mut malloc_size = 0;
        let mut placeholder_index = 1;
        let mut row_values = Vec::with_capacity(batch_size);
        for _ in 0..batch_size {
            let mut col_values = Vec::with_capacity(self.rdb_tb_meta.cols.len());
            for col in self.rdb_tb_meta.cols.iter() {
                col_values.push(self.get_placeholder(placeholder_index, col)?);
                placeholder_index += 1;
            }
            row_values.push(format!("({})", col_values.join(",")));
        }

        let mut sql = format!(
            "INSERT INTO {}.{}({}) VALUES{}",
            self.escape(&self.rdb_tb_meta.schema),
            self.escape(&self.rdb_tb_meta.tb),
            self.escape_cols(&self.rdb_tb_meta.cols).join(","),
            row_values.join(",")
        );

        let mut cols = Vec::with_capacity(batch_size.saturating_mul(self.rdb_tb_meta.cols.len()));
        let mut binds = Vec::with_capacity(batch_size.saturating_mul(self.rdb_tb_meta.cols.len()));
        for row_data in data.iter().skip(start_index).take(batch_size) {
            malloc_size += row_data.data_size;
            let after = row_data.require_after()?;
            for col_name in self.rdb_tb_meta.cols.iter() {
                cols.push(col_name.clone());
                binds.push(Self::get_col_value(after, col_name)?);
            }
        }

        if replace {
            if self.mysql_tb_meta.is_some() {
                sql = format!("REPLACE{}", sql.trim_start_matches("INSERT"));
            } else if let Some(conflict_clause) = self.get_batch_conflict_clause() {
                sql = format!("{} {}", sql, conflict_clause);
            }
        }
        Ok((RdbQueryInfo { sql, cols, binds }, malloc_size))
    }

    /// The batch counterpart of the single row upsert built by [`Self::get_replace_query`].
    /// New values come from `EXCLUDED` instead of extra binds, so the clause adds no placeholders.
    /// Returns None when the db type has no ON CONFLICT support (mysql uses REPLACE, GaussDBOracle
    /// deletes first, see [`Self::get_batch_replace_delete_query`]), or when the table has no
    /// id cols to use as the conflict target.
    fn get_batch_conflict_clause(&self) -> Option<String> {
        if !matches!(self.db_type, DbType::Pg | DbType::GaussDBPg)
            || self.rdb_tb_meta.id_cols.is_empty()
        {
            return None;
        }

        let set_pairs: Vec<String> = self
            .rdb_tb_meta
            .cols
            .iter()
            .filter(|col| !self.rdb_tb_meta.id_cols.contains(*col))
            .map(|col| {
                let escaped_col = self.escape(col);
                format!("{}=EXCLUDED.{}", escaped_col, escaped_col)
            })
            .collect();

        let conflict_action = if set_pairs.is_empty() {
            // when all columns are primary keys, use DO NOTHING instead of DO UPDATE SET
            "DO NOTHING".to_string()
        } else {
            format!("DO UPDATE SET {}", set_pairs.join(","))
        };

        Some(format!(
            "ON CONFLICT ({}) {}",
            self.escape_cols(&self.rdb_tb_meta.id_cols).join(","),
            conflict_action
        ))
    }

    /// The batch counterpart of [`Self::get_replace_delete_query`]: GaussDBOracle supports no
    /// ON CONFLICT, so a batch insert with replace deletes the incoming keys first.
    pub fn get_batch_replace_delete_query<'a>(
        &self,
        data: &'a [RowData],
        start_index: usize,
        batch_size: usize,
    ) -> anyhow::Result<RdbQueryInfo<'a>> {
        let rows: Vec<&'a RowData> = data.iter().skip(start_index).take(batch_size).collect();
        let where_in_sql = self.get_where_in_info(&rows, 0, rows.len(), true)?;
        let sql = format!(
            "DELETE FROM {}.{} WHERE {}",
            self.escape(&self.rdb_tb_meta.schema),
            self.escape(&self.rdb_tb_meta.tb),
            where_in_sql
        );

        let cap = rows.len().saturating_mul(self.rdb_tb_meta.id_cols.len());
        let mut cols = Vec::with_capacity(cap);
        let mut binds = Vec::with_capacity(cap);
        for row_data in rows.iter() {
            let after = row_data.require_after()?;
            for col in self.rdb_tb_meta.id_cols.iter() {
                cols.push(col.clone());
                binds.push(Self::get_col_value(after, col)?);
            }
        }
        Ok(RdbQueryInfo { sql, cols, binds })
    }

    fn get_replace_query<'a>(
        &self,
        row_data: &'a RowData,
        placeholder: bool,
    ) -> anyhow::Result<RdbQueryInfo<'a>> {
        let mut query_info = self.get_insert_query(row_data, placeholder)?;
        if matches!(self.db_type, DbType::Pg | DbType::GaussDBPg) {
            let mut index = query_info.cols.len() + 1;
            let after = row_data.require_after()?;
            let mut set_pairs = Vec::with_capacity(self.rdb_tb_meta.cols.len());
            for col in self.rdb_tb_meta.cols.iter() {
                if self.rdb_tb_meta.id_cols.contains(col) {
                    continue;
                }
                let col_value = Self::get_col_value(after, col)?;
                let sql_value = self.get_sql_value(index, col, &col_value, placeholder)?;
                let set_pair = format!(r#""{}"={}"#, col, sql_value);
                set_pairs.push(set_pair);
                query_info.cols.push(col.clone());
                query_info.binds.push(col_value);
                index += 1;
            }

            let conflict_clause = if set_pairs.is_empty() {
                // when all columns are primary keys, use DO NOTHING instead of DO UPDATE SET
                "DO NOTHING".to_string()
            } else {
                format!("DO UPDATE SET {}", set_pairs.join(","))
            };

            query_info.sql = format!(
                "{} ON CONFLICT ({}) {}",
                query_info.sql,
                SqlUtil::escape_cols(&self.rdb_tb_meta.id_cols, &self.db_type).join(","),
                conflict_clause
            );
            return Ok(query_info);
        } else if self.db_type != DbType::GaussDBOracle {
            query_info.sql = format!("REPLACE{}", query_info.sql.trim_start_matches("INSERT"));
        }
        Ok(query_info)
    }

    pub fn get_replace_delete_query<'a>(
        &self,
        row_data: &'a RowData,
    ) -> anyhow::Result<RdbQueryInfo<'a>> {
        let after = row_data.require_after()?;
        let (where_sql, not_null_cols) = self.get_where_info(1, after, true)?;
        let sql = format!(
            "DELETE FROM {}.{} WHERE {}",
            self.escape(&self.rdb_tb_meta.schema),
            self.escape(&self.rdb_tb_meta.tb),
            where_sql
        );

        let mut cols = Vec::with_capacity(not_null_cols.len());
        let mut binds = Vec::with_capacity(not_null_cols.len());
        for col_name in not_null_cols.iter() {
            cols.push(col_name.clone());
            binds.push(Self::get_col_value(after, col_name)?);
        }
        Ok(RdbQueryInfo { sql, cols, binds })
    }

    fn get_insert_query<'a>(
        &self,
        row_data: &'a RowData,
        placeholder: bool,
    ) -> anyhow::Result<RdbQueryInfo<'a>> {
        let mut cols = Vec::with_capacity(self.rdb_tb_meta.cols.len());
        let mut binds = Vec::with_capacity(self.rdb_tb_meta.cols.len());
        let after = row_data.require_after()?;
        for col_name in self.rdb_tb_meta.cols.iter() {
            cols.push(col_name.clone());
            binds.push(Self::get_col_value(after, col_name)?);
        }

        let mut col_values = Vec::with_capacity(self.rdb_tb_meta.cols.len());
        for i in 0..self.rdb_tb_meta.cols.len() {
            let sql_value =
                self.get_sql_value(i + 1, &self.rdb_tb_meta.cols[i], &binds[i], placeholder)?;
            col_values.push(sql_value);
        }

        let sql = format!(
            "INSERT INTO {}.{}({}) VALUES({})",
            self.escape(&self.rdb_tb_meta.schema),
            self.escape(&self.rdb_tb_meta.tb),
            self.escape_cols(&self.rdb_tb_meta.cols).join(","),
            col_values.join(",")
        );

        Ok(RdbQueryInfo { sql, cols, binds })
    }

    fn get_delete_query<'a>(
        &self,
        row_data: &'a RowData,
        placeholder: bool,
    ) -> anyhow::Result<RdbQueryInfo<'a>> {
        let before = row_data.require_before()?;
        let (where_sql, not_null_cols) = self.get_where_info(1, before, placeholder)?;
        let mut sql = format!(
            "DELETE FROM {}.{} WHERE {}",
            self.escape(&self.rdb_tb_meta.schema),
            self.escape(&self.rdb_tb_meta.tb),
            where_sql
        );
        if self.rdb_tb_meta.key_map.is_empty() {
            sql += " LIMIT 1";
        }

        let mut cols = Vec::with_capacity(self.rdb_tb_meta.id_cols.len());
        let mut binds = Vec::with_capacity(self.rdb_tb_meta.id_cols.len());
        for col_name in not_null_cols.iter() {
            cols.push(col_name.clone());
            binds.push(Self::get_col_value(before, col_name)?);
        }
        Ok(RdbQueryInfo { sql, cols, binds })
    }

    fn get_update_query<'a>(
        &self,
        row_data: &'a RowData,
        placeholder: bool,
    ) -> anyhow::Result<RdbQueryInfo<'a>> {
        let before = row_data.require_before()?;
        let after = row_data.require_after()?;

        let mut index = 1;
        let set_cols = self.matched_target_cols(after)?;
        let mut set_pairs = Vec::with_capacity(self.rdb_tb_meta.cols.len());
        for col in set_cols.iter() {
            let col_value = Self::get_col_value(after, col)?;
            let sql_value = self.get_sql_value(index, col, &col_value, placeholder)?;
            set_pairs.push(format!("{}={}", self.escape(col), sql_value));
            index += 1;
        }

        if set_pairs.is_empty() {
            bail! {Error::Unexpected(format!(
                "schema: {}, tb: {}, no cols in after, which should not happen in update",
                self.rdb_tb_meta.schema, self.rdb_tb_meta.tb
            ))}
        }

        let (where_sql, not_null_cols) = self.get_where_info(index, before, placeholder)?;
        let mut sql = format!(
            "UPDATE {}.{} SET {} WHERE {}",
            self.escape(&self.rdb_tb_meta.schema),
            self.escape(&self.rdb_tb_meta.tb),
            set_pairs.join(","),
            where_sql,
        );
        if self.rdb_tb_meta.key_map.is_empty() {
            sql += " LIMIT 1";
        }

        let mut cols = set_cols.clone();
        let mut binds = Vec::new();
        for col_name in set_cols.iter() {
            binds.push(Self::get_col_value(after, col_name)?);
        }
        for col_name in not_null_cols.iter() {
            cols.push(col_name.clone());
            binds.push(Self::get_col_value(before, col_name)?);
        }
        Ok(RdbQueryInfo { sql, cols, binds })
    }

    fn matched_target_cols(
        &self,
        values: &HashMap<String, ColValue>,
    ) -> anyhow::Result<Vec<String>> {
        let mut cols = Vec::new();
        for col in self.rdb_tb_meta.cols.iter() {
            if Self::get_col_value(values, col)?.is_some() {
                cols.push(col.clone());
            }
        }
        Ok(cols)
    }

    pub fn get_select_query<'a>(&self, row_data: &'a RowData) -> anyhow::Result<RdbQueryInfo<'a>> {
        self.get_select_query_internal(row_data, true)
    }

    pub fn get_select_query_sql(&self, row_data: &RowData) -> anyhow::Result<String> {
        Ok(self.get_select_query_internal(row_data, false)?.sql + ";")
    }

    fn get_select_query_internal<'a>(
        &self,
        row_data: &'a RowData,
        placeholder: bool,
    ) -> anyhow::Result<RdbQueryInfo<'a>> {
        let after = row_data.require_after()?;
        let (where_sql, not_null_cols) = self.get_where_info(1, after, placeholder)?;
        let mut sql = format!(
            "SELECT {} FROM {}.{} WHERE {}",
            self.build_extract_cols_str()?,
            self.escape(&self.rdb_tb_meta.schema),
            self.escape(&self.rdb_tb_meta.tb),
            where_sql,
        );

        if self.rdb_tb_meta.key_map.is_empty() {
            sql += " LIMIT 1";
        }

        let mut cols = Vec::with_capacity(not_null_cols.len());
        let mut binds = Vec::with_capacity(not_null_cols.len());
        for col_name in not_null_cols.iter() {
            cols.push(col_name.clone());
            binds.push(Self::get_col_value(after, col_name)?);
        }
        Ok(RdbQueryInfo { sql, cols, binds })
    }

    pub fn get_batch_select_query<'a>(
        &self,
        data: &[&'a RowData],
        start_index: usize,
        batch_size: usize,
    ) -> anyhow::Result<RdbQueryInfo<'a>> {
        self.get_batch_select_query_internal(data, start_index, batch_size, true)
    }

    pub fn get_batch_select_query_sql(
        &self,
        data: &[&RowData],
        start_index: usize,
        batch_size: usize,
    ) -> anyhow::Result<String> {
        Ok(self
            .get_batch_select_query_internal(data, start_index, batch_size, false)?
            .sql
            + ";")
    }

    fn get_batch_select_query_internal<'a>(
        &self,
        data: &[&'a RowData],
        start_index: usize,
        batch_size: usize,
        placeholder: bool,
    ) -> anyhow::Result<RdbQueryInfo<'a>> {
        let where_sql = self.get_where_in_info(data, start_index, batch_size, placeholder)?;
        let sql = format!(
            "SELECT {} FROM {}.{} WHERE {}",
            self.build_extract_cols_str()?,
            self.escape(&self.rdb_tb_meta.schema),
            self.escape(&self.rdb_tb_meta.tb),
            where_sql,
        );

        let mut cols =
            Vec::with_capacity(batch_size.saturating_mul(self.rdb_tb_meta.id_cols.len()));
        let mut binds =
            Vec::with_capacity(batch_size.saturating_mul(self.rdb_tb_meta.id_cols.len()));
        for &row_data in data.iter().skip(start_index).take(batch_size) {
            let after = row_data.require_after()?;
            for col in self.rdb_tb_meta.id_cols.iter() {
                cols.push(col.clone());
                let col_value = Self::get_col_value(after, col)?;
                if col_value.is_none() || matches!(col_value, Some(ColValue::None)) {
                    bail! {
                        "schema: {}, tb: {}, where col: {} is NULL, which should not happen in batch select",
                        self.rdb_tb_meta.schema, self.rdb_tb_meta.tb, col
                    }
                }
                binds.push(col_value);
            }
        }
        Ok(RdbQueryInfo { sql, cols, binds })
    }

    pub fn build_extract_cols_str(&self) -> anyhow::Result<String> {
        let mut extract_cols = Vec::new();
        for col in self.rdb_tb_meta.cols.iter() {
            if self.ignore_cols.is_some_and(|cols| cols.contains(col)) {
                continue;
            }

            if let Some(tb_meta) = self.pg_tb_meta {
                if matches!(self.db_type, DbType::GaussDBMySQL) {
                    extract_cols.push(self.escape(col));
                    continue;
                }
                let col_type = tb_meta.get_col_type(col)?;
                let extract_type = PgColValueConvertor::get_extract_type(col_type);
                let extract_col = if extract_type.is_empty() {
                    self.escape(col)
                } else {
                    format!("{}::{}", self.escape(col), extract_type)
                };
                extract_cols.push(extract_col);
            } else {
                extract_cols.push(self.escape(col));
            }
        }
        Ok(extract_cols.join(","))
    }

    fn get_where_info(
        &self,
        mut index: usize,
        col_value_map: &HashMap<String, ColValue>,
        placeholder: bool,
    ) -> anyhow::Result<(String, Vec<String>)> {
        let mut where_sql = String::new();
        let mut not_null_cols = Vec::with_capacity(self.rdb_tb_meta.id_cols.len());

        for col in self.rdb_tb_meta.id_cols.iter() {
            if !where_sql.is_empty() {
                where_sql += " AND";
            }

            let escaped_col = self.escape(col);
            let col_value = Self::get_col_value(col_value_map, col)?;
            if let Some(value) = col_value {
                if *value == ColValue::None {
                    where_sql = format!("{} {} IS NULL", where_sql, escaped_col);
                } else {
                    let sql_value = self.get_sql_value(index, col, &col_value, placeholder)?;
                    where_sql = format!("{} {} = {}", where_sql, escaped_col, sql_value);
                    not_null_cols.push(col.clone());
                }
            } else {
                where_sql = format!("{} {} IS NULL", where_sql, escaped_col);
            }

            index += 1;
        }
        Ok((where_sql.trim_start().into(), not_null_cols))
    }

    fn get_where_in_info(
        &self,
        data: &[&RowData],
        start_index: usize,
        batch_size: usize,
        placeholder: bool,
    ) -> anyhow::Result<String> {
        let mut all_placeholders = Vec::with_capacity(batch_size);
        let mut placeholder_index = 1;
        for row_data in data.iter().skip(start_index).take(batch_size) {
            let after = row_data.require_after()?;
            let mut placeholders = Vec::with_capacity(self.rdb_tb_meta.id_cols.len());
            for col in self.rdb_tb_meta.id_cols.iter() {
                let sql_value = if placeholder {
                    self.get_placeholder(placeholder_index, col)?
                } else {
                    let col_value = Self::get_col_value(after, col)?;
                    self.get_sql_value(placeholder_index, col, &col_value, false)?
                };
                placeholders.push(sql_value);
                placeholder_index += 1;
            }
            all_placeholders.push(format!("({})", placeholders.join(",")));
        }

        Ok(format!(
            "({}) IN ({})",
            self.escape_cols(&self.rdb_tb_meta.id_cols).join(","),
            all_placeholders.join(",")
        ))
    }

    fn get_col_value<'a>(
        col_values: &'a HashMap<String, ColValue>,
        col: &str,
    ) -> anyhow::Result<Option<&'a ColValue>> {
        if let Some(value) = col_values.get(col) {
            return Ok(Some(value));
        }

        let mut matches = col_values
            .iter()
            .filter(|(key, _)| key.eq_ignore_ascii_case(col));
        let Some((matched_col, value)) = matches.next() else {
            return Ok(None);
        };

        if let Some((ambiguous_col, _)) = matches.next() {
            bail!(
                "ambiguous case-insensitive column match for target col: {}, matched cols: {}, {}",
                col,
                matched_col,
                ambiguous_col
            );
        }

        Ok(Some(value))
    }

    fn get_sql_value(
        &self,
        index: usize,
        col: &str,
        col_value: &Option<&ColValue>,
        placeholder: bool,
    ) -> anyhow::Result<String> {
        if placeholder {
            return self.get_placeholder(index, col);
        }

        let col_value = match col_value {
            Some(value) => value,
            None => return Ok("NULL".to_string()),
        };

        if self.mysql_tb_meta.is_some() {
            return self.get_mysql_sql_value(col, col_value);
        }

        Ok(self.get_pg_sql_value(col_value))
    }

    fn get_pg_sql_value(&self, col_value: &ColValue) -> String {
        match col_value {
            ColValue::Blob(v) => format!(r#"'\x{}'"#, hex::encode(v)),
            // For numeric types, we should not quote them in SQL
            ColValue::Tiny(_)
            | ColValue::UnsignedTiny(_)
            | ColValue::Short(_)
            | ColValue::UnsignedShort(_)
            | ColValue::Long(_)
            | ColValue::UnsignedLong(_)
            | ColValue::LongLong(_)
            | ColValue::UnsignedLongLong(_)
            | ColValue::Float(_)
            | ColValue::Double(_)
            | ColValue::Decimal(_) => col_value
                .to_option_string()
                .unwrap_or_else(|| "NULL".to_string()),
            _ => Self::quote_pg_string_literal(col_value),
        }
    }

    fn quote_pg_string_literal(col_value: &ColValue) -> String {
        if let Some(string) = col_value.to_option_string() {
            format!(r#"'{}'"#, string.replace('\'', "''"))
        } else {
            "NULL".to_string()
        }
    }

    fn get_mysql_sql_value(&self, col: &str, col_value: &ColValue) -> anyhow::Result<String> {
        let (value, is_hex_str) = match col_value {
            // varchar, char, tinytext, mediumtext, longtext, text
            ColValue::RawString(v) => SqlUtil::binary_to_str(v),

            // tinyblob, mediumblob, longblob, blob, varbinary, binary
            ColValue::Blob(v) => (hex::encode(v), true),

            _ => {
                if let Some(v) = col_value.to_option_string() {
                    (v, false)
                } else {
                    return Ok("NULL".to_string());
                }
            }
        };

        if is_hex_str {
            return Ok(format!("x'{}'", value));
        }

        let mysql_meta = self
            .mysql_tb_meta
            .as_ref()
            .context("mysql table meta missing while formatting mysql sql value")?;
        let col_type = mysql_meta.get_col_type(col)?;
        let is_str = match col_type {
            MysqlColType::DateTime { .. }
            | MysqlColType::Time { .. }
            | MysqlColType::Date { .. }
            | MysqlColType::Timestamp { .. }
            | MysqlColType::Binary { .. }
            | MysqlColType::VarBinary { .. }
            | MysqlColType::Json => true,
            MysqlColType::Enum { .. } => !matches!(col_value, ColValue::Enum(_)),
            MysqlColType::Set { .. } => !matches!(col_value, ColValue::Set(_)),
            _ => col_type.is_string(),
        };

        if is_str {
            // INSERT INTO tb1 VALUES(1, 'abc''');
            Ok(format!(r#"'{}'"#, value.replace('\'', "\'\'")))
        } else {
            Ok(value)
        }
    }

    fn get_placeholder(&self, index: usize, col: &str) -> anyhow::Result<String> {
        if let Some(tb_meta) = self.pg_tb_meta {
            if matches!(self.db_type, DbType::GaussDBMySQL) {
                return Ok(format!("${}", index));
            }
            let col_type = tb_meta.get_col_type(col)?;
            if col_type.schema_name != "pg_catalog" {
                // for user-defined types, we need to add schema name as prefix, otherwise it will cause error
                return Ok(format!(
                    "${}::\"{}\".\"{}\"",
                    index, col_type.schema_name, col_type.alias
                ));
            }
            // TODO: workaround for types like bit(3)
            let col_type_name = if col_type.alias == "bit" {
                "varbit"
            } else {
                &col_type.alias
            };
            return Ok(format!("${}::{}", index, col_type_name));
        }

        Ok("?".to_string())
    }

    fn escape(&self, origin: &str) -> String {
        SqlUtil::escape_by_db_type(origin, &self.db_type)
    }

    fn escape_cols(&self, cols: &Vec<String>) -> Vec<String> {
        SqlUtil::escape_cols(cols, &self.db_type)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use dt_common::meta::{
        col_value::ColValue,
        pg::{pg_col_type::PgColType, pg_tb_meta::PgTbMeta, pg_value_type::PgValueType},
        rdb_tb_meta::RdbTbMeta,
        row_data::RowData,
        row_type::RowType,
    };

    use super::RdbQueryBuilder;

    #[test]
    fn batch_insert_matches_column_names_case_insensitively() {
        let tb_meta = pg_tb_meta("public", "t_oracle_to_gaussdb_oracle");
        let row_data = oracle_row_data();
        let rows = [row_data];
        let query_builder = RdbQueryBuilder::new_for_pg_compatible(
            &tb_meta,
            None,
            dt_common::config::config_enums::DbType::GaussDBOracle,
        );

        let (query_info, _) = query_builder
            .get_batch_insert_query(&rows, 0, 1, false)
            .unwrap();

        assert_eq!(
            query_info.sql,
            r#"INSERT INTO "public"."t_oracle_to_gaussdb_oracle"("id","tracer","payload") VALUES($1::int4,$2::text,$3::text)"#
        );
        assert_eq!(query_info.binds[0], Some(&ColValue::LongLong(3)));
        assert_eq!(
            query_info.binds[1],
            Some(&ColValue::String("cdc_insert".to_string()))
        );
        assert_eq!(
            query_info.binds[2],
            Some(&ColValue::String("after_insert".to_string()))
        );
    }

    #[test]
    fn case_insensitive_match_rejects_ambiguous_source_columns() {
        let mut col_values = HashMap::new();
        col_values.insert("ID".to_string(), ColValue::LongLong(3));
        col_values.insert("Id".to_string(), ColValue::LongLong(4));

        let err = RdbQueryBuilder::get_col_value(&col_values, "id").unwrap_err();

        assert!(format!("{err:#}").contains("ambiguous case-insensitive column match"));
    }

    #[test]
    fn gaussdb_oracle_replace_does_not_use_on_conflict() {
        let tb_meta = pg_tb_meta("public", "t_oracle_to_gaussdb_oracle");
        let row_data = oracle_row_data();
        let query_builder = RdbQueryBuilder::new_for_pg_compatible(
            &tb_meta,
            None,
            dt_common::config::config_enums::DbType::GaussDBOracle,
        );

        let query_info = query_builder.get_query_info(&row_data, true).unwrap();

        assert!(!query_info.sql.contains("ON CONFLICT"));
        assert_eq!(
            query_info.sql,
            r#"INSERT INTO "public"."t_oracle_to_gaussdb_oracle"("id","tracer","payload") VALUES($1::int4,$2::text,$3::text)"#
        );
    }

    #[test]
    fn gaussdb_oracle_replace_delete_uses_case_insensitive_key_value() {
        let tb_meta = pg_tb_meta("public", "t_oracle_to_gaussdb_oracle");
        let row_data = oracle_row_data();
        let query_builder = RdbQueryBuilder::new_for_pg_compatible(
            &tb_meta,
            None,
            dt_common::config::config_enums::DbType::GaussDBOracle,
        );

        let query_info = query_builder.get_replace_delete_query(&row_data).unwrap();

        assert_eq!(
            query_info.sql,
            r#"DELETE FROM "public"."t_oracle_to_gaussdb_oracle" WHERE "id" = $1::int4"#
        );
        assert_eq!(query_info.binds[0], Some(&ColValue::LongLong(3)));
    }

    #[test]
    fn pg_batch_insert_replace_upserts_on_conflict() {
        let tb_meta = pg_tb_meta("public", "t_batch");
        let rows = [pg_row_data(1), pg_row_data(2)];
        let query_builder = RdbQueryBuilder::new_for_pg(&tb_meta, None);

        let (query_info, _) = query_builder
            .get_batch_insert_query(&rows, 0, 2, true)
            .unwrap();

        assert_eq!(
            query_info.sql,
            r#"INSERT INTO "public"."t_batch"("id","tracer","payload") VALUES($1::int4,$2::text,$3::text),($4::int4,$5::text,$6::text) ON CONFLICT ("id") DO UPDATE SET "tracer"=EXCLUDED."tracer","payload"=EXCLUDED."payload""#
        );
        // the conflict clause must not consume placeholders, binds stay one per inserted col
        assert_eq!(query_info.binds.len(), 6);
        assert_eq!(query_info.cols.len(), 6);
    }

    #[test]
    fn pg_batch_insert_without_replace_keeps_plain_insert() {
        let tb_meta = pg_tb_meta("public", "t_batch");
        let rows = [pg_row_data(1)];
        let query_builder = RdbQueryBuilder::new_for_pg(&tb_meta, None);

        let (query_info, _) = query_builder
            .get_batch_insert_query(&rows, 0, 1, false)
            .unwrap();

        assert!(!query_info.sql.contains("ON CONFLICT"));
    }

    #[test]
    fn gaussdb_pg_batch_insert_replace_upserts_on_conflict() {
        let tb_meta = pg_tb_meta("public", "t_batch");
        let rows = [pg_row_data(1)];
        let query_builder = RdbQueryBuilder::new_for_pg_compatible(
            &tb_meta,
            None,
            dt_common::config::config_enums::DbType::GaussDBPg,
        );

        let (query_info, _) = query_builder
            .get_batch_insert_query(&rows, 0, 1, true)
            .unwrap();

        assert!(query_info
            .sql
            .contains(r#"ON CONFLICT ("id") DO UPDATE SET"#));
    }

    #[test]
    fn pg_batch_insert_replace_does_nothing_when_all_cols_are_keys() {
        let mut tb_meta = pg_tb_meta("public", "t_batch");
        tb_meta.basic.id_cols = tb_meta.basic.cols.clone();
        let rows = [pg_row_data(1)];
        let query_builder = RdbQueryBuilder::new_for_pg(&tb_meta, None);

        let (query_info, _) = query_builder
            .get_batch_insert_query(&rows, 0, 1, true)
            .unwrap();

        assert!(query_info
            .sql
            .ends_with(r#"ON CONFLICT ("id","tracer","payload") DO NOTHING"#));
    }

    #[test]
    fn pg_batch_insert_replace_skips_conflict_clause_without_id_cols() {
        let mut tb_meta = pg_tb_meta("public", "t_batch");
        tb_meta.basic.id_cols = vec![];
        let rows = [pg_row_data(1)];
        let query_builder = RdbQueryBuilder::new_for_pg(&tb_meta, None);

        let (query_info, _) = query_builder
            .get_batch_insert_query(&rows, 0, 1, true)
            .unwrap();

        assert!(!query_info.sql.contains("ON CONFLICT"));
    }

    #[test]
    fn gaussdb_oracle_batch_insert_replace_uses_delete_then_insert() {
        let tb_meta = pg_tb_meta("public", "t_oracle_to_gaussdb_oracle");
        let rows = [oracle_row_data(), oracle_row_data()];
        let query_builder = RdbQueryBuilder::new_for_pg_compatible(
            &tb_meta,
            None,
            dt_common::config::config_enums::DbType::GaussDBOracle,
        );

        let (insert_info, _) = query_builder
            .get_batch_insert_query(&rows, 0, 2, true)
            .unwrap();
        assert!(!insert_info.sql.contains("ON CONFLICT"));

        let delete_info = query_builder
            .get_batch_replace_delete_query(&rows, 0, 2)
            .unwrap();
        assert_eq!(
            delete_info.sql,
            r#"DELETE FROM "public"."t_oracle_to_gaussdb_oracle" WHERE ("id") IN (($1::int4),($2::int4))"#
        );
        assert_eq!(delete_info.cols, vec!["id".to_string(), "id".to_string()]);
        assert_eq!(delete_info.binds[0], Some(&ColValue::LongLong(3)));
        assert_eq!(delete_info.binds[1], Some(&ColValue::LongLong(3)));
    }

    fn pg_row_data(id: i64) -> RowData {
        let mut after = HashMap::new();
        after.insert("id".to_string(), ColValue::LongLong(id));
        after.insert("tracer".to_string(), ColValue::String("t".to_string()));
        after.insert("payload".to_string(), ColValue::String("p".to_string()));
        RowData::new(
            "public".to_string(),
            "t_batch".to_string(),
            RowType::Insert,
            None,
            Some(after),
        )
    }

    fn oracle_row_data() -> RowData {
        let mut after = HashMap::new();
        after.insert("ID".to_string(), ColValue::LongLong(3));
        after.insert(
            "TRACER".to_string(),
            ColValue::String("cdc_insert".to_string()),
        );
        after.insert(
            "PAYLOAD".to_string(),
            ColValue::String("after_insert".to_string()),
        );
        RowData::new(
            "public".to_string(),
            "t_oracle_to_gaussdb_oracle".to_string(),
            RowType::Insert,
            None,
            Some(after),
        )
    }

    fn pg_tb_meta(schema: &str, tb: &str) -> PgTbMeta {
        let cols = vec![
            "id".to_string(),
            "tracer".to_string(),
            "payload".to_string(),
        ];
        PgTbMeta {
            basic: RdbTbMeta {
                schema: schema.to_string(),
                tb: tb.to_string(),
                cols: cols.clone(),
                id_cols: vec!["id".to_string()],
                ..Default::default()
            },
            oid: 1,
            col_type_map: cols
                .into_iter()
                .map(|col| {
                    let col_type = if col == "id" {
                        pg_col_type("int4", PgValueType::Int32)
                    } else {
                        pg_col_type("text", PgValueType::String)
                    };
                    (col, col_type)
                })
                .collect(),
        }
    }

    fn pg_col_type(alias: &str, value_type: PgValueType) -> PgColType {
        PgColType {
            value_type,
            name: alias.to_string(),
            alias: alias.to_string(),
            oid: 0,
            parent_oid: 0,
            element_oid: 0,
            category: String::new(),
            enum_values: None,
            schema_name: "pg_catalog".to_string(),
        }
    }
}
